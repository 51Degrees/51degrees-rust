/* *********************************************************************
 * This Original Work is copyright of 51 Degrees Mobile Experts Limited.
 * Copyright 2026 51 Degrees Mobile Experts Limited, Davidson House,
 * Forbury Square, Reading, Berkshire, United Kingdom RG1 3EU.
 *
 * This Original Work is licensed under the European Union Public Licence
 * (EUPL) v.1.2 and is subject to its terms as set out below.
 *
 * If a copy of the EUPL was not distributed with this file, You can obtain
 * one at https://opensource.org/licenses/EUPL-1.2.
 *
 * The 'Compatible Licences' set out in the Appendix to the EUPL (as may be
 * amended by the European Commission) shall be deemed incompatible for
 * the purposes of the Work and the provisions of the compatibility
 * clause in Article 5 of the EUPL shall not apply.
 *
 * If using the Work as, or as part of, a network application, by
 * including the attribution notice(s) required under Article 5 of the EUPL
 * in the end user terms of the application under an appropriate heading,
 * such notice(s) shall fulfill the requirements of that article.
 * ********************************************************************* */

//! Safe RAII wrappers over the native IP Intelligence engine.
//!
//! Mirrors the Device Detection wrappers in [`crate::dd`]. [`Manager`] owns a
//! native IP Intelligence resource manager loaded from an `.ipi` data file and
//! frees it on drop. [`Results`] owns a native results structure, holds an
//! [`Arc`] to the manager so the data set stays loaded, and frees the results
//! on drop.
//!
//! [`Results::process_evidence`] looks up the client IP through the native
//! evidence entry point. It reads the evidence keys the data file defines, in
//! the order the native lookup tries them, and passes on every value it can
//! read as an IP address. The engine links its own copy of the shared common C
//! library sources built with wide file offsets, which is ABI incompatible with
//! the copy `fiftyone-common-sys` builds, so the native evidence array is made
//! with that copy's functions in `fiftyone-ip-intelligence-sys`. Values are
//! read by name into the same reusable per-thread buffer the Device Detection
//! wrapper uses.
//!
//! IP Intelligence values are inherently weighted: a property may resolve to
//! several candidate values, each with a confidence weighting.
//! [`Results::value_as_string`] flattens those to a single separated string,
//! whereas [`Results::values_weighted`] returns each candidate paired with its
//! raw `u16` weighting (highest first), which the Phase 3 on-premise engine
//! surfaces as `AspectPropertyValue<Vec<WeightedValue<T>>>`.
//!
//! # Thread safety
//!
//! As for Device Detection, [`Manager`] is [`Send`] and [`Sync`] (the native
//! manager is reference counted and lock protected) and [`Results`] is [`Send`]
//! but not [`Sync`] (per-thread scratch). A data set that reads from its file
//! after it is loaded holds one file handle for each concurrent operation it
//! was opened to expect, so no more calls that can read the file run at once
//! than there are handles, and a call beyond them waits for one. The native
//! library ends the process when a read of a property's values finds no free
//! handle.

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::path::Path;
use std::ptr::NonNull;
use std::sync::{Arc, OnceLock};

use fiftyone_ip_intelligence_sys as sys;
use fiftyone_pipeline_core::{constants, Error, Evidence, Result};

use crate::buffer::with_value_string;
use crate::evidence::parse_client_ip;
use crate::file_reads::{FileReadPermit, FileReads};
use crate::profile::PerformanceProfile;
use crate::status::{status_name, status_to_result, take_message, NativeException, NativeStatus};

// ---------------------------------------------------------------------------
// Status and exception trait wiring for the IP Intelligence common C library
// build
// ---------------------------------------------------------------------------

impl NativeStatus for sys::StatusCode {
    fn is_success(&self) -> bool {
        *self == sys::StatusCode::Success
    }

    fn name(&self) -> &'static str {
        status_name(*self)
    }
}

impl NativeException for sys::Exception {
    fn cleared() -> Self {
        sys::Exception::cleared()
    }

    fn is_okay(&self) -> bool {
        sys::Exception::is_okay(self)
    }

    unsafe fn message_ptr(&mut self) -> *const c_char {
        sys::common::fiftyoneDegreesExceptionGetMessage(self)
    }

    unsafe fn free_message(ptr: *mut c_void) {
        sys::common::fiftyoneDegreesFree(ptr);
    }
}

/// The stable name of an IP Intelligence status code.
fn status_name(status: sys::StatusCode) -> &'static str {
    status_name!(sys::StatusCode, status)
}

/// Resolve a [`PerformanceProfile`] to a pointer to the matching predefined IP
/// Intelligence configuration global. The configuration is opaque, so only its
/// address is taken, which is what the init functions expect.
fn config_for(profile: PerformanceProfile) -> *mut sys::ConfigIpi {
    // Taking the address of a `static mut` exported by the C library is safe.
    // The pointer is only read through by the init functions.
    match profile {
        PerformanceProfile::InMemory => {
            std::ptr::addr_of_mut!(sys::fiftyoneDegreesIpiInMemoryConfig)
        }
        PerformanceProfile::HighPerformance => {
            std::ptr::addr_of_mut!(sys::fiftyoneDegreesIpiHighPerformanceConfig)
        }
        PerformanceProfile::LowMemory => {
            std::ptr::addr_of_mut!(sys::fiftyoneDegreesIpiLowMemoryConfig)
        }
        PerformanceProfile::Balanced => {
            std::ptr::addr_of_mut!(sys::fiftyoneDegreesIpiBalancedConfig)
        }
        PerformanceProfile::Default => {
            std::ptr::addr_of_mut!(sys::fiftyoneDegreesIpiDefaultConfig)
        }
    }
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

/// A loaded IP Intelligence data set.
///
/// Owns a native [`sys::ResourceManager`] initialized from an `.ipi` data file.
/// Shared across threads behind the [`Arc`] returned by [`Manager::open`]. The
/// data set is freed once the last reference and every referencing [`Results`]
/// have been dropped.
pub struct Manager {
    manager: NonNull<sys::ResourceManager>,
    /// The evidence keys the data file defines, in the order the native lookup
    /// tries them, read once when the file is opened.
    evidence_keys: Vec<EvidenceKey>,
    /// The required property name to index map, built on first use so a read
    /// by name costs one hash lookup rather than a native name search.
    property_index: OnceLock<HashMap<String, c_int>>,
    /// Permits for the calls that can read the data file, one for each of its
    /// file handles, or [`None`] when the data set is held in memory.
    file_reads: Option<FileReads>,
}

/// One evidence key the data file defines, split into the parts the native
/// evidence array takes.
struct EvidenceKey {
    /// The pipeline key, for example `query.client-ip-51d`.
    key: String,
    /// The native category of the key's prefix.
    prefix: sys::EvidencePrefix,
    /// The header name after the prefix, as the native key.
    field: CString,
}

// Safety: the native resource manager is internally reference counted and lock
// protected per `src/common/resource.h`, so a shared `&Manager` is sound to use
// from several threads, and the file read permits are lock protected.
// Therefore the manager is both `Send` and `Sync`.
unsafe impl Send for Manager {}
unsafe impl Sync for Manager {}

impl Manager {
    /// Open an IP Intelligence data file with the given performance profile,
    /// requesting all available properties.
    pub fn open(path: impl AsRef<Path>, profile: PerformanceProfile) -> Result<Arc<Manager>> {
        Self::open_with_properties(path, profile, None)
    }

    /// Open an IP Intelligence data file requesting only the named properties,
    /// or all properties when `properties` is [`None`].
    pub fn open_with_properties(
        path: impl AsRef<Path>,
        profile: PerformanceProfile,
        properties: Option<&[&str]>,
    ) -> Result<Arc<Manager>> {
        Self::open_with_options(path, profile, properties, None)
    }

    /// Open an IP Intelligence data file with the named properties and an
    /// optional expected concurrency.
    ///
    /// `concurrency` sizes the file-handle pool the file-backed collections use
    /// under the `LowMemory` and `Balanced` profiles, which is how many calls
    /// can read the data file at once. A call beyond that waits for a handle,
    /// so it should be the number of threads that will look up through the
    /// data set at once. [`None`] keeps the profile's default. It has no effect
    /// on the in-memory profiles, where every collection is already resident.
    pub fn open_with_options(
        path: impl AsRef<Path>,
        profile: PerformanceProfile,
        properties: Option<&[&str]>,
        concurrency: Option<u16>,
    ) -> Result<Arc<Manager>> {
        let path = path.as_ref();
        let path_c = crate::path::native_path(path)?;

        let property_list = properties.map(|names| names.join(","));
        let property_list_c = property_list
            .as_deref()
            .map(CString::new)
            .transpose()
            .map_err(|_| Error::Native {
                status: String::from("InvalidInput"),
                message: String::from("a property name contains an interior nul byte"),
            })?;

        let mut required = sys::PropertiesRequired::all_properties();
        if let Some(list) = property_list_c.as_ref() {
            required.string = list.as_ptr();
        }

        let boxed = Box::new(sys::ResourceManager::zeroed());
        let manager_ptr = Box::into_raw(boxed);

        let mut exception = sys::Exception::cleared();
        // Safety: all pointers are valid for the call and the path is null
        // terminated. The shim copies the configuration before setting the
        // concurrency, so the shared global is not changed.
        let status = unsafe {
            sys::fiftyoneDegreesShimIpiInitManagerFromFile(
                manager_ptr,
                config_for(profile),
                concurrency.map_or(-1, i32::from),
                &mut required,
                path_c.as_ptr(),
                &mut exception,
            )
        };

        if let Err(err) = status_to_result(status, || status_detail(status, &path_c)) {
            // Safety: `manager_ptr` came from `Box::into_raw` and was not
            // populated by a failed init.
            unsafe { drop(Box::from_raw(manager_ptr)) };
            return Err(err);
        }
        if let Err(err) = exception.check() {
            unsafe {
                sys::fiftyoneDegreesResourceManagerFree(manager_ptr);
                drop(Box::from_raw(manager_ptr));
            }
            return Err(err);
        }

        let manager = NonNull::new(manager_ptr).expect("init produced a non-null manager");
        // Safety: the manager was initialized above.
        let evidence_keys = unsafe { read_evidence_keys(manager.as_ptr()) };
        // Safety: as above.
        let handles = unsafe { sys::fiftyoneDegreesShimIpiGetFileHandleCount(manager.as_ptr()) };
        Ok(Arc::new(Manager {
            manager,
            evidence_keys,
            property_index: OnceLock::new(),
            file_reads: FileReads::for_handles(handles),
        }))
    }

    fn as_ptr(&self) -> *mut sys::ResourceManager {
        self.manager.as_ptr()
    }

    /// A permit for a call that can read the data file, waiting while every
    /// file handle is in use. [`None`] when the data set is held in memory.
    fn file_read(&self) -> Option<FileReadPermit<'_>> {
        self.file_reads.as_ref().map(FileReads::permit)
    }

    /// The evidence keys the data file defines, in the order the native lookup
    /// tries them, lowercased.
    ///
    /// Each header the data file names is accepted as a query value and as a
    /// server value, as in the other 51Degrees languages. The native lookup
    /// tries every query value, in the data file's order, before any server
    /// value, so the query keys come first.
    pub fn evidence_keys(&self) -> impl ExactSizeIterator<Item = &str> + '_ {
        self.evidence_keys.iter().map(|key| key.key.as_str())
    }

    /// The data set's name, which is the data file's tier (for example `Lite`,
    /// `Enterprise` or `TAC`), or [`None`] when it cannot be read.
    ///
    /// Read from the data set header through this crate's shim, mirroring the
    /// Hash engine. The on-premise engine uses this to report the real tier
    /// rather than a fixed default.
    pub fn data_set_name(&self) -> Option<String> {
        let mut buffer = vec![0u8; 128];
        let _file_read = self.file_read();
        // Safety: the buffer is writable for its full length and the manager is
        // initialized for the lifetime of `self`.
        let written = unsafe {
            sys::fiftyoneDegreesShimIpiGetDataSetName(
                self.as_ptr(),
                buffer.as_mut_ptr() as *mut c_char,
                buffer.len(),
            )
        };
        if written == 0 {
            return None;
        }
        let end = written.min(buffer.len());
        let name = String::from_utf8_lossy(&buffer[..end]).into_owned();
        if name.trim().is_empty() {
            None
        } else {
            Some(name)
        }
    }

    /// The data set's embedded published date as `(year, month, day)`, or
    /// [`None`] when it cannot be read.
    ///
    /// Read from the data set header through this crate's shim. A returned date
    /// with a zero year, month or day is treated as unset and yields [`None`].
    pub fn data_set_published(&self) -> Option<(i32, u32, u32)> {
        let mut year: c_int = 0;
        let mut month: c_int = 0;
        let mut day: c_int = 0;
        // Safety: the manager is initialized and each output pointer is writable.
        let ok = unsafe {
            sys::fiftyoneDegreesShimIpiGetDataSetPublished(
                self.as_ptr(),
                &mut year,
                &mut month,
                &mut day,
            )
        };
        if ok == 0 || year <= 0 || month <= 0 || day <= 0 {
            return None;
        }
        Some((year, month as u32, day as u32))
    }

    /// The names of every property the data set was initialized with, in
    /// required-property index order.
    ///
    /// When the engine opens the file without a property restriction this is the
    /// data file's full property catalogue, so it reflects the tier (an ASN file
    /// carries the autonomous-system and registered-range properties, while an
    /// Enterprise file additionally carries the location and behavioural ones).
    /// Read from the data set's `available` properties through this crate's shim.
    pub fn available_property_names(&self) -> Vec<String> {
        // Safety: the manager is initialized for the lifetime of `self`.
        let count = unsafe { sys::fiftyoneDegreesShimIpiGetRequiredPropertyCount(self.as_ptr()) };
        let mut names = Vec::with_capacity(count as usize);
        for index in 0..count {
            let mut buffer = vec![0u8; 256];
            // Safety: the buffer is writable for its full length and the manager
            // is initialized.
            let written = unsafe {
                sys::fiftyoneDegreesShimIpiGetRequiredPropertyName(
                    self.as_ptr(),
                    index as c_int,
                    buffer.as_mut_ptr() as *mut c_char,
                    buffer.len(),
                )
            };
            if written == 0 {
                continue;
            }
            let end = written.min(buffer.len());
            let name = String::from_utf8_lossy(&buffer[..end]).into_owned();
            if !name.trim().is_empty() {
                names.push(name);
            }
        }
        names
    }

    /// Resolve a property name to its required property index, building the
    /// name to index map on first use.
    ///
    /// Returns [`None`] when the property is not one of the required
    /// properties. The lookup is case sensitive, as the native data set is, so
    /// pass the property's spelling from the data file. An index belongs to the
    /// data set it came from, so look it up again after a reload.
    pub fn required_property_index(&self, property_name: &str) -> Option<c_int> {
        self.property_index_map().get(property_name).copied()
    }

    /// The property name to required property index map, built on first use.
    /// The first caller builds it and racing callers wait for it. Once it is
    /// set, a lookup takes no lock and writes no shared memory.
    fn property_index_map(&self) -> &HashMap<String, c_int> {
        self.property_index.get_or_init(|| {
            self.available_property_names()
                .into_iter()
                .enumerate()
                .map(|(index, name)| (name, index as c_int))
                .collect()
        })
    }

    /// Refuse an index that is not one of this data set's required property
    /// indexes.
    fn check_property_index(&self, index: c_int) -> Result<()> {
        let count = self.property_index_map().len();
        if index < 0 || index as usize >= count {
            return Err(Error::Native {
                status: String::from("InvalidInput"),
                message: format!("{index} is not a required property index (0..{count})"),
            });
        }
        Ok(())
    }

    /// Create a per-thread results structure for running lookups.
    pub fn create_results(self: &Arc<Self>) -> Result<Results> {
        // Safety: the manager is initialized.
        let results = unsafe { sys::fiftyoneDegreesResultsIpiCreate(self.as_ptr()) };
        let results = NonNull::new(results).ok_or_else(|| Error::Native {
            status: String::from("InsufficientMemory"),
            message: String::from("failed to allocate IP Intelligence results"),
        })?;
        Ok(Results {
            results,
            manager: Arc::clone(self),
        })
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        // Safety: the manager was initialized by a successful `open` and every
        // referencing `Results` held an `Arc`, so they have all been dropped.
        unsafe {
            sys::fiftyoneDegreesResourceManagerFree(self.manager.as_ptr());
            drop(Box::from_raw(self.manager.as_ptr()));
        }
    }
}

/// Read the evidence keys the data set behind `manager` defines, being every
/// header it names under the query prefix and then under the server prefix,
/// in the data set's order. That is the order the native lookup tries them.
///
/// # Safety
/// `manager` must be initialized.
unsafe fn read_evidence_keys(manager: *mut sys::ResourceManager) -> Vec<EvidenceKey> {
    let count = sys::fiftyoneDegreesShimIpiGetUniqueHeaderCount(manager);
    let mut headers = Vec::with_capacity(count as usize);
    for index in 0..count {
        let mut buffer = [0u8; 256];
        let written = sys::fiftyoneDegreesShimIpiGetUniqueHeaderName(
            manager,
            index,
            buffer.as_mut_ptr() as *mut c_char,
            buffer.len(),
        );
        let name = String::from_utf8_lossy(&buffer[..written.min(buffer.len())]).to_lowercase();
        if !name.is_empty() {
            headers.push(name);
        }
    }

    let prefixes = [
        (constants::EVIDENCE_QUERY_PREFIX, sys::EvidencePrefix::Query),
        (
            constants::EVIDENCE_SERVER_PREFIX,
            sys::EvidencePrefix::Server,
        ),
    ];
    let mut keys = Vec::with_capacity(headers.len() * prefixes.len());
    for (prefix, native_prefix) in prefixes {
        for header in &headers {
            keys.push(EvidenceKey {
                key: format!("{prefix}{}{header}", constants::EVIDENCE_SEPARATOR),
                prefix: native_prefix,
                // The name came from a C string, so it holds no nul byte.
                field: CString::new(header.as_str()).expect("a header name has no nul"),
            });
        }
    }
    keys
}

// ---------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------

/// A per-thread IP Intelligence results structure.
///
/// Produced by [`Manager::create_results`]. Run a lookup with
/// [`Results::process_ip`] or [`Results::process_evidence`], then read values
/// by name with [`Results::value_as_string`]. Holds an [`Arc`] to its
/// [`Manager`].
pub struct Results {
    results: NonNull<sys::ResultsIpi>,
    manager: Arc<Manager>,
}

// Safety: a native results structure may be moved between threads, so `Results`
// is `Send`. It is per-thread scratch mutated in place, so it is not `Sync`.
unsafe impl Send for Results {}

impl Results {
    /// The manager whose data set these results read from.
    pub fn manager(&self) -> &Arc<Manager> {
        &self.manager
    }

    fn as_ptr(&self) -> *mut sys::ResultsIpi {
        self.results.as_ptr()
    }

    /// Run a lookup from an IP address string (IPv4 or IPv6).
    ///
    /// An IPv4-mapped IPv6 address, such as `::ffff:1.1.1.1`, gives the answers
    /// of its IPv4 address.
    pub fn process_ip(&mut self, ip_address: &str) -> Result<()> {
        // The native parser reads one byte past the supplied length, expecting a
        // string terminator (or another break character such as a space or comma)
        // to sit at that position. The native examples pass a null terminated C
        // string with strlen as the length, so the byte at the end is the nul.
        // A Rust string slice is not null terminated, so the byte just past it is
        // arbitrary and the parse fails with an incorrect-format error. Pass an
        // owned null terminated copy so the terminator is present, with the length
        // excluding the nul to match the strlen convention.
        let ip_c = CString::new(ip_address).map_err(|_| Error::Native {
            status: String::from("IncorrectIpAddressFormat"),
            message: String::from("IP address contains an interior nul byte"),
        })?;
        let mut exception = sys::Exception::cleared();
        let _file_read = self.manager.file_read();
        // Safety: `ip_c` is a null terminated buffer valid for the duration of the
        // call, and the length passed excludes the terminator the parser reads.
        unsafe {
            sys::fiftyoneDegreesResultsIpiFromIpAddressString(
                self.as_ptr(),
                ip_c.as_ptr(),
                ip_address.len(),
                &mut exception,
            );
        }
        exception.check()
    }

    /// Run a lookup from the client IP evidence the data file defines.
    ///
    /// Takes the values of [`Manager::evidence_keys`] from `evidence` and passes
    /// each one that reads as an IP address, in its canonical form, to the
    /// native evidence lookup, which uses the first in its order. A value that
    /// does not read as an IP address is left out, as the .NET engine does,
    /// because the native lookup would stop at it rather than try the next key.
    /// The accepted forms are those of [`crate::evidence::parse_client_ip`]. An
    /// IPv4-mapped IPv6 address gives the answers of its IPv4 address.
    ///
    /// Returns an [`Error::Native`] with status `InvalidInput` when the evidence
    /// holds no client IP address that can be read.
    pub fn process_evidence(&mut self, evidence: &Evidence) -> Result<()> {
        let mut pairs = Vec::new();
        for key in &self.manager.evidence_keys {
            let Some(address) = evidence.get(&key.key).and_then(parse_client_ip) else {
                continue;
            };
            // A formatted address never holds a nul byte.
            let value = CString::new(address.to_string()).expect("an IP address has no nul");
            pairs.push((key, value));
        }
        if pairs.is_empty() {
            return Err(Error::Native {
                status: String::from("InvalidInput"),
                message: String::from("evidence contains no client IP address to look up"),
            });
        }

        let _file_read = self.manager.file_read();
        // Safety: the array is created, filled and freed here. The keys belong
        // to the manager, and the values to `pairs`, so both outlive the call.
        unsafe {
            let array = sys::common::fiftyoneDegreesEvidenceCreate(pairs.len() as u32);
            if array.is_null() {
                return Err(Error::Native {
                    status: String::from("InsufficientMemory"),
                    message: String::from("failed to allocate IP Intelligence evidence"),
                });
            }
            for (key, value) in &pairs {
                sys::common::fiftyoneDegreesEvidenceAddString(
                    array,
                    key.prefix,
                    key.field.as_ptr(),
                    value.as_ptr(),
                );
            }
            let mut exception = sys::Exception::cleared();
            sys::fiftyoneDegreesResultsIpiFromEvidence(self.as_ptr(), array, &mut exception);
            sys::common::fiftyoneDegreesEvidenceFree(array);
            exception.check()
        }
    }

    /// Read the value(s) for the named property as a single string, joining
    /// multiple values with `separator`.
    ///
    /// Returns [`None`] when the property has no value in the results. The value
    /// is read into the reusable per-thread buffer.
    pub fn value_as_string(&self, property_name: &str, separator: &str) -> Result<Option<String>> {
        let property_c = CString::new(property_name).map_err(|_| Error::Native {
            status: String::from("InvalidInput"),
            message: String::from("property name contains an interior nul byte"),
        })?;
        let separator_c = CString::new(separator).map_err(|_| Error::Native {
            status: String::from("InvalidInput"),
            message: String::from("separator contains an interior nul byte"),
        })?;

        let results = self.as_ptr();
        let mut exception = sys::Exception::cleared();
        let _file_read = self.manager.file_read();
        // Safety: the writer only touches its buffer and returns the native
        // available-character count. The strings outlive every invocation.
        let value = unsafe {
            with_value_string(
                |buffer, length| {
                    sys::fiftyoneDegreesResultsIpiGetValuesString(
                        results,
                        property_c.as_ptr(),
                        buffer,
                        length,
                        separator_c.as_ptr(),
                        &mut exception,
                    )
                },
                |text| text.to_owned(),
            )
        };
        exception.check()?;
        Ok(value)
    }

    /// Read the value(s) of a required property by the index
    /// [`Manager::required_property_index`] returned for it, joining multiple
    /// values with `separator`.
    ///
    /// The same values as [`value_as_string`](Self::value_as_string), without a
    /// name lookup on either side of the native boundary and without
    /// allocating the property name and separator on each call, for callers
    /// that read the same properties on every lookup. Returns [`None`] when
    /// the results carry no value, and an error for an index outside this data
    /// set's required properties.
    pub fn value_as_string_by_index(
        &self,
        index: c_int,
        separator: &CStr,
    ) -> Result<Option<String>> {
        self.manager.check_property_index(index)?;
        let results = self.as_ptr();
        let mut exception = sys::Exception::cleared();
        let _file_read = self.manager.file_read();
        // Safety: the writer only touches its buffer and returns the native
        // available-character count. The separator outlives every invocation
        // and the index is in range for this data set.
        let value = unsafe {
            with_value_string(
                |buffer, length| {
                    sys::fiftyoneDegreesResultsIpiGetValuesStringByRequiredPropertyIndex(
                        results,
                        index,
                        buffer,
                        length,
                        separator.as_ptr(),
                        &mut exception,
                    )
                },
                |text| text.to_owned(),
            )
        };
        exception.check()?;
        Ok(value)
    }

    /// Read the weighted value(s) for the named property, each rendered as a
    /// string paired with its raw `u16` weighting.
    ///
    /// IP Intelligence properties may resolve to several candidate values, each
    /// carrying a confidence weighting. This reads the native weighted value
    /// collection for the property and returns one `(value, weighting)` pair per
    /// candidate, sorted by descending weighting so the most confident value is
    /// first. The weighting is the native raw weighting scaled into a `u16`,
    /// matching the [`fiftyone_pipeline_core::WeightedValue`] raw weighting width
    /// the Phase 3 on-premise engine surfaces as
    /// `AspectPropertyValue<Vec<WeightedValue<T>>>`.
    ///
    /// Returns an empty vector when the property is not one of the required
    /// properties or the results carry no value for it, mirroring how
    /// [`Results::value_as_string`] reports an absent value as [`None`].
    pub fn values_weighted(&self, property_name: &str) -> Result<Vec<(String, u16)>> {
        Ok(self
            .read_collection(property_name)?
            .into_iter()
            .map(|(_, value, weighting)| (value, weighting))
            .collect())
    }

    /// [`Self::values_weighted`] by the index
    /// [`Manager::required_property_index`] returned for the property, which
    /// skips the name lookup. Returns an error for an index outside this data
    /// set's required properties.
    pub fn values_weighted_by_index(&self, index: c_int) -> Result<Vec<(String, u16)>> {
        self.manager.check_property_index(index)?;
        Ok(self
            .read_collection_by_index(index)?
            .into_iter()
            .map(|(_, value, weighting)| (value, weighting))
            .collect())
    }

    /// Read the value collection for a property as typed tuples
    /// `(value_type, rendered_value, weighting)`, ordered high weighting first.
    ///
    /// Works for both weighted and plain properties: a plain property comes back
    /// as a single tuple whose value type is a non-weighted variant. Returns an
    /// empty vector when the property is not one of the required properties or
    /// the results carry no value for it.
    fn read_collection(
        &self,
        property_name: &str,
    ) -> Result<Vec<(sys::PropertyValueType, String, u16)>> {
        // An unknown property has no values, so report an empty list rather
        // than an error, matching the string getter's absent-value behavior.
        let Some(index) = self.manager.required_property_index(property_name) else {
            return Ok(Vec::new());
        };
        self.read_collection_by_index(index)
    }

    /// [`Self::read_collection`] by a required property index the caller has
    /// checked is in range.
    fn read_collection_by_index(
        &self,
        index: c_int,
    ) -> Result<Vec<(sys::PropertyValueType, String, u16)>> {
        let indexes = [index];
        let mut exception = sys::Exception::cleared();
        let _file_read = self.manager.file_read();
        // Safety: `results` is valid and the index array is readable for its
        // single element. With no scratch buffer passed, the library converts
        // values to text in one of its own and frees it before returning. The
        // returned collection owns its memory and is released below on every
        // path.
        let mut collection = unsafe {
            sys::fiftyoneDegreesResultsIpiGetValuesCollection(
                self.as_ptr(),
                indexes.as_ptr(),
                indexes.len() as u32,
                std::ptr::null_mut(),
                &mut exception,
            )
        };

        // Read the collection while it is alive, then release it regardless of
        // whether reading raised an exception, so no native memory leaks.
        let read = if exception.is_okay() {
            // Safety: the collection came from the getter above and is intact.
            unsafe { collect_weighted(&collection) }
        } else {
            Vec::new()
        };
        // Safety: the collection came from the matching getter and is released
        // exactly once here.
        unsafe { sys::fiftyoneDegreesWeightedValuesCollectionRelease(&mut collection) };

        exception.check()?;

        let mut values = read;
        // Highest weighting first. The native order is not guaranteed sorted, so
        // sort descending by weighting here.
        values.sort_by_key(|item| std::cmp::Reverse(item.2));
        Ok(values)
    }

    /// Read a property as a typed [`PropertyRead`], deciding plain vs weighted
    /// from the value type the data file declares for it.
    ///
    /// A weighted property (value type in the `Weighted*` range) yields
    /// [`PropertyRead::Weighted`] with every candidate. A plain property yields
    /// the most probable single value as [`PropertyRead::String`],
    /// [`PropertyRead::Float`] or [`PropertyRead::Integer`] according to its
    /// type. A property with no value yields [`PropertyRead::Absent`].
    pub fn read_property(&self, property_name: &str) -> Result<PropertyRead> {
        Ok(property_read_from(self.read_collection(property_name)?))
    }

    /// [`Self::read_property`] by the index
    /// [`Manager::required_property_index`] returned for the property, which
    /// skips the name lookup. Returns an error for an index outside this data
    /// set's required properties.
    pub fn read_property_by_index(&self, index: c_int) -> Result<PropertyRead> {
        self.manager.check_property_index(index)?;
        Ok(property_read_from(self.read_collection_by_index(index)?))
    }
}

/// Type a value collection as a [`PropertyRead`], deciding plain against
/// weighted from the value type the data file declares.
fn property_read_from(items: Vec<(sys::PropertyValueType, String, u16)>) -> PropertyRead {
    use sys::PropertyValueType as Ty;
    let Some(value_type) = items.first().map(|item| item.0) else {
        return PropertyRead::Absent;
    };
    if is_weighted_type(value_type) {
        let pairs = items
            .into_iter()
            .map(|(_, value, weighting)| (value, weighting))
            .collect();
        return PropertyRead::Weighted(pairs);
    }
    // Plain property: take the most probable (first) rendered value and
    // parse it to its scalar type.
    let Some((_, top, _)) = items.into_iter().next() else {
        return PropertyRead::Absent;
    };
    match value_type {
        Ty::Double | Ty::SinglePrecisionFloat | Ty::Declination | Ty::Azimuth => top
            .trim()
            .parse::<f32>()
            .map(PropertyRead::Float)
            .unwrap_or_else(|_| PropertyRead::String(top)),
        Ty::Integer | Ty::SingleByte => top
            .trim()
            .parse::<i32>()
            .map(PropertyRead::Integer)
            .unwrap_or_else(|_| PropertyRead::String(top)),
        Ty::Boolean => parse_bool(&top)
            .map(PropertyRead::Boolean)
            .unwrap_or(PropertyRead::String(top)),
        _ => PropertyRead::String(top),
    }
}

/// A property value read from a lookup, typed by the data file's declared value
/// type for the property.
///
/// The on-premise engine maps each variant onto the matching shared-model
/// builder, so weighted properties stay weighted and plain properties carry a
/// single typed value.
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyRead {
    /// The property had no value in the results.
    Absent,
    /// A plain string value (string, IP range, WKB geometry, boolean rendering
    /// and any other non-numeric scalar).
    String(String),
    /// A plain single-precision float value (latitude, longitude).
    Float(f32),
    /// A plain integer value (time-zone offset, accuracy radius, diversities).
    Integer(i32),
    /// A plain boolean value, when the data file declares the property's value
    /// type as boolean rather than storing its rendered string.
    Boolean(bool),
    /// A weighted list of `(value, raw_weighting)` candidates, high weighting
    /// first (the genuinely weighted properties).
    Weighted(Vec<(String, u16)>),
}

/// Parse a rendered boolean value into a `bool`.
///
/// Accepts the textual forms the data file and cloud use (`true`/`false`,
/// `1`/`0`, `yes`/`no`), matched case-insensitively. Returns [`None`] for any
/// other text so the caller can fall back to surfacing the raw string.
pub(crate) fn parse_bool(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Some(true),
        "false" | "0" | "no" => Some(false),
        _ => None,
    }
}

/// Render every weighted value in a collection to a `(string, u16)` pair.
///
/// Each item is a pointer to a [`sys::WeightedValueHeader`] that may be
/// reinterpreted as the concrete `Weighted*` structure its `value_type`
/// selects. The raw weighting is a native `u32`, scaled into a `u16` for the
/// pipeline weighted value width. Items whose value type is not one of the
/// supported weighted leaf types are skipped.
///
/// # Safety
/// `collection` must be an intact collection returned by
/// [`sys::fiftyoneDegreesResultsIpiGetValuesCollection`] and not yet released.
unsafe fn collect_weighted(
    collection: &sys::WeightedValuesCollection,
) -> Vec<(sys::PropertyValueType, String, u16)> {
    use sys::PropertyValueType as Ty;

    if collection.items.is_null() {
        return Vec::new();
    }
    let count = collection.items_count as isize;
    let mut out = Vec::with_capacity(count.max(0) as usize);
    for i in 0..count {
        // Safety: `items` points at `items_count` header pointers.
        let header = *collection.items.offset(i);
        if header.is_null() {
            continue;
        }
        let value_type = (*header).value_type;
        let weighting = scale_weighting((*header).raw_weighting);
        // Safety: the header is non-null and its `value_type` selects which
        // concrete structure the pointer may be cast to.
        let rendered = match value_type {
            Ty::WeightedString | Ty::String | Ty::JavaScript => {
                let ws = header as *const sys::WeightedString;
                let value = (*ws).value;
                if value.is_null() {
                    None
                } else {
                    Some(CStr::from_ptr(value).to_string_lossy().into_owned())
                }
            }
            Ty::WeightedInt | Ty::Integer => {
                let wi = header as *const sys::WeightedInt;
                Some((*wi).value.to_string())
            }
            Ty::WeightedDouble | Ty::Double => {
                let wd = header as *const sys::WeightedDouble;
                Some((*wd).value.to_string())
            }
            Ty::WeightedBool | Ty::Boolean => {
                let wb = header as *const sys::WeightedBool;
                Some((*wb).value.to_string())
            }
            Ty::WeightedByte | Ty::SingleByte => {
                let wb = header as *const sys::WeightedByte;
                Some((*wb).value.to_string())
            }
            // Other value types are not weighted leaf scalars in this surface
            // and are skipped rather than guessed at.
            _ => None,
        };
        if let Some(text) = rendered {
            out.push((value_type, text, weighting));
        }
    }
    out
}

/// Whether a property value type is one of the weighted list variants
/// (`WEIGHTED_STRING`..=`WEIGHTED_WKB_R`, discriminants 14..=21).
///
/// This is the single source of truth for "is this property weighted": it is
/// decided by the value type the data file declares for the property, not by any
/// hard-coded property-name list, so a new weighted property is handled the
/// moment the data declares it weighted.
fn is_weighted_type(value_type: sys::PropertyValueType) -> bool {
    use sys::PropertyValueType as Ty;
    matches!(
        value_type,
        Ty::WeightedString
            | Ty::WeightedInt
            | Ty::WeightedDouble
            | Ty::WeightedBool
            | Ty::WeightedSingle
            | Ty::WeightedByte
            | Ty::WeightedIpAddress
            | Ty::WeightedWkbReduced
    )
}

/// Scale a native raw weighting into the `u16` width the pipeline
/// [`fiftyone_pipeline_core::WeightedValue`] uses.
///
/// The native raw weighting is a `u32` out of a maximum of `65535 * 65535`, so
/// dividing it by 65535 gives the same proportion out of 65535, as the .NET and
/// Java bindings do.
fn scale_weighting(raw: u32) -> u16 {
    (raw / u32::from(u16::MAX)) as u16
}

impl Drop for Results {
    fn drop(&mut self) {
        // Safety: the results came from `ResultsIpiCreate` and have not been
        // freed. Freeing releases the data set reference. The manager is freed
        // later when the last `Arc` is dropped (results before manager).
        unsafe { sys::fiftyoneDegreesResultsIpiFree(self.results.as_ptr()) };
    }
}

/// The message for a failed init, naming the data file. An exception holds no
/// file name, so its message would say `null` instead.
fn status_detail(status: sys::StatusCode, path: &CStr) -> String {
    // Safety: the path is null terminated and outlives the call, and the library
    // allocated the message.
    unsafe {
        take_message::<sys::Exception>(
            sys::common::fiftyoneDegreesStatusGetMessage(status, path.as_ptr()),
            "native initialization failed with no message",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Resolve an IP Intelligence data file at run time, searching the
    /// environment override, the `cxx` submodule and the
    /// wider Workspace tree. Returns [`None`] when none is present so the smoke
    /// tests can skip cleanly.
    ///
    /// The ASN file checked into the `ip-intelligence-data` repository loads
    /// against this source revision and is the convenient default. Point
    /// `51DEGREES_IPI_PATH` at another data file to run the lookups against it.
    fn ipi_data_file() -> Option<PathBuf> {
        if let Ok(path) = std::env::var("51DEGREES_IPI_PATH") {
            let path = PathBuf::from(path);
            if path.is_file() {
                return Some(path);
            }
        }
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()?
            .to_path_buf();
        let name = "51Degrees-IPIV4AsnIpiV41.ipi";
        let candidates = [
            workspace
                .join("cxx")
                .join("ip-intelligence-data")
                .join(name),
            workspace
                .parent()
                .map(|p| p.join("ip-intelligence-data").join(name))
                .unwrap_or_default(),
        ];
        candidates.into_iter().find(|p| p.is_file())
    }

    /// Open a data file that was found, failing if it does not load, because
    /// a file the vendored sources cannot read is a fault, not a reason to skip.
    fn open_found(data_file: &Path, profile: PerformanceProfile) -> Arc<Manager> {
        Manager::open(data_file, profile).unwrap_or_else(|err| {
            panic!(
                "the IP Intelligence data file {} should load: {err}",
                data_file.display()
            )
        })
    }

    /// Open the data file through the safe `Manager`, run a public IP
    /// lookup and read a value back.
    #[test]
    fn lite_lookup_via_safe_api() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping safe-API lookup");
            return;
        };
        let manager = open_found(&data_file, PerformanceProfile::Default);

        let mut results = manager.create_results().expect("results should allocate");
        results
            .process_ip("185.28.167.77")
            .expect("looking up a public IP should not raise an exception");

        let candidates = [
            "RegisteredCountry",
            "RegisteredName",
            "RegisteredOwner",
            "IpRangeStart",
            "IpRangeEnd",
            "Mcc",
        ];
        let mut found = false;
        for name in candidates {
            if let Some(value) = results
                .value_as_string(name, "|")
                .expect("reading a property should not error")
            {
                eprintln!("{name} = {value}");
                found = true;
            }
        }
        assert!(found, "at least one common property should return a value");
    }

    /// Look up a public IP and read a weighted property through
    /// [`Results::values_weighted`], asserting at least one `(value, weighting)`
    /// pair is returned and that the pairs are sorted by descending weighting.
    /// The candidates cover the ASN file CI uses as well as the larger files.
    #[test]
    fn lite_weighted_lookup() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping weighted lookup");
            return;
        };
        let manager = open_found(&data_file, PerformanceProfile::Default);

        let mut results = manager.create_results().expect("results should allocate");
        results
            .process_ip("185.28.167.77")
            .expect("looking up a public IP should not raise an exception");

        // An unknown property has no weighted values and must not error.
        assert!(
            results
                .values_weighted("ThisPropertyDoesNotExist")
                .expect("an unknown property should not error")
                .is_empty(),
            "an unknown property should return no weighted values"
        );

        let candidates = [
            "Asn",
            "AsnName",
            "RegisteredCountry",
            "RegisteredName",
            "RegisteredOwner",
            "IpRangeStart",
            "IpRangeEnd",
            "Mcc",
        ];
        let mut total_pairs = 0usize;
        for name in candidates {
            let weighted = results
                .values_weighted(name)
                .expect("reading a weighted property should not error");
            if !weighted.is_empty() {
                // The pairs must be sorted by descending weighting.
                for window in weighted.windows(2) {
                    assert!(
                        window[0].1 >= window[1].1,
                        "weighted values for {name} should be sorted descending by weighting"
                    );
                }
                for (value, weighting) in &weighted {
                    eprintln!("{name} = {value} (weighting {weighting})");
                }
                total_pairs += weighted.len();
            }
        }
        assert!(
            total_pairs > 0,
            "at least one candidate property should return a weighted (value, weighting) pair"
        );
    }

    /// Reading by index gives the values reading by name gives, for every
    /// property the data file carries, as a string and as weighted values.
    #[test]
    fn reading_by_index_gives_the_values_reading_by_name_gives() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping reads by index");
            return;
        };
        let manager = open_found(&data_file, PerformanceProfile::Default);
        let mut results = manager.create_results().expect("results should allocate");
        results
            .process_ip("185.28.167.77")
            .expect("looking up a public IP should not raise an exception");
        let names = manager.available_property_names();
        assert!(!names.is_empty());
        for name in &names {
            let index = manager
                .required_property_index(name)
                .unwrap_or_else(|| panic!("{name} has an index"));
            assert_eq!(
                results.value_as_string_by_index(index, c"|").unwrap(),
                results.value_as_string(name, "|").unwrap(),
                "{name}"
            );
            assert_eq!(
                results.values_weighted_by_index(index).unwrap(),
                results.values_weighted(name).unwrap(),
                "{name}"
            );
        }
        assert!(manager.required_property_index("NoSuchProperty").is_none());
    }

    /// A data set that reads from its file has a file handle for each
    /// concurrent operation it was opened to expect, and one held in memory
    /// has none, so its calls take no permits.
    #[test]
    fn file_handles_follow_the_expected_concurrency() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping file handles");
            return;
        };
        for (profile, concurrency, handles) in [
            (PerformanceProfile::LowMemory, Some(3), 3),
            (PerformanceProfile::Balanced, None, 10),
            (PerformanceProfile::HighPerformance, Some(3), 0),
            (PerformanceProfile::InMemory, None, 0),
        ] {
            let manager = Manager::open_with_options(&data_file, profile, None, concurrency)
                .expect("data file loads");
            // Safety: the manager is initialized.
            let count = unsafe { sys::fiftyoneDegreesShimIpiGetFileHandleCount(manager.as_ptr()) };
            assert_eq!(count, handles, "{}", profile.as_str());
            assert_eq!(
                manager.file_reads.is_some(),
                handles > 0,
                "{}",
                profile.as_str()
            );
        }
    }

    /// An index outside the data set's required properties is refused.
    #[test]
    fn an_index_outside_the_data_set_is_an_error() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping the index range check");
            return;
        };
        let manager = open_found(&data_file, PerformanceProfile::Default);
        let mut results = manager.create_results().expect("results should allocate");
        results
            .process_ip("185.28.167.77")
            .expect("looking up a public IP should not raise an exception");
        let count = manager.available_property_names().len() as c_int;
        assert!(results.value_as_string_by_index(count, c"|").is_err());
        assert!(results.value_as_string_by_index(-1, c"|").is_err());
        assert!(results.values_weighted_by_index(count).is_err());
    }

    #[test]
    fn a_native_weighting_keeps_its_proportion() {
        const NATIVE_MAX: u32 = 65535 * 65535;
        assert_eq!(scale_weighting(NATIVE_MAX), u16::MAX);
        assert_eq!(scale_weighting(65535 * 32768), 32768);
        assert_eq!(scale_weighting(65535), 1);
        assert_eq!(scale_weighting(0), 0);
    }

    /// The weightings of a property's values are proportions of one, so
    /// together they come to no more than 65535.
    #[test]
    fn a_propertys_weightings_add_up_to_no_more_than_one() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping the weighting total");
            return;
        };
        let manager = open_found(&data_file, PerformanceProfile::Default);
        let mut results = manager.create_results().expect("results should allocate");
        let mut checked = 0;
        for ip in [
            "185.28.167.77",
            "81.2.69.142",
            "102.89.32.1",
            "2a00:23c5:a1a6:a01::1",
        ] {
            results.process_ip(ip).expect("the lookup should run");
            for name in manager.available_property_names() {
                let weighted = results
                    .values_weighted(&name)
                    .expect("weighted values should read");
                if weighted.is_empty() {
                    continue;
                }
                let total: u32 = weighted.iter().map(|(_, w)| u32::from(*w)).sum();
                assert!(
                    total <= u32::from(u16::MAX),
                    "{name} for {ip} has weightings totalling {total}: {weighted:?}"
                );
                checked += 1;
            }
        }
        assert!(checked > 0, "some property should have weighted values");
    }

    /// Drive the same lookup from pipeline evidence carrying the client IP, then
    /// confirm evidence without an IP is a clean error.
    #[test]
    fn lite_lookup_from_evidence() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping evidence lookup");
            return;
        };
        let manager = open_found(&data_file, PerformanceProfile::HighPerformance);
        let mut results = manager.create_results().expect("results should allocate");

        let with_ip = Evidence::builder()
            .add("server.client-ip", "185.28.167.77")
            .build();
        results
            .process_evidence(&with_ip)
            .expect("processing evidence with a client IP should not error");

        let without_ip = Evidence::builder().add("header.user-agent", "x").build();
        assert!(
            results.process_evidence(&without_ip).is_err(),
            "evidence with no client IP should be an error"
        );
    }

    /// Every header the data file names is a key under both prefixes, with
    /// every query key before any server key, each group in the same order.
    #[test]
    fn the_evidence_keys_are_the_data_files_headers_query_first() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping evidence keys");
            return;
        };
        let manager = open_found(&data_file, PerformanceProfile::Default);
        let keys: Vec<&str> = manager.evidence_keys().collect();
        let (query, server) = keys.split_at(keys.len() / 2);
        let query: Vec<&str> = query
            .iter()
            .filter_map(|k| k.strip_prefix("query."))
            .collect();
        let server: Vec<&str> = server
            .iter()
            .filter_map(|k| k.strip_prefix("server."))
            .collect();
        assert!(
            !query.is_empty(),
            "the data file should define headers: {keys:?}"
        );
        assert_eq!(
            query.len() * 2,
            keys.len(),
            "query keys come first: {keys:?}"
        );
        assert_eq!(
            query, server,
            "both prefixes take the same headers: {keys:?}"
        );
        for key in ["query.client-ip-51d", "server.client-ip"] {
            assert!(keys.contains(&key), "{key} should be accepted: {keys:?}");
        }
    }

    /// Run a lookup from `evidence` and read back the autonomous system.
    fn asn_from(manager: &Arc<Manager>, evidence: &[(&str, &str)]) -> Result<Option<String>> {
        let mut builder = Evidence::builder();
        for (key, value) in evidence {
            builder = builder.add(*key, *value);
        }
        let mut results = manager.create_results()?;
        results.process_evidence(&builder.build())?;
        results.value_as_string("Asn", ",")
    }

    /// A Cloudflare address, which the data files give as AS13335.
    const CLOUDFLARE: &str = "1.1.1.1";
    /// A Quad9 address, which the data files give as AS19281.
    const QUAD9: &str = "9.9.9.9";

    #[test]
    fn a_query_value_is_used_before_a_server_value() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping evidence order");
            return;
        };
        let manager = open_found(&data_file, PerformanceProfile::HighPerformance);
        let asn = asn_from(
            &manager,
            &[("server.client-ip", CLOUDFLARE), ("query.client-ip", QUAD9)],
        )
        .expect("the lookup should succeed");
        assert!(
            asn.as_deref().unwrap_or_default().contains("AS19281"),
            "got {asn:?}"
        );
    }

    #[test]
    fn a_value_that_is_not_an_address_gives_way_to_the_next_key() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping evidence fall through");
            return;
        };
        let manager = open_found(&data_file, PerformanceProfile::HighPerformance);
        // The native lookup stops at a value it cannot read, so without the
        // values being read first this fails with an incorrect format error.
        let asn = asn_from(
            &manager,
            &[
                ("query.client-ip-51d", "unknown"),
                ("server.client-ip", CLOUDFLARE),
            ],
        )
        .expect("the lookup should succeed");
        assert!(
            asn.as_deref().unwrap_or_default().contains("AS13335"),
            "got {asn:?}"
        );
    }

    #[test]
    fn a_51d_key_and_an_address_with_a_port_are_read() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping 51d keys");
            return;
        };
        let manager = open_found(&data_file, PerformanceProfile::HighPerformance);
        for value in [
            "1.1.1.1:443",
            "[2606:4700:4700::1111]:443",
            "1.1.1.1, 10.0.0.1",
        ] {
            let asn = asn_from(&manager, &[("query.client-ip-51d", value)])
                .unwrap_or_else(|err| panic!("{value} should be looked up: {err}"));
            assert!(
                asn.as_deref().unwrap_or_default().contains("AS13335"),
                "{value} gave {asn:?}"
            );
        }
    }

    #[test]
    fn evidence_with_no_readable_address_is_an_error() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping unreadable evidence");
            return;
        };
        let manager = open_found(&data_file, PerformanceProfile::HighPerformance);
        let err = asn_from(&manager, &[("server.client-ip", "unknown")])
            .expect_err("evidence with no readable address should be an error");
        assert!(matches!(err, Error::Native { ref status, .. } if status == "InvalidInput"));
    }
}
