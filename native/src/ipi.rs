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
//! was opened to expect, and the native library makes a read that finds none
//! free wait for one. The native library ends the process when a read of a
//! property's values fails.

use std::ffi::{CStr, CString};
use std::net::{IpAddr, Ipv4Addr};
use std::ops::Range;
use std::os::raw::{c_char, c_int, c_void};
use std::path::Path;
use std::ptr::NonNull;
use std::sync::Arc;

use fiftyone_ip_intelligence_sys as sys;
use fiftyone_pipeline_core::{constants, round_angle, Error, Evidence, PropertyValueType, Result};

use crate::buffer::{with_c_str, with_value_string};
use crate::evidence::parse_client_ip;
use crate::held_memory::HeldMemory;
use crate::metadata::{metadata_read_failed, read_metadata_string, take_value_string};
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

/// The size of an IP Intelligence data file's header in bytes, being how much
/// of a data file the C library compares when it looks for a copy of the file
/// to reuse.
pub fn data_file_header_size() -> usize {
    // Safety: the shim reads no state and takes no arguments.
    unsafe { sys::fiftyoneDegreesShimIpiGetHeaderSize() }
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
    /// The data set the manager was opened with, held until [`Drop`], so a
    /// name lookup reads it without taking a reference and threads sharing
    /// the manager write no shared memory to look a name up.
    data_set: NonNull<sys::DataSetIpi>,
    /// The number of required properties, which a read by index must stay
    /// below.
    required_count: u32,
    /// The data a load from memory reads in place. Fields drop after
    /// [`Drop::drop`] has freed the data set, so the data outlives it.
    _memory: Option<HeldMemory>,
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
// from several threads. The data set held for name lookups is only read.
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
    /// data set at once. [`None`] keeps the profile's default, one for each
    /// processor. Any value loads, and the pool holds at most 64 handles. It
    /// has no effect on `InMemory`, and none on `HighPerformance` but for the
    /// values of properties not named when some are, which it reads from the
    /// file.
    pub fn open_with_options(
        path: impl AsRef<Path>,
        profile: PerformanceProfile,
        properties: Option<&[&str]>,
        concurrency: Option<u16>,
    ) -> Result<Arc<Manager>> {
        let path = path.as_ref();
        let path_c = crate::path::native_path(path)?;
        Self::init(
            properties,
            None,
            |manager, required, exception| {
                // Safety: all pointers are valid for the call and the path is
                // null terminated. The shim copies the configuration before
                // setting the concurrency, so the shared global is not
                // changed.
                unsafe {
                    sys::fiftyoneDegreesShimIpiInitManagerFromFile(
                        manager,
                        config_for(profile),
                        concurrency.map_or(-1, i32::from),
                        required,
                        path_c.as_ptr(),
                        exception,
                    )
                }
            },
            |status| status_detail(status, Some(&path_c)),
        )
    }

    /// Load an IP Intelligence data set from `data`, the whole of a data file
    /// held in memory, requesting only the named properties, or all
    /// properties when `properties` is [`None`].
    ///
    /// The data set reads its collections from `data` where it lies, so the
    /// manager keeps `data` until the data set is freed. Every profile holds
    /// the data set in memory this way, so none opens a file pool, and the
    /// profile's collection and concurrency settings do not apply.
    pub fn open_from_memory(
        data: Vec<u8>,
        profile: PerformanceProfile,
        properties: Option<&[&str]>,
    ) -> Result<Arc<Manager>> {
        let memory = HeldMemory::new(data);
        let size = sys::FileOffset::try_from(memory.length()).map_err(|_| Error::Native {
            status: String::from("InvalidInput"),
            message: format!(
                "the data is {} bytes, more than the C library reads from memory",
                memory.length()
            ),
        })?;
        let start = memory.start();
        Self::init(
            properties,
            Some(memory),
            |manager, required, exception| {
                // Safety: every pointer is valid for the call, the shim copies
                // the configuration so that the memory is neither freed nor
                // copied to a file, and `start` is the first of `size` bytes
                // that the manager keeps for as long as the data set.
                unsafe {
                    sys::fiftyoneDegreesShimIpiInitManagerFromMemory(
                        manager,
                        config_for(profile),
                        required,
                        start,
                        size,
                        exception,
                    )
                }
            },
            // The C++ engine names no file for a load from memory.
            |status| status_detail(status, None),
        )
    }

    /// Initialize a manager with `init`, requesting the named properties, or
    /// all of them when `properties` is [`None`]. `memory` is the data a load
    /// from memory reads, kept until the data set is freed, and `detail`
    /// gives the message for a failed status.
    fn init(
        properties: Option<&[&str]>,
        memory: Option<HeldMemory>,
        init: impl FnOnce(
            *mut sys::ResourceManager,
            *mut sys::PropertiesRequired,
            *mut sys::Exception,
        ) -> sys::StatusCode,
        detail: impl FnOnce(sys::StatusCode) -> String,
    ) -> Result<Arc<Manager>> {
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
        let status = init(manager_ptr, &mut required, &mut exception);

        if let Err(err) = status_to_result(status, || detail(status)) {
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
        // Safety: as above. The reference is released in `Drop`, before the
        // manager is freed.
        let data_set = unsafe { sys::fiftyoneDegreesDataSetIpiGet(manager.as_ptr()) };
        let data_set = NonNull::new(data_set).expect("an initialized manager has a data set");
        // Safety: as above.
        let required_count =
            unsafe { sys::fiftyoneDegreesShimIpiGetRequiredPropertyCount(manager.as_ptr()) };
        Ok(Arc::new(Manager {
            manager,
            evidence_keys,
            data_set,
            required_count,
            _memory: memory,
        }))
    }

    fn as_ptr(&self) -> *mut sys::ResourceManager {
        self.manager.as_ptr()
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

    /// The number of handles to the data file that the data set opened when
    /// it loaded, which stay open until it is freed. Every profile but
    /// `InMemory` opens one for each concurrent operation it expects, to at
    /// most 64. `InMemory` opens none.
    pub fn file_pool_size(&self) -> u16 {
        // Safety: the manager is initialized for the lifetime of `self`.
        unsafe { sys::fiftyoneDegreesShimIpiGetFilePoolSize(self.as_ptr()) }
    }

    /// The data set's name, which is the data file's tier (for example `Lite`,
    /// `Enterprise` or `TAC`), or [`None`] when it cannot be read.
    ///
    /// Read from the data set header through this crate's shim, mirroring the
    /// Hash engine. The on-premise engine uses this to report the real tier
    /// rather than a fixed default.
    pub fn data_set_name(&self) -> Option<String> {
        let mut buffer = vec![0u8; 128];
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

    /// The date the data file was published, from its header, as `(year,
    /// month, day)`, or [`None`] when the header holds no valid date.
    pub fn data_set_published(&self) -> Option<(i32, u32, u32)> {
        self.header_date(sys::fiftyoneDegreesIpiGetPublished)
    }

    /// The date the next data file is expected, from the data file header,
    /// as `(year, month, day)`, or [`None`] when the header holds no valid
    /// date.
    pub fn data_set_next_update(&self) -> Option<(i32, u32, u32)> {
        self.header_date(sys::fiftyoneDegreesIpiGetNextUpdate)
    }

    /// The date `read` gives, or [`None`] when the C library finds it is not
    /// a valid date.
    fn header_date(
        &self,
        read: unsafe extern "C" fn(*mut sys::ResourceManager, *mut sys::Date) -> bool,
    ) -> Option<(i32, u32, u32)> {
        let mut date = sys::Date::default();
        // Safety: the manager is initialized and `date` is writable.
        let valid = unsafe { read(self.as_ptr(), &mut date) };
        let sys::Date { year, month, day } = date;
        valid.then(|| (i32::from(year), u32::from(month), u32::from(day)))
    }

    /// The names of every property the data set was initialized with, in
    /// required-property index order. With properties named, these are the
    /// names given, in the order given, including any the data file has no
    /// property for.
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

    /// The required property index of the property named, as the C library
    /// finds it among the names the data set was opened with, ignoring case.
    ///
    /// Returns [`None`] when the property is not one of the required
    /// properties, or the name holds a nul byte. A lookup writes no shared
    /// memory, so threads sharing the manager do not contend. An index belongs
    /// to the data set it came from, so look it up again after a reload.
    pub fn required_property_index(&self, property_name: &str) -> Option<c_int> {
        // Safety: the data set is held until `Drop`, and the name is null
        // terminated for the length of the call.
        let index = with_c_str(property_name, |name| unsafe {
            sys::fiftyoneDegreesShimIpiDataSetGetRequiredPropertyIndexFromName(
                self.data_set.as_ptr(),
                name,
            )
        })?;
        (index >= 0).then_some(index)
    }

    /// Refuse an index that is not one of this data set's required property
    /// indexes.
    fn check_property_index(&self, index: c_int) -> Result<()> {
        let count = self.required_count as usize;
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

pub use crate::metadata::DataFileValue;

// The shared readers in `crate::metadata` take the shim's constants as these.
const _: () = assert!(
    sys::SHIM_STRING_FAILED == crate::metadata::STRING_FAILED
        && sys::SHIM_VALUE_NAME == crate::metadata::VALUE_NAME
        && sys::SHIM_VALUE_DESCRIPTION == crate::metadata::VALUE_DESCRIPTION
        && sys::SHIM_VALUE_URL == crate::metadata::VALUE_URL
);

/// The metadata the data file holds for one of its properties, as the C++
/// `PropertyMetaDataBuilderIpi` in 51degrees-cxx reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DataFileProperty {
    /// The property's place among the data file's properties, which
    /// [`Manager::data_file_property_values`] takes.
    pub index: u32,
    /// The property's name.
    pub name: String,
    /// The type of the property's values, from the type the data file names.
    /// Every weighted type is a [`PropertyValueType::KeyValueList`], an area
    /// is a [`PropertyValueType::WktString`], and an IP address or a
    /// coordinate is a [`PropertyValueType::String`], because the pipeline
    /// has no type of its own for them.
    pub value_type: PropertyValueType,
    /// True when the data set was opened with the property, so results can
    /// hold its values.
    pub available: bool,
    /// The index [`Results::read_property_by_index`] takes for the property,
    /// or [`None`] when results do not hold it. The C library finds the
    /// property among the names the data set was opened with ignoring case,
    /// so this is found for a property named in another case.
    pub required_property_index: Option<c_int>,
    /// The property's category, or empty when it has none.
    pub category: String,
    /// The property's description, or empty when it has none.
    pub description: String,
    /// A URL with more about the property, or empty when it has none.
    pub url: String,
    /// The id of the component the property belongs to, or 0 when the data
    /// file names none.
    pub component_id: u8,
    /// The order to display the property in among the others.
    pub display_order: u8,
    /// True when every profile must have a value for the property.
    pub mandatory: bool,
    /// True when a profile can have several values for the property.
    pub list: bool,
    /// True when the property is kept only for backward compatibility.
    pub obsolete: bool,
    /// True when the property should be shown in lists of properties.
    pub show: bool,
    /// True when the property's values should be shown in lists of values.
    pub show_values: bool,
    /// The data tiers whose data files hold the property, in the data file's
    /// order, an empty name included, as the C++ code gives them.
    pub data_tiers: Vec<String>,
    /// The property's default value, or [`None`] when it has none. The C++
    /// code writes the value the property names as its default as text, a
    /// number to 15 decimal places, and looks that text up among the
    /// property's values, so a default that is not among them is none.
    pub default_value: Option<DataFileValue>,
    /// The number of values the data file holds for the property. It is 0
    /// when the property's value indexes are -1 or reach past the data
    /// file's values.
    pub value_count: u32,
}

/// The metadata the data file holds for one of its components, such as the
/// location or the network.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DataFileComponent {
    /// The component's id, which stays the same from one data file to the
    /// next.
    pub component_id: u8,
    /// The component's name.
    pub name: String,
    /// The offset of the component's default profile in the data file, or
    /// [`None`] when the component has none. An IP Intelligence data file
    /// holds no profile ids, so the C++ code, and the .NET package on it,
    /// give this offset as the default profile's id.
    pub default_profile_offset: Option<u32>,
}

/// The fields of a data file property that are not strings.
struct PropertyFields {
    value_type: u8,
    available: u8,
    required_index: c_int,
    component_id: u8,
    display_order: u8,
    is_mandatory: u8,
    is_list: u8,
    is_obsolete: u8,
    show: u8,
    show_values: u8,
    value_count: u32,
    default_value_number: i32,
    map_count: u32,
}

impl Manager {
    /// Every property the data file defines, in the data file's order. A
    /// property the data set was opened without is listed as unavailable.
    ///
    /// A property that cannot be read is an error rather than left out, as
    /// the C++ code the other SDKs call raises one.
    pub fn data_file_properties(&self) -> Result<Vec<DataFileProperty>> {
        let count = self.data_file_property_count();
        (0..count)
            .map(|index| {
                self.read_data_file_property(index)
                    .ok_or_else(|| metadata_read_failed(&format!("property {index}")))
            })
            .collect()
    }

    /// The components the data file defines, in the data file's order. A
    /// default profile or a name that cannot be read is an error, as the C++
    /// code throws for a default profile it cannot read.
    pub fn data_file_components(&self) -> Result<Vec<DataFileComponent>> {
        // Safety: the manager is initialized for the lifetime of `self`.
        let count = unsafe { sys::fiftyoneDegreesShimIpiGetComponentCount(self.as_ptr()) };
        let mut components = Vec::with_capacity(count as usize);
        for index in 0..count {
            let mut component_id = 0u8;
            let mut default_profile_offset = 0u32;
            let mut has_default_profile = 0u8;
            // Safety: every output pointer is a writable local.
            let read = unsafe {
                sys::fiftyoneDegreesShimIpiGetComponent(
                    self.as_ptr(),
                    index,
                    &mut component_id,
                    &mut default_profile_offset,
                    &mut has_default_profile,
                )
            };
            if read == 0 {
                return Err(metadata_read_failed(&format!(
                    "the default profile of component {index}"
                )));
            }
            // Safety: the buffer is writable for the length given.
            let name = read_metadata_string(|buffer, length| unsafe {
                sys::fiftyoneDegreesShimIpiGetComponentName(self.as_ptr(), index, buffer, length)
            })
            .ok_or_else(|| metadata_read_failed(&format!("component {index}")))?;
            components.push(DataFileComponent {
                component_id,
                name,
                default_profile_offset: (has_default_profile != 0)
                    .then_some(default_profile_offset),
            });
        }
        Ok(components)
    }

    /// The values the data file holds for the property at `property_index`
    /// among [`Self::data_file_properties`], in the data file's order, or none
    /// when the index is out of range. A property or value that cannot be
    /// read is an error.
    ///
    /// Each value's name is its text, written as the C++ code the other SDKs
    /// call writes it, so a coordinate has three decimal places and an IP
    /// address is written in full. The values are read from the data set
    /// each time, because a data file can hold millions of them, and
    /// [`Self::data_file_property_value_range`] reads part of the list.
    pub fn data_file_property_values(&self, property_index: u32) -> Result<Vec<DataFileValue>> {
        self.data_file_property_value_range(property_index, 0..u32::MAX)
    }

    /// The values at the places `range` covers among those
    /// [`Self::data_file_property_values`] gives for the property at
    /// `property_index`, so a caller that shows the first 20 reads only 20.
    /// The part of `range` past the last value is left out, so a range that
    /// starts there gives none.
    pub fn data_file_property_value_range(
        &self,
        property_index: u32,
        range: Range<u32>,
    ) -> Result<Vec<DataFileValue>> {
        if property_index >= self.data_file_property_count() {
            return Ok(Vec::new());
        }
        self.read_values(property_index, range)
            .ok_or_else(|| metadata_read_failed(&format!("values of property {property_index}")))
    }

    /// The value `name` names among those [`Self::data_file_property_values`]
    /// gives for the property at `property_index`, or [`None`] when the index
    /// is out of range or the property has no such value. A property or
    /// value that cannot be read is an error.
    ///
    /// A property's values are in order, so the value is found by the C
    /// library's binary search, as the C++ code the other SDKs call finds it,
    /// without reading the others. The search compares a number written to as
    /// many places as the text of `name` before any null has characters plus
    /// three, at most 15, so the value given can be written otherwise, as
    /// `0.063` is given for `0.0625`, and a number's own text, written to
    /// three places, can find nothing, as it finds nothing in the C++ code.
    ///
    /// As in the C++ code, `name` is searched up to its first null, so
    /// `AS13335\0x` finds `AS13335` and the text after the null is not
    /// searched.
    pub fn data_file_property_value(
        &self,
        property_index: u32,
        name: &str,
    ) -> Result<Option<DataFileValue>> {
        if property_index >= self.data_file_property_count() {
            return Ok(None);
        }
        // The C++ code searches with the text before the first null, where a
        // C string ends.
        let searched = name.split('\0').next().unwrap_or(name);
        let c_name = CString::new(searched).expect("the text before a null holds none");
        let mut values: Vec<DataFileValue> = Vec::new();
        // Safety: `c_name` is null terminated, and `take_value_string` treats
        // the context as the `Vec<DataFileValue>` it points to, which outlives
        // the call and is not used elsewhere during it.
        let found = unsafe {
            sys::fiftyoneDegreesShimIpiFindDataFilePropertyValue(
                self.as_ptr(),
                property_index,
                c_name.as_ptr(),
                take_value_string,
                (&mut values as *mut Vec<DataFileValue>).cast::<c_void>(),
            )
        };
        match (found, values.pop()) {
            (sys::SHIM_FIND_FOUND, Some(value)) if values.is_empty() => Ok(Some(value)),
            (sys::SHIM_FIND_NOT_FOUND, None) => Ok(None),
            _ => Err(metadata_read_failed(&format!(
                "value {name:?} of property {property_index}"
            ))),
        }
    }

    /// The number of properties the data file defines.
    fn data_file_property_count(&self) -> u32 {
        // Safety: the manager is initialized for the lifetime of `self`.
        unsafe { sys::fiftyoneDegreesShimIpiGetDataFilePropertyCount(self.as_ptr()) }
    }

    /// The values at the places `range` covers among those the data file
    /// holds for the property at `property_index`, read in one shim call, or
    /// [`None`] when the property or any of them cannot be read.
    fn read_values(&self, property_index: u32, range: Range<u32>) -> Option<Vec<DataFileValue>> {
        let mut values: Vec<DataFileValue> = Vec::new();
        // Safety: `take_value_string` treats the context as the
        // `Vec<DataFileValue>` it points to, which outlives the call and is
        // not used elsewhere during it.
        let read = unsafe {
            sys::fiftyoneDegreesShimIpiGetDataFilePropertyValues(
                self.as_ptr(),
                property_index,
                range.start,
                range.end.saturating_sub(range.start),
                take_value_string,
                (&mut values as *mut Vec<DataFileValue>).cast::<c_void>(),
            )
        };
        (read != 0).then_some(values)
    }

    /// The fields of the data file property at `index` that are not strings,
    /// or [`None`] when the index is out of range or the property, its name
    /// or its default value cannot be read.
    fn read_property_fields(&self, index: u32) -> Option<PropertyFields> {
        let mut fields = PropertyFields {
            value_type: 0,
            available: 0,
            required_index: -1,
            component_id: 0,
            display_order: 0,
            is_mandatory: 0,
            is_list: 0,
            is_obsolete: 0,
            show: 0,
            show_values: 0,
            value_count: 0,
            default_value_number: -1,
            map_count: 0,
        };
        // Safety: every output pointer is a writable field of `fields`.
        let read = unsafe {
            sys::fiftyoneDegreesShimIpiGetDataFileProperty(
                self.as_ptr(),
                index,
                &mut fields.value_type,
                &mut fields.available,
                &mut fields.required_index,
                &mut fields.component_id,
                &mut fields.display_order,
                &mut fields.is_mandatory,
                &mut fields.is_list,
                &mut fields.is_obsolete,
                &mut fields.show,
                &mut fields.show_values,
                &mut fields.value_count,
                &mut fields.default_value_number,
                &mut fields.map_count,
            )
        };
        (read != 0).then_some(fields)
    }

    /// The data file property at `index`, or [`None`] when the index is out
    /// of range or any part of it cannot be read.
    fn read_data_file_property(&self, index: u32) -> Option<DataFileProperty> {
        let fields = self.read_property_fields(index)?;
        let string = |field| {
            // Safety: the buffer is writable for the length given.
            read_metadata_string(|buffer, length| unsafe {
                sys::fiftyoneDegreesShimIpiGetDataFilePropertyString(
                    self.as_ptr(),
                    index,
                    field,
                    buffer,
                    length,
                )
            })
        };
        let data_tiers = (0..fields.map_count)
            .map(|map| {
                // Safety: the buffer is writable for the length given.
                read_metadata_string(|buffer, length| unsafe {
                    sys::fiftyoneDegreesShimIpiGetDataFilePropertyMap(
                        self.as_ptr(),
                        index,
                        map,
                        buffer,
                        length,
                    )
                })
            })
            .collect::<Option<Vec<String>>>()?;
        let default_value = match u32::try_from(fields.default_value_number) {
            Ok(number) => Some(self.read_values(index, number..number + 1)?.pop()?),
            Err(_) => None,
        };
        Some(DataFileProperty {
            index,
            name: string(sys::SHIM_PROPERTY_NAME)?,
            value_type: data_file_value_type(fields.value_type, fields.is_list != 0),
            available: fields.available != 0,
            required_property_index: (fields.required_index >= 0).then_some(fields.required_index),
            category: string(sys::SHIM_PROPERTY_CATEGORY)?,
            description: string(sys::SHIM_PROPERTY_DESCRIPTION)?,
            url: string(sys::SHIM_PROPERTY_URL)?,
            component_id: fields.component_id,
            display_order: fields.display_order,
            mandatory: fields.is_mandatory != 0,
            list: fields.is_list != 0,
            obsolete: fields.is_obsolete != 0,
            show: fields.show != 0,
            show_values: fields.show_values != 0,
            data_tiers,
            default_value,
            value_count: fields.value_count,
        })
    }
}

/// The value type of a data file property from the C library's
/// `fiftyoneDegreesPropertyValueType`, following the type names the C++
/// `PropertyMetaDataBuilderIpi` gives. Every weighted type is a
/// [`PropertyValueType::KeyValueList`], as a lookup gives each weighted
/// property's candidates. That includes `weightedbyte`, which .NET's
/// `PropertyMetaDataIpi.Type` has no case for and so gives as a string. An
/// area, `wkb` or `wkbr`, is a [`PropertyValueType::WktString`], as .NET
/// types it. An IP address or a coordinate is a [`PropertyValueType::String`],
/// as is a type the builder does not name, because the pipeline has no type
/// of its own for them.
fn data_file_value_type(value_type: u8, is_list: bool) -> PropertyValueType {
    use sys::PropertyValueType as Ty;
    let is = |ty: Ty| i32::from(value_type) == ty as i32;
    if is(Ty::Integer) {
        PropertyValueType::Integer
    } else if is(Ty::SinglePrecisionFloat) {
        PropertyValueType::Float
    } else if is(Ty::Double) {
        PropertyValueType::Double
    } else if is(Ty::Boolean) {
        PropertyValueType::Bool
    } else if is(Ty::JavaScript) {
        PropertyValueType::JavaScript
    } else if is(Ty::String) && is_list {
        PropertyValueType::StringList
    } else if is(Ty::Wkb) || is(Ty::WkbReduced) {
        PropertyValueType::WktString
    } else if i32::from(value_type) >= Ty::WeightedString as i32
        && i32::from(value_type) <= Ty::WeightedWkbReduced as i32
    {
        PropertyValueType::KeyValueList
    } else {
        PropertyValueType::String
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        // Safety: the manager was initialized by a successful `open` and every
        // referencing `Results` held an `Arc`, so they have all been dropped.
        // The data set reference held for name lookups is released first, so
        // freeing the manager then frees the data set.
        unsafe {
            sys::fiftyoneDegreesDataSetIpiRelease(self.data_set.as_ptr());
            sys::fiftyoneDegreesResourceManagerFree(self.manager.as_ptr());
            drop(Box::from_raw(self.manager.as_ptr()));
        }
    }
}

/// Read the evidence keys the data set behind `manager` defines from the C
/// library, lowercased, with every query key and then every server key, each
/// in the data set's order. That is the order the native lookup tries them,
/// and the order the specification gives the list in. The C library lists a
/// header's query and server keys together.
///
/// # Safety
/// `manager` must be initialized.
unsafe fn read_evidence_keys(manager: *mut sys::ResourceManager) -> Vec<EvidenceKey> {
    let data_set = sys::fiftyoneDegreesDataSetIpiGet(manager);
    if data_set.is_null() {
        return Vec::new();
    }
    // The data set is held until the release below, so both calls read the
    // same one. A null buffer only finds the length, and the buffer is then
    // writable for the keys and the null terminator.
    let length = sys::fiftyoneDegreesIpiGetEvidenceKeys(
        data_set,
        std::ptr::null_mut(),
        0,
        EVIDENCE_KEY_SEPARATOR.as_ptr(),
    );
    let mut buffer = vec![0u8; length + 1];
    sys::fiftyoneDegreesIpiGetEvidenceKeys(
        data_set,
        buffer.as_mut_ptr() as *mut c_char,
        buffer.len(),
        EVIDENCE_KEY_SEPARATOR.as_ptr(),
    );
    sys::fiftyoneDegreesDataSetIpiRelease(data_set);
    buffer.truncate(length);
    let text = String::from_utf8_lossy(&buffer).to_lowercase();

    let prefixes = [
        (constants::EVIDENCE_QUERY_PREFIX, sys::EvidencePrefix::Query),
        (
            constants::EVIDENCE_SERVER_PREFIX,
            sys::EvidencePrefix::Server,
        ),
    ];
    let mut keys = Vec::new();
    for (prefix, native_prefix) in prefixes {
        for key in text.split(',') {
            let Some((key_prefix, field)) = key.split_once(constants::EVIDENCE_SEPARATOR) else {
                continue;
            };
            if key_prefix != prefix || field.is_empty() {
                continue;
            }
            keys.push(EvidenceKey {
                key: key.to_owned(),
                prefix: native_prefix,
                // The text came from a C string, so it holds no nul byte.
                field: CString::new(field).expect("a header name has no nul"),
            });
        }
    }
    keys
}

/// The separator the C library writes between evidence keys. No header name
/// holds a comma.
const EVIDENCE_KEY_SEPARATOR: &CStr = c",";

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
    /// Evidence holding no client IP address that can be read is looked up
    /// with none, as the .NET engine looks it up, so the results are empty.
    /// [`Self::has_values_by_index`] then answers false for every property,
    /// with the reason [`Self::no_value_message_by_index`] gives.
    pub fn process_evidence(&mut self, evidence: &Evidence) -> Result<()> {
        self.process_client_ip_evidence(evidence).map(|_| ())
    }

    /// [`Self::process_evidence`], giving the client IP evidence it read.
    pub fn process_client_ip_evidence(&mut self, evidence: &Evidence) -> Result<ClientIpEvidence> {
        let mut client_ip = ClientIpEvidence {
            address: None,
            supplied: false,
        };
        let mut pairs = Vec::new();
        for key in &self.manager.evidence_keys {
            let Some(text) = evidence.get(&key.key) else {
                continue;
            };
            client_ip.supplied |= !text.trim().is_empty();
            let Some(address) = parse_client_ip(text) else {
                continue;
            };
            if client_ip.address.is_none() {
                client_ip.address = Some(address.to_canonical());
            }
            // A formatted address never holds a nul byte.
            let value = CString::new(address.to_string()).expect("an IP address has no nul");
            pairs.push((key, value));
        }
        self.process_pairs(&pairs)?;
        Ok(client_ip)
    }

    /// Look up the evidence `pairs` hold, each an evidence key and its value.
    /// With no pairs the lookup clears any earlier one and finds nothing.
    fn process_pairs(&self, pairs: &[(&EvidenceKey, CString)]) -> Result<()> {
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
            for (key, value) in pairs {
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
    /// A value the data file stores as an Azimuth or Declination, such as a
    /// latitude or a longitude, is given to 3 decimal places, as
    /// [`Results::value_as_string`] gives it. A value stored as an IP address,
    /// such as `IpRangeStart`, is written as [`ip_address_text`] writes it,
    /// where [`Results::value_as_string`] gives every field of an IPv6
    /// address in full.
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
        // Safety: `results` is valid, and the shim checks the index.
        let stored_type =
            unsafe { sys::fiftyoneDegreesShimIpiGetStoredValueType(self.as_ptr(), index) };

        // Read the collection while it is alive, then release it regardless of
        // whether reading raised an exception, so no native memory leaks.
        let read = if exception.is_okay() {
            // Safety: the collection came from the getter above and is intact.
            unsafe { collect_weighted(&collection, stored_type) }
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
    /// type. A property with no value yields [`PropertyRead::Absent`]. Each
    /// value's text is as [`Self::values_weighted`] writes it.
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

    /// Why the property at `index`, as [`Manager::required_property_index`]
    /// gives it, has no value in these results, in the C library's words,
    /// which .NET gives as the reason too. Empty results, from evidence with
    /// no address, give "The results are empty. This is probably because we
    /// don't have this data in our database.". Returns an error for an index
    /// outside this data set's required properties.
    pub fn no_value_message_by_index(&self, index: c_int) -> Result<String> {
        self.manager.check_property_index(index)?;
        let mut exception = sys::Exception::cleared();
        // Safety: `results` is valid and the index is one of its data set's
        // required properties.
        let reason = unsafe {
            sys::fiftyoneDegreesResultsIpiGetNoValueReason(self.as_ptr(), index, &mut exception)
        };
        exception.check()?;
        // Safety: the C library gives a static string for every reason.
        let message = unsafe { sys::fiftyoneDegreesResultsIpiGetNoValueReasonMessage(reason) };
        if message.is_null() {
            return Ok(String::new());
        }
        // Safety: the string is static and null terminated.
        Ok(unsafe { CStr::from_ptr(message) }
            .to_string_lossy()
            .into_owned())
    }

    /// Whether the property at `index`, as [`Manager::required_property_index`]
    /// gives it, has a value in these results, as the C library decides it and
    /// as .NET asks before it reads one. Empty results have no value for any
    /// property, although reading a mandatory property's values gives its
    /// default. Returns an error for an index outside this data set's required
    /// properties.
    pub fn has_values_by_index(&self, index: c_int) -> Result<bool> {
        self.manager.check_property_index(index)?;
        let mut exception = sys::Exception::cleared();
        // Safety: `results` is valid and the index is one of its data set's
        // required properties.
        let has_values = unsafe {
            sys::fiftyoneDegreesResultsIpiGetHasValues(self.as_ptr(), index, &mut exception)
        };
        exception.check()?;
        Ok(has_values)
    }
}

/// The client IP evidence a lookup read, as
/// [`Results::process_client_ip_evidence`] gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ClientIpEvidence {
    /// The address looked up, being the first value under
    /// [`Manager::evidence_keys`] that reads as an IP address, in their order,
    /// or [`None`] when none does. An IPv4-mapped IPv6 address, such as
    /// `::ffff:1.1.1.1`, is given as the IPv4 address it holds, which is what
    /// [RFC 4291 section 2.5.5.2](https://www.rfc-editor.org/rfc/rfc4291#section-2.5.5.2)
    /// says the form is and the address the C library looks up.
    pub address: Option<IpAddr>,
    /// Whether any of those keys held a value that is not blank. With no
    /// address, this tells evidence that held none apart from evidence whose
    /// values do not read as an address.
    pub supplied: bool,
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
/// supported weighted leaf types are skipped. `stored_type` is the type the
/// data file stores the property's values as, which decides how a double is
/// rendered, and an IP address is written as [`ip_address_text`] writes it.
///
/// # Safety
/// `collection` must be an intact collection returned by
/// [`sys::fiftyoneDegreesResultsIpiGetValuesCollection`] and not yet released.
unsafe fn collect_weighted(
    collection: &sys::WeightedValuesCollection,
    stored_type: c_int,
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
                    let text = CStr::from_ptr(value).to_string_lossy().into_owned();
                    Some(if stored_type == Ty::IpAddress as c_int {
                        address_text_of(text)
                    } else {
                        text
                    })
                }
            }
            Ty::WeightedInt | Ty::Integer => {
                let wi = header as *const sys::WeightedInt;
                Some((*wi).value.to_string())
            }
            Ty::WeightedDouble | Ty::Double => {
                let wd = header as *const sys::WeightedDouble;
                Some(double_text((*wd).value, stored_type))
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

/// Render a double from a value collection, given the type the data file
/// stores it as.
///
/// A value converted from an Azimuth or Declination is given to 3 decimal
/// places by [`round_angle`], as the specification requires, so it reads as
/// the `f32` nearest the rounded decimal. A stored 32-bit float is given with
/// the float's own digits, as the double holds it exactly. Any other double
/// keeps every place it has.
fn double_text(value: f64, stored_type: c_int) -> String {
    use sys::PropertyValueType as Ty;
    if stored_type == Ty::Declination as c_int || stored_type == Ty::Azimuth as c_int {
        round_angle(value).to_string()
    } else if stored_type == Ty::SinglePrecisionFloat as c_int {
        (value as f32).to_string()
    } else {
        value.to_string()
    }
}

/// The sixth field of an ISATAP address, whose last 32 bits are an IPv4
/// address.
const ISATAP_FIELD: u16 = 0x5efe;

/// The longest text of an IPv6 address, being six fields and an IPv4
/// address.
const IPV6_TEXT_MAX_BYTES: usize = 45;

/// The text of `address` as .NET's `IPAddress.ToString` writes it.
///
/// An IPv6 address follows
/// [RFC 5952 section 4](https://www.rfc-editor.org/rfc/rfc5952#section-4),
/// in lower case, without leading zeros, and with the longest run of two or
/// more zero fields, the first of equal runs, written as `::`. Its last 32
/// bits are written as an IPv4 address when its first four fields are zero,
/// its seventh is not, and the fifth and sixth are `0:0` (`::8.8.8.8`),
/// `0:ffff` (`::ffff:8.8.8.8`) or `ffff:0` (`::ffff:0:8.8.8.8`), and for an
/// ISATAP address (`fe80::5efe:8.8.8.8`). Every other address is written in
/// hexadecimal, including one of those forms whose seventh field is zero
/// (`::ffff:0:102`) and one under the RFC 6052 prefix `64:ff9b::/96`
/// (`64:ff9b::808:808`).
///
/// [RFC 5952 section 5](https://www.rfc-editor.org/rfc/rfc5952#section-5)
/// recommends the IPv4 form for addresses under the prefixes of RFC 4291
/// and RFC 2765 (replaced by RFC 6145 and then RFC 7915), `::ffff:0:102`
/// among them, and
/// [RFC 6052 section 2.4](https://www.rfc-editor.org/rfc/rfc6052#section-2.4)
/// allows it for `64:ff9b::/96`. Neither requires it, so this function
/// writes these forms as .NET does. Rust's `Display` uses the IPv4 form
/// for `::ffff:` alone, whatever the seventh field.
pub fn ip_address_text(address: IpAddr) -> String {
    use std::fmt::Write as _;
    let address = match address {
        IpAddr::V4(address) => return address.to_string(),
        IpAddr::V6(address) => address,
    };
    let fields = address.segments();
    let ipv4 = writes_ipv4_in_ipv6(&fields);
    let mut text = String::with_capacity(IPV6_TEXT_MAX_BYTES);
    push_ipv6_fields(&mut text, &fields[..if ipv4 { 6 } else { 8 }]);
    if ipv4 {
        if !text.ends_with(':') {
            text.push(':');
        }
        let [.., a, b, c, d] = address.octets();
        write!(text, "{}", Ipv4Addr::new(a, b, c, d)).expect("a String takes any text");
    }
    text
}

/// Whether [`ip_address_text`] writes the last 32 bits of the IPv6 address
/// with these `fields` as an IPv4 address.
fn writes_ipv4_in_ipv6(fields: &[u16; 8]) -> bool {
    let ipv4_prefix = fields[..4].iter().all(|field| *field == 0)
        && fields[6] != 0
        && matches!((fields[4], fields[5]), (0, 0) | (0, 0xffff) | (0xffff, 0));
    let isatap = fields[4] == 0 && fields[5] == ISATAP_FIELD;
    ipv4_prefix || isatap
}

/// Write `fields` to `text` in hexadecimal with `:` between them, and the
/// longest run of two or more zero fields, the first of equal runs, as `::`.
fn push_ipv6_fields(text: &mut String, fields: &[u16]) {
    use std::fmt::Write as _;
    let mut longest = 0..0;
    let mut start = 0;
    for (place, field) in fields.iter().enumerate() {
        if *field != 0 {
            start = place + 1;
        } else if place + 1 - start > longest.len() {
            longest = start..place + 1;
        }
    }
    let push = |text: &mut String, part: &[u16]| {
        for (place, field) in part.iter().enumerate() {
            if place > 0 {
                text.push(':');
            }
            write!(text, "{field:x}").expect("a String takes any text");
        }
    };
    if longest.len() > 1 {
        push(text, &fields[..longest.start]);
        text.push_str("::");
        push(text, &fields[longest.end..]);
    } else {
        push(text, fields);
    }
}

/// `text` as [`ip_address_text`] writes the address it holds, or `text`
/// itself when it holds none.
fn address_text_of(text: String) -> String {
    match text.parse::<IpAddr>() {
        Ok(address) => ip_address_text(address),
        Err(_) => text,
    }
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

/// The message for a failed init, naming the data file, or `null` for a load
/// from memory. An exception holds no file name, so its message would say
/// `null` instead.
fn status_detail(status: sys::StatusCode, path: Option<&CStr>) -> String {
    // Safety: the path is null terminated and outlives the call, or null for
    // a load from memory, and the library allocated the message.
    unsafe {
        take_message::<sys::Exception>(
            sys::common::fiftyoneDegreesStatusGetMessage(
                status,
                path.map_or(std::ptr::null(), CStr::as_ptr),
            ),
            "native initialization failed with no message",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// The ASN file in the data repository.
    const ASN_FILE_NAME: &str = "51Degrees-IPIV4AsnIpiV41.ipi";

    /// Resolve an IP Intelligence data file at run time, searching the
    /// environment override, the `cxx` submodule and the
    /// wider Workspace tree. Returns [`None`] when none is present so the smoke
    /// tests can skip cleanly.
    ///
    /// The ASN file from the data repository, which its script downloads, loads
    /// against this source revision and is the convenient default. Point
    /// `_51DEGREES_IPI_PATH` at another data file to run the lookups against it.
    fn ipi_data_file() -> Option<PathBuf> {
        if let Ok(path) = std::env::var("_51DEGREES_IPI_PATH") {
            let path = PathBuf::from(path);
            if path.is_file() {
                return Some(path);
            }
        }
        asn_file()
    }

    /// The ASN file from the data repository, for a test that holds several
    /// copies of the data file and so needs a small one whatever
    /// `_51DEGREES_IPI_PATH` names.
    fn asn_file() -> Option<PathBuf> {
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()?
            .to_path_buf();
        let candidates = [
            workspace
                .join("cxx")
                .join("data")
                .join("ip-intelligence")
                .join(ASN_FILE_NAME),
            workspace
                .parent()
                .map(|p| p.join("data").join("ip-intelligence").join(ASN_FILE_NAME))
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

    /// The C library finds a property name ignoring case, so a name in
    /// another case than the data file's gives the property's index, and
    /// every read by name gives the property's answer.
    #[test]
    fn a_name_in_another_case_than_the_data_files_is_read() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping a name in another case");
            return;
        };
        let manager = open_found(&data_file, PerformanceProfile::Default);
        let index = manager
            .required_property_index("Asn")
            .expect("the data file has Asn");
        for name in ["asn", "ASN", "aSN"] {
            assert_eq!(manager.required_property_index(name), Some(index), "{name}");
        }
        let mut results = manager.create_results().expect("results should allocate");
        results
            .process_ip(CLOUDFLARE)
            .expect("looking up a public IP should not raise an exception");
        let weighted = results.values_weighted("Asn").expect("Asn is read");
        assert!(!weighted.is_empty(), "{CLOUDFLARE} has an Asn");
        assert_eq!(
            results.values_weighted("asn").expect("asn is read"),
            weighted
        );
        assert_eq!(
            results.read_property("ASN").expect("ASN is read"),
            results.read_property("Asn").expect("Asn is read")
        );
    }

    /// The C library's type names of the collections `manager`'s data set
    /// holds its strings and its values in.
    fn strings_and_values_types(manager: &Manager) -> (String, String) {
        let mut strings: *const c_char = std::ptr::null();
        let mut values: *const c_char = std::ptr::null();
        // Safety: the manager is initialized and both outputs are writable.
        let found = unsafe {
            sys::fiftyoneDegreesShimIpiGetCollectionTypeNames(
                manager.as_ptr(),
                &mut strings,
                &mut values,
            )
        };
        assert_eq!(found, 1, "the data set is available");
        // Safety: both names are constants of the C library.
        let name = |text| {
            unsafe { CStr::from_ptr(text) }
                .to_string_lossy()
                .into_owned()
        };
        (name(strings), name(values))
    }

    /// `HighPerformance` with some properties named holds in memory only the
    /// values those properties read, and the named property gives the
    /// answer it gives with none named.
    #[test]
    fn high_performance_naming_some_holds_only_what_they_read() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping held values");
            return;
        };
        let profile = PerformanceProfile::HighPerformance;
        let some = Manager::open_with_properties(&data_file, profile, Some(&["Asn"]))
            .expect("data file loads");
        let every = Manager::open(&data_file, profile).expect("data file loads");
        assert_eq!(strings_and_values_types(&some).1, "CollectionHeld");
        let evidence = [("query.client-ip-51d", CLOUDFLARE)];
        let asn = asn_from(&some, &evidence).expect("Asn reads");
        assert!(asn.is_some(), "{CLOUDFLARE} has an autonomous system");
        assert_eq!(asn, asn_from(&every, &evidence).expect("Asn reads"));
    }

    /// `HighPerformance` with no property named holds every string and value
    /// in memory.
    #[test]
    fn high_performance_naming_none_holds_everything() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping held values");
            return;
        };
        let every = Manager::open(&data_file, PerformanceProfile::HighPerformance)
            .expect("data file loads");
        assert_eq!(
            strings_and_values_types(&every),
            (
                String::from("CollectionMemory"),
                String::from("CollectionMemory")
            )
        );
    }

    /// Every profile but `InMemory` opens a handle to the data file for each
    /// concurrent operation it expects, to at most 64, and keeps it open,
    /// `HighPerformance` included. With no concurrency named, it expects one
    /// for each processor.
    #[test]
    fn the_file_pool_has_a_handle_for_each_expected_operation() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping the file pool");
            return;
        };
        // Safety: the function reads no state.
        let processors =
            unsafe { sys::common::fiftyoneDegreesThreadingGetProcessorCount() }.min(64);
        for (profile, concurrency, handles) in [
            (PerformanceProfile::LowMemory, Some(3), 3),
            (PerformanceProfile::Balanced, None, processors),
            (PerformanceProfile::Balanced, Some(500), 64),
            (PerformanceProfile::HighPerformance, Some(3), 3),
            (PerformanceProfile::InMemory, Some(3), 0),
            (PerformanceProfile::InMemory, None, 0),
        ] {
            let manager = Manager::open_with_options(&data_file, profile, None, concurrency)
                .expect("data file loads");
            assert_eq!(
                manager.file_pool_size(),
                handles,
                "{} {concurrency:?}",
                profile.as_str()
            );
        }
    }

    /// Where the published date starts in an IP Intelligence data file
    /// header, after the four version numbers, two 16 byte tags, the
    /// copyright offset, the age, the profile offset shift and the name and
    /// format offsets. The next update date follows it.
    const PUBLISHED_AT: usize = 66;

    /// The published and next update dates of the data file `bytes` holds,
    /// read from the bytes as the header lays them out.
    fn header_dates(bytes: &[u8]) -> ((i32, u32, u32), (i32, u32, u32)) {
        let date = |at: usize| {
            (
                i32::from(i16::from_le_bytes([bytes[at], bytes[at + 1]])),
                u32::from(bytes[at + 2]),
                u32::from(bytes[at + 3]),
            )
        };
        (date(PUBLISHED_AT), date(PUBLISHED_AT + 4))
    }

    /// The header ends after the ten collection headers that follow the two
    /// dates, each 16 bytes as the library is built with 64 bit file offsets,
    /// and the strings collection, the first in the file, starts there.
    #[test]
    fn the_header_size_is_the_ip_intelligence_data_file_header() {
        use std::io::Read;
        const DATE_SIZE: usize = 4;
        const COLLECTION_HEADER_SIZE: usize = 16;
        const COLLECTION_COUNT: usize = 10;
        const START_SIZE: usize = std::mem::size_of::<u64>();
        let strings_at = PUBLISHED_AT + 2 * DATE_SIZE;
        assert_eq!(
            data_file_header_size(),
            strings_at + COLLECTION_COUNT * COLLECTION_HEADER_SIZE
        );
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping the header size");
            return;
        };
        let mut header = vec![0u8; strings_at + START_SIZE];
        std::fs::File::open(&data_file)
            .and_then(|mut file| file.read_exact(&mut header))
            .expect("the header reads");
        let start: [u8; START_SIZE] = header[strings_at..].try_into().expect("a start");
        assert_eq!(
            u64::from_le_bytes(start),
            u64::try_from(data_file_header_size()).expect("a header size")
        );
    }

    /// The dates read are the ones in the data file header, whether the data
    /// set reads the file or holds it in memory.
    #[test]
    fn the_dates_are_the_ones_the_data_file_header_holds() {
        let Some(data_file) = asn_file() else {
            eprintln!("no ASN data file found; skipping the header dates");
            return;
        };
        let bytes = std::fs::read(&data_file).expect("data file reads");
        let (published, next) = header_dates(&bytes);
        assert!(published < next, "{published:?} {next:?}");
        for manager in [
            open_found(&data_file, PerformanceProfile::LowMemory),
            Manager::open_from_memory(bytes, PerformanceProfile::LowMemory, None)
                .expect("data loads"),
        ] {
            assert_eq!(manager.data_set_published(), Some(published));
            assert_eq!(manager.data_set_next_update(), Some(next));
        }
    }

    /// A header date with a year that is not positive, a month outside 1 to
    /// 12 or a day outside 1 to 31 is not a date, so it is not read. The year
    /// -1 would otherwise be read as a date before year 1.
    #[test]
    fn a_header_holding_no_valid_date_gives_no_dates() {
        let Some(data_file) = asn_file() else {
            eprintln!("no ASN data file found; skipping dates not valid");
            return;
        };
        let bytes = std::fs::read(&data_file).expect("data file reads");
        let with_dates = |date: [u8; 4]| {
            let mut data = bytes.clone();
            for at in [PUBLISHED_AT, PUBLISHED_AT + 4] {
                data[at..at + 4].copy_from_slice(&date);
            }
            data
        };
        let year_2001 = 2001i16.to_le_bytes();
        for (case, date) in [
            ("zeroed", [0, 0, 0, 0]),
            ("year -1", [0xFF, 0xFF, 1, 1]),
            ("month 13", [year_2001[0], year_2001[1], 13, 1]),
            ("day 32", [year_2001[0], year_2001[1], 1, 32]),
        ] {
            let manager =
                Manager::open_from_memory(with_dates(date), PerformanceProfile::LowMemory, None)
                    .expect("data loads");
            assert_eq!(manager.data_set_published(), None, "{case}");
            assert_eq!(manager.data_set_next_update(), None, "{case}");
        }
        let manager = Manager::open_from_memory(
            with_dates([year_2001[0], year_2001[1], 12, 31]),
            PerformanceProfile::LowMemory,
            None,
        )
        .expect("data loads");
        assert_eq!(manager.data_set_published(), Some((2001, 12, 31)));
        assert_eq!(manager.data_set_next_update(), Some((2001, 12, 31)));
    }

    /// A day the month does not have is not a date, so neither date is read,
    /// whilst 29 February is read in a leap year. A year divisible by 100 is
    /// a leap year only when it is also divisible by 400.
    #[test]
    fn a_day_the_month_lacks_is_not_a_date() {
        let Some(data_file) = asn_file() else {
            eprintln!("no ASN data file found; skipping the days months lack");
            return;
        };
        let bytes = std::fs::read(&data_file).expect("data file reads");
        let cases: [(i16, u8, u8, bool); 7] = [
            (2001, 2, 30, false),
            (2001, 2, 29, false),
            (1900, 2, 29, false),
            (2001, 4, 31, false),
            (2004, 2, 29, true),
            (2000, 2, 29, true),
            (2001, 4, 30, true),
        ];
        for (year, month, day, read) in cases {
            let [low, high] = year.to_le_bytes();
            let mut data = bytes.clone();
            for at in [PUBLISHED_AT, PUBLISHED_AT + 4] {
                data[at..at + 4].copy_from_slice(&[low, high, month, day]);
            }
            let manager = Manager::open_from_memory(data, PerformanceProfile::LowMemory, None)
                .expect("data loads");
            let date = (i32::from(year), u32::from(month), u32::from(day));
            let expected = read.then_some(date);
            assert_eq!(manager.data_set_published(), expected, "{date:?}");
            assert_eq!(manager.data_set_next_update(), expected, "{date:?}");
        }
    }

    /// A data set loaded from memory under any profile opens no file handle,
    /// takes the property named, and gives the evidence keys, properties
    /// and answers the file gives, after the caller has dropped its own
    /// reference to the manager.
    #[test]
    fn a_data_set_loads_from_memory_as_it_does_from_the_file() {
        let Some(data_file) = asn_file() else {
            eprintln!("no ASN data file found; skipping loading from memory");
            return;
        };
        let bytes = std::fs::read(&data_file).expect("data file reads");
        let all = open_found(&data_file, PerformanceProfile::InMemory).available_property_names();
        assert!(all.len() > 1, "{all:?}");
        let named = [all[0].as_str()];
        for profile in [
            PerformanceProfile::LowMemory,
            PerformanceProfile::Balanced,
            PerformanceProfile::HighPerformance,
            PerformanceProfile::InMemory,
        ] {
            for properties in [None, Some(&named[..])] {
                let case = format!("{} {properties:?}", profile.as_str());
                let from_file =
                    Manager::open_with_options(&data_file, profile, properties, Some(3))
                        .expect("data file loads");
                let from_memory = Manager::open_from_memory(bytes.clone(), profile, properties)
                    .expect("data loads");
                assert_eq!(from_memory.file_pool_size(), 0, "{case}");
                let names = from_memory.available_property_names();
                assert_eq!(names, from_file.available_property_names(), "{case}");
                assert_eq!(
                    from_memory.evidence_keys().collect::<Vec<_>>(),
                    from_file.evidence_keys().collect::<Vec<_>>(),
                    "{case}"
                );
                let mut lookups = Vec::new();
                for address in ["1.1.1.1", "2606:4700:4700::1111"] {
                    let mut expected = from_file.create_results().expect("results allocate");
                    expected.process_ip(address).expect("an address");
                    let mut results = from_memory.create_results().expect("results allocate");
                    results.process_ip(address).expect("an address");
                    lookups.push((address, expected, results));
                }
                // The results keep the data set, and the memory it reads.
                drop(from_memory);
                for (address, expected, results) in &lookups {
                    for name in &names {
                        assert_eq!(
                            results.value_as_string(name, "|").expect("value reads"),
                            expected.value_as_string(name, "|").expect("value reads"),
                            "{name} {address} {case}"
                        );
                    }
                }
            }
        }
    }

    /// Data that is not exactly the whole of a data file is refused with the
    /// C library's status. Too few bytes for the header, or for a collection
    /// the header describes, is corrupt data, a header of another version is
    /// the wrong version, and bytes left over after the last collection are
    /// out of bounds.
    #[test]
    fn data_that_is_not_a_whole_data_file_is_refused() {
        let Some(data_file) = asn_file() else {
            eprintln!("no ASN data file found; skipping refused data");
            return;
        };
        let bytes = std::fs::read(&data_file).expect("data file reads");
        let half = bytes[..bytes.len() / 2].to_vec();
        let mut extra_byte = bytes.clone();
        extra_byte.push(0);
        for (name, data, expected) in [
            ("nothing", Vec::new(), "CorruptData"),
            ("zeros", vec![0u8; 1024], "IncorrectVersion"),
            (
                "the first 1024 bytes",
                bytes[..1024].to_vec(),
                "CorruptData",
            ),
            ("half the file", half, "CorruptData"),
            ("the file and a byte", extra_byte, "PointerOutOfBounds"),
        ] {
            let error = Manager::open_from_memory(data, PerformanceProfile::InMemory, None)
                .err()
                .unwrap_or_else(|| panic!("{name} should be refused"));
            let Error::Native { message, status } = &error else {
                panic!("{name}: {error}");
            };
            assert_eq!(status, expected, "{name}: {message}");
            assert!(!message.is_empty(), "{name}");
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

    /// A double converted from an Azimuth or Declination is given to 3
    /// decimal places, so a plain float reads as the `f32` nearest the
    /// rounded decimal.
    #[test]
    fn an_angle_is_given_to_three_decimal_places() {
        use sys::PropertyValueType as Ty;
        let declination = Ty::Declination as c_int;
        let azimuth = Ty::Azimuth as c_int;
        // A stored Declination of 7723 converts to 21.21250038148137.
        let latitude = 7723.0 * 90.0 / 32767.0;
        assert_eq!(double_text(latitude, declination), "21.213");
        assert_eq!(double_text(-latitude, declination), "-21.213");
        assert_eq!(double_text(180.0, azimuth), "180");
        // A stored Azimuth of 4274 converts to 23.47849971007416.
        assert_eq!(double_text(4274.0 * 180.0 / 32767.0, azimuth), "23.478");
        assert_eq!(
            property_read_from(vec![(
                Ty::Double,
                double_text(latitude, declination),
                u16::MAX,
            )]),
            PropertyRead::Float(21.213)
        );
    }

    /// A double the data file does not store as an Azimuth or Declination
    /// keeps its places, whether stored as a double, a string or an integer,
    /// and a stored 32-bit float keeps the float's own digits.
    #[test]
    fn a_double_that_is_not_an_angle_keeps_its_places() {
        use sys::PropertyValueType as Ty;
        let latitude = 7723.0 * 90.0 / 32767.0;
        assert_eq!(
            double_text(latitude, Ty::Double as c_int),
            "21.21250038148137"
        );
        assert_eq!(double_text(0.0004, Ty::Double as c_int), "0.0004");
        assert_eq!(double_text(0.0004, Ty::String as c_int), "0.0004");
        assert_eq!(double_text(-0.0001, Ty::String as c_int), "-0.0001");
        assert_eq!(double_text(42.0, Ty::Integer as c_int), "42");
        assert_eq!(
            double_text(f64::from(0.1_f32), Ty::SinglePrecisionFloat as c_int),
            "0.1"
        );
        assert_eq!(
            double_text(f64::from(18.5427_f32), Ty::SinglePrecisionFloat as c_int),
            "18.5427"
        );
        // The type could not be read.
        assert_eq!(double_text(0.0004, -1), "0.0004");
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
    /// confirm evidence without an IP is looked up and finds nothing.
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
        results
            .process_evidence(&without_ip)
            .expect("evidence with no client IP should be looked up");
        let index = manager
            .required_property_index("Asn")
            .expect("the data file has Asn");
        assert!(!results.has_values_by_index(index).expect("asked"));
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

    /// The web request a web integration gives with the evidence, under
    /// `server.http-request`, is not one of its text values, so no address is
    /// read from it. The lookup takes the evidence's own address, and with
    /// none it finds nothing from the request and returns without a panic.
    #[test]
    fn no_address_is_read_from_the_web_request() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping the web request");
            return;
        };
        let manager = open_found(&data_file, PerformanceProfile::HighPerformance);
        let request = || {
            Evidence::builder()
                .add("server.client-ip", QUAD9)
                .add("query.client-ip-51d", QUAD9)
                .build()
        };
        let lookup = |evidence: Evidence| -> Result<Option<String>> {
            let mut results = manager.create_results()?;
            results.process_evidence(&evidence)?;
            results.value_as_string("Asn", ",")
        };

        let asn = lookup(
            Evidence::builder()
                .add("server.client-ip", CLOUDFLARE)
                .http_request(request())
                .build(),
        )
        .expect("the lookup should succeed");
        assert!(
            asn.as_deref().unwrap_or_default().contains("AS13335"),
            "got {asn:?}"
        );

        let alone = lookup(Evidence::builder().http_request(request()).build());
        assert!(
            !matches!(&alone, Ok(Some(asn)) if asn.contains("AS19281")),
            "got {alone:?}"
        );
    }

    #[test]
    fn a_51d_key_and_an_address_with_a_port_are_read() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping 51d keys");
            return;
        };
        let manager = open_found(&data_file, PerformanceProfile::HighPerformance);
        for value in ["1.1.1.1:443", "[2606:4700:4700::1111]:443"] {
            let asn = asn_from(&manager, &[("query.client-ip-51d", value)])
                .unwrap_or_else(|err| panic!("{value} should be looked up: {err}"));
            assert!(
                asn.as_deref().unwrap_or_default().contains("AS13335"),
                "{value} gave {asn:?}"
            );
        }
    }

    /// The reason the C library gives every property of empty results.
    const NO_RESULTS_MESSAGE: &str =
        "The results are empty. This is probably because we don't have this data in our database.";

    fn evidence_of(pairs: &[(&str, &str)]) -> Evidence {
        let mut builder = Evidence::builder();
        for (key, value) in pairs {
            builder = builder.add(*key, *value);
        }
        builder.build()
    }

    /// Evidence with no address that reads as one is looked up with none, as
    /// .NET looks it up, so no property has a value and each gives the C
    /// library's reason. A blank value counts as no value supplied.
    #[test]
    fn evidence_with_no_readable_address_finds_nothing() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping unreadable evidence");
            return;
        };
        let manager = open_found(&data_file, PerformanceProfile::HighPerformance);
        for (pairs, supplied) in [
            (&[][..], false),
            (&[("header.user-agent", "x")][..], false),
            (&[("server.client-ip", "")][..], false),
            (&[("server.client-ip", " ")][..], false),
            (&[("server.client-ip", "unknown")][..], true),
            (
                &[("query.client-ip-51d", "unknown"), ("server.client-ip", "")][..],
                true,
            ),
        ] {
            let mut results = manager.create_results().expect("results allocate");
            let client_ip = results
                .process_client_ip_evidence(&evidence_of(pairs))
                .unwrap_or_else(|err| panic!("{pairs:?} should be looked up: {err}"));
            assert_eq!(
                client_ip,
                ClientIpEvidence {
                    address: None,
                    supplied
                },
                "{pairs:?}"
            );
            for name in manager.available_property_names() {
                let index = manager
                    .required_property_index(&name)
                    .expect("an available property has an index");
                assert!(
                    !results.has_values_by_index(index).expect("asked"),
                    "{name} for {pairs:?}"
                );
                assert_eq!(
                    results.no_value_message_by_index(index).expect("reason"),
                    NO_RESULTS_MESSAGE,
                    "{name} for {pairs:?}"
                );
            }
        }
    }

    /// A lookup with no address clears the lookup before it on the same
    /// results. Reading a mandatory property's values with nothing found
    /// gives its default, which is why .NET asks whether there are values
    /// first.
    #[test]
    fn a_lookup_with_no_address_clears_the_one_before() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping a cleared lookup");
            return;
        };
        let manager = open_found(&data_file, PerformanceProfile::HighPerformance);
        let index = manager
            .required_property_index("Asn")
            .expect("the data file has Asn");
        let mut results = manager.create_results().expect("results allocate");
        results
            .process_evidence(&evidence_of(&[("server.client-ip", CLOUDFLARE)]))
            .expect("the lookup should succeed");
        assert!(results.has_values_by_index(index).expect("asked"));
        let found = results.value_as_string("Asn", ",").expect("read");
        assert!(
            found.as_deref().unwrap_or_default().contains("AS13335"),
            "got {found:?}"
        );
        results
            .process_evidence(&evidence_of(&[]))
            .expect("the lookup should succeed");
        assert!(!results.has_values_by_index(index).expect("asked"));
        assert_eq!(
            results.no_value_message_by_index(index).expect("reason"),
            NO_RESULTS_MESSAGE
        );
        assert_ne!(results.value_as_string("Asn", ",").expect("read"), found);
    }

    /// The address given back is the one the lookup used, the first that
    /// reads as an address in the order of the evidence keys, with an
    /// IPv4-mapped address given as its IPv4 address.
    #[test]
    fn the_address_given_back_is_the_one_looked_up() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping the address looked up");
            return;
        };
        let manager = open_found(&data_file, PerformanceProfile::HighPerformance);
        for (pairs, address, asn) in [
            (
                &[
                    ("query.client-ip-51d", "unknown"),
                    ("server.client-ip", CLOUDFLARE),
                    ("query.client-ip", QUAD9),
                ][..],
                QUAD9,
                "AS19281",
            ),
            (
                &[("server.client-ip", "[2606:4700:4700::1111]:443")][..],
                "2606:4700:4700::1111",
                "AS13335",
            ),
            (
                &[("server.client-ip", "::ffff:1.1.1.1")][..],
                CLOUDFLARE,
                "AS13335",
            ),
        ] {
            let mut results = manager.create_results().expect("results allocate");
            let client_ip = results
                .process_client_ip_evidence(&evidence_of(pairs))
                .unwrap_or_else(|err| panic!("{pairs:?} should be looked up: {err}"));
            assert_eq!(
                client_ip,
                ClientIpEvidence {
                    address: Some(address.parse().expect("an address")),
                    supplied: true
                },
                "{pairs:?}"
            );
            let found = results.value_as_string("Asn", ",").expect("read");
            assert!(
                found.as_deref().unwrap_or_default().contains(asn),
                "{pairs:?} gave {found:?}"
            );
        }
    }

    /// A value under the first evidence key that is not wholly an address,
    /// such as a list, a value with white space around it, a zone index or a
    /// leading zero, is skipped and the next key's address is read, as
    /// "Reading the client IP value" in the IP Intelligence specification
    /// says. The rule is at <https://github.com/51Degrees/specifications/blob/5a250650766c025c42aa9d7943bd6781f4447646/ip-intelligence-specification/pipeline-elements/ip-intelligence-on-premise.md#L165-L264>.
    #[test]
    fn a_value_that_is_not_wholly_an_address_gives_way_to_the_next_key() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping the next key");
            return;
        };
        let manager = open_found(&data_file, PerformanceProfile::HighPerformance);
        for value in [
            "1.1.1.1, 10.0.0.1",
            " 1.1.1.1",
            "1.1.1.1 ",
            "fe80::1%eth0",
            "[fe80::1%eth0]:443",
            "01.1.1.1",
            "1.1.1.01",
            "[fe80::1%25eth0]",
            "1.1.1.0/24",
            "1.1.1.1 x",
        ] {
            let pairs = [("query.client-ip-51d", value), ("query.client-ip", QUAD9)];
            let mut results = manager.create_results().expect("results allocate");
            let client_ip = results
                .process_client_ip_evidence(&evidence_of(&pairs))
                .unwrap_or_else(|err| panic!("{value:?} should be looked up: {err}"));
            assert_eq!(
                client_ip,
                ClientIpEvidence {
                    address: Some(QUAD9.parse().expect("an address")),
                    supplied: true
                },
                "{value:?}"
            );
            let found = results.value_as_string("Asn", ",").expect("read");
            assert!(
                found.as_deref().unwrap_or_default().contains("AS19281"),
                "{value:?} gave {found:?}"
            );
        }
    }

    /// The profiles the metadata tests open the data file with, being one
    /// that reads the file on each call, one that caches and one that holds
    /// it all in memory.
    const METADATA_PROFILES: [PerformanceProfile; 3] = [
        PerformanceProfile::LowMemory,
        PerformanceProfile::Balanced,
        PerformanceProfile::InMemory,
    ];

    /// The data file property named `name`, which the test needs.
    fn file_property<'a>(properties: &'a [DataFileProperty], name: &str) -> &'a DataFileProperty {
        properties
            .iter()
            .find(|property| property.name == name)
            .unwrap_or_else(|| panic!("the data file defines {name}"))
    }

    #[test]
    fn every_data_file_property_is_listed_and_only_named_ones_are_available() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping property listing");
            return;
        };
        for profile in METADATA_PROFILES {
            let every = open_found(&data_file, profile);
            let properties = every
                .data_file_properties()
                .expect("the data file's properties should be read");
            assert!(!properties.is_empty(), "{profile:?}");
            for (index, property) in properties.iter().enumerate() {
                assert_eq!(property.index, index as u32, "{}", property.name);
                assert!(property.available, "{profile:?} {}", property.name);
                assert_eq!(
                    property.required_property_index,
                    every.required_property_index(&property.name),
                    "{profile:?} {}",
                    property.name
                );
            }

            // Named in another case, as the C library matches names.
            let named = Manager::open_with_properties(&data_file, profile, Some(&["asnname"]))
                .expect("the data file should open with AsnName named");
            let restricted = named
                .data_file_properties()
                .expect("the data file's properties should be read");
            let names = |list: &[DataFileProperty]| -> Vec<String> {
                list.iter().map(|property| property.name.clone()).collect()
            };
            assert_eq!(names(&restricted), names(&properties), "{profile:?}");
            for property in &restricted {
                let is_named = property.name == "AsnName";
                assert_eq!(
                    property.available, is_named,
                    "{profile:?} {}",
                    property.name
                );
                assert_eq!(
                    property.required_property_index,
                    is_named.then_some(0),
                    "{profile:?} {}",
                    property.name
                );
            }
        }
    }

    #[test]
    fn the_asn_property_is_described_as_the_data_file_gives_it() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping Asn metadata");
            return;
        };
        for profile in METADATA_PROFILES {
            let manager = open_found(&data_file, profile);
            let properties = manager
                .data_file_properties()
                .expect("the data file's properties should be read");
            let components = manager
                .data_file_components()
                .expect("the data file's components should be read");
            let asn = file_property(&properties, "Asn");
            assert_eq!(asn.value_type, PropertyValueType::String);
            assert_eq!(asn.category, "Asn");
            assert!(
                asn.description.starts_with("Autonomous System Number"),
                "{}",
                asn.description
            );
            assert_eq!(
                asn.url,
                "https://en.wikipedia.org/wiki/Autonomous_system_(Internet)"
            );
            assert_eq!(asn.data_tiers, ["IPIV4Asn", "IPIV4Enterprise"]);
            assert_eq!(asn.display_order, 1);
            assert!(asn.mandatory && !asn.list && !asn.obsolete);
            assert!(!asn.show && !asn.show_values);
            let component = components
                .iter()
                .find(|component| component.component_id == asn.component_id)
                .expect("the Asn property's component is listed");
            assert_eq!(component.name, "Asn");
            let default = asn.default_value.as_ref().expect("Asn has a default");
            assert_eq!(default.name, "Unknown");
            assert_eq!(
                (default.description.as_str(), default.url.as_str()),
                ("", "")
            );
        }
    }

    /// The geometry names the C library's WKT writer starts an area's text
    /// with, from `wkbtot.c`.
    const WKT_GEOMETRIES: [&str; 18] = [
        "GEOMETRY",
        "POINT",
        "LINESTRING",
        "POLYGON",
        "MULTIPOINT",
        "MULTILINESTRING",
        "MULTIPOLYGON",
        "GEOMETRYCOLLECTION",
        "CIRCULARSTRING",
        "COMPOUNDCURVE",
        "CURVEPOLYGON",
        "MULTICURVE",
        "MULTISURFACE",
        "CURVE",
        "SURFACE",
        "POLYHEDRALSURFACE",
        "TIN",
        "TRIANGLE",
    ];

    /// True when `text`, a value written as text to 3 places, may not find
    /// the value it was written from, because the value is a number or an
    /// area with coordinates that the data file can hold to more places. The
    /// C++ code and .NET's `GetValue` find nothing for such a text either.
    fn may_hold_more_places(value_type: PropertyValueType, text: &str) -> bool {
        let number = matches!(
            value_type,
            PropertyValueType::Float | PropertyValueType::Double
        ) || (text.contains('.') && text.parse::<f64>().is_ok());
        let geometry = text.split([' ', '(']).next().unwrap_or_default();
        number || (WKT_GEOMETRIES.contains(&geometry) && text.contains('('))
    }

    #[test]
    fn only_a_number_or_an_area_with_coordinates_may_miss_by_its_text() {
        use PropertyValueType as Type;
        for (value_type, text) in [
            (Type::Float, "51.507"),
            (Type::Float, "0"),
            (Type::Double, "-0.128"),
            (Type::String, "-0.128"),
            (Type::String, "POINT (51.507 -0.128)"),
            (Type::String, "POLYGON ((1.5 2.5,3.5 4.5,1.5 2.5))"),
        ] {
            assert!(may_hold_more_places(value_type, text), "{text}");
        }
        for (value_type, text) in [
            (Type::Integer, "0"),
            (Type::Integer, "-1"),
            (Type::String, "255.255.255.255"),
            (Type::String, "POLYGON EMPTY"),
            (Type::String, "Unknown"),
            (Type::String, "Unknown (none)"),
        ] {
            assert!(!may_hold_more_places(value_type, text), "{text}");
        }
    }

    #[test]
    fn each_property_gives_as_many_values_as_it_counts_and_its_default_among_them() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping property values");
            return;
        };
        let manager = open_found(&data_file, PerformanceProfile::LowMemory);
        let properties = manager
            .data_file_properties()
            .expect("the data file's properties should be read");
        for property in &properties {
            // Read up to a thousand, as an Enterprise property can hold
            // millions.
            let shown = property.value_count.min(1000);
            let values = manager
                .data_file_property_value_range(property.index, 0..shown)
                .expect("the values should be read");
            assert_eq!(values.len() as u32, shown, "{}", property.name);
            if let Some(default) = &property.default_value {
                let found = manager
                    .data_file_property_value(property.index, &default.name)
                    .expect("the default should be searched for");
                if found.is_some() || !may_hold_more_places(property.value_type, &default.name) {
                    assert_eq!(found.as_ref(), Some(default), "{}", property.name);
                }
            }
            let past_end = manager
                .data_file_property_value_range(property.index, property.value_count..u32::MAX)
                .expect("a range past the end should be read");
            assert!(past_end.is_empty(), "{}", property.name);
        }

        let asn = file_property(&properties, "Asn");
        let first: Vec<String> = manager
            .data_file_property_value_range(asn.index, 0..3)
            .expect("the values should be read")
            .into_iter()
            .map(|value| value.name)
            .collect();
        let all = manager
            .data_file_property_values(asn.index)
            .expect("the values should be read");
        assert_eq!(all.len() as u32, asn.value_count);
        let all_first: Vec<&str> = all[..3].iter().map(|value| value.name.as_str()).collect();
        assert_eq!(first, all_first);
        // The ASN file in the data repository changes only when the submodule
        // moves, so its first values are known. Other files gain values.
        if data_file
            .file_name()
            .is_some_and(|name| name == ASN_FILE_NAME)
        {
            assert_eq!(first, ["AS0", "AS1", "AS1000"]);
        }
        assert_eq!(
            manager
                .data_file_property_value_range(asn.index, 1..3)
                .expect("the values should be read"),
            all[1..3]
        );

        let outside = properties.len() as u32;
        assert!(manager
            .data_file_property_values(outside)
            .expect("an index outside the file gives no values")
            .is_empty());
        assert_eq!(
            manager
                .data_file_property_value(outside, "AS13335")
                .expect("an index outside the file gives no value"),
            None
        );
    }

    #[test]
    fn a_value_is_found_by_its_exact_text() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping value search");
            return;
        };
        let manager = open_found(&data_file, PerformanceProfile::LowMemory);
        let properties = manager
            .data_file_properties()
            .expect("the data file's properties should be read");
        let asn = file_property(&properties, "Asn");
        let found = manager
            .data_file_property_value(asn.index, "AS13335")
            .expect("the value should be searched for")
            .expect("the data file holds AS13335");
        assert_eq!(found.name, "AS13335");
        for missing in ["as13335", "NotAValue", "AS13335 "] {
            assert_eq!(
                manager
                    .data_file_property_value(asn.index, missing)
                    .expect("the value should be searched for"),
                None,
                "{missing}"
            );
        }
    }

    #[test]
    fn a_name_is_searched_up_to_its_first_null() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping value search");
            return;
        };
        let manager = open_found(&data_file, PerformanceProfile::LowMemory);
        let properties = manager
            .data_file_properties()
            .expect("the data file's properties should be read");
        let asn = file_property(&properties, "Asn");
        let search = |name: &str| {
            manager
                .data_file_property_value(asn.index, name)
                .expect("the value should be searched for")
                .map(|value| value.name)
        };
        // .NET's GetValue gives AS13335 for each, because the C++ search
        // ends at the null.
        for name in ["AS13335\0", "AS13335\0x", "AS13335\0AS15169"] {
            assert_eq!(search(name).as_deref(), Some("AS13335"), "{name:?}");
        }
        assert_eq!(search("NotAValue\0AS13335"), None);
    }

    #[test]
    fn each_property_belongs_to_one_of_the_data_files_components() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping components");
            return;
        };
        let manager = open_found(&data_file, PerformanceProfile::LowMemory);
        let components = manager
            .data_file_components()
            .expect("the data file's components should be read");
        assert!(!components.is_empty());
        let mut ids: Vec<u8> = components.iter().map(|c| c.component_id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), components.len(), "{components:?}");
        for component in &components {
            assert!(!component.name.is_empty(), "{components:?}");
        }
        assert!(components.iter().any(|c| c.name == "Asn"), "{components:?}");
        for property in manager
            .data_file_properties()
            .expect("the data file's properties should be read")
        {
            assert!(
                ids.contains(&property.component_id),
                "{} has component {}",
                property.name,
                property.component_id
            );
        }
    }

    /// Set where the Enterprise file must be given, as in the comparison with
    /// .NET, so a test that needs it fails rather than skips.
    const REQUIRE_ENTERPRISE: &str = "_51DEGREES_REQUIRE_ENTERPRISE";

    /// The length of the buffer the shim gives the C library for a value's
    /// text, `SHIM_TEXT_BUFFER_LENGTH` in `shim.c`.
    const SHIM_TEXT_BUFFER_LENGTH: usize = 256;

    /// Skip `what` for `reason`, or fail when [`REQUIRE_ENTERPRISE`] is set.
    fn skip_without_enterprise(reason: &str, what: &str) {
        assert!(
            std::env::var_os(REQUIRE_ENTERPRISE).is_none(),
            "{REQUIRE_ENTERPRISE} is set, but {reason}"
        );
        eprintln!("{reason}; skipping {what}");
    }

    /// The Enterprise file `_51DEGREES_IPI_PATH` names, opened in LowMemory,
    /// and its properties, or [`None`] after skipping `what` when there is
    /// none.
    fn enterprise_file(what: &str) -> Option<(Arc<Manager>, Vec<DataFileProperty>)> {
        let Some(data_file) = ipi_data_file() else {
            skip_without_enterprise("no IP Intelligence data file found", what);
            return None;
        };
        let manager = open_found(&data_file, PerformanceProfile::LowMemory);
        let properties = manager
            .data_file_properties()
            .expect("the data file's properties should be read");
        if !properties
            .iter()
            .any(|property| property.name == "Latitude")
        {
            let reason = format!("{} is not an Enterprise file", data_file.display());
            skip_without_enterprise(&reason, what);
            return None;
        }
        Some((manager, properties))
    }

    /// The types of the properties an Enterprise file defines that are not
    /// strings, as the data file names them.
    #[test]
    fn an_enterprise_file_names_each_type() {
        let Some((_, properties)) = enterprise_file("types") else {
            return;
        };
        for (name, value_type) in [
            ("Latitude", PropertyValueType::Float),
            ("IsTor", PropertyValueType::Bool),
            ("AccuracyRadiusMax", PropertyValueType::Integer),
            ("CountryCodesGeographical", PropertyValueType::KeyValueList),
            ("IpRangeStart", PropertyValueType::String),
            ("Areas", PropertyValueType::WktString),
        ] {
            assert_eq!(
                file_property(&properties, name).value_type,
                value_type,
                "{name}"
            );
        }
    }

    /// An Enterprise file's values are given as text as .NET gives them. An
    /// IP range starts at an address, a coordinate has at most 3 decimal
    /// places, and an area is WKT text. The first area is longer than the
    /// shim's buffer, so the C library writes it into a block it allocates.
    #[test]
    fn an_enterprise_file_gives_each_value_as_text() {
        let Some((manager, properties)) = enterprise_file("value text") else {
            return;
        };
        let first = |name: &str, count: u32| -> Vec<String> {
            let property = file_property(&properties, name);
            let values = manager
                .data_file_property_value_range(property.index, 0..count)
                .expect("the values should be read");
            assert_eq!(values.len() as u32, count, "{name}");
            values.into_iter().map(|value| value.name).collect()
        };

        let area = &first("Areas", 1)[0];
        assert!(area.len() > SHIM_TEXT_BUFFER_LENGTH, "{area}");
        let keyword = area.split('(').next().unwrap_or_default();
        assert!(
            ["POLYGON", "MULTIPOLYGON", "GEOMETRYCOLLECTION"].contains(&keyword),
            "{area}"
        );
        assert!(area.ends_with(')'), "{area}");

        for start in first("IpRangeStart", 10) {
            assert!(start.parse::<std::net::IpAddr>().is_ok(), "{start}");
        }
        for name in ["Latitude", "Longitude"] {
            for text in first(name, 100) {
                assert!(text.parse::<f64>().is_ok(), "{name} {text}");
                let places = text.split_once('.').map_or(0, |(_, places)| places.len());
                assert!(places <= 3, "{name} {text}");
            }
        }
    }

    /// Each address is written as .NET's `IPAddress.ToString` writes it.
    #[test]
    fn an_address_is_written_as_dotnet_writes_it() {
        for (given, written) in [
            ("185.28.167.0", "185.28.167.0"),
            ("0.0.0.0", "0.0.0.0"),
            ("2a00:23c0:0000:0000:0000:0000:0000:0000", "2a00:23c0::"),
            ("2A00:23C0::", "2a00:23c0::"),
            ("2a00:23c5:a1a6:a01::1", "2a00:23c5:a1a6:a01::1"),
            (
                "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
                "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
            ),
            ("2001:db8:0:0:1:0:0:1", "2001:db8::1:0:0:1"),
            ("2001:0:0:1:0:0:0:1", "2001:0:0:1::1"),
            ("1:0:0:2:0:0:0:3", "1:0:0:2::3"),
            ("0:0:0:1:0:0:0:0", "0:0:0:1::"),
            ("::", "::"),
            ("::1", "::1"),
            ("::8.8.8.8", "::8.8.8.8"),
            ("::0.0.1.2", "::102"),
            ("::ffff:8.8.8.8", "::ffff:8.8.8.8"),
            ("::ffff:0.0.1.2", "::ffff:0:102"),
            ("::ffff:0:0", "::ffff:0:0"),
            ("::ffff:0:8.8.8.8", "::ffff:0:8.8.8.8"),
            ("::ffff:0:0.0.1.2", "::ffff:0:0:102"),
            ("0:0:0:0:ffff:0:0:0", "::ffff:0:0:0"),
            ("fe80::5efe:8.8.8.8", "fe80::5efe:8.8.8.8"),
            ("fe80::5efe:0.0.0.1", "fe80::5efe:0.0.0.1"),
            ("::5efe:0.0.0.0", "::5efe:0.0.0.0"),
            ("2001:db8:1:2:0:5efe:1.2.3.4", "2001:db8:1:2:0:5efe:1.2.3.4"),
            ("2001:db8::200:5efe:1.2.3.4", "2001:db8::200:5efe:102:304"),
            ("64:ff9b::8.8.8.8", "64:ff9b::808:808"),
        ] {
            let address: IpAddr = given.parse().expect("an address");
            assert_eq!(ip_address_text(address), written, "{given}");
        }
        assert_eq!(address_text_of(String::from("N/A")), "N/A");
    }

    /// An Enterprise file's IP ranges are written as .NET writes them, so an
    /// IPv6 range the C library writes with every field in full is
    /// compressed.
    #[test]
    fn an_ip_range_is_written_as_dotnet_writes_it() {
        let Some((manager, _)) = enterprise_file("IP range text") else {
            return;
        };
        let mut compressed = 0;
        for address in ["185.28.167.77", "2a00:23c5:a1a6:a01::1"] {
            let mut results = manager.create_results().expect("results allocate");
            results
                .process_ip(address)
                .expect("the lookup should succeed");
            for name in ["IpRangeStart", "IpRangeEnd"] {
                let written = results
                    .value_as_string(name, "|")
                    .expect("the C library's text reads")
                    .unwrap_or_else(|| panic!("{name} for {address} has a value"));
                // The C library writes each value as `"text":weighting`.
                let native = written
                    .strip_prefix('"')
                    .and_then(|rest| rest.rsplit_once('"'))
                    .map(|(text, _)| text)
                    .unwrap_or_else(|| panic!("{name} for {address}: {written}"));
                let values = results.values_weighted(name).expect("the values read");
                let [(text, _)] = values.as_slice() else {
                    panic!("{name} for {address} has one value: {values:?}");
                };
                let range: IpAddr = native
                    .parse()
                    .unwrap_or_else(|_| panic!("{name} for {address}: {native}"));
                assert_eq!(*text, ip_address_text(range), "{name} for {address}");
                compressed += usize::from(*text != native);
            }
        }
        assert!(
            compressed > 0,
            "the C library wrote every range as .NET does"
        );
    }

    /// The text the C library writes for `value` to `places` decimal places,
    /// more than six, one place at a time as `addDoubleByPlace` in
    /// `stringBuilder.c` writes it, with trailing zeros left out.
    fn written_by_place(value: f64, places: usize) -> String {
        // `(int)value` in C, which truncates toward zero.
        let whole = value as i32;
        let mut fraction = value - f64::from(whole);
        let mut sign = "";
        if fraction < 0.0 {
            if whole == 0 {
                sign = "-";
            }
            fraction = -fraction;
        }
        let mut digits: Vec<u8> = Vec::with_capacity(places);
        while digits.len() < places && fraction != 0.0 {
            fraction *= 10.0;
            // `(char)fracPart` in C, which truncates.
            let digit = fraction as u8;
            fraction -= f64::from(digit);
            digits.push(digit);
            if digits.len() == places && fraction >= 0.5 {
                while digits.last() == Some(&9) {
                    digits.pop();
                }
                let Some(last) = digits.last_mut() else {
                    let away = if value < 0.0 { whole - 1 } else { whole + 1 };
                    return away.to_string();
                };
                *last += 1;
                break;
            }
        }
        while digits.last() == Some(&0) {
            digits.pop();
        }
        if digits.is_empty() {
            return whole.to_string();
        }
        let digits: String = digits
            .iter()
            .map(|digit| char::from(b'0' + digit))
            .collect();
        format!("{sign}{whole}.{digits}")
    }

    /// A value is found by its text to more places than it is given with, as
    /// the C++ code finds it, and given with its own text. An Enterprise file
    /// stores a latitude as a whole number of steps of 90 / 32767 degrees, so
    /// the text to 3 places gives the number, which the test writes to 15
    /// places as the C library writes it. The value's own text is compared
    /// with each value written to as many places as the text has characters
    /// plus three, so it finds nothing, as in the C++ code.
    #[test]
    fn a_value_is_found_by_its_text_to_more_places() {
        let Some((manager, properties)) = enterprise_file("values found by more places") else {
            return;
        };
        let latitude = file_property(&properties, "Latitude");
        let values = manager
            .data_file_property_value_range(latitude.index, 0..100)
            .expect("the values should be read");
        assert_eq!(values.len(), 100);
        for value in &values {
            let steps = (value.name.parse::<f64>().expect("a number") * 32767.0 / 90.0).round();
            let longer = written_by_place(steps * 90.0 / 32767.0, 15);
            assert_ne!(longer, value.name);
            let found = manager
                .data_file_property_value(latitude.index, &longer)
                .expect("the value should be searched for");
            assert_eq!(found.as_ref(), Some(value), "{longer}");
            let own = manager
                .data_file_property_value(latitude.index, &value.name)
                .expect("the value should be searched for");
            assert_eq!(own, None, "{}", value.name);
        }
    }

    /// Places in an IP Intelligence data file, from the packed
    /// `fiftyoneDegreesDataSetIpiHeader` in the C library's `ipi.h` and
    /// `fiftyoneDegreesProperty` in `property.h`. A collection's header is
    /// its start in eight bytes, then its length and its count in four each,
    /// and a string is its length in two bytes, then its characters.
    const NAME_HEADER: usize = 58;
    const STRINGS_HEADER: usize = 74;
    const MAPS_HEADER: usize = 106;
    const PROPERTIES_HEADER: usize = 122;
    const MAP_COUNT: usize = 36;
    const FIRST_MAP_INDEX: usize = 40;

    fn read_u32(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
    }

    fn read_start(bytes: &[u8], header: usize) -> usize {
        let start = u64::from_le_bytes(bytes[header..header + 8].try_into().expect("eight bytes"));
        usize::try_from(start).expect("the collection starts in memory")
    }

    /// A data tier with an empty name is kept in its place, as the C++
    /// `PropertyMetaDataBuilderIpi` keeps every name it reads. A copy of the
    /// ASN file has the first character of the first property's first tier
    /// other than the file's own made a null, so every property in that
    /// tier lists an empty name there.
    #[test]
    fn an_empty_data_tier_name_is_kept() {
        let Some(data_file) = asn_file() else {
            eprintln!("no ASN data file found; skipping an empty tier name");
            return;
        };
        let mut bytes = std::fs::read(&data_file).expect("the data file is read");
        let strings = read_start(&bytes, STRINGS_HEADER);
        let maps = read_start(&bytes, MAPS_HEADER);
        let first_property = read_start(&bytes, PROPERTIES_HEADER);
        let own_name = read_u32(&bytes, NAME_HEADER);
        let first_map = read_u32(&bytes, first_property + FIRST_MAP_INDEX);
        let map_count = read_u32(&bytes, first_property + MAP_COUNT);
        let tier = (first_map..first_map + map_count)
            .map(|map| read_u32(&bytes, maps + 4 * map as usize))
            .find(|offset| *offset != own_name)
            .expect("the first property is in a tier other than the file's own")
            as usize;
        let at = strings + tier;
        let length = u16::from_le_bytes([bytes[at], bytes[at + 1]]) as usize;
        let name = String::from_utf8(bytes[at + 2..at + 1 + length].to_vec())
            .expect("the tier's name is text");
        bytes[at + 2] = 0;

        let original = open_found(&data_file, PerformanceProfile::LowMemory)
            .data_file_properties()
            .expect("the data file's properties should be read");
        let mut expected = original.clone();
        for property in &mut expected {
            for listed in &mut property.data_tiers {
                if *listed == name {
                    listed.clear();
                }
            }
        }
        assert_ne!(expected, original, "a property is in the {name} tier");

        let dir = std::env::temp_dir().join(format!(
            "fiftyone-native-ipi-empty-tier-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the folder is made");
        let copy = dir.join("51Degrees.ipi");
        std::fs::write(&copy, &bytes).expect("the copy is written");
        for profile in METADATA_PROFILES {
            let manager = open_found(&copy, profile);
            let properties = manager
                .data_file_properties()
                .expect("the copy's properties should be read");
            assert_eq!(properties, expected, "{profile:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Calls a shim string reader with a buffer that starts full, and gives
    /// the length it returns, or [`None`] for `SHIM_STRING_FAILED`, checking
    /// that a failed read leaves the buffer empty.
    fn shim_string_length(read: impl FnOnce(*mut c_char, usize) -> usize) -> Option<usize> {
        let mut buffer = [b'x'; 64];
        let length = read(buffer.as_mut_ptr() as *mut c_char, buffer.len());
        if length == sys::SHIM_STRING_FAILED {
            assert_eq!(buffer[0], 0, "a failed read leaves the buffer empty");
            return None;
        }
        Some(length)
    }

    #[test]
    fn each_shim_string_reader_fails_for_an_index_past_the_end() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping string readers");
            return;
        };
        let manager = open_found(&data_file, PerformanceProfile::LowMemory);
        let count = manager.data_file_property_count();
        let fields = [
            sys::SHIM_PROPERTY_NAME,
            sys::SHIM_PROPERTY_DESCRIPTION,
            sys::SHIM_PROPERTY_CATEGORY,
            sys::SHIM_PROPERTY_URL,
        ];
        for field in fields {
            // Safety: the buffer is writable for the length given.
            let read = |index| {
                shim_string_length(|buffer, length| unsafe {
                    sys::fiftyoneDegreesShimIpiGetDataFilePropertyString(
                        manager.as_ptr(),
                        index,
                        field,
                        buffer,
                        length,
                    )
                })
            };
            assert!(read(0).is_some(), "field {field}");
            assert_eq!(read(count), None, "field {field}");
        }
        // Safety: the buffer is writable for the length given.
        let field_past_url = shim_string_length(|buffer, length| unsafe {
            sys::fiftyoneDegreesShimIpiGetDataFilePropertyString(
                manager.as_ptr(),
                0,
                sys::SHIM_PROPERTY_URL + 1,
                buffer,
                length,
            )
        });
        assert_eq!(field_past_url, None);
        // Safety: the buffer is writable for the length given.
        let map = |property, map| {
            shim_string_length(|buffer, length| unsafe {
                sys::fiftyoneDegreesShimIpiGetDataFilePropertyMap(
                    manager.as_ptr(),
                    property,
                    map,
                    buffer,
                    length,
                )
            })
        };
        assert!(map(0, 0).is_some());
        assert_eq!(map(count, 0), None);
        assert_eq!(map(0, u32::MAX), None);
        // Safety: the manager is initialized.
        let components = unsafe { sys::fiftyoneDegreesShimIpiGetComponentCount(manager.as_ptr()) };
        // Safety: the buffer is writable for the length given.
        let component = |index| {
            shim_string_length(|buffer, length| unsafe {
                sys::fiftyoneDegreesShimIpiGetComponentName(manager.as_ptr(), index, buffer, length)
            })
        };
        assert!(component(0).is_some());
        assert_eq!(component(components), None);
        let (mut id, mut offset, mut has) = (0u8, 0u32, 0u8);
        // Safety: every output pointer is a writable local.
        let read = unsafe {
            sys::fiftyoneDegreesShimIpiGetComponent(
                manager.as_ptr(),
                components,
                &mut id,
                &mut offset,
                &mut has,
            )
        };
        assert_eq!(read, 0);
    }
}
