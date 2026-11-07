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

mod profiles;

pub use profiles::DataFileProfile;

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::ops::Range;
use std::os::raw::{c_char, c_int, c_long, c_void};
use std::path::Path;
use std::ptr::NonNull;
use std::sync::{Arc, OnceLock};

use fiftyone_device_detection_sys as sys;
use fiftyone_pipeline_core::{constants, Error, PropertyValueType, Result};

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
/// For the `LowMemory`, `Balanced` and `Default` profiles the collections are
/// read from file through a fixed-size handle pool, so the pool must be sized
/// for the number of threads that will use the data set concurrently. Every
/// profile but `InMemory` opens the pool, `HighPerformance` included (see
/// [`Manager::file_pool_size`]). This mirrors the
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

/// Settings that change a performance profile's native configuration before
/// a Hash data set loads, as the builders of the other 51Degrees ports set
/// them. Each is [`None`] to keep the profile's own value. Start from
/// [`HashOptions::default`] and set the fields wanted.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct HashOptions {
    /// The expected number of threads processing through the data set at
    /// once, which sizes the caches of the profiles that read from the file
    /// and the file handle pool. See [`Manager::open_with_options`].
    pub concurrency: Option<u16>,
    /// The most difference in hash value a match may have. The profiles'
    /// own value is 0.
    pub difference: Option<i32>,
    /// The most drift, in character positions, a match may have. The
    /// profiles' own value is 0.
    pub drift: Option<i32>,
    /// Whether a detection that matches nothing gives each component's
    /// default profile rather than no profile. The profiles' own value is
    /// false.
    pub allow_unmatched: Option<bool>,
    /// Whether a detection records the characters of each User-Agent it
    /// matched. The profiles' own value is true.
    pub update_matched_user_agent: Option<bool>,
    /// Whether detection tries the performance graph. The profiles' own
    /// value is false.
    pub use_performance_graph: Option<bool>,
    /// Whether detection tries the predictive graph, after the performance
    /// graph when both are used. The profiles' own value is true.
    pub use_predictive_graph: Option<bool>,
}

impl HashOptions {
    /// Write each setting that is not [`None`] into `config`.
    fn apply(&self, config: &mut sys::ConfigHash) {
        if let Some(concurrency) = self.concurrency {
            set_pool_concurrency(config, concurrency);
        }
        if let Some(difference) = self.difference {
            config.difference = difference;
        }
        if let Some(drift) = self.drift {
            config.drift = drift;
        }
        if let Some(allow) = self.allow_unmatched {
            config.b.allow_unmatched = allow;
        }
        if let Some(update) = self.update_matched_user_agent {
            config.b.update_matched_user_agent = update;
        }
        if let Some(use_graph) = self.use_performance_graph {
            config.use_performance_graph = use_graph;
        }
        if let Some(use_graph) = self.use_predictive_graph {
            config.use_predictive_graph = use_graph;
        }
    }
}

/// The most expected concurrency `profile` loads with, or [`None`] when any
/// concurrency loads. A collection read from the file through a cache splits
/// the cache into one part for each concurrent operation, and the C library
/// refuses a part smaller than the number of parts, so the smallest such
/// cache sets the limit (its capacity's whole square root).
fn most_concurrency(profile: PerformanceProfile) -> Option<u16> {
    // Safety: `config_for` returns a valid pointer to an initialized config
    // global, and copying out of it is a plain read of a `repr(C)` value.
    let config = unsafe { *config_for(profile) };
    [
        config.strings,
        config.components,
        config.maps,
        config.properties,
        config.values,
        config.profiles,
        config.root_nodes,
        config.nodes,
        config.profile_offsets,
    ]
    .into_iter()
    .filter(|collection| !collection.loaded && collection.capacity > 0)
    .map(|collection| u16::try_from(collection.capacity.isqrt()).unwrap_or(u16::MAX))
    .min()
}

/// The expected concurrency a Hash engine opens `profile` with when none is
/// named, which the specification gives as the number of processors. It is
/// held at the most the profile's caches take, so the profile still loads on
/// a machine with more processors (22 under `Balanced` and `Default`, whose
/// values cache holds 500 entries). `LowMemory` and `HighPerformance` have no
/// limit, so their file handle pool has one handle for each processor (see
/// [`Manager::file_pool_size`]).
pub fn default_concurrency(profile: PerformanceProfile) -> u16 {
    let processors = std::thread::available_parallelism()
        .map_or(1, |count| u16::try_from(count.get()).unwrap_or(u16::MAX));
    concurrency_within(processors, profile)
}

/// `concurrency`, or the most `profile` loads with when that is less.
fn concurrency_within(concurrency: u16, profile: PerformanceProfile) -> u16 {
    most_concurrency(profile).map_or(concurrency, |most| concurrency.min(most))
}

/// A copy of `profile`'s predefined configuration changed by `options`, so a
/// data set's options never change the shared global the other data sets read.
fn profile_config(profile: PerformanceProfile, options: &HashOptions) -> sys::ConfigHash {
    // Safety: `config_for` returns a valid pointer to an initialized config
    // global, and copying out of it is a plain read of a `repr(C)` value.
    let mut config = unsafe { *config_for(profile) };
    options.apply(&mut config);
    config
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
    /// The data a load from memory reads in place. Fields drop after
    /// [`Drop::drop`] has freed the data set, so the data outlives it.
    _memory: Option<Box<[u8]>>,
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
    /// under the `LowMemory`, `Balanced` and `Default` profiles, which is how
    /// many calls can read the data file at once. A call beyond that waits for
    /// a handle, so it should be the number of threads that will process
    /// through the data set at once. [`None`] keeps the profile's default.
    /// `HighPerformance` holds every collection in memory, so detection never
    /// waits on the pool, but it still opens one handle for each concurrent
    /// operation and keeps them open until the data set is freed (see
    /// [`Manager::file_pool_size`]). `InMemory` opens no pool.
    pub fn open_with_options(
        path: impl AsRef<Path>,
        profile: PerformanceProfile,
        properties: Option<&[&str]>,
        concurrency: Option<u16>,
    ) -> Result<Arc<Manager>> {
        let options = HashOptions {
            concurrency,
            ..HashOptions::default()
        };
        Self::open_with_hash_options(path, profile, properties, &options)
    }

    /// Open a Hash data file with the named properties, changing the
    /// profile's native configuration by `options` first.
    pub fn open_with_hash_options(
        path: impl AsRef<Path>,
        profile: PerformanceProfile,
        properties: Option<&[&str]>,
        options: &HashOptions,
    ) -> Result<Arc<Manager>> {
        let path = path.as_ref();
        let path_c = crate::path::native_path(path)?;
        let mut config = profile_config(profile, options);
        Self::init(
            properties,
            None,
            |manager, required, exception| {
                // Safety: every pointer is valid for the call, the config is
                // read only for the call and the path is null terminated.
                unsafe {
                    sys::fiftyoneDegreesHashInitManagerFromFile(
                        manager,
                        &mut config,
                        required,
                        path_c.as_ptr(),
                        exception,
                    )
                }
            },
            |status| status_detail(status, path_c.as_ptr()),
        )
    }

    /// Load a Hash data set from `data`, the whole of a data file held in
    /// memory, with the named properties, changing the profile's native
    /// configuration by `options` first.
    ///
    /// The data set reads its collections from `data` where it lies, so the
    /// manager keeps `data` until the data set is freed. Every profile holds
    /// the data set in memory this way, so none opens a file pool. The C
    /// library takes the length as a C `long`, so where that is 32 bits, as on
    /// Windows, `data` must be shorter than 2 GiB.
    pub fn open_from_memory(
        data: Vec<u8>,
        profile: PerformanceProfile,
        properties: Option<&[&str]>,
        options: &HashOptions,
    ) -> Result<Arc<Manager>> {
        let mut memory = data.into_boxed_slice();
        let size = c_long::try_from(memory.len()).map_err(|_| Error::Native {
            status: String::from("InvalidInput"),
            message: format!(
                "the data is {} bytes, more than the C library reads from \
                 memory on this platform",
                memory.len()
            ),
        })?;
        let mut config = profile_config(profile, options);
        // The memory is Rust's and is dropped with the manager, so the C
        // library must not free it, and there is no file to copy.
        config.b.b.free_data = false;
        config.b.b.use_temp_file = false;
        let start = memory.as_mut_ptr().cast::<c_void>();
        Self::init(
            properties,
            Some(memory),
            |manager, required, exception| {
                // Safety: every pointer is valid for the call, the config is
                // read only for the call, and `start` is the first of `size`
                // bytes that the manager keeps for as long as the data set.
                unsafe {
                    sys::fiftyoneDegreesHashInitManagerFromMemory(
                        manager,
                        &mut config,
                        required,
                        start,
                        size,
                        exception,
                    )
                }
            },
            // The C++ engine names no file for a load from memory.
            |status| status_detail(status, std::ptr::null()),
        )
    }

    /// Initialize a manager with `init`, requesting the named properties, or
    /// all of them when `properties` is [`None`]. `memory` is the data a load
    /// from memory reads, kept until the data set is freed, and `detail`
    /// gives the message for a failed status.
    fn init(
        properties: Option<&[&str]>,
        memory: Option<Box<[u8]>>,
        init: impl FnOnce(
            *mut sys::ResourceManager,
            *mut sys::PropertiesRequired,
            *mut sys::Exception,
        ) -> sys::StatusCode,
        detail: impl FnOnce(sys::StatusCode) -> String,
    ) -> Result<Arc<Manager>> {
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

        let mut exception = sys::Exception::cleared();
        let status = init(manager_ptr, &mut required, &mut exception);

        if let Err(err) = status_to_result(status, || detail(status)) {
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
            _memory: memory,
        }))
    }

    /// The date the data file was published, from its header, as `(year,
    /// month, day)`, or [`None`] when the header holds no valid date.
    pub fn data_set_published(&self) -> Option<(i32, u32, u32)> {
        self.header_date(sys::fiftyoneDegreesShimHashGetDataSetPublished)
    }

    /// The date the next data file is expected, from the data file header,
    /// as `(year, month, day)`, or [`None`] when the header holds no valid
    /// date.
    pub fn data_set_next_update(&self) -> Option<(i32, u32, u32)> {
        self.header_date(sys::fiftyoneDegreesShimHashGetDataSetNextUpdate)
    }

    fn header_date(
        &self,
        read: unsafe extern "C" fn(
            *mut sys::ResourceManager,
            *mut i32,
            *mut i32,
            *mut i32,
        ) -> c_int,
    ) -> Option<(i32, u32, u32)> {
        let (mut year, mut month, mut day) = (0, 0, 0);
        // Safety: the manager is initialized and each output pointer is
        // writable.
        let found = unsafe { read(self.as_ptr(), &mut year, &mut month, &mut day) };
        if found == 0 || year <= 0 || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return None;
        }
        Some((year, u32::try_from(month).ok()?, u32::try_from(day).ok()?))
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

    /// The number of handles to the data file that the data set opened when
    /// it loaded, which stay open until it is freed. Every profile but
    /// `InMemory` opens one for each concurrent operation it expects, the
    /// largest concurrency of its collections, `HighPerformance` included,
    /// although that profile holds every collection in memory and never
    /// reads through them. `InMemory` opens none.
    pub fn file_pool_size(&self) -> u16 {
        // Safety: the manager is initialized for the lifetime of `self`.
        unsafe { sys::fiftyoneDegreesShimHashGetFilePoolSize(self.as_ptr()) }
    }

    /// The number of required properties in the loaded data set, which is the
    /// number of names it was opened with, or the number of its properties
    /// when it was opened naming none.
    pub fn property_count(&self) -> u32 {
        // Safety: the manager is initialized for the lifetime of `self`.
        unsafe { sys::fiftyoneDegreesShimHashGetRequiredPropertyCount(self.as_ptr()) }
    }

    /// The names of the required properties, in index order. With properties
    /// named, these are the names given, in the order given, including any the
    /// data set has no property for.
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

/// The length of the buffer a metadata string is first read into. A longer
/// string is read again into a buffer that fits it.
const METADATA_STRING_BUFFER_LENGTH: usize = 256;

/// The metadata the data file holds for one of its properties, as the C++
/// `PropertyMetaDataBuilderHash` in 51degrees-cxx reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DataFileProperty {
    /// The property's place among the data file's properties, which
    /// [`Manager::data_file_property_values`] takes.
    pub index: u32,
    /// The property's name.
    pub name: String,
    /// The type of the property's values. A string property that can hold
    /// several values is a [`PropertyValueType::StringList`].
    pub value_type: PropertyValueType,
    /// True when the data set was opened with the property, so results can
    /// hold its values.
    pub available: bool,
    /// The index [`Results::value_as_string_by_index`] takes for the
    /// property, or [`None`] when results do not hold it. The C library finds
    /// the property among the names the data set was opened with ignoring
    /// case, so this is found for a property named in another case.
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
    /// order.
    pub data_tiers: Vec<String>,
    /// The property's default value, which a profile with no other value for
    /// the property has, or [`None`] when it has none.
    pub default_value: Option<DataFileValue>,
    /// The number of values the data file holds for the property. It is 0
    /// when the property's value indexes are -1 or reach past the data
    /// file's values, as the C library counts them.
    pub value_count: u32,
}

/// The metadata the data file holds for one of its components, such as the
/// hardware or the browser.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DataFileComponent {
    /// The component's id, which stays the same from one data file to the
    /// next.
    pub component_id: u8,
    /// The component's name.
    pub name: String,
    /// The profile id of the component's default profile.
    pub default_profile_id: u32,
}

/// The metadata the data file holds for one value of a property.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DataFileValue {
    /// The value as a string.
    pub name: String,
    /// The value's description, or empty when it has none.
    pub description: String,
    /// A URL with more about the value, or empty when it has none.
    pub url: String,
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
        let _file_read = self.file_read();
        // Safety: the manager is initialized for the lifetime of `self`.
        let count = unsafe { sys::fiftyoneDegreesShimHashGetDataFilePropertyCount(self.as_ptr()) };
        (0..count)
            .map(|index| {
                self.read_data_file_property(index)
                    .ok_or_else(|| metadata_read_failed(&format!("property {index}")))
            })
            .collect()
    }

    /// The components the data file defines, in the data file's order. A
    /// default profile or a name that cannot be read is an error, as the C++
    /// builder throws for a default profile it cannot read.
    pub fn data_file_components(&self) -> Result<Vec<DataFileComponent>> {
        let _file_read = self.file_read();
        // Safety: the manager is initialized for the lifetime of `self`.
        let count = unsafe { sys::fiftyoneDegreesShimHashGetComponentCount(self.as_ptr()) };
        let mut components = Vec::with_capacity(count as usize);
        for index in 0..count {
            let mut component_id = 0u8;
            let mut default_profile_id = 0u32;
            // Safety: both output pointers are writable locals.
            let read = unsafe {
                sys::fiftyoneDegreesShimHashGetComponent(
                    self.as_ptr(),
                    index,
                    &mut component_id,
                    &mut default_profile_id,
                )
            };
            if read == 0 {
                return Err(metadata_read_failed(&format!(
                    "the default profile of component {index}"
                )));
            }
            // Safety: the buffer is writable for the length given.
            let name = read_metadata_string(|buffer, length| unsafe {
                sys::fiftyoneDegreesShimHashGetComponentName(self.as_ptr(), index, buffer, length)
            })
            .ok_or_else(|| metadata_read_failed(&format!("component {index}")))?;
            components.push(DataFileComponent {
                component_id,
                name,
                default_profile_id,
            });
        }
        Ok(components)
    }

    /// The values the data file holds for the property at `property_index`
    /// among [`Self::data_file_properties`], in the data file's order, or none
    /// when the index is out of range. A property or value that cannot be
    /// read is an error.
    ///
    /// The values are read from the data set each time, because a data file
    /// can hold hundreds of thousands of them, and
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
        let _file_read = self.file_read();
        // Safety: the manager is initialized for the lifetime of `self`.
        let count = unsafe { sys::fiftyoneDegreesShimHashGetDataFilePropertyCount(self.as_ptr()) };
        if property_index >= count {
            return Ok(Vec::new());
        }
        self.read_values(property_index, range)
            .ok_or_else(|| metadata_read_failed(&format!("values of property {property_index}")))
    }

    /// The value named `name`, matched exactly, among those
    /// [`Self::data_file_property_values`] gives for the property at
    /// `property_index`, or [`None`] when the index is out of range or the
    /// property has no such value. A property or value that cannot be read is
    /// an error.
    ///
    /// A property's values are in order of their names, so the value is found
    /// by the C library's binary search, as the C++ code the other SDKs call
    /// finds it, without reading the others. A value the data file holds out
    /// of that order can be missed, as the C++ misses it.
    pub fn data_file_property_value(
        &self,
        property_index: u32,
        name: &str,
    ) -> Result<Option<DataFileValue>> {
        let _file_read = self.file_read();
        // Safety: the manager is initialized for the lifetime of `self`.
        let count = unsafe { sys::fiftyoneDegreesShimHashGetDataFilePropertyCount(self.as_ptr()) };
        if property_index >= count {
            return Ok(None);
        }
        // A data file's strings end at their first null, so no value has a
        // name holding one.
        let Ok(c_name) = CString::new(name) else {
            return Ok(None);
        };
        let mut values: Vec<DataFileValue> = Vec::new();
        // Safety: `c_name` is null terminated, and `take_value_string` treats
        // the context as the `Vec<DataFileValue>` it points to, which outlives
        // the call and is not used elsewhere during it.
        let found = unsafe {
            sys::fiftyoneDegreesShimHashFindDataFilePropertyValue(
                self.as_ptr(),
                property_index,
                c_name.as_ptr(),
                take_value_string,
                (&mut values as *mut Vec<DataFileValue>).cast::<c_void>(),
            )
        };
        // The C search answers 0, the file's first value, when reading a value
        // gives nothing and sets no exception, so a value of another name is a
        // failure.
        match (found, values.pop()) {
            (sys::SHIM_FIND_FOUND, Some(value)) if values.is_empty() && value.name == name => {
                Ok(Some(value))
            }
            (sys::SHIM_FIND_NOT_FOUND, None) => Ok(None),
            _ => Err(metadata_read_failed(&format!(
                "value {name:?} of property {property_index}"
            ))),
        }
    }

    /// The values at the places `range` covers among those the data file
    /// holds for the property at `property_index`, read in one shim call, or
    /// [`None`] when the property or any of them cannot be read. The caller
    /// holds the file read permit.
    fn read_values(&self, property_index: u32, range: Range<u32>) -> Option<Vec<DataFileValue>> {
        let mut values: Vec<DataFileValue> = Vec::new();
        // Safety: `take_value_string` treats the context as the
        // `Vec<DataFileValue>` it points to, which outlives the call and is
        // not used elsewhere during it.
        let read = unsafe {
            sys::fiftyoneDegreesShimHashGetDataFilePropertyValues(
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
    /// or [`None`] when the index is out of range or the property or its name
    /// cannot be read. The caller holds the file read permit.
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
            sys::fiftyoneDegreesShimHashGetDataFileProperty(
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
    /// of range or any part of it cannot be read. The caller holds the file
    /// read permit.
    fn read_data_file_property(&self, index: u32) -> Option<DataFileProperty> {
        let fields = self.read_property_fields(index)?;
        let string = |field| {
            // Safety: the buffer is writable for the length given.
            read_metadata_string(|buffer, length| unsafe {
                sys::fiftyoneDegreesShimHashGetDataFilePropertyString(
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
                    sys::fiftyoneDegreesShimHashGetDataFilePropertyMap(
                        self.as_ptr(),
                        index,
                        map,
                        buffer,
                        length,
                    )
                })
            })
            .collect::<Option<Vec<String>>>()?
            .into_iter()
            .filter(|tier| !tier.is_empty())
            .collect();
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
/// `fiftyoneDegreesPropertyValueType`, as the C++ `PropertyMetaDataBuilderHash`
/// names it. A type the builder does not name is a string, as there.
fn data_file_value_type(value_type: u8, is_list: bool) -> PropertyValueType {
    use sys::{
        FIFTYONE_DEGREES_PROPERTY_VALUE_TYPE_BOOLEAN as BOOLEAN,
        FIFTYONE_DEGREES_PROPERTY_VALUE_TYPE_DOUBLE as DOUBLE,
        FIFTYONE_DEGREES_PROPERTY_VALUE_TYPE_INTEGER as INTEGER,
        FIFTYONE_DEGREES_PROPERTY_VALUE_TYPE_JAVASCRIPT as JAVASCRIPT,
        FIFTYONE_DEGREES_PROPERTY_VALUE_TYPE_STRING as STRING,
    };
    match value_type {
        INTEGER => PropertyValueType::Integer,
        DOUBLE => PropertyValueType::Double,
        BOOLEAN => PropertyValueType::Bool,
        JAVASCRIPT => PropertyValueType::JavaScript,
        STRING if is_list => PropertyValueType::StringList,
        _ => PropertyValueType::String,
    }
}

/// Take one string of a property value from
/// `fiftyoneDegreesShimHashGetDataFilePropertyValues` or
/// `fiftyoneDegreesShimHashFindDataFilePropertyValue` into the
/// `Vec<DataFileValue>` that `context` points to. The shim gives each value's
/// name first, which starts a new value, then its description and URL.
unsafe extern "C" fn take_value_string(context: *mut c_void, field: c_int, value: *const c_char) {
    // Safety: `Manager::read_values` and `Manager::data_file_property_value`
    // pass their own `Vec<DataFileValue>` as the context, and the shim passes
    // a null terminated string valid for this call.
    let (values, text) = unsafe {
        (
            &mut *context.cast::<Vec<DataFileValue>>(),
            CStr::from_ptr(value).to_string_lossy().into_owned(),
        )
    };
    match field {
        sys::SHIM_VALUE_NAME => values.push(DataFileValue {
            name: text,
            description: String::new(),
            url: String::new(),
        }),
        sys::SHIM_VALUE_DESCRIPTION => {
            if let Some(last) = values.last_mut() {
                last.description = text;
            }
        }
        sys::SHIM_VALUE_URL => {
            if let Some(last) = values.last_mut() {
                last.url = text;
            }
        }
        _ => {}
    }
}

/// Read a metadata string a shim function writes, or [`None`] when the shim
/// could not read it. `read` writes as much of the string as fits into the
/// buffer it is given and returns the string's full length, so a longer
/// string is read again into a buffer that fits it.
fn read_metadata_string(mut read: impl FnMut(*mut c_char, usize) -> usize) -> Option<String> {
    let mut buffer = vec![0u8; METADATA_STRING_BUFFER_LENGTH];
    let mut length = read(buffer.as_mut_ptr() as *mut c_char, buffer.len());
    if length == sys::SHIM_STRING_FAILED {
        return None;
    }
    if length >= buffer.len() {
        buffer = vec![0u8; length + 1];
        length = read(buffer.as_mut_ptr() as *mut c_char, buffer.len());
        if length == sys::SHIM_STRING_FAILED {
            return None;
        }
    }
    let end = length.min(buffer.len() - 1);
    Some(String::from_utf8_lossy(&buffer[..end]).into_owned())
}

/// The error for a part of the data file's metadata that could not be read.
fn metadata_read_failed(what: &str) -> Error {
    Error::Native {
        status: String::from("CollectionFailure"),
        message: format!("the data file's metadata could not be read ({what})"),
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
    /// These are set by the walk of each component's graph and live on the
    /// native results, not in the data file's property values, so they are
    /// read directly here rather than through
    /// [`value_as_string`](Self::value_as_string) (which only resolves real
    /// data-file properties). Any graph not yet walked is walked first, which
    /// can read the data file. Returns [`None`] only when the native results
    /// cannot be read, or a graph cannot be read from the data file.
    pub fn match_metrics(&self) -> Option<MatchMetrics> {
        let mut method = 0;
        let mut difference = 0;
        let mut drift = 0;
        let mut iterations = 0;
        let mut matched_nodes = 0;
        let _file_read = self.manager.file_read();
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
    /// when nothing matched, or a graph cannot be read from the data file.
    /// Any graph not yet walked is walked first, which can read the data file.
    pub fn matched_user_agents(&self) -> Vec<String> {
        let _file_read = self.manager.file_read();
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

/// The message for a failed init, naming the data file at `path`, or `null`
/// when `path` is null, as for data loaded from memory. An exception holds no
/// file name, so its message would say `null` instead.
fn status_detail(status: sys::StatusCode, path: *const c_char) -> String {
    // Safety: the path is null, which the library names "null", or null
    // terminated and outlives the call, and the library allocated the message.
    unsafe {
        take_message::<sys::Exception>(
            sys::fiftyoneDegreesStatusGetMessage(status, path),
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
    pub(super) fn data_file() -> Option<PathBuf> {
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
                    .join("data")
                    .join("device-detection")
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

    /// Every profile but `InMemory` opens a handle to the data file for each
    /// concurrent operation it expects and keeps it open, `HighPerformance`
    /// included, although its reads never use the handles.
    #[test]
    fn the_file_pool_has_a_handle_for_each_expected_operation() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping the file pool");
            return;
        };
        for (profile, concurrency, handles) in [
            (PerformanceProfile::LowMemory, Some(3), 3),
            (PerformanceProfile::Balanced, None, 10),
            (PerformanceProfile::HighPerformance, Some(3), 3),
            (PerformanceProfile::HighPerformance, Some(40), 40),
            (PerformanceProfile::HighPerformance, None, 10),
            (PerformanceProfile::InMemory, Some(3), 0),
            (PerformanceProfile::InMemory, None, 0),
        ] {
            let manager = Manager::open_with_options(&path, profile, None, concurrency)
                .expect("data file loads");
            assert_eq!(
                manager.file_pool_size(),
                handles,
                "{} {concurrency:?}",
                profile.as_str()
            );
        }
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

    /// Where the published date starts in a Hash data file header, after the
    /// four version numbers, two 16 byte tags, the copyright offset, the age,
    /// the minimum User-Agent count and the name and format offsets. The next
    /// update date follows it.
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

    /// The dates read are the ones in the data file header, whether the data
    /// set reads the file or holds it in memory.
    #[test]
    fn the_dates_are_the_ones_the_data_file_header_holds() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping the header dates");
            return;
        };
        let bytes = std::fs::read(&path).expect("data file reads");
        let (published, next) = header_dates(&bytes);
        assert!(published < next, "{published:?} {next:?}");
        for manager in [
            Manager::open(&path, PerformanceProfile::LowMemory).expect("data file loads"),
            Manager::open_from_memory(
                bytes,
                PerformanceProfile::LowMemory,
                None,
                &HashOptions::default(),
            )
            .expect("data loads"),
        ] {
            assert_eq!(manager.data_set_published(), Some(published));
            assert_eq!(manager.data_set_next_update(), Some(next));
        }
    }

    /// A data set loaded from memory under any profile opens no file handle
    /// and gives the answers and properties the file gives, after the caller
    /// has dropped its own reference to the manager.
    #[test]
    fn a_data_set_loads_from_memory_as_it_does_from_the_file() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping loading from memory");
            return;
        };
        let bytes = std::fs::read(&path).expect("data file reads");
        let named = ["IsMobile", "DeviceId", "PlatformName"];
        for profile in [
            PerformanceProfile::LowMemory,
            PerformanceProfile::HighPerformance,
            PerformanceProfile::InMemory,
        ] {
            let options = HashOptions {
                concurrency: Some(3),
                ..HashOptions::default()
            };
            let from_file =
                Manager::open_with_hash_options(&path, profile, Some(&named[..]), &options)
                    .expect("data file loads");
            let from_memory =
                Manager::open_from_memory(bytes.clone(), profile, Some(&named[..]), &options)
                    .expect("data loads");
            assert_eq!(from_memory.file_pool_size(), 0, "{}", profile.as_str());
            assert!(from_memory.file_reads.is_none(), "{}", profile.as_str());
            assert_eq!(
                from_memory.property_names(),
                from_file.property_names(),
                "{}",
                profile.as_str()
            );
            let expected = detect(&from_file, "header.user-agent", CHROME_ON_WINDOWS);
            let results = detect(&from_memory, "header.user-agent", CHROME_ON_WINDOWS);
            drop(from_memory);
            for name in ["IsMobile", "PlatformName"] {
                assert_eq!(
                    results.value_as_string(name, "|").expect("value reads"),
                    expected.value_as_string(name, "|").expect("value reads"),
                    "{name} {}",
                    profile.as_str()
                );
            }
            assert_eq!(
                results.device_id().expect("device id reads"),
                expected.device_id().expect("device id reads"),
                "{}",
                profile.as_str()
            );
        }
    }

    /// Data that is not exactly the whole of a data file is refused with the
    /// C library's status. Too few bytes for the header, or for a collection
    /// the header describes, is corrupt data, a header of another version is
    /// the wrong version, and bytes left over after the last collection are
    /// out of bounds.
    #[test]
    fn data_that_is_not_a_whole_data_file_is_refused() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping refused data");
            return;
        };
        let bytes = std::fs::read(&path).expect("data file reads");
        let half = bytes[..bytes.len() / 2].to_vec();
        let mut extra_byte = bytes.clone();
        extra_byte.push(0);
        for (name, data, expected) in [
            ("nothing", Vec::new(), "CorruptData"),
            ("zeros", vec![0u8; 200], "IncorrectVersion"),
            ("the header alone", bytes[..200].to_vec(), "CorruptData"),
            ("half the file", half, "CorruptData"),
            ("the file and a byte", extra_byte, "PointerOutOfBounds"),
        ] {
            let error = Manager::open_from_memory(
                data,
                PerformanceProfile::InMemory,
                None,
                &HashOptions::default(),
            )
            .err()
            .unwrap_or_else(|| panic!("{name} should be refused"));
            let Error::Native { message, status } = &error else {
                panic!("{name}: {error}");
            };
            assert_eq!(status, expected, "{name}: {message}");
            assert!(!message.is_empty(), "{name}");
        }
    }

    /// A profile whose collections read through caches takes at most the
    /// concurrency its smallest cache can be split into, and the others take
    /// any.
    #[test]
    fn the_smallest_cache_sets_the_most_concurrency() {
        assert_eq!(most_concurrency(PerformanceProfile::Balanced), Some(22));
        assert_eq!(most_concurrency(PerformanceProfile::Default), Some(22));
        for profile in [
            PerformanceProfile::LowMemory,
            PerformanceProfile::HighPerformance,
            PerformanceProfile::InMemory,
        ] {
            assert_eq!(most_concurrency(profile), None, "{}", profile.as_str());
        }
        let balanced = default_concurrency(PerformanceProfile::Balanced);
        assert!((1..=22).contains(&balanced), "{balanced}");
        for (processors, profile, expected) in [
            (64, PerformanceProfile::Balanced, 22),
            (64, PerformanceProfile::Default, 22),
            (8, PerformanceProfile::Balanced, 8),
            (64, PerformanceProfile::LowMemory, 64),
            (64, PerformanceProfile::HighPerformance, 64),
        ] {
            assert_eq!(
                concurrency_within(processors, profile),
                expected,
                "{processors} {}",
                profile.as_str()
            );
        }
    }

    /// Unset options leave the profile's configuration as it is, and set
    /// ones change only their own fields.
    #[test]
    fn options_change_only_what_they_name() {
        // Safety: as in `open_with_hash_options`.
        let profile = unsafe { *config_for(PerformanceProfile::Balanced) };
        let fields = |config: &sys::ConfigHash| {
            (
                config.difference,
                config.drift,
                config.b.allow_unmatched,
                config.b.update_matched_user_agent,
                config.use_performance_graph,
                config.use_predictive_graph,
                config.values.concurrency,
                config.values.capacity,
                config.b.b.all_in_memory,
            )
        };
        let mut unchanged = profile;
        HashOptions::default().apply(&mut unchanged);
        assert_eq!(fields(&unchanged), fields(&profile));
        assert_eq!(
            fields(&profile),
            (0, 0, false, true, false, true, 10, 500, false)
        );

        let mut changed = profile;
        HashOptions {
            concurrency: Some(3),
            difference: Some(4),
            drift: Some(5),
            allow_unmatched: Some(true),
            update_matched_user_agent: Some(false),
            use_performance_graph: Some(true),
            use_predictive_graph: Some(false),
        }
        .apply(&mut changed);
        assert_eq!(
            fields(&changed),
            (4, 5, true, false, true, false, 3, 500, false)
        );
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

    /// The profiles that hold the data set in memory and the ones that read
    /// it from the file, so the metadata readers are run against both.
    pub(super) const METADATA_PROFILES: [PerformanceProfile; 3] = [
        PerformanceProfile::LowMemory,
        PerformanceProfile::Balanced,
        PerformanceProfile::InMemory,
    ];

    #[test]
    fn every_data_file_property_is_listed_whether_or_not_it_was_opened() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping data file properties");
            return;
        };
        for profile in METADATA_PROFILES {
            let all = Manager::open(&path, profile).expect("data file loads");
            let properties = all.data_file_properties().expect("the properties are read");
            // Opened naming no property, every property is available, so the
            // data file lists exactly the required properties, which the C
            // library keeps in name order rather than the data file's.
            assert_eq!(properties.len(), all.property_count() as usize);
            assert!(properties.iter().all(|p| p.available));
            let names: Vec<&str> = properties.iter().map(|p| p.name.as_str()).collect();
            let mut sorted = names.clone();
            sorted.sort_unstable();
            let mut required = all.property_names();
            required.sort_unstable();
            assert_eq!(sorted, required);
            for (index, property) in properties.iter().enumerate() {
                assert_eq!(property.index as usize, index);
            }

            let one = Manager::open_with_properties(&path, profile, Some(&["IsMobile"]))
                .expect("data file loads");
            let restricted = one.data_file_properties().expect("the properties are read");
            let restricted_names: Vec<&str> = restricted.iter().map(|p| p.name.as_str()).collect();
            assert_eq!(restricted_names, names, "{}", profile.as_str());
            let available: Vec<&str> = restricted
                .iter()
                .filter(|p| p.available)
                .map(|p| p.name.as_str())
                .collect();
            assert_eq!(available, ["IsMobile"], "{}", profile.as_str());
            for property in &properties {
                assert_eq!(
                    property.required_property_index,
                    all.required_property_index(&property.name),
                    "{}",
                    property.name
                );
            }
        }
    }

    #[test]
    fn a_property_named_in_another_case_is_found_and_read() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping a name in another case");
            return;
        };
        let manager =
            Manager::open_with_properties(&path, PerformanceProfile::Default, Some(&["ismobile"]))
                .expect("data file loads");
        let properties = manager
            .data_file_properties()
            .expect("the properties are read");
        let is_mobile = properties
            .iter()
            .find(|p| p.name == "IsMobile")
            .expect("IsMobile is in every Hash data file");
        assert!(is_mobile.available);
        let index = is_mobile
            .required_property_index
            .expect("the C library finds IsMobile ignoring case");
        // The C library gives back the data file's spelling for a name it
        // finds, so the required property names hold `IsMobile` too.
        assert_eq!(manager.property_names(), ["IsMobile"]);
        assert_eq!(manager.required_property_index("IsMobile"), Some(index));
        let results = detect(&manager, "header.user-agent", CHROME_ON_WINDOWS);
        let value = results
            .value_as_string_by_index(index, c"|")
            .expect("IsMobile is read");
        assert_eq!(value.as_deref(), Some("False"));
    }

    #[test]
    fn a_data_file_property_carries_the_metadata_the_file_holds() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping property metadata");
            return;
        };
        for profile in METADATA_PROFILES {
            let manager = Manager::open(&path, profile).expect("data file loads");
            let properties = manager
                .data_file_properties()
                .expect("the properties are read");
            let find = |name: &str| {
                properties
                    .iter()
                    .find(|p| p.name == name)
                    .unwrap_or_else(|| panic!("{name} is in every Hash data file"))
            };
            let tier = manager
                .data_set_name()
                .expect("the data file names its tier");

            let is_mobile = find("IsMobile");
            assert_eq!(is_mobile.value_type, PropertyValueType::Bool);
            assert_eq!(is_mobile.category, "Device");
            let default_value = is_mobile.default_value.as_ref().expect("IsMobile has one");
            assert_eq!(default_value.name, "False");
            assert!(!default_value.description.is_empty());
            assert_eq!(is_mobile.component_id, 1, "IsMobile is a hardware property");
            assert!(is_mobile.mandatory && is_mobile.show && is_mobile.show_values);
            assert!(!is_mobile.list && !is_mobile.obsolete);
            assert!(
                is_mobile.data_tiers.contains(&tier),
                "{:?}",
                is_mobile.data_tiers
            );
            assert!(is_mobile.data_tiers.iter().all(|t| !t.is_empty()));
            assert_eq!(is_mobile.value_count, 2);
            // The description is longer than the first buffer, so it is only
            // whole when the second read gives the rest.
            assert!(
                is_mobile.description.len() > METADATA_STRING_BUFFER_LENGTH,
                "{}",
                is_mobile.description
            );
            assert!(is_mobile.description.starts_with("Indicates if the device"));
            assert!(!is_mobile.description.contains('\0'));

            let browser_name = find("BrowserName");
            assert_eq!(browser_name.value_type, PropertyValueType::String);
            assert_eq!(
                browser_name.component_id, 3,
                "BrowserName is a browser property"
            );
            assert!(browser_name.url.starts_with("http"), "{}", browser_name.url);

            let script = find("JavascriptHardwareProfile");
            assert_eq!(script.value_type, PropertyValueType::JavaScript);
            assert!(properties.iter().all(|p| !p.name.is_empty()));
        }
    }

    /// The numbers are `fiftyoneDegreesPropertyValueType` in the C library's
    /// `src/common/propertyValueType.h`, and the types are those the C++
    /// `PropertyMetaDataBuilderHash::getPropertyType` gives. The Lite file
    /// has no double or list property, so only this test covers them.
    #[test]
    fn each_c_value_type_is_the_type_the_cxx_builder_names() {
        let cases = [
            (0, false, PropertyValueType::String),
            (0, true, PropertyValueType::StringList),
            (1, false, PropertyValueType::Integer),
            (1, true, PropertyValueType::Integer),
            (2, false, PropertyValueType::Double),
            (3, false, PropertyValueType::Bool),
            (4, false, PropertyValueType::JavaScript),
            // A single precision float, which the builder does not name.
            (5, false, PropertyValueType::String),
        ];
        for (value_type, is_list, expected) in cases {
            assert_eq!(
                data_file_value_type(value_type, is_list),
                expected,
                "type {value_type}, list {is_list}"
            );
        }
    }

    #[test]
    fn the_components_are_the_four_the_data_file_defines() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping components");
            return;
        };
        for profile in METADATA_PROFILES {
            let manager = Manager::open(&path, profile).expect("data file loads");
            let components = manager
                .data_file_components()
                .expect("the components are read");
            let ids_and_names: Vec<(u8, &str)> = components
                .iter()
                .map(|c| (c.component_id, c.name.as_str()))
                .collect();
            assert_eq!(
                ids_and_names,
                [
                    (1, "HardwarePlatform"),
                    (2, "SoftwarePlatform"),
                    (3, "BrowserUA"),
                    (4, "Crawler"),
                ],
                "{}",
                profile.as_str()
            );
            assert!(components.iter().all(|c| c.default_profile_id > 0));
        }
    }

    #[test]
    fn a_property_gives_every_value_the_data_file_holds_for_it() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping property values");
            return;
        };
        for profile in METADATA_PROFILES {
            let manager = Manager::open(&path, profile).expect("data file loads");
            let properties = manager
                .data_file_properties()
                .expect("the properties are read");
            let is_mobile = properties
                .iter()
                .find(|p| p.name == "IsMobile")
                .expect("IsMobile is in every Hash data file");
            let values = manager
                .data_file_property_values(is_mobile.index)
                .expect("the values are read");
            let mut names: Vec<&str> = values.iter().map(|v| v.name.as_str()).collect();
            names.sort_unstable();
            assert_eq!(names, ["False", "True"], "{}", profile.as_str());

            for property in &properties {
                let values = manager
                    .data_file_property_values(property.index)
                    .expect("the values are read");
                assert_eq!(
                    values.len(),
                    property.value_count as usize,
                    "{}",
                    property.name
                );
                if let Some(default) = &property.default_value {
                    assert!(
                        values.contains(default),
                        "{}'s default value {} is one of its values",
                        property.name,
                        default.name
                    );
                }
            }
            let outside = u32::try_from(properties.len()).expect("few properties");
            assert!(manager
                .data_file_property_values(outside)
                .expect("the values are read")
                .is_empty());
        }
    }

    #[test]
    fn a_range_of_values_is_that_part_of_the_whole_list() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping value ranges");
            return;
        };
        for profile in METADATA_PROFILES {
            let manager = Manager::open(&path, profile).expect("data file loads");
            let properties = manager
                .data_file_properties()
                .expect("the properties are read");
            for property in &properties {
                let all = manager
                    .data_file_property_values(property.index)
                    .expect("the values are read");
                let count = property.value_count;
                let past = count.saturating_add(5);
                let ranges = [
                    0..0,
                    0..1,
                    1..3,
                    count / 2..count,
                    count.saturating_sub(1)..past,
                    count..past,
                    past..u32::MAX,
                    Range { start: 3, end: 1 },
                    0..u32::MAX,
                ];
                for range in ranges {
                    let part = manager
                        .data_file_property_value_range(property.index, range.clone())
                        .expect("the values are read");
                    let end = (range.end as usize).min(all.len());
                    let start = (range.start as usize).min(end);
                    assert_eq!(
                        part,
                        all[start..end],
                        "{} {range:?} {}",
                        property.name,
                        profile.as_str()
                    );
                }
            }
            let outside = u32::try_from(properties.len()).expect("few properties");
            assert!(manager
                .data_file_property_value_range(outside, 0..5)
                .expect("the values are read")
                .is_empty());
        }
    }

    #[test]
    fn a_default_value_is_read_from_its_own_place() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping default values");
            return;
        };
        for profile in METADATA_PROFILES {
            let manager = Manager::open(&path, profile).expect("data file loads");
            let properties = manager
                .data_file_properties()
                .expect("the properties are read");
            // In the Lite file IsMobile's default is its first value and
            // ScreenPixelsWidth's its last, so a default read from the wrong
            // place gives the wrong name for one of them.
            for (name, expected) in [("IsMobile", "False"), ("ScreenPixelsWidth", "Unknown")] {
                let property = properties
                    .iter()
                    .find(|p| p.name == name)
                    .expect("the property is in every Hash data file");
                let default = property
                    .default_value
                    .as_ref()
                    .expect("the property has a default value");
                assert_eq!(default.name, expected, "{name} {}", profile.as_str());
            }
        }
    }

    #[test]
    fn values_carry_their_descriptions_and_urls() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping value strings");
            return;
        };
        for profile in METADATA_PROFILES {
            let manager = Manager::open(&path, profile).expect("data file loads");
            let properties = manager
                .data_file_properties()
                .expect("the properties are read");
            let values = |name: &str| {
                let property = properties
                    .iter()
                    .find(|p| p.name == name)
                    .expect("the property is in every Hash data file");
                manager
                    .data_file_property_values(property.index)
                    .expect("the values are read")
            };
            let is_mobile = values("IsMobile");
            assert!(
                is_mobile.iter().all(|value| !value.description.is_empty()),
                "{}: {is_mobile:?}",
                profile.as_str()
            );
            let vendors = values("PlatformVendor");
            assert!(
                vendors.iter().any(|value| value.url.starts_with("http")),
                "{}",
                profile.as_str()
            );
            for value in &vendors {
                assert!(
                    value.url.is_empty() || value.url.starts_with("http"),
                    "{} {value:?}",
                    profile.as_str()
                );
                assert!(!value.description.starts_with("http"), "{value:?}");
            }
        }
    }

    #[test]
    fn a_value_is_found_by_its_exact_name() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping finding values");
            return;
        };
        for profile in METADATA_PROFILES {
            let manager = Manager::open(&path, profile).expect("data file loads");
            let properties = manager
                .data_file_properties()
                .expect("the properties are read");
            let find = |index: u32, name: &str| {
                manager
                    .data_file_property_value(index, name)
                    .expect("the values are read")
            };
            // Every value of a property whose values are in order of their
            // names is found by its name, and is the one the whole list holds.
            // The C search can miss a value of a property whose values are not
            // in order, so for those a value found must be the one the list
            // holds under that name.
            let (mut in_order, mut checked) = (0usize, 0u32);
            for property in &properties {
                let values = manager
                    .data_file_property_values(property.index)
                    .expect("the values are read");
                let ordered = values.windows(2).all(|pair| pair[0].name < pair[1].name);
                for value in &values {
                    let what = format!("{} {} {}", property.name, value.name, profile.as_str());
                    match find(property.index, &value.name) {
                        Some(found) => assert_eq!(&found, value, "{what}"),
                        None => assert!(!ordered, "{what}"),
                    }
                }
                if ordered {
                    in_order += 1;
                    checked += property.value_count;
                }
            }
            assert!(in_order > 0, "{}", profile.as_str());
            assert!(checked > 0, "{}", profile.as_str());

            // The name must match exactly, so another case, part of the name,
            // more than the name, or another property's value finds nothing.
            let index_of = |name: &str| {
                properties
                    .iter()
                    .find(|p| p.name == name)
                    .expect("the property is in every Hash data file")
                    .index
            };
            let is_mobile = index_of("IsMobile");
            assert!(find(index_of("BrowserName"), "Safari").is_some());
            for name in [
                "true", "TRUE", "Tru", "True ", "TrueX", "", "True\0", "Safari",
            ] {
                assert_eq!(find(is_mobile, name), None, "{name:?} {}", profile.as_str());
            }
            let outside = u32::try_from(properties.len()).expect("few properties");
            assert_eq!(find(outside, "True"), None, "{}", profile.as_str());
        }
    }

    /// Places in a Hash data file, from the packed
    /// `fiftyoneDegreesDataSetHashHeader` in the C library's `hash.h` and
    /// `fiftyoneDegreesProperty` in `property.h`. A collection's header is its
    /// start, its length in bytes and its count, each four bytes.
    const PROPERTIES_HEADER: usize = 110;
    const VALUES_HEADER: usize = 122;
    const PROPERTY_SIZE: usize = 44;
    const NAME_OFFSET: usize = 12;
    const FIRST_VALUE_INDEX: usize = 28;
    const LAST_VALUE_INDEX: usize = 32;

    fn read_u32(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
    }

    fn write_u32(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn a_property_whose_values_are_not_all_in_the_file_has_none() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping stored value indexes");
            return;
        };
        let mut bytes = std::fs::read(&path).expect("the data file is read");
        let start = read_u32(&bytes, PROPERTIES_HEADER) as usize;
        let length = read_u32(&bytes, PROPERTIES_HEADER + 4) as usize;
        let count = read_u32(&bytes, PROPERTIES_HEADER + 8) as usize;
        let values_count = read_u32(&bytes, VALUES_HEADER + 8);
        assert_eq!(length, count * PROPERTY_SIZE);
        let record = |index: u32| start + index as usize * PROPERTY_SIZE;

        let manager = Manager::open(&path, PerformanceProfile::InMemory).expect("data file loads");
        let original = manager
            .data_file_properties()
            .expect("the properties are read");
        let original_values: Vec<Vec<DataFileValue>> = original
            .iter()
            .map(|property| {
                manager
                    .data_file_property_values(property.index)
                    .expect("the values are read")
            })
            .collect();
        drop(manager);
        assert_eq!(original.len(), count);
        // Each record's indexes give the count read through the shim, so the
        // records are read from the right places.
        for property in &original {
            let first = read_u32(&bytes, record(property.index) + FIRST_VALUE_INDEX);
            let last = read_u32(&bytes, record(property.index) + LAST_VALUE_INDEX);
            assert_eq!(
                last.wrapping_sub(first).wrapping_add(1),
                property.value_count,
                "{}",
                property.name
            );
        }
        let index_of = |name: &str| {
            original
                .iter()
                .find(|p| p.name == name)
                .expect("the property is in every Hash data file")
                .index
        };

        // Both indexes -1, a last index one past the values with the default
        // value still inside the range, and a first index after the last.
        let no_values = index_of("BrowserName");
        let past_the_end = index_of("ScreenPixelsWidth");
        let reversed = index_of("PlatformName");
        assert!(original[past_the_end as usize].default_value.is_some());
        write_u32(&mut bytes, record(no_values) + FIRST_VALUE_INDEX, u32::MAX);
        write_u32(&mut bytes, record(no_values) + LAST_VALUE_INDEX, u32::MAX);
        write_u32(
            &mut bytes,
            record(past_the_end) + LAST_VALUE_INDEX,
            values_count,
        );
        let last = read_u32(&bytes, record(reversed) + LAST_VALUE_INDEX);
        write_u32(&mut bytes, record(reversed) + FIRST_VALUE_INDEX, last + 2);
        let changed = [no_values, past_the_end, reversed];

        let dir = std::env::temp_dir().join(format!(
            "fiftyone-native-value-indexes-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the folder is made");
        let patched = dir.join("51Degrees.hash");
        std::fs::write(&patched, &bytes).expect("the copy is written");
        for profile in METADATA_PROFILES {
            let manager = Manager::open(&patched, profile).expect("the copy loads");
            let properties = manager
                .data_file_properties()
                .expect("the properties are read");
            assert_eq!(properties.len(), original.len());
            for (property, before) in properties.iter().zip(&original) {
                if !changed.contains(&property.index) {
                    assert_eq!(property, before, "{}", profile.as_str());
                    continue;
                }
                let what = format!("{} {}", property.name, profile.as_str());
                assert_eq!(property.value_count, 0, "{what}");
                assert_eq!(property.default_value, None, "{what}");
                assert!(
                    manager
                        .data_file_property_values(property.index)
                        .expect("the values are read")
                        .is_empty(),
                    "{what}"
                );
                assert!(
                    manager
                        .data_file_property_value_range(property.index, 0..5)
                        .expect("the values are read")
                        .is_empty(),
                    "{what}"
                );
                let names = &original_values[property.index as usize];
                assert!(!names.is_empty(), "{what}");
                for value in names {
                    assert_eq!(
                        manager
                            .data_file_property_value(property.index, &value.name)
                            .expect("the values are read"),
                        None,
                        "{what} {}",
                        value.name
                    );
                }
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Whether the data set was created with a property is found by the
    /// property's name, so a property whose name cannot be read cannot be
    /// read at all, as the C++ builder raises an exception there. A data set
    /// reads every property's name when it loads, so only a read that fails
    /// later reaches this. LowMemory reads each property from the file when
    /// it is asked for, so the last property's name is made unreadable in the
    /// file once the data set has loaded.
    #[test]
    fn a_property_whose_name_cannot_be_read_is_not_read() {
        use std::io::{Seek, SeekFrom, Write};

        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping unreadable names");
            return;
        };
        let bytes = std::fs::read(&path).expect("the data file is read");
        let start = read_u32(&bytes, PROPERTIES_HEADER) as usize;
        let length = read_u32(&bytes, PROPERTIES_HEADER + 4) as usize;
        let count = read_u32(&bytes, PROPERTIES_HEADER + 8);
        assert_eq!(length, count as usize * PROPERTY_SIZE);
        let last = count - 1;

        let dir = std::env::temp_dir().join(format!(
            "fiftyone-native-unreadable-name-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the folder is made");
        let copy = dir.join("51Degrees.hash");
        std::fs::write(&copy, &bytes).expect("the copy is written");
        let manager = Manager::open(&copy, PerformanceProfile::LowMemory).expect("the copy loads");
        {
            let _file_read = manager.file_read();
            let fields = manager
                .read_property_fields(last)
                .expect("the last property is read");
            assert_eq!(fields.available, 1);
        }

        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(&copy)
            .expect("the copy opens to write");
        let name_offset = start + last as usize * PROPERTY_SIZE + NAME_OFFSET;
        file.seek(SeekFrom::Start(name_offset as u64))
            .expect("the name offset is found");
        file.write_all(&(i32::MAX as u32).to_le_bytes())
            .expect("the name offset is written");
        drop(file);
        {
            let _file_read = manager.file_read();
            assert!(manager.read_property_fields(0).is_some());
            assert!(manager.read_property_fields(last).is_none());
        }
        let error = manager
            .data_file_properties()
            .expect_err("the last property cannot be read");
        assert!(
            error.to_string().contains(&format!("(property {last})")),
            "{error}"
        );
        drop(manager);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A reader for `read_metadata_string` that writes `text` as the shim
    /// does and returns its full length, or returns `SHIM_STRING_FAILED` on
    /// call number `fail_on`, counting from one. `calls` counts the calls.
    fn shim_like_reader<'a>(
        text: &'a str,
        fail_on: Option<usize>,
        calls: &'a mut usize,
    ) -> impl FnMut(*mut c_char, usize) -> usize + 'a {
        move |buffer, length| {
            *calls += 1;
            // Safety: `read_metadata_string` passes `length` writable bytes,
            // and `length` is never zero.
            let buffer = unsafe { std::slice::from_raw_parts_mut(buffer as *mut u8, length) };
            buffer[0] = 0;
            if Some(*calls) == fail_on {
                return sys::SHIM_STRING_FAILED;
            }
            let written = text.len().min(length - 1);
            buffer[..written].copy_from_slice(&text.as_bytes()[..written]);
            buffer[written] = 0;
            text.len()
        }
    }

    #[test]
    fn a_metadata_string_that_fails_on_either_call_is_none() {
        let fits = "x".repeat(METADATA_STRING_BUFFER_LENGTH - 1);
        let too_long = "x".repeat(METADATA_STRING_BUFFER_LENGTH);
        let read = |text: &str, fail_on| {
            let mut calls = 0;
            let result = read_metadata_string(shim_like_reader(text, fail_on, &mut calls));
            (result, calls)
        };

        assert_eq!(read(&fits, None), (Some(fits.clone()), 1));
        assert_eq!(read(&too_long, None), (Some(too_long.clone()), 2));
        assert_eq!(read(&fits, Some(1)), (None, 1));
        assert_eq!(read(&too_long, Some(1)), (None, 1));
        assert_eq!(read(&too_long, Some(2)), (None, 2));
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
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping string readers");
            return;
        };
        let fields = [
            sys::SHIM_PROPERTY_NAME,
            sys::SHIM_PROPERTY_DESCRIPTION,
            sys::SHIM_PROPERTY_CATEGORY,
            sys::SHIM_PROPERTY_URL,
        ];
        for profile in METADATA_PROFILES {
            let manager = Manager::open(&path, profile).expect("data file loads");
            let _file_read = manager.file_read();
            let ptr = manager.as_ptr();
            // Safety (every call below): the manager is initialized for the
            // lifetime of `manager`, and each buffer is writable for the
            // length given.
            let properties = unsafe { sys::fiftyoneDegreesShimHashGetDataFilePropertyCount(ptr) };
            let components = unsafe { sys::fiftyoneDegreesShimHashGetComponentCount(ptr) };
            let string = |index, field| {
                shim_string_length(|buffer, length| unsafe {
                    sys::fiftyoneDegreesShimHashGetDataFilePropertyString(
                        ptr, index, field, buffer, length,
                    )
                })
            };
            let tier = |index, map| {
                shim_string_length(|buffer, length| unsafe {
                    sys::fiftyoneDegreesShimHashGetDataFilePropertyMap(
                        ptr, index, map, buffer, length,
                    )
                })
            };
            let component = |index| {
                shim_string_length(|buffer, length| unsafe {
                    sys::fiftyoneDegreesShimHashGetComponentName(ptr, index, buffer, length)
                })
            };
            let profile = profile.as_str();

            let last = properties - 1;
            for field in fields {
                assert!(string(last, field).is_some(), "{profile} field {field}");
                for index in [properties, u32::MAX] {
                    assert_eq!(string(index, field), None, "{profile} {index} {field}");
                }
            }
            for field in [-1, sys::SHIM_PROPERTY_URL + 1] {
                assert_eq!(string(last, field), None, "{profile} field {field}");
            }

            let (index, maps) = (0..properties)
                .map(|index| {
                    let fields = manager
                        .read_property_fields(index)
                        .expect("the property is read");
                    (index, fields.map_count)
                })
                .find(|&(_, maps)| maps > 0)
                .expect("a property is in a data tier");
            assert!(tier(index, maps - 1).is_some(), "{profile}");
            assert_eq!(tier(index, maps), None, "{profile}");
            assert_eq!(tier(index, u32::MAX), None, "{profile}");
            assert_eq!(tier(properties, 0), None, "{profile}");

            assert!(component(components - 1).is_some(), "{profile}");
            for index in [components, u32::MAX] {
                assert_eq!(component(index), None, "{profile} {index}");
            }
        }
    }
}
