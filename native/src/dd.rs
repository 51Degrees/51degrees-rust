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

//! Safe RAII wrappers over the native Hash device detection engine.
//!
//! [`Manager`] owns a native resource manager loaded from a Hash data file and
//! frees it on drop. [`Results`] owns a native results structure, holds an
//! [`Arc`] to the manager so the data set cannot be freed while results exist,
//! and frees the results on drop. Property name to required-property-index
//! resolution is computed once per manager and cached, and values are read into
//! a reusable per-thread buffer.
//!
//! # Thread safety
//!
//! The native resource manager is internally reference counted and lock
//! protected (see `src/common/resource.h`), so a [`Manager`] is both [`Send`]
//! and [`Sync`] and a single one may be shared across threads behind an
//! [`Arc`]. A data set that reads from its file after it is loaded holds one
//! file handle for each concurrent operation it was opened to expect, and a
//! read that finds none free fails. So no more calls that can read the file
//! run at once than there are handles, and a call beyond them waits for one.
//! A native results structure is per-thread scratch, so [`Results`] is
//! [`Send`] (it may be moved to another thread) but not [`Sync`] (it must not be
//! used from two threads at once). This matches the pipeline rule that one flow
//! data, and therefore one results structure, belongs to a single thread at a
//! time.

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::path::Path;
use std::ptr::NonNull;
use std::sync::{Arc, OnceLock};

use fiftyone_device_detection_sys as sys;
use fiftyone_pipeline_core::{constants, Error, Result};

use crate::buffer::with_value_string;
use crate::evidence::with_native_evidence;
use crate::file_reads::{FileReadPermit, FileReads};
use crate::profile::PerformanceProfile;
use crate::status::{status_name, status_to_result, take_message, NativeException, NativeStatus};

/// The character a data set puts between the header names it joins into one
/// pseudo header, for example the User-Agent Client Hints it reads together.
const PSEUDO_HEADER_SEPARATOR: char = '\x1F';

// ---------------------------------------------------------------------------
// Status and exception trait wiring for the Device Detection common C library
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
        sys::fiftyoneDegreesExceptionGetMessage(self)
    }

    unsafe fn free_message(ptr: *mut c_void) {
        sys::fiftyoneDegreesFree(ptr);
    }
}

/// The stable name of a Device Detection status code.
fn status_name(status: sys::StatusCode) -> &'static str {
    status_name!(sys::StatusCode, status)
}

/// Resolve a [`PerformanceProfile`] to a pointer to the matching predefined Hash
/// configuration global.
///
/// The returned pointer references a mutable static owned by the native library.
/// The init functions read through it without modifying it for the lifetime of
/// the call, so handing them the shared global is sound.
fn config_for(profile: PerformanceProfile) -> *mut sys::ConfigHash {
    // Taking the address of a `static mut` exported by the C library is safe.
    // The address is stable for the program lifetime, and the pointer is only
    // read through (not dereferenced for writing) by the init functions.
    match profile {
        PerformanceProfile::InMemory => {
            std::ptr::addr_of_mut!(sys::fiftyoneDegreesHashInMemoryConfig)
        }
        PerformanceProfile::HighPerformance => {
            std::ptr::addr_of_mut!(sys::fiftyoneDegreesHashHighPerformanceConfig)
        }
        PerformanceProfile::LowMemory => {
            std::ptr::addr_of_mut!(sys::fiftyoneDegreesHashLowMemoryConfig)
        }
        PerformanceProfile::Balanced => {
            std::ptr::addr_of_mut!(sys::fiftyoneDegreesHashBalancedConfig)
        }
        PerformanceProfile::Default => {
            std::ptr::addr_of_mut!(sys::fiftyoneDegreesHashDefaultConfig)
        }
    }
}

/// Set the expected concurrency on every collection in a Hash configuration.
///
/// For the `LowMemory` and `Balanced` profiles the collections are read from
/// file through a fixed-size handle pool, so the pool must be sized for the
/// number of threads that will use the data set concurrently. This mirrors the
/// `setConcurrency` helper in the other 51Degrees ports, which applies the value
/// to every collection's config. A value of zero is treated by the native
/// library as a single concurrent operation.
fn set_pool_concurrency(config: &mut sys::ConfigHash, concurrency: u16) {
    for collection in [
        &mut config.strings,
        &mut config.components,
        &mut config.maps,
        &mut config.properties,
        &mut config.values,
        &mut config.profiles,
        &mut config.root_nodes,
        &mut config.nodes,
        &mut config.profile_offsets,
    ] {
        collection.concurrency = concurrency;
    }
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

/// A loaded Hash device detection data set.
///
/// Owns a native [`sys::ResourceManager`] initialized from a data file. Cloning
/// the [`Arc`] returned by [`Manager::open`] shares one loaded data set across
/// threads cheaply. The data set is freed when the last reference is dropped,
/// after every [`Results`] referencing it has also been dropped (the [`Arc`]
/// they hold guarantees that ordering).
pub struct Manager {
    /// The native manager, heap allocated so its address is stable for the
    /// native handle bookkeeping. Boxed and leaked into a raw pointer, freed in
    /// [`Drop`].
    manager: NonNull<sys::ResourceManager>,
    /// Property name to required-property-index map, built on the first
    /// lookup. Every later lookup reads it with no lock, so concurrent readers
    /// never contend.
    property_index: OnceLock<HashMap<String, c_int>>,
    /// Permits for the calls that can read the data file, one for each of its
    /// file handles, or [`None`] when the data set is held in memory.
    file_reads: Option<FileReads>,
}

// Safety: the native resource manager is internally reference counted and
// lock protected per `src/common/resource.h`, so concurrent access from several
// threads through a shared `&Manager` is sound. The cached property index is
// set once through a `OnceLock` and only read after that, and the file read
// permits are lock protected. Therefore the manager is both `Send` and `Sync`.
unsafe impl Send for Manager {}
unsafe impl Sync for Manager {}

impl Manager {
    /// Open a Hash data file and load it with the given performance profile.
    ///
    /// All available properties are requested. To restrict the property set,
    /// use [`Manager::open_with_properties`].
    pub fn open(path: impl AsRef<Path>, profile: PerformanceProfile) -> Result<Arc<Manager>> {
        Self::open_with_properties(path, profile, None)
    }

    /// Open a Hash data file requesting only the named properties, or all
    /// properties when `properties` is [`None`].
    ///
    /// Limiting the property set reduces the memory footprint of the loaded data
    /// set, matching the `properties` option in the other 51Degrees ports.
    ///
    /// A component with no named property reads as `0` in the device id. Naming
    /// `DeviceId` resolves every component, so the device id is complete.
    pub fn open_with_properties(
        path: impl AsRef<Path>,
        profile: PerformanceProfile,
        properties: Option<&[&str]>,
    ) -> Result<Arc<Manager>> {
        Self::open_with_options(path, profile, properties, None)
    }

    /// Open a Hash data file with the named properties and an optional expected
    /// concurrency.
    ///
    /// `concurrency` sizes the file-handle pool the file-backed collections use
    /// under the `LowMemory` and `Balanced` profiles, which is how many calls
    /// can read the data file at once. A call beyond that waits for a handle,
    /// so it should be the number of threads that will process through the
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

        // A separated property list, kept alive for the duration of the init
        // call because `PropertiesRequired` only borrows the string pointer.
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

        // Heap allocate a zeroed manager so its address is stable.
        let boxed = Box::new(sys::ResourceManager::zeroed());
        let manager_ptr = Box::into_raw(boxed);

        // Copy the predefined configuration for this profile into a local value
        // so an expected-concurrency override sizes this engine's handle pool
        // without mutating the shared global the other engines read.
        // Safety: `config_for` returns a valid pointer to an initialized config
        // global; copying out of it is a plain read of a `repr(C)` value.
        let mut config = unsafe { *config_for(profile) };
        if let Some(concurrency) = concurrency {
            set_pool_concurrency(&mut config, concurrency);
        }

        let mut exception = sys::Exception::cleared();
        // Safety: all pointers are valid for the duration of the call. The
        // config is read only for the call and the path is null terminated.
        let status = unsafe {
            sys::fiftyoneDegreesHashInitManagerFromFile(
                manager_ptr,
                &mut config,
                &mut required,
                path_c.as_ptr(),
                &mut exception,
            )
        };

        if let Err(err) = status_to_result(status, || status_detail(status, &path_c)) {
            // The manager was not populated, so free just the box.
            // Safety: `manager_ptr` came from `Box::into_raw` above.
            unsafe { drop(Box::from_raw(manager_ptr)) };
            return Err(err);
        }
        // A clear status but a set exception is still a failure.
        if let Err(err) = exception.check() {
            unsafe {
                sys::fiftyoneDegreesResourceManagerFree(manager_ptr);
                drop(Box::from_raw(manager_ptr));
            }
            return Err(err);
        }

        // Safety: a successful init leaves a non-null, initialized manager.
        let manager = NonNull::new(manager_ptr).expect("init produced a non-null manager");
        // Safety: the manager was initialized above.
        let handles = unsafe { sys::fiftyoneDegreesShimHashGetFileHandleCount(manager.as_ptr()) };
        Ok(Arc::new(Manager {
            manager,
            property_index: OnceLock::new(),
            file_reads: FileReads::for_handles(handles),
        }))
    }

    /// The raw native manager pointer. For the engine wrappers in this crate.
    fn as_ptr(&self) -> *mut sys::ResourceManager {
        self.manager.as_ptr()
    }

    /// A permit for a call that can read the data file, waiting while every
    /// file handle is in use. [`None`] when the data set is held in memory.
    fn file_read(&self) -> Option<FileReadPermit<'_>> {
        self.file_reads.as_ref().map(FileReads::permit)
    }

    /// The number of required (available) properties in the loaded data set.
    pub fn property_count(&self) -> u32 {
        // Safety: the manager is initialized for the lifetime of `self`.
        unsafe { sys::fiftyoneDegreesShimHashGetRequiredPropertyCount(self.as_ptr()) }
    }

    /// The names of the required (available) properties, in index order.
    pub fn property_names(&self) -> Vec<String> {
        let count = self.property_count();
        let mut names = Vec::with_capacity(count as usize);
        let mut buffer = vec![0u8; 256];
        for index in 0..count as c_int {
            // Safety: the buffer is writable for its full length.
            let written = unsafe {
                sys::fiftyoneDegreesShimHashGetRequiredPropertyName(
                    self.as_ptr(),
                    index,
                    buffer.as_mut_ptr() as *mut c_char,
                    buffer.len(),
                )
            };
            if written > 0 {
                let end = (written as usize).min(buffer.len());
                names.push(String::from_utf8_lossy(&buffer[..end]).into_owned());
            }
        }
        names
    }

    /// The header evidence keys the data set reads, lowercased, being each
    /// header it names as a `header.` key and as a `query.` key, in the data
    /// set's order.
    ///
    /// These are the keys the C++ engine's `getKeys` lists for the headers. A
    /// pseudo header, which the data set builds by joining several headers, is
    /// left out, because a request never carries one.
    pub fn evidence_keys(&self) -> Vec<String> {
        // Safety: the manager is initialized for the lifetime of `self`.
        let count = unsafe { sys::fiftyoneDegreesShimHashGetEvidenceKeyCount(self.as_ptr()) };
        let mut keys = Vec::with_capacity(count as usize * 2);
        let mut buffer = vec![0u8; 256];
        for index in 0..count {
            // Safety: the buffer is writable for its full length.
            let written = unsafe {
                sys::fiftyoneDegreesShimHashGetEvidenceKey(
                    self.as_ptr(),
                    index,
                    buffer.as_mut_ptr() as *mut c_char,
                    buffer.len(),
                )
            };
            let end = written.min(buffer.len());
            let name = String::from_utf8_lossy(&buffer[..end]).to_lowercase();
            if name.is_empty() || name.contains(PSEUDO_HEADER_SEPARATOR) {
                continue;
            }
            for prefix in [
                constants::EVIDENCE_HTTP_HEADER_PREFIX,
                constants::EVIDENCE_QUERY_PREFIX,
            ] {
                keys.push(format!("{prefix}{}{name}", constants::EVIDENCE_SEPARATOR));
            }
        }
        keys
    }

    /// The data set's name, which is the data file's tier (for example `Lite`,
    /// `Enterprise` or `TAC`), read from the data file header.
    ///
    /// Returns [`None`] when the native data set does not expose a name, so a
    /// caller can fall back to its own default rather than assert a tier.
    pub fn data_set_name(&self) -> Option<String> {
        let mut buffer = vec![0u8; 128];
        let _file_read = self.file_read();
        // Safety: the buffer is writable for its full length and the manager is
        // initialized for the lifetime of `self`.
        let written = unsafe {
            sys::fiftyoneDegreesShimHashGetDataSetName(
                self.as_ptr(),
                buffer.as_mut_ptr() as *mut c_char,
                buffer.len(),
            )
        };
        if written == 0 {
            return None;
        }
        let end = (written as usize).min(buffer.len());
        let name = String::from_utf8_lossy(&buffer[..end]).into_owned();
        if name.trim().is_empty() {
            None
        } else {
            Some(name)
        }
    }

    /// Resolve a property name to its required-property-index, computing and
    /// caching the full name to index map on first use.
    ///
    /// Returns [`None`] when the property is not one of the required properties.
    /// The lookup is case sensitive, matching the native data set, so callers
    /// should pass the canonical property spelling.
    pub fn required_property_index(&self, property_name: &str) -> Option<c_int> {
        self.property_index_map().get(property_name).copied()
    }

    /// The property name to required-property-index map, built on first use.
    /// The first caller builds it and racing callers wait for it. Once it is
    /// set, a lookup takes no lock and writes no shared memory.
    fn property_index_map(&self) -> &HashMap<String, c_int> {
        self.property_index
            .get_or_init(|| self.build_property_index())
    }

    /// Build the property name to index map by enumerating the data set.
    fn build_property_index(&self) -> HashMap<String, c_int> {
        self.property_names()
            .into_iter()
            .enumerate()
            .map(|(index, name)| (name, index as c_int))
            .collect()
    }

    /// Create a per-thread results structure for running detections.
    pub fn create_results(self: &Arc<Self>) -> Result<Results> {
        // Overrides are on, so the values the client-side JavaScript sends back
        // (for example the real screen size) replace the values matched from
        // the headers, as they do in the other 51Degrees SDKs. With a
        // capacity of zero the engine ignores every override. The capacity is
        // the number of available properties, which is at least the number
        // evidence can override. The User-Agent capacity before it is ignored
        // by the C library.
        let overrides = self.property_count().max(1);
        // Safety: the manager is initialized.
        let results = unsafe { sys::fiftyoneDegreesResultsHashCreate(self.as_ptr(), 0, overrides) };
        let results = NonNull::new(results).ok_or_else(|| Error::Native {
            status: String::from("InsufficientMemory"),
            message: String::from("failed to allocate Hash results"),
        })?;
        Ok(Results {
            results,
            manager: Arc::clone(self),
        })
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        // Safety: the manager was initialized by a successful `open`, and every
        // `Results` referencing it held an `Arc` to this `Manager`, so all of
        // them have been dropped (and their data set references released) before
        // this runs. Freeing the manager then frees the data set, and the box is
        // reclaimed afterwards.
        unsafe {
            sys::fiftyoneDegreesResourceManagerFree(self.manager.as_ptr());
            drop(Box::from_raw(self.manager.as_ptr()));
        }
    }
}

// ---------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------

/// A per-thread Hash detection results structure.
///
/// Produced by [`Manager::create_results`]. Run a detection with
/// [`Results::process_evidence`], [`Results::process_user_agent`] or
/// [`Results::process_device_id`], then read values with
/// [`Results::value_as_string`]. Holds an [`Arc`] to its [`Manager`] so the data
/// set stays loaded while the results live.
pub struct Results {
    results: NonNull<sys::ResultsHash>,
    manager: Arc<Manager>,
}

// Safety: a native results structure may be moved between threads, so `Results`
// is `Send`. It is per-thread scratch that the native engine mutates in place
// during processing and value reads, so it must not be shared across threads
// concurrently and is deliberately not `Sync`.
unsafe impl Send for Results {}

impl Results {
    /// The manager whose data set these results read from.
    pub fn manager(&self) -> &Arc<Manager> {
        &self.manager
    }

    fn as_ptr(&self) -> *mut sys::ResultsHash {
        self.results.as_ptr()
    }

    /// Run a detection from pipeline evidence.
    ///
    /// The evidence is marshalled into the native evidence array (reusing the
    /// thread-local pool) and processed. Header and query values are read as
    /// user agent style evidence and server values as the client IP.
    ///
    /// A header longer than 500 bytes is ignored with no error, and detection
    /// moves on to the next header.
    pub fn process_evidence(&mut self, evidence: &fiftyone_pipeline_core::Evidence) -> Result<()> {
        let mut exception = sys::Exception::cleared();
        let results = self.as_ptr();
        let _file_read = self.manager.file_read();
        // Safety: the array the closure receives is valid only for the call,
        // which is exactly the window the native processor reads it in.
        unsafe {
            with_native_evidence(evidence, |array| {
                sys::fiftyoneDegreesResultsHashFromEvidence(results, array, &mut exception);
            });
        }
        exception.check()
    }

    /// Run a detection from a single User-Agent string.
    ///
    /// A User-Agent longer than 500 bytes is ignored with no error, which gives
    /// the same results as no evidence.
    pub fn process_user_agent(&mut self, user_agent: &str) -> Result<()> {
        let mut exception = sys::Exception::cleared();
        let _file_read = self.manager.file_read();
        // Safety: the user agent bytes are valid for the duration of the call.
        unsafe {
            sys::fiftyoneDegreesResultsHashFromUserAgent(
                self.as_ptr(),
                user_agent.as_ptr() as *const c_char,
                user_agent.len(),
                &mut exception,
            );
        }
        exception.check()
    }

    /// Run a detection from a 51Degrees device id. Returns the number of valid
    /// profiles parsed from the id.
    pub fn process_device_id(&mut self, device_id: &str) -> Result<i32> {
        // The native parser reads each profile id up to the first byte that
        // is not a digit, whatever the length it is given, so the id is
        // passed as a null terminated copy. A Rust string slice is not null
        // terminated, and digits just past it would join the last profile id.
        let device_id_c = CString::new(device_id).map_err(|_| Error::Native {
            status: String::from("InvalidInput"),
            message: String::from("device id contains an interior nul byte"),
        })?;
        let mut exception = sys::Exception::cleared();
        let _file_read = self.manager.file_read();
        // Safety: `device_id_c` is null terminated and valid for the call.
        let profiles = unsafe {
            sys::fiftyoneDegreesResultsHashFromDeviceId(
                self.as_ptr(),
                device_id_c.as_ptr(),
                device_id.len(),
                &mut exception,
            )
        };
        exception.check()?;
        Ok(profiles)
    }

    /// Read the device id of the results, the per-component profile ids joined
    /// with `-` (for example `15364-21385-53251-18092`).
    ///
    /// The device id is a Hash match metric rather than a data-file property, so
    /// it is not reachable through [`Results::value_as_string`] (the by-name
    /// reader only resolves required data-file properties). It is read through
    /// the dedicated native getter instead. Returns [`None`] when no detection
    /// has produced a device id.
    pub fn device_id(&self) -> Result<Option<String>> {
        // A device id is four profile ids and three separators, comfortably
        // within a small fixed buffer, so a single read with a generous buffer
        // avoids a growable allocation on this metric path.
        let mut buffer = [0u8; 256];
        let mut exception = sys::Exception::cleared();
        let _file_read = self.manager.file_read();
        // Safety: the results are valid, the buffer is writable for its full
        // length, and the exception is valid. The getter null terminates within
        // the buffer and returns the buffer pointer, or null when it did not fit.
        let written = unsafe {
            sys::fiftyoneDegreesHashGetDeviceIdFromResults(
                self.as_ptr(),
                buffer.as_mut_ptr() as *mut c_char,
                buffer.len(),
                &mut exception,
            )
        };
        exception.check()?;
        if written.is_null() {
            return Ok(None);
        }
        // The getter wrote a null terminated string into the buffer. Trim at the
        // first nul and decode lossily, matching the value string reader.
        let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
        if end == 0 {
            return Ok(None);
        }
        Ok(Some(String::from_utf8_lossy(&buffer[..end]).into_owned()))
    }

    /// True when the results carry a value for the named property.
    ///
    /// Returns `false` when the property is not one of the required properties.
    pub fn has_values(&self, property_name: &str) -> Result<bool> {
        let Some(index) = self.manager.required_property_index(property_name) else {
            return Ok(false);
        };
        let mut exception = sys::Exception::cleared();
        let _file_read = self.manager.file_read();
        // Safety: the results are valid and the index is a real required index.
        let has = unsafe {
            sys::fiftyoneDegreesResultsHashGetHasValues(self.as_ptr(), index, &mut exception)
        };
        exception.check()?;
        Ok(has)
    }

    /// Read the value(s) for the named property as a single string, joining
    /// multiple values with `separator`.
    ///
    /// The value is read into the reusable per-thread buffer, which grows only
    /// when a value does not fit. Returns [`None`] when the property is not a
    /// required property or has no value in the results.
    pub fn value_as_string(&self, property_name: &str, separator: &str) -> Result<Option<String>> {
        // Resolve once through the cached index. An unknown property has no
        // value, matching the engine's own behavior.
        if self
            .manager
            .required_property_index(property_name)
            .is_none()
        {
            return Ok(None);
        }

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
        // Safety: the writer only touches the buffer it is given and returns the
        // native available-character count. The property and separator strings
        // outlive every invocation.
        let value = unsafe {
            with_value_string(
                |buffer, length| {
                    sys::fiftyoneDegreesResultsHashGetValuesString(
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
    /// The same values as [`value_as_string`](Self::value_as_string), without
    /// a name lookup on either side of the native boundary and without
    /// allocating the property name and separator on each call, for callers
    /// that read the same properties on every detection. An index belongs to
    /// the data set it came from, so look it up again after a reload. Returns
    /// [`None`] when the results carry no value, and an error for an index
    /// outside this data set's required properties.
    pub fn value_as_string_by_index(
        &self,
        index: c_int,
        separator: &CStr,
    ) -> Result<Option<String>> {
        let count = self.manager.property_index_map().len();
        if index < 0 || index as usize >= count {
            return Err(Error::Native {
                status: String::from("InvalidInput"),
                message: format!("{index} is not a required property index (0..{count})"),
            });
        }
        let results = self.as_ptr();
        let mut exception = sys::Exception::cleared();
        let _file_read = self.manager.file_read();
        // Safety: the writer only touches the buffer it is given and returns the
        // native available-character count. The separator outlives every
        // invocation and the index is in range for this data set.
        let value = unsafe {
            with_value_string(
                |buffer, length| {
                    sys::fiftyoneDegreesResultsHashGetValuesStringByRequiredPropertyIndex(
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

    /// The match metrics of the detection: the match method, difference,
    /// drift, iterations and matched-node count.
    ///
    /// The native results hold one result for each component, such as the
    /// hardware or the browser, each from the header that matched it. As in
    /// the C++ results, the method is the highest of any result and the other
    /// metrics are summed across them. With no results every metric is zero,
    /// which is the method `NONE`.
    ///
    /// These are computed during detection and live on the native results, not
    /// in the data file's property values, so they are read directly here
    /// rather than through [`value_as_string`](Self::value_as_string) (which
    /// only resolves real data-file properties). Returns [`None`] only when the
    /// native results cannot be read.
    pub fn match_metrics(&self) -> Option<MatchMetrics> {
        let mut method = 0;
        let mut difference = 0;
        let mut drift = 0;
        let mut iterations = 0;
        let mut matched_nodes = 0;
        // Safety: the results came from `ResultsHashCreate` and remain valid for
        // the lifetime of `self`; every output pointer is writable.
        let ok = unsafe {
            sys::fiftyoneDegreesShimHashGetResultMetrics(
                self.as_ptr(),
                &mut method,
                &mut difference,
                &mut drift,
                &mut iterations,
                &mut matched_nodes,
            )
        };
        if ok == 0 {
            return None;
        }
        Some(MatchMetrics {
            method,
            difference,
            drift,
            iterations,
            matched_nodes,
        })
    }

    /// The matched User-Agent of each result, without repeats, in result
    /// order. Each is the evidence value the component was matched from, with
    /// the characters the match did not use replaced by underscores. Empty
    /// when nothing matched.
    pub fn matched_user_agents(&self) -> Vec<String> {
        // Safety: the results came from `ResultsHashCreate` and remain valid
        // for the lifetime of `self`.
        let count = unsafe { sys::fiftyoneDegreesShimHashGetResultCount(self.as_ptr()) };
        let mut user_agents: Vec<String> = Vec::new();
        let mut buffer = vec![0u8; MATCHED_USER_AGENT_BUFFER_LENGTH];
        for index in 0..count {
            // Safety: as above, and the buffer is writable for its full length.
            let written = unsafe {
                sys::fiftyoneDegreesShimHashGetMatchedUserAgent(
                    self.as_ptr(),
                    index,
                    buffer.as_mut_ptr() as *mut c_char,
                    buffer.len(),
                )
            };
            if written == 0 {
                continue;
            }
            let user_agent = String::from_utf8_lossy(&buffer[..written.min(buffer.len())]);
            if !user_agents.iter().any(|known| *known == user_agent) {
                user_agents.push(user_agent.into_owned());
            }
        }
        user_agents
    }
}

/// The buffer a matched User-Agent is read into. The native default records at
/// most 500 characters of each.
const MATCHED_USER_AGENT_BUFFER_LENGTH: usize = 4096;

/// The Hash match metrics for a single detection, read from the native results.
///
/// They describe how the match was found rather than the device itself: the
/// algorithm used and how far the evidence was from a clean match. Each is
/// taken across the results, one for each component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchMetrics {
    /// The highest match method enum value of any result (0 None,
    /// 1 Performance, 2 Combined, 3 Predictive). Use
    /// [`method_name`](Self::method_name) for the label.
    pub method: i32,
    /// The total difference in hash values between the evidence and the match.
    /// The larger the value, the less confident the match.
    pub difference: i32,
    /// The total drift, in character positions, of the matched substrings from
    /// where they were expected.
    pub drift: i32,
    /// The number of graph nodes visited to find the match.
    pub iterations: i32,
    /// The number of hash nodes matched within the evidence.
    pub matched_nodes: i32,
}

impl MatchMetrics {
    /// The match method as the specification names it: `NONE`,
    /// `PERFORMANCE`, `COMBINED` or `PREDICTIVE`.
    pub fn method_name(&self) -> &'static str {
        match self.method {
            1 => "PERFORMANCE",
            2 => "COMBINED",
            3 => "PREDICTIVE",
            _ => "NONE",
        }
    }
}

impl Drop for Results {
    fn drop(&mut self) {
        // Safety: the results came from `ResultsHashCreate` and have not been
        // freed. Freeing them releases the data set reference. The manager is
        // freed later when the last `Arc` is dropped, satisfying the documented
        // free order (results before manager).
        unsafe { sys::fiftyoneDegreesResultsHashFree(self.results.as_ptr()) };
    }
}

/// The message for a failed init, naming the data file. An exception holds no
/// file name, so its message would say `null` instead.
fn status_detail(status: sys::StatusCode, path: &CStr) -> String {
    // Safety: the path is null terminated and outlives the call, and the library
    // allocated the message.
    unsafe {
        take_message::<sys::Exception>(
            sys::fiftyoneDegreesStatusGetMessage(status, path.as_ptr()),
            "native initialization failed with no message",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use fiftyone_pipeline_core::Evidence;

    /// The Hash data file `51DEGREES_DD_PATH` names, or the Lite file in the
    /// `cxx` submodule.
    fn data_file() -> Option<PathBuf> {
        if let Ok(path) = std::env::var("51DEGREES_DD_PATH") {
            let path = PathBuf::from(path);
            if path.is_file() {
                return Some(path);
            }
        }
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()?
            .to_path_buf();
        let roots = [workspace.clone(), workspace.parent()?.to_path_buf()];
        roots
            .into_iter()
            .map(|root| {
                root.join("cxx")
                    .join("device-detection-data")
                    .join("51Degrees-LiteV4.1.hash")
            })
            .find(|path| path.is_file())
    }

    const CHROME_ON_WINDOWS: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
        AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

    fn detect(manager: &Arc<Manager>, key: &str, value: &str) -> Results {
        let mut results = manager.create_results().expect("results allocate");
        results
            .process_evidence(&Evidence::builder().add(key, value).build())
            .expect("detection succeeds");
        results
    }

    /// The metrics of each result, read one at a time.
    fn metrics_of_each_result(results: &Results) -> Vec<MatchMetrics> {
        // Safety: the results are valid for the duration of the borrow.
        let count = unsafe { sys::fiftyoneDegreesShimHashGetResultCount(results.as_ptr()) };
        (0..count)
            .map(|index| {
                let mut metrics = MatchMetrics {
                    method: -1,
                    difference: -1,
                    drift: -1,
                    iterations: -1,
                    matched_nodes: -1,
                };
                // Safety: as above, and every output pointer is writable.
                let read = unsafe {
                    sys::fiftyoneDegreesShimHashGetResultMetricsAt(
                        results.as_ptr(),
                        index,
                        &mut metrics.method,
                        &mut metrics.difference,
                        &mut metrics.drift,
                        &mut metrics.iterations,
                        &mut metrics.matched_nodes,
                    )
                };
                assert_eq!(read, 1, "result {index} should be readable");
                metrics
            })
            .collect()
    }

    #[test]
    fn match_metrics_are_taken_across_every_component() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping match metrics");
            return;
        };
        let manager = Manager::open(&path, PerformanceProfile::Default).expect("data file loads");
        let results = detect(&manager, "header.user-agent", CHROME_ON_WINDOWS);
        let each = metrics_of_each_result(&results);
        assert!(each.len() > 1, "a result for each component: {each:?}");

        let metrics = results.match_metrics().expect("metrics are readable");
        assert_eq!(metrics.method, each.iter().map(|m| m.method).max().unwrap());
        assert_eq!(
            metrics.difference,
            each.iter().map(|m| m.difference).sum::<i32>()
        );
        assert_eq!(metrics.drift, each.iter().map(|m| m.drift).sum::<i32>());
        assert_eq!(
            metrics.iterations,
            each.iter().map(|m| m.iterations).sum::<i32>()
        );
        assert_eq!(
            metrics.matched_nodes,
            each.iter().map(|m| m.matched_nodes).sum::<i32>()
        );
        assert!(
            metrics.iterations > each[0].iterations,
            "{metrics:?} {each:?}"
        );
    }

    #[test]
    fn no_match_gives_zero_metrics_and_the_method_none() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping metrics with no match");
            return;
        };
        let manager = Manager::open(&path, PerformanceProfile::Default).expect("data file loads");
        let results = detect(&manager, "header.x-not-read", "nothing to match");
        let metrics = results.match_metrics().expect("metrics are readable");
        assert_eq!(
            metrics,
            MatchMetrics {
                method: 0,
                difference: 0,
                drift: 0,
                iterations: 0,
                matched_nodes: 0,
            }
        );
        assert_eq!(metrics.method_name(), "NONE");
        assert!(results.matched_user_agents().is_empty());
    }

    #[test]
    fn methods_are_named_as_the_specification_names_them() {
        for (method, name) in [
            (0, "NONE"),
            (1, "PERFORMANCE"),
            (2, "COMBINED"),
            (3, "PREDICTIVE"),
        ] {
            let metrics = MatchMetrics {
                method,
                difference: 0,
                drift: 0,
                iterations: 0,
                matched_nodes: 0,
            };
            assert_eq!(metrics.method_name(), name);
        }
    }

    /// Each matched User-Agent is the evidence with the characters its match
    /// did not use replaced by underscores.
    #[test]
    fn each_matched_user_agent_keeps_the_characters_its_match_used() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping matched User-Agents");
            return;
        };
        let manager = Manager::open(&path, PerformanceProfile::Default).expect("data file loads");
        let results = detect(&manager, "header.user-agent", CHROME_ON_WINDOWS);
        let matched = results.matched_user_agents();
        assert!(!matched.is_empty());
        for user_agent in &matched {
            assert_eq!(user_agent.len(), CHROME_ON_WINDOWS.len(), "{user_agent}");
            for (kept, original) in user_agent.chars().zip(CHROME_ON_WINDOWS.chars()) {
                assert!(kept == '_' || kept == original, "{user_agent}");
            }
        }
    }

    #[test]
    fn reading_by_index_gives_the_values_reading_by_name_gives() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping reads by index");
            return;
        };
        let manager = Manager::open(&path, PerformanceProfile::Default).expect("data file loads");
        let mobile = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
            AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";
        let names = manager.property_names();
        assert!(!names.is_empty());
        for user_agent in [CHROME_ON_WINDOWS, mobile] {
            let results = detect(&manager, "header.user-agent", user_agent);
            for name in &names {
                let index = manager
                    .required_property_index(name)
                    .expect("a listed property has an index");
                assert_eq!(
                    results
                        .value_as_string_by_index(index, c"|")
                        .expect("read by index"),
                    results.value_as_string(name, "|").expect("read by name"),
                    "{name}"
                );
            }
        }
    }

    /// A device id read from a slice ends where the slice ends, however the
    /// bytes after it in memory read.
    #[test]
    fn a_device_id_ends_where_its_slice_ends() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping device id slices");
            return;
        };
        let manager = Manager::open(&path, PerformanceProfile::Default).expect("data file loads");
        let detected = detect(&manager, "header.user-agent", CHROME_ON_WINDOWS);
        let device_id = detected
            .device_id()
            .expect("the device id reads")
            .expect("a detection has a device id");
        let profiles = device_id.split('-').filter(|id| *id != "0").count() as i32;

        // Digits straight after the slice, as a larger buffer holding the id
        // can have.
        let held = format!("{device_id}5-1");
        let mut results = manager.create_results().expect("results allocate");
        let found = results
            .process_device_id(&held[..device_id.len()])
            .expect("the device id is processed");
        assert_eq!(found, profiles, "{device_id}");
        assert_eq!(
            results.device_id().expect("the device id reads"),
            Some(device_id)
        );
    }

    /// A data set that reads from its file has a file handle for each
    /// concurrent operation it was opened to expect, and one held in memory
    /// has none, so its calls take no permits.
    #[test]
    fn file_handles_follow_the_expected_concurrency() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping file handles");
            return;
        };
        for (profile, concurrency, handles) in [
            (PerformanceProfile::LowMemory, Some(3), 3),
            (PerformanceProfile::Balanced, None, 10),
            (PerformanceProfile::HighPerformance, Some(3), 0),
            (PerformanceProfile::InMemory, None, 0),
        ] {
            let manager = Manager::open_with_options(&path, profile, None, concurrency)
                .expect("data file loads");
            // Safety: the manager is initialized.
            let count = unsafe { sys::fiftyoneDegreesShimHashGetFileHandleCount(manager.as_ptr()) };
            assert_eq!(count, handles, "{}", profile.as_str());
            assert_eq!(
                manager.file_reads.is_some(),
                handles > 0,
                "{}",
                profile.as_str()
            );
        }
    }

    #[test]
    fn an_index_outside_the_data_set_is_an_error() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping index range");
            return;
        };
        let manager = Manager::open(&path, PerformanceProfile::Default).expect("data file loads");
        let results = detect(&manager, "header.user-agent", CHROME_ON_WINDOWS);
        let count = manager.property_count() as c_int;
        for index in [-1, count] {
            let err = results
                .value_as_string_by_index(index, c"|")
                .expect_err("an index outside the data set should be refused");
            assert!(matches!(err, Error::Native { ref status, .. } if status == "InvalidInput"));
        }
    }
}
