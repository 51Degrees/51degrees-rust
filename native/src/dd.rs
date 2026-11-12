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
//! frees it on drop. Its reloads replace the data set in place through the C
//! library's reload functions. [`Results`] owns a native results structure,
//! which holds the data set it was created from until the results are freed,
//! and an [`Arc`] to the manager, and frees the results on drop. Values are
//! read into a reusable per-thread buffer.
//!
//! # Thread safety
//!
//! The native resource manager is internally reference counted and lock
//! protected (see `src/common/resource.h`), so a [`Manager`] is both [`Send`]
//! and [`Sync`] and a single one may be shared across threads behind an
//! [`Arc`]. A data set that reads from its file after it is loaded holds one
//! file handle for each concurrent operation it was opened to expect, and the
//! native library makes a read that finds none free wait for one.
//! A native results structure is per-thread scratch, so [`Results`] is
//! [`Send`] (it may be moved to another thread) but not [`Sync`] (it must not be
//! used from two threads at once). This matches the pipeline rule that one flow
//! data, and therefore one results structure, belongs to a single thread at a
//! time.

mod profiles;

pub use profiles::DataFileProfile;

use std::ffi::{CStr, CString};
use std::ops::Range;
use std::os::raw::{c_char, c_int, c_long, c_void};
use std::path::Path;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex, PoisonError};

use fiftyone_device_detection_sys as sys;
use fiftyone_pipeline_core::{Error, PropertyValueType, Result};

use crate::buffer::{with_c_str, with_value_string};
use crate::evidence::with_native_evidence;
use crate::held_memory::HeldMemory;
use crate::kept::{Kept, NameIndexes};
use crate::metadata::{metadata_read_failed, read_metadata_string, take_value_string};
use crate::profile::PerformanceProfile;
use crate::status::{status_name, status_to_result, take_message, NativeException, NativeStatus};

mod value_profiles;

/// The separator the C library writes between evidence keys. No header or
/// property name holds a comma.
const EVIDENCE_KEY_SEPARATOR: &CStr = c",";

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
/// to every collection's config. The C library takes zero as one concurrent
/// operation for each processor, and opens at most 64 file handles whatever
/// the value.
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

/// Settings for loading a Hash data set. Each [`Option`] changes a
/// performance profile's native configuration, as the builders of the other
/// 51Degrees ports set them, and is [`None`] to keep the profile's own value.
/// Start from [`HashOptions::default`] and set the fields wanted.
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
    /// The properties whose values the data set indexes, when it loads, to
    /// the profiles holding each, so that
    /// [`Manager::profile_ids_for_property_value`] finds them without
    /// reading every profile. A property is indexed whether or not it is
    /// requested, and whatever the profile. A name the data file has no
    /// property of indexes nothing. Empty, the default, indexes none. The
    /// names go to the C library's init function, not into the
    /// configuration.
    pub indexed_properties: Vec<String>,
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

/// The expected concurrency a Hash engine opens with when none is named,
/// which the specification gives as the number of processors, whatever the
/// profile. The C library counts them, as it does for an IP Intelligence
/// data set opened with none named, so the two engines open with the same.
/// Any concurrency loads, because a cache with fewer entries than the
/// concurrency squared is split into fewer parts, one with fewer entries
/// than the concurrency is grown to it, and the file handle pool holds at
/// most 64 handles (see [`Manager::file_pool_size`]).
pub fn default_concurrency(_profile: PerformanceProfile) -> u16 {
    // Safety: the function reads no state, and gives at least 1.
    unsafe { fiftyone_common_sys::fiftyoneDegreesThreadingGetProcessorCount() }
}

/// A copy of `profile`'s predefined configuration changed by `options`, so a
/// data set's options never change the shared global the other data sets read.
fn profile_config(profile: PerformanceProfile, options: &HashOptions) -> sys::ConfigHash {
    // Safety: `config_for` returns a valid pointer to an initialized config
    // global, and copying out of it is a plain read of a `repr(C)` value.
    let mut config = unsafe { *config_for(profile) };
    // Header names are read as the data set holds them, never with `HTTP_`
    // before them, as .NET's Hash engine builder sets, so the evidence keys
    // carry the plain names.
    config.b.b.uses_upper_prefixed_headers = false;
    options.apply(&mut config);
    config
}

/// The size of a Hash data file's header in bytes, being how much of a data
/// file the C library compares when it looks for a copy of the file to reuse.
pub fn data_file_header_size() -> usize {
    // Safety: the shim reads no state and takes no arguments.
    unsafe { sys::fiftyoneDegreesShimHashGetHeaderSize() }
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

/// A Hash device detection data set, loaded into a native resource manager.
///
/// Owns a native [`sys::ResourceManager`] initialized from a data file. Cloning
/// the [`Arc`] returned by [`Manager::open`] shares it across threads cheaply.
/// [`Manager::reload_from_file`] and the other reloads replace the data set in
/// place, as the C library's reload does, and each call reads the data set in
/// use when it is made. A replaced data set is freed once the last
/// [`Results`] created from it is dropped, and the manager when the last
/// reference to it is dropped, after every [`Results`] (the [`Arc`] they hold
/// guarantees that ordering).
pub struct Manager {
    /// The native manager, heap allocated so its address is stable for the
    /// native handle bookkeeping. Boxed and leaked into a raw pointer, freed in
    /// [`Drop`].
    manager: NonNull<sys::ResourceManager>,
    /// The number of required properties, which a read by index must stay
    /// below. A reload keeps the names of the data set it replaces, so it
    /// keeps this number.
    required_count: u32,
    /// The required property index of each name in the data set in use, so
    /// a name is looked up with no lock and no write to shared memory.
    names: Kept<NameIndexes>,
    /// Held while a reload replaces the data set, because the C library's
    /// reload reads the data set in use without taking a reference to it.
    reload_lock: Mutex<()>,
    /// The data a load from memory reads in place. Fields drop after
    /// [`Drop::drop`] has freed the data set, so the data outlives it. It is
    /// kept after a reload, until the manager is dropped, because the C
    /// library does not say when the data set reading it is freed.
    _memory: Option<HeldMemory>,
}

// Safety: the native resource manager is internally reference counted and
// lock protected per `src/common/resource.h`, so concurrent access from several
// threads through a shared `&Manager` is sound, and reloads run one at a time
// under `reload_lock`. Therefore the manager is both `Send` and `Sync`.
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
    /// through the data set at once. [`None`] keeps the profile's default,
    /// one for each processor. Any value loads, and the pool holds at most 64
    /// handles. `HighPerformance` holds in memory everything detection reads,
    /// so detection never waits on the pool, but it still opens one handle for
    /// each concurrent operation, to at most 64, and keeps them open until the
    /// data set is freed (see [`Manager::file_pool_size`]). `InMemory` opens
    /// no pool.
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
    /// profile's native configuration by `options` first, and indexing the
    /// properties `options` names to index.
    pub fn open_with_hash_options(
        path: impl AsRef<Path>,
        profile: PerformanceProfile,
        properties: Option<&[&str]>,
        options: &HashOptions,
    ) -> Result<Arc<Manager>> {
        let path = path.as_ref();
        let path_c = crate::path::native_path(path)?;
        let mut config = profile_config(profile, options);
        let indexed = value_profiles::IndexedNames::new(&options.indexed_properties)?;
        Self::init(
            properties,
            None,
            |manager, required, exception| {
                // Safety: every pointer is valid for the call, the config is
                // read only for the call, the path is null terminated and
                // `indexed` holds its names until the call returns.
                unsafe {
                    sys::fiftyoneDegreesHashInitManagerFromFileWithIndexedProperties(
                        manager,
                        &mut config,
                        required,
                        path_c.as_ptr(),
                        indexed.as_ptr(),
                        indexed.count(),
                        exception,
                    )
                }
            },
            |status| status_detail(status, path_c.as_ptr()),
        )
    }

    /// Load a Hash data set from `data`, the whole of a data file held in
    /// memory, with the named properties, changing the profile's native
    /// configuration by `options` first, and indexing the properties
    /// `options` names to index.
    ///
    /// The data set reads its collections from `data` where it lies, so the
    /// manager keeps `data` until the manager is dropped, a reload
    /// notwithstanding. Every profile holds the data set in memory this way,
    /// so none opens a file pool. The C library takes the length as a C
    /// `long`, so where that is 32 bits, as on Windows, `data` must be shorter
    /// than 2 GiB.
    pub fn open_from_memory(
        data: Vec<u8>,
        profile: PerformanceProfile,
        properties: Option<&[&str]>,
        options: &HashOptions,
    ) -> Result<Arc<Manager>> {
        let memory = HeldMemory::new(data);
        let size = c_long::try_from(memory.length()).map_err(|_| Error::Native {
            status: String::from("InvalidInput"),
            message: format!(
                "the data is {} bytes, more than the C library reads from \
                 memory on this platform",
                memory.length()
            ),
        })?;
        let mut config = profile_config(profile, options);
        // The memory is Rust's and is dropped with the manager, so the C
        // library must not free it, and there is no file to copy.
        config.b.b.free_data = false;
        config.b.b.use_temp_file = false;
        let indexed = value_profiles::IndexedNames::new(&options.indexed_properties)?;
        let start = memory.start();
        Self::init(
            properties,
            Some(memory),
            |manager, required, exception| {
                // Safety: every pointer is valid for the call, the config is
                // read only for the call, `start` is the first of `size`
                // bytes that the manager keeps for as long as the data set,
                // and `indexed` holds its names until the call returns.
                unsafe {
                    sys::fiftyoneDegreesHashInitManagerFromMemoryWithIndexedProperties(
                        manager,
                        &mut config,
                        required,
                        start,
                        size,
                        indexed.as_ptr(),
                        indexed.count(),
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
        memory: Option<HeldMemory>,
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
        let required_count =
            unsafe { sys::fiftyoneDegreesShimHashGetRequiredPropertyCount(manager.as_ptr()) };
        // Safety: as above.
        let names = Kept::new(unsafe { read_name_indexes(manager.as_ptr()) });
        Ok(Arc::new(Manager {
            manager,
            required_count,
            names,
            reload_lock: Mutex::new(()),
            _memory: memory,
        }))
    }

    /// Reload the data set from the data file it was loaded from, with the
    /// settings and the property names it was opened with, as the C
    /// library's `fiftyoneDegreesHashReloadManagerFromOriginalFile` does.
    ///
    /// The new data set keeps the names of the one it replaces, in their
    /// order, so an index names the same property after the reload, and a
    /// manager opened naming no property keeps the names of the first data
    /// file it loaded (see [`Manager::property_names`]). A property only the
    /// new data file has is then not among them. [`Results`] created before
    /// the reload keep reading the data set they were created from, which is
    /// freed when the last of them is dropped. A reload that fails leaves the
    /// data set in use. Reloads of one manager run one at a time.
    pub fn reload_from_original_file(&self) -> Result<()> {
        self.reload(
            |manager, exception| {
                // Safety: the manager is initialized and the caller holds the
                // reload lock.
                unsafe { sys::fiftyoneDegreesHashReloadManagerFromOriginalFile(manager, exception) }
            },
            |status| status_detail(status, std::ptr::null()),
        )
    }

    /// Reload the data set from the data file at `path`, as
    /// [`Manager::reload_from_original_file`] does.
    pub fn reload_from_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let path_c = crate::path::native_path(path.as_ref())?;
        self.reload(
            |manager, exception| {
                // Safety: the manager is initialized, the caller holds the
                // reload lock and the path is null terminated.
                unsafe {
                    sys::fiftyoneDegreesHashReloadManagerFromFile(
                        manager,
                        path_c.as_ptr(),
                        exception,
                    )
                }
            },
            |status| status_detail(status, path_c.as_ptr()),
        )
    }

    /// Reload the data set from `data`, the whole of a data file held in
    /// memory, as [`Manager::reload_from_original_file`] does.
    ///
    /// The C library copies `data` into memory that the new data set frees,
    /// as the C++ engine's `refreshData` does, so the caller keeps nothing.
    /// The C library takes the length as a C `long`, so where that is 32
    /// bits, as on Windows, `data` must be shorter than 2 GiB.
    pub fn reload_from_memory(&self, data: &[u8]) -> Result<()> {
        let length = c_long::try_from(data.len()).map_err(|_| Error::Native {
            status: String::from("InvalidInput"),
            message: format!(
                "the data is {} bytes, more than the C library reads from \
                 memory on this platform",
                data.len()
            ),
        })?;
        self.reload(
            |manager, exception| {
                // Safety: the manager is initialized, the caller holds the
                // reload lock, and `data` is `length` bytes, which the shim
                // copies during the call.
                unsafe {
                    sys::fiftyoneDegreesShimHashReloadManagerFromMemoryCopy(
                        manager,
                        data.as_ptr().cast::<c_void>(),
                        length,
                        exception,
                    )
                }
            },
            // The C++ engine names no file for a load from memory.
            |status| status_detail(status, std::ptr::null()),
        )
    }

    /// Run `reload` on the native manager while holding the reload lock,
    /// with `detail` giving the message for a failed status.
    fn reload(
        &self,
        reload: impl FnOnce(*mut sys::ResourceManager, *mut sys::Exception) -> sys::StatusCode,
        detail: impl FnOnce(sys::StatusCode) -> String,
    ) -> Result<()> {
        let _reloading = self
            .reload_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut exception = sys::Exception::cleared();
        let status = reload(self.as_ptr(), &mut exception);
        status_to_result(status, || detail(status))?;
        exception.check()?;
        // Safety: the manager is initialized.
        self.names
            .replace(unsafe { read_name_indexes(self.as_ptr()) }, |a, b| a == b);
        Ok(())
    }

    /// The date the data file was published, from its header, as `(year,
    /// month, day)`, or [`None`] when the header holds no valid date.
    pub fn data_set_published(&self) -> Option<(i32, u32, u32)> {
        self.header_date(sys::fiftyoneDegreesHashGetPublished)
    }

    /// The date the next data file is expected, from the data file header,
    /// as `(year, month, day)`, or [`None`] when the header holds no valid
    /// date.
    pub fn data_set_next_update(&self) -> Option<(i32, u32, u32)> {
        self.header_date(sys::fiftyoneDegreesHashGetNextUpdate)
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

    /// The raw native manager pointer. For the engine wrappers in this crate.
    fn as_ptr(&self) -> *mut sys::ResourceManager {
        self.manager.as_ptr()
    }

    /// The number of handles to the data file that the data set opened when
    /// it loaded, which stay open until it is freed. Every profile but
    /// `InMemory` opens one for each concurrent operation it expects, the
    /// largest concurrency of its collections, to at most 64, and
    /// `HighPerformance` too, which reads through them only for the values
    /// and strings of properties not named when some are. `InMemory` opens
    /// none.
    pub fn file_pool_size(&self) -> u16 {
        // Safety: the manager is initialized for the lifetime of `self`.
        unsafe { sys::fiftyoneDegreesShimHashGetFilePoolSize(self.as_ptr()) }
    }

    /// The number of required properties in the loaded data set, which is the
    /// number of names it was opened with, or the number of its properties
    /// when it was opened naming none. A reload keeps the names, so it keeps
    /// the number.
    pub fn property_count(&self) -> u32 {
        // Safety: the manager is initialized for the lifetime of `self`.
        unsafe { sys::fiftyoneDegreesShimHashGetRequiredPropertyCount(self.as_ptr()) }
    }

    /// The names of the required properties, in index order. With properties
    /// named, these are the names given, in the order given, including any the
    /// data set has no property for. With none named, these are the names of
    /// the first data file loaded, which every reload keeps.
    pub fn property_names(&self) -> Vec<String> {
        // Safety: the manager is initialized for the lifetime of `self`.
        unsafe { read_property_names(self.as_ptr()) }
    }

    /// The evidence keys the data set reads, lowercased, in the C library's
    /// order, which is the list the C++ engine's `getKeys` gives: a
    /// `header.` and a `query.` key for each header the data set names,
    /// other than a pseudo header, then a `cookie.` and a `query.` key for
    /// each property the evidence can override, then the keys of the device
    /// id, the high entropy values, the structured User-Agent and the profile
    /// ids.
    pub fn evidence_keys(&self) -> Vec<String> {
        // Safety: the manager is initialized for the lifetime of `self`.
        let data_set = unsafe { sys::fiftyoneDegreesDataSetHashGet(self.as_ptr()) };
        if data_set.is_null() {
            return Vec::new();
        }
        // Safety: the data set is held until the release below, so both calls
        // read the same one. A null buffer only finds the length, and the
        // buffer is then writable for the keys and the null terminator.
        let text = unsafe {
            let length = sys::fiftyoneDegreesHashGetEvidenceKeys(
                data_set,
                std::ptr::null_mut(),
                0,
                EVIDENCE_KEY_SEPARATOR.as_ptr(),
            );
            let mut buffer = vec![0u8; length + 1];
            sys::fiftyoneDegreesHashGetEvidenceKeys(
                data_set,
                buffer.as_mut_ptr() as *mut c_char,
                buffer.len(),
                EVIDENCE_KEY_SEPARATOR.as_ptr(),
            );
            sys::fiftyoneDegreesDataSetHashRelease(data_set);
            buffer.truncate(length);
            String::from_utf8_lossy(&buffer).to_lowercase()
        };
        text.split(',')
            .filter(|key| !key.is_empty())
            .map(str::to_owned)
            .collect()
    }

    /// The data set's name, which is the data file's tier (for example `Lite`,
    /// `Enterprise` or `TAC`), read from the data file header.
    ///
    /// Returns [`None`] when the native data set does not expose a name, so a
    /// caller can fall back to its own default rather than assert a tier.
    pub fn data_set_name(&self) -> Option<String> {
        let mut buffer = vec![0u8; 128];
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

    /// The required property index of the property named, as the C library
    /// finds it among the names of the data set in use, ignoring case.
    ///
    /// Returns [`None`] when the property is not one of the required
    /// properties, or the name holds a nul byte. The name is found in a table
    /// the manager reads from each data set it loads, so a lookup takes no
    /// lock and writes no shared memory, and threads sharing the manager do
    /// not contend. [`Results`] look a name up in the data set they hold.
    pub fn required_property_index(&self, property_name: &str) -> Option<c_int> {
        self.names.current().find(property_name)
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
        // Safety: the results were created above and are not yet freed.
        let data_set = unsafe { sys::fiftyoneDegreesShimHashResultsGetDataSet(results.as_ptr()) };
        let Some(data_set) = NonNull::new(data_set) else {
            // Safety: the results were created above and nothing else has them.
            unsafe { sys::fiftyoneDegreesResultsHashFree(results.as_ptr()) };
            return Err(Error::Native {
                status: String::from("NullPointer"),
                message: String::from("the Hash results hold no data set"),
            });
        };
        Ok(Results {
            results,
            data_set,
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
    /// The index [`Results::value_as_string_by_index`] and
    /// [`Results::values_by_index`] take for the property, or [`None`] when
    /// results do not hold it. The C library finds the property among the
    /// names the data set was opened with ignoring case, so this is found for
    /// a property named in another case.
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
    /// The property's default value, or [`None`] when it has none. As the
    /// C++ code the other SDKs call finds it, it is the property's own value
    /// with the name of the value the data file gives as the default, or with
    /// an empty name when the file gives none.
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
        // Safety: the manager is initialized for the lifetime of `self`.
        let count = unsafe { sys::fiftyoneDegreesShimHashGetDataFilePropertyCount(self.as_ptr()) };
        if property_index >= count {
            return Ok(Vec::new());
        }
        self.read_values(property_index, range)
            .ok_or_else(|| metadata_read_failed(&format!("values of property {property_index}")))
    }

    /// The value named `name`, matched exactly up to any null, among those
    /// [`Self::data_file_property_values`] gives for the property at
    /// `property_index`, or [`None`] when the index is out of range or the
    /// property has no such value. A property or value that cannot be read is
    /// an error.
    ///
    /// A property's values are in order of their names, so the value is found
    /// by the C library's binary search, as the C++ code the other SDKs call
    /// finds it, without reading the others. A value the data file holds out
    /// of that order can be missed, as the C++ misses it.
    ///
    /// As in the C++ code, `name` is matched up to its first null, so
    /// `True\0x` finds `True`.
    pub fn data_file_property_value(
        &self,
        property_index: u32,
        name: &str,
    ) -> Result<Option<DataFileValue>> {
        // Safety: the manager is initialized for the lifetime of `self`.
        let count = unsafe { sys::fiftyoneDegreesShimHashGetDataFilePropertyCount(self.as_ptr()) };
        if property_index >= count {
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
            (sys::SHIM_FIND_FOUND, Some(value)) if values.is_empty() && value.name == searched => {
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
    /// [`None`] when the property or any of them cannot be read.
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
    /// of range or any part of it cannot be read.
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

/// The names of the required properties of the data set `manager` holds, in
/// index order, as [`Manager::property_names`] gives them.
///
/// # Safety
/// `manager` must be initialized.
unsafe fn read_property_names(manager: *mut sys::ResourceManager) -> Vec<String> {
    let count = sys::fiftyoneDegreesShimHashGetRequiredPropertyCount(manager);
    let mut names = Vec::with_capacity(count as usize);
    let mut buffer = vec![0u8; 256];
    for index in 0..count as c_int {
        // The buffer is writable for its full length.
        let written = sys::fiftyoneDegreesShimHashGetRequiredPropertyName(
            manager,
            index,
            buffer.as_mut_ptr() as *mut c_char,
            buffer.len(),
        );
        if written > 0 {
            let end = written.min(buffer.len());
            names.push(String::from_utf8_lossy(&buffer[..end]).into_owned());
        }
    }
    names
}

/// The required property index the C library gives each required property
/// name of the data set `manager` holds.
///
/// # Safety
/// `manager` must be initialized, and no reload may replace its data set
/// during the call.
unsafe fn read_name_indexes(manager: *mut sys::ResourceManager) -> NameIndexes {
    let mut found = Vec::new();
    for name in read_property_names(manager) {
        // A name the C library gave holds no nul byte.
        let index = match CString::new(name.as_str()) {
            Ok(name_c) => sys::fiftyoneDegreesShimHashGetRequiredPropertyIndexFromName(
                manager,
                name_c.as_ptr(),
            ),
            Err(_) => -1,
        };
        found.push((name, index));
    }
    NameIndexes::new(found)
}

impl Drop for Manager {
    fn drop(&mut self) {
        // Safety: the manager was initialized by a successful `open`, and every
        // `Results` referencing it held an `Arc` to this `Manager`, so all of
        // them have been dropped (and their data set references released) before
        // this runs, and no reload is running. Freeing the manager frees the
        // data set in use, and the box is reclaimed afterwards.
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
/// [`Results::value_as_string`]. The native results hold the data set they
/// were created from, which a reload of the manager does not change, until
/// they are freed. Holds an [`Arc`] to its [`Manager`] too.
pub struct Results {
    results: NonNull<sys::ResultsHash>,
    /// The data set the native results hold, in which a name is looked up.
    data_set: NonNull<sys::DataSetHash>,
    manager: Arc<Manager>,
}

// Safety: a native results structure may be moved between threads, so `Results`
// is `Send`. It is per-thread scratch that the native engine mutates in place
// during processing and value reads, so it must not be shared across threads
// concurrently and is deliberately not `Sync`.
unsafe impl Send for Results {}

impl Results {
    /// The manager these results were created from. They read the data set
    /// it held then.
    pub fn manager(&self) -> &Arc<Manager> {
        &self.manager
    }

    fn as_ptr(&self) -> *mut sys::ResultsHash {
        self.results.as_ptr()
    }

    /// The required property index of the property named in the data set
    /// these results hold, ignoring case, or [`None`] when it is not one of
    /// its required properties or the name holds a nul byte.
    fn required_property_index(&self, property_name: &str) -> Option<c_int> {
        // Safety: the results hold the data set until they are freed, and the
        // name is null terminated for the length of the call.
        let index = with_c_str(property_name, |name| unsafe {
            sys::fiftyoneDegreesShimHashDataSetGetRequiredPropertyIndexFromName(
                self.data_set.as_ptr(),
                name,
            )
        })?;
        (index >= 0).then_some(index)
    }

    /// Whether these results and `other` hold the same data set.
    ///
    /// Results hold the data set in use when they were created, a data set
    /// results hold is not freed, so no later one can take its place in
    /// memory, and a manager never takes back a data set a reload has
    /// replaced. So results created before some reads of a manager and
    /// results created after them hold the same data set only when no reload
    /// replaced it between, which shows that the reads all came from it.
    pub fn same_data_set(&self, other: &Results) -> bool {
        self.data_set == other.data_set
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
        let Some(index) = self.required_property_index(property_name) else {
            return Ok(false);
        };
        let mut exception = sys::Exception::cleared();
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
        // An unknown property has no value, matching the engine's own
        // behavior.
        if self.required_property_index(property_name).is_none() {
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
    /// that read the same properties on every detection. A reload keeps the
    /// names, so an index names the same property after it. Returns [`None`]
    /// when the results carry no value, and an error for an index outside the
    /// data set's required properties.
    pub fn value_as_string_by_index(
        &self,
        index: c_int,
        separator: &CStr,
    ) -> Result<Option<String>> {
        let count = self.manager.required_count as usize;
        if index < 0 || index as usize >= count {
            return Err(Error::Native {
                status: String::from("InvalidInput"),
                message: format!("{index} is not a required property index (0..{count})"),
            });
        }
        let results = self.as_ptr();
        let mut exception = sys::Exception::cleared();
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

    /// Read each value of a required property by the index
    /// [`Manager::required_property_index`] returned for it, as the C++
    /// `ResultsHash` in 51degrees-cxx reads them.
    ///
    /// Unlike [`value_as_string_by_index`](Self::value_as_string_by_index),
    /// the values are kept apart, so one empty value is told apart from none
    /// and a value holding a separator stays whole. Returns [`None`] when the
    /// results hold no value for the property, as
    /// `fiftyoneDegreesResultsHashGetHasValues` decides for a detection with
    /// no evidence, one that matched nothing or a component with no profile,
    /// and an empty list when the matched profile holds no value for it. A
    /// reload keeps the names, so an index names the same property after it.
    /// Returns an error for an index outside the data set's required
    /// properties, or when the values cannot be read.
    pub fn values_by_index(&self, index: c_int) -> Result<Option<Vec<String>>> {
        let count = self.manager.required_count as usize;
        if index < 0 || index as usize >= count {
            return Err(Error::Native {
                status: String::from("InvalidInput"),
                message: format!("{index} is not a required property index (0..{count})"),
            });
        }
        let mut values: Vec<String> = Vec::new();
        let mut exception = sys::Exception::cleared();
        // Safety: the index is in range for this data set, and
        // `take_result_value` treats the context as the `Vec<String>` it
        // points to, which nothing else uses during the call.
        let read = unsafe {
            sys::fiftyoneDegreesShimHashGetResultValues(
                self.as_ptr(),
                index,
                take_result_value,
                (&mut values as *mut Vec<String>).cast::<c_void>(),
                &mut exception,
            )
        };
        exception.check()?;
        match read {
            sys::SHIM_VALUES_READ => Ok(Some(values)),
            sys::SHIM_VALUES_NONE => Ok(None),
            _ => Err(Error::Native {
                status: String::from("NoValuesAvailable"),
                message: String::from("No values are available for the property requested"),
            }),
        }
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
    /// cannot be read, or a graph this call walks cannot be read from the
    /// data file. The walks stop at the first that fails, and a graph is never
    /// walked twice, so after a failed walk a later call walks only the graphs
    /// still to walk and gives the metrics the results then hold, as the C++
    /// results do.
    pub fn match_metrics(&self) -> Option<MatchMetrics> {
        let mut method = sys::HashMatchMethod::None;
        let mut difference = 0;
        let mut drift = 0;
        let mut iterations = 0;
        let mut matched_nodes = 0;
        let mut exception = sys::Exception::cleared();
        // Safety: the results came from `ResultsHashCreate` and remain valid
        // for the lifetime of `self`, and every output pointer and the
        // exception are writable.
        unsafe {
            sys::fiftyoneDegreesResultsHashGetMatchMetrics(
                self.as_ptr(),
                &mut method,
                &mut difference,
                &mut drift,
                &mut iterations,
                &mut matched_nodes,
                &mut exception,
            )
        };
        if !exception.is_okay() {
            return None;
        }
        Some(MatchMetrics {
            method: method as i32,
            difference,
            drift,
            iterations,
            matched_nodes,
        })
    }

    /// The matched User-Agent of each result, without repeats, in result
    /// order. Each is the evidence value the component was matched from, with
    /// the characters the match did not use replaced by underscores. Empty
    /// when nothing matched, and empty or partial when a graph cannot be
    /// read from the data file.
    /// Any graph not yet walked is walked first, which can read the data file.
    /// [`try_matched_user_agents`](Self::try_matched_user_agents) tells a
    /// failed walk apart.
    pub fn matched_user_agents(&self) -> Vec<String> {
        self.read_matched_user_agents()
    }

    /// The matched User-Agents, as
    /// [`matched_user_agents`](Self::matched_user_agents) gives them, or
    /// [`None`] when a graph this call walks cannot be read from the data
    /// file. As with [`match_metrics`](Self::match_metrics), after a failed
    /// walk a later call walks only the graphs still to walk and gives the
    /// User-Agents the results then hold.
    pub fn try_matched_user_agents(&self) -> Option<Vec<String>> {
        if !self.walk_pending_graphs() {
            return None;
        }
        Some(self.read_matched_user_agents())
    }

    /// Walk the graph of each component not yet walked, and return false
    /// when a walk fails.
    fn walk_pending_graphs(&self) -> bool {
        let mut exception = sys::Exception::cleared();
        // Safety: the results came from `ResultsHashCreate` and remain valid
        // for the lifetime of `self`, and the exception is writable. A null
        // output pointer skips its metric, so the call only walks the graphs.
        unsafe {
            sys::fiftyoneDegreesResultsHashGetMatchMetrics(
                self.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut exception,
            )
        };
        exception.is_okay()
    }

    /// The matched User-Agent of each result, without repeats, in result
    /// order, walking any graph not yet walked.
    fn read_matched_user_agents(&self) -> Vec<String> {
        // Safety: the results came from `ResultsHashCreate` and remain valid
        // for the lifetime of `self`.
        let count = unsafe { sys::fiftyoneDegreesShimHashGetResultCount(self.as_ptr()) };
        let mut user_agents: Vec<String> = Vec::new();
        for index in 0..count {
            let mut exception = sys::Exception::cleared();
            // Safety: as above, and the exception is writable. A failed walk
            // gives null, which is skipped like a result with none recorded.
            let matched = unsafe {
                sys::fiftyoneDegreesResultsHashGetMatchedUserAgent(
                    self.as_ptr(),
                    index,
                    &mut exception,
                )
            };
            if matched.is_null() {
                continue;
            }
            // Safety: the C library null terminates the string, which stays
            // valid until the results are populated again or freed, and
            // neither can happen while `self` is borrowed.
            let user_agent = unsafe { CStr::from_ptr(matched) }.to_string_lossy();
            if user_agent.is_empty() {
                continue;
            }
            if !user_agents.iter().any(|known| *known == user_agent) {
                user_agents.push(user_agent.into_owned());
            }
        }
        user_agents
    }

    /// The number of components, such as the hardware or the browser, whose
    /// graph is still to be walked. When the data set was opened with no
    /// properties named, a detection walks no graph, and the first read of
    /// one of a component's properties walks that component's graph. The
    /// match metrics, the matched User-Agents and the device id need every
    /// graph, so reading any of them walks the rest. When properties were
    /// named, the detection walks the graphs they need and this is zero.
    pub fn pending_graphs(&self) -> u32 {
        // Safety: the results came from `ResultsHashCreate` and remain valid
        // for the lifetime of `self`. Counting reads nothing from the file.
        unsafe { sys::fiftyoneDegreesShimHashGetPendingGraphCount(self.as_ptr()) }
    }
}

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

/// Take one value from `fiftyoneDegreesShimHashGetResultValues` into the
/// `Vec<String>` that `context` points to.
///
/// # Safety
/// `context` must point to a `Vec<String>` that nothing else uses during the
/// call, and `value` must be a null terminated string.
unsafe extern "C" fn take_result_value(context: *mut c_void, _field: c_int, value: *const c_char) {
    // Safety: the caller's contract above.
    let (values, text) = unsafe {
        (
            &mut *context.cast::<Vec<String>>(),
            CStr::from_ptr(value).to_string_lossy().into_owned(),
        )
    };
    values.push(text);
}

/// The script the client runs for `body`, a value of the
/// `JavascriptHardwareProfile` property, as the C library writes it.
///
/// The body pushes the ids of the profiles it finds onto a `profileIds` array
/// it does not declare. The script declares the array, runs the body, then
/// writes the ids to the `51D_ProfileIds` cookie, which the engine reads back
/// as evidence. An empty body gives the script too. As in C, `body` is read up
/// to its first null.
pub fn hardware_profile_script(body: &str) -> String {
    let body = body.split('\0').next().unwrap_or(body);
    let body = CString::new(body).expect("the text before a null holds none");
    // Safety: `body` is null terminated. A null buffer only finds the length,
    // and the buffer is then writable for the script and the null terminator.
    let script = unsafe {
        let length = sys::fiftyoneDegreesHashGetHardwareProfileScript(
            body.as_ptr(),
            std::ptr::null_mut(),
            0,
        );
        let mut buffer = vec![0u8; length + 1];
        sys::fiftyoneDegreesHashGetHardwareProfileScript(
            body.as_ptr(),
            buffer.as_mut_ptr() as *mut c_char,
            buffer.len(),
        );
        buffer.truncate(length);
        buffer
    };
    String::from_utf8_lossy(&script).into_owned()
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
    use crate::metadata::METADATA_STRING_BUFFER_LENGTH;
    use std::path::PathBuf;

    use fiftyone_pipeline_core::Evidence;

    /// The text the C++ results give for a `JavascriptHardwareProfile`
    /// value, `body` wrapped in the script that sets the profile ids cookie.
    fn cpp_hardware_profile_script(body: &str) -> String {
        format!(
            "var profileIds = []\n{body}\n\
             document.cookie = \"51D_ProfileIds=\" + profileIds.join(\"|\")"
        )
    }

    #[test]
    fn hardware_profile_script_is_the_text_the_cpp_results_wrote() {
        for body in ["profileIds.push(1)", ""] {
            assert_eq!(
                hardware_profile_script(body),
                cpp_hardware_profile_script(body),
                "the script for {body:?}"
            );
        }
        // Longer than any first guess at the length, so the length the C
        // library asks for is what sizes the buffer.
        let long = "profileIds.push(1);".repeat(1000);
        assert_eq!(
            hardware_profile_script(&long),
            cpp_hardware_profile_script(&long)
        );
        // C reads a body up to its first null.
        assert_eq!(
            hardware_profile_script("profileIds.push(1)\0x"),
            cpp_hardware_profile_script("profileIds.push(1)")
        );
    }

    /// The Hash data file `_51DEGREES_DD_PATH` names, or the Lite file in the
    /// `cxx` submodule.
    pub(super) fn data_file() -> Option<PathBuf> {
        if let Ok(path) = std::env::var("_51DEGREES_DD_PATH") {
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
                let mut method = sys::HashMatchMethod::Length;
                let mut metrics = MatchMetrics {
                    method: -1,
                    difference: -1,
                    drift: -1,
                    iterations: -1,
                    matched_nodes: -1,
                };
                let mut exception = sys::Exception::cleared();
                // Safety: as above, and every output pointer and the
                // exception are writable.
                let read = unsafe {
                    sys::fiftyoneDegreesResultsHashGetMatchMetricsForResult(
                        results.as_ptr(),
                        index,
                        &mut method,
                        &mut metrics.difference,
                        &mut metrics.drift,
                        &mut metrics.iterations,
                        &mut metrics.matched_nodes,
                        &mut exception,
                    )
                };
                assert!(read, "result {index} should be readable");
                assert!(exception.is_okay(), "result {index} walks");
                metrics.method = method as i32;
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

    /// With no properties named, a detection walks no graph, reading a
    /// property walks only its component's graph, and reading what needs
    /// every component walks the rest.
    #[test]
    fn a_read_walks_only_the_graphs_it_needs() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping the graph walks");
            return;
        };
        let manager = Manager::open(&path, PerformanceProfile::Default).expect("data file loads");
        let results = detect(&manager, "header.user-agent", CHROME_ON_WINDOWS);
        // Safety: the results are valid for the duration of the borrow.
        let count = unsafe { sys::fiftyoneDegreesShimHashGetResultCount(results.as_ptr()) };
        assert!(count > 1, "a result for each component: {count}");
        assert_eq!(results.pending_graphs(), count);
        results
            .value_as_string("IsMobile", "|")
            .expect("IsMobile reads");
        assert_eq!(results.pending_graphs(), count - 1);
        results.match_metrics().expect("metrics are readable");
        assert_eq!(results.pending_graphs(), 0);

        let results = detect(&manager, "header.user-agent", CHROME_ON_WINDOWS);
        results.device_id().expect("the device id reads");
        assert_eq!(results.pending_graphs(), 0, "the device id");
        let results = detect(&manager, "header.user-agent", CHROME_ON_WINDOWS);
        assert!(!results.matched_user_agents().is_empty());
        assert_eq!(results.pending_graphs(), 0, "the matched User-Agents");
        let results = detect(&manager, "header.user-agent", CHROME_ON_WINDOWS);
        let checked = results
            .try_matched_user_agents()
            .expect("every graph walks");
        assert_eq!(results.pending_graphs(), 0, "the checked User-Agents");
        assert!(!checked.is_empty());
        assert_eq!(checked, results.matched_user_agents());
    }

    /// With properties named, a detection walks the graphs they need, so
    /// none is left for a read to walk.
    #[test]
    fn named_properties_leave_no_graph_to_walk() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping the named graph walks");
            return;
        };
        let manager =
            Manager::open_with_properties(&path, PerformanceProfile::Default, Some(&["IsMobile"]))
                .expect("data file loads");
        let results = detect(&manager, "header.user-agent", CHROME_ON_WINDOWS);
        assert_eq!(results.pending_graphs(), 0);
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

    /// Reading each value apart gives the values the joined read joins, and
    /// results that matched nothing hold no value for any property.
    #[test]
    fn values_read_apart_are_the_values_the_joined_read_joins() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping values read apart");
            return;
        };
        let manager = Manager::open(&path, PerformanceProfile::Default).expect("data file loads");
        let names = manager.property_names();
        let index = |name: &str| {
            manager
                .required_property_index(name)
                .expect("a listed property has an index")
        };
        let matched = detect(&manager, "header.user-agent", CHROME_ON_WINDOWS);
        let mut read = 0;
        for name in &names {
            let values = matched.values_by_index(index(name)).expect("read apart");
            let joined = matched
                .value_as_string_by_index(index(name), c"|")
                .expect("read joined");
            if let Some(values) = values {
                read += 1;
                assert_eq!(Some(values.join("|")), joined, "{name}");
            }
        }
        assert!(read > 1, "{read} of {} properties read", names.len());
        assert_eq!(
            matched
                .values_by_index(index("IsMobile"))
                .expect("read apart"),
            Some(vec!["False".to_owned()])
        );

        let nothing = detect(&manager, "header.x-not-read", "nothing to match");
        for name in &names {
            assert_eq!(
                nothing.values_by_index(index(name)).expect("read apart"),
                None,
                "{name}"
            );
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

    /// The handles the C library opens when no concurrency is named, one
    /// for each processor as it counts them, to at most 64.
    fn handles_for_the_processors() -> u16 {
        // Safety: the function reads no state.
        unsafe { fiftyone_common_sys::fiftyoneDegreesThreadingGetProcessorCount() }.min(64)
    }

    /// Every profile but `InMemory` opens a handle to the data file for each
    /// concurrent operation it expects, to at most 64, and keeps it open,
    /// `HighPerformance` included. With no concurrency named, it expects one
    /// for each processor.
    #[test]
    fn the_file_pool_has_a_handle_for_each_expected_operation() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping the file pool");
            return;
        };
        let processors = handles_for_the_processors();
        for (profile, concurrency, handles) in [
            (PerformanceProfile::LowMemory, Some(3), 3),
            (PerformanceProfile::Balanced, None, processors),
            (PerformanceProfile::Balanced, Some(500), 64),
            (PerformanceProfile::HighPerformance, Some(3), 3),
            (PerformanceProfile::HighPerformance, Some(40), 40),
            (PerformanceProfile::HighPerformance, None, processors),
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

    /// The C library's name for a collection held whole in memory.
    const IN_MEMORY: &str = "CollectionMemory";

    /// The C library's name for a collection holding in memory only what the
    /// required properties read, and reading the rest from the file.
    const HELD: &str = "CollectionHeld";

    /// The C library's type names of the collections `manager`'s data set
    /// holds its strings and its values in.
    fn strings_and_values_types(manager: &Manager) -> (String, String) {
        let mut strings: *const c_char = std::ptr::null();
        let mut values: *const c_char = std::ptr::null();
        // Safety: the manager is initialized and both outputs are writable.
        let found = unsafe {
            sys::fiftyoneDegreesShimHashGetCollectionTypeNames(
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
    /// values those properties read and the strings those values use, and
    /// reads the others from the data file when asked. The named property
    /// gives the value it gives with none named, and so do the values of a
    /// property not named.
    #[test]
    fn high_performance_naming_some_holds_only_what_they_read() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping held collections");
            return;
        };
        let profile = PerformanceProfile::HighPerformance;
        let some = Manager::open_with_properties(&path, profile, Some(&["IsMobile"]))
            .expect("data file loads");
        let every = Manager::open(&path, profile).expect("data file loads");
        assert_eq!(
            strings_and_values_types(&some),
            (String::from(HELD), String::from(HELD))
        );

        let is_mobile = |manager: &Arc<Manager>| {
            detect(manager, "header.user-agent", CHROME_ON_WINDOWS)
                .value_as_string("IsMobile", ",")
                .expect("IsMobile reads")
        };
        assert_eq!(is_mobile(&some), Some(String::from("False")));
        assert_eq!(is_mobile(&some), is_mobile(&every));

        // Which properties are available differs by design. What the data
        // file says of each, read through the values and strings, does not.
        let metadata = |manager: &Manager| -> Vec<_> {
            manager
                .data_file_properties()
                .expect("properties read")
                .into_iter()
                .map(|property| {
                    (
                        property.name,
                        property.description,
                        property.default_value,
                        property.value_count,
                    )
                })
                .collect()
        };
        assert_eq!(metadata(&some), metadata(&every));
        let properties = every.data_file_properties().expect("properties read");
        let browser_name = properties
            .iter()
            .position(|property| property.name == "BrowserName")
            .and_then(|index| u32::try_from(index).ok())
            .expect("the data file has BrowserName");
        let values = some
            .data_file_property_values(browser_name)
            .expect("values read from the file");
        assert!(!values.is_empty(), "BrowserName has values");
        assert_eq!(
            values,
            every
                .data_file_property_values(browser_name)
                .expect("values read")
        );
    }

    /// `HighPerformance` with no property named holds every string and value
    /// in memory.
    #[test]
    fn high_performance_naming_none_holds_everything() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping held collections");
            return;
        };
        let every =
            Manager::open(&path, PerformanceProfile::HighPerformance).expect("data file loads");
        assert_eq!(
            strings_and_values_types(&every),
            (String::from(IN_MEMORY), String::from(IN_MEMORY))
        );
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

    /// A header whose dates are zero, or whose year is -1 with a valid month
    /// and day, holds no valid date, so neither date is read. The year -1
    /// would otherwise be read as a date before year 1.
    #[test]
    fn a_header_holding_no_valid_date_gives_no_dates() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping the dates that are not valid");
            return;
        };
        let bytes = std::fs::read(&path).expect("data file reads");
        let mut zeroed = bytes.clone();
        zeroed[PUBLISHED_AT..PUBLISHED_AT + 8].fill(0);
        let mut year_minus_one = bytes;
        for at in [PUBLISHED_AT, PUBLISHED_AT + 4] {
            year_minus_one[at..at + 4].copy_from_slice(&[0xFF, 0xFF, 1, 1]);
        }
        assert_eq!(header_dates(&year_minus_one), ((-1, 1, 1), (-1, 1, 1)));
        for (case, data) in [("zeroed", zeroed), ("year -1", year_minus_one)] {
            let manager = Manager::open_from_memory(
                data,
                PerformanceProfile::LowMemory,
                None,
                &HashOptions::default(),
            )
            .expect("data loads");
            assert_eq!(manager.data_set_published(), None, "{case}");
            assert_eq!(manager.data_set_next_update(), None, "{case}");
        }
    }

    /// A day the month does not have is not a date, so neither date is read,
    /// whilst 29 February is read in a leap year. A year divisible by 100 is
    /// a leap year only when it is also divisible by 400.
    #[test]
    fn a_day_the_month_lacks_is_not_a_date() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping the days months lack");
            return;
        };
        let bytes = std::fs::read(&path).expect("data file reads");
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
            let manager = Manager::open_from_memory(
                data,
                PerformanceProfile::LowMemory,
                None,
                &HashOptions::default(),
            )
            .expect("data loads");
            let date = (i32::from(year), u32::from(month), u32::from(day));
            let expected = read.then_some(date);
            assert_eq!(manager.data_set_published(), expected, "{date:?}");
            assert_eq!(manager.data_set_next_update(), expected, "{date:?}");
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

    /// A reload from a file, from the original file and from memory keeps the
    /// names and their indexes, and results created before it read the data
    /// set they were created from, by name and by index, as new results read
    /// the new one, with names and with none.
    #[test]
    fn a_reload_keeps_the_names_and_the_results_created_before_it() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping the reloads");
            return;
        };
        let bytes = std::fs::read(&path).expect("data file reads");
        let separator = c"|";
        for named in [None, Some(&["PlatformName", "IsMobile"][..])] {
            let manager = Manager::open_with_hash_options(
                &path,
                PerformanceProfile::HighPerformance,
                named,
                &HashOptions::default(),
            )
            .expect("data file loads");
            let names = manager.property_names();
            let index = manager
                .required_property_index("IsMobile")
                .expect("IsMobile is a required property");
            let expected = detect(&manager, "header.user-agent", CHROME_ON_WINDOWS);
            for reload in ["file", "original file", "memory"] {
                let context = format!("{reload}, named {named:?}");
                let before = detect(&manager, "header.user-agent", CHROME_ON_WINDOWS);
                match reload {
                    "file" => manager.reload_from_file(&path),
                    "original file" => manager.reload_from_original_file(),
                    _ => manager.reload_from_memory(&bytes),
                }
                .expect("the data set reloads");
                assert_eq!(manager.property_names(), names, "{context}");
                assert_eq!(
                    manager.required_property_index("IsMobile"),
                    Some(index),
                    "{context}"
                );
                let after = detect(&manager, "header.user-agent", CHROME_ON_WINDOWS);
                assert!(!before.same_data_set(&after), "{context}");
                assert!(
                    after.same_data_set(&manager.create_results().expect("results allocate")),
                    "{context}"
                );
                for results in [&before, &after] {
                    for name in ["IsMobile", "PlatformName"] {
                        assert_eq!(
                            results.has_values(name).expect("has values reads"),
                            expected.has_values(name).expect("has values reads"),
                            "{name}, {context}"
                        );
                        assert_eq!(
                            results.value_as_string(name, "|").expect("value reads"),
                            expected.value_as_string(name, "|").expect("value reads"),
                            "{name}, {context}"
                        );
                    }
                    assert_eq!(
                        results
                            .value_as_string_by_index(index, separator)
                            .expect("value reads"),
                        expected
                            .value_as_string_by_index(index, separator)
                            .expect("value reads"),
                        "{context}"
                    );
                    assert_eq!(
                        results.values_by_index(index).expect("values read"),
                        expected.values_by_index(index).expect("values read"),
                        "{context}"
                    );
                }
            }
        }
    }

    /// A reload that fails, from a file that is not there, from part of a data
    /// file or from nothing, gives the C library's status and leaves the data
    /// set in use.
    #[test]
    fn a_failed_reload_leaves_the_data_set_in_use() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping the failed reloads");
            return;
        };
        let bytes = std::fs::read(&path).expect("data file reads");
        let manager = Manager::open(&path, PerformanceProfile::InMemory).expect("data file loads");
        let names = manager.property_names();
        let is_mobile = |manager: &Arc<Manager>| {
            detect(manager, "header.user-agent", CHROME_ON_WINDOWS)
                .value_as_string("IsMobile", "|")
                .expect("value reads")
        };
        let expected = is_mobile(&manager);
        let missing = path.with_file_name("no-such-data-file.hash");
        for (name, reloaded, status) in [
            (
                "a missing file",
                manager.reload_from_file(&missing),
                "FileNotFound",
            ),
            (
                "half the file",
                manager.reload_from_memory(&bytes[..bytes.len() / 2]),
                "CorruptData",
            ),
            ("nothing", manager.reload_from_memory(&[]), "CorruptData"),
        ] {
            let error = reloaded
                .err()
                .unwrap_or_else(|| panic!("{name} should be refused"));
            let Error::Native {
                status: given,
                message,
            } = &error
            else {
                panic!("{name}: {error}");
            };
            assert_eq!(given, status, "{name}: {message}");
            assert_eq!(manager.property_names(), names, "{name}");
            assert_eq!(is_mobile(&manager), expected, "{name}");
        }
    }

    /// Every profile expects one concurrent operation for each processor
    /// when none is named, and loads with any concurrency named, the
    /// `Balanced` values cache of 500 entries taking more than its square
    /// root.
    #[test]
    fn every_profile_takes_any_concurrency() {
        // Safety: the function reads no state.
        let processors =
            unsafe { fiftyone_common_sys::fiftyoneDegreesThreadingGetProcessorCount() };
        for profile in [
            PerformanceProfile::LowMemory,
            PerformanceProfile::Balanced,
            PerformanceProfile::Default,
            PerformanceProfile::HighPerformance,
            PerformanceProfile::InMemory,
        ] {
            assert_eq!(
                default_concurrency(profile),
                processors,
                "{}",
                profile.as_str()
            );
        }
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping concurrency");
            return;
        };
        for profile in [PerformanceProfile::Balanced, PerformanceProfile::Default] {
            let manager = Manager::open_with_options(&path, profile, None, Some(500))
                .expect("any concurrency loads");
            let results = detect(&manager, "header.user-agent", CHROME_ON_WINDOWS);
            assert_eq!(
                results
                    .value_as_string("IsMobile", ",")
                    .expect("IsMobile reads"),
                Some(String::from("False")),
                "{}",
                profile.as_str()
            );
        }
    }

    /// The evidence keys are the C library's list for the data set,
    /// lowercased: each header's `header.` and `query.` keys, a `cookie.`
    /// and a `query.` key for each property the evidence can override, then
    /// the seven keys every data set reads, with no `HTTP_` before a header
    /// name.
    #[test]
    fn the_evidence_keys_are_the_data_sets_list_lowercased() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping the evidence keys");
            return;
        };
        let manager = Manager::open(&path, PerformanceProfile::Default).expect("data file loads");
        let keys = manager.evidence_keys();
        let fixed = [
            "query.51d_deviceid",
            "query.51d_gethighentropyvalues",
            "query.51d_structureduseragent",
            "cookie.51d_gethighentropyvalues",
            "cookie.51d_structureduseragent",
            "query.51d_profileids",
            "cookie.51d_profileids",
        ];
        assert!(keys.len() > fixed.len(), "{keys:?}");
        assert_eq!(&keys[keys.len() - fixed.len()..], fixed, "{keys:?}");
        for key in &keys {
            assert_eq!(*key, key.to_lowercase(), "{keys:?}");
            assert!(!key.contains(".http_"), "{keys:?}");
        }
        for pair in [
            ["header.user-agent", "query.user-agent"],
            [
                "cookie.51d_javascripthardwareprofile",
                "query.51d_javascripthardwareprofile",
            ],
        ] {
            assert!(keys.windows(2).any(|w| w == pair), "{pair:?} {keys:?}");
        }
    }

    /// No profile reads a header name with `HTTP_` before it, as .NET sets
    /// in its Hash engine builder, so the header keys are the names the data
    /// set holds.
    #[test]
    fn no_profile_reads_http_prefixed_header_names() {
        for profile in [
            PerformanceProfile::LowMemory,
            PerformanceProfile::Balanced,
            PerformanceProfile::Default,
            PerformanceProfile::HighPerformance,
            PerformanceProfile::InMemory,
        ] {
            let config = profile_config(profile, &HashOptions::default());
            assert!(
                !config.b.b.uses_upper_prefixed_headers,
                "{}",
                profile.as_str()
            );
        }
    }

    /// The copy of a profile's configuration a data set opens with keeps
    /// the profile's `load_required_properties_only`, which only
    /// `HighPerformance` sets.
    #[test]
    fn the_copied_configuration_keeps_load_required_properties_only() {
        for profile in [
            PerformanceProfile::LowMemory,
            PerformanceProfile::Balanced,
            PerformanceProfile::Default,
            PerformanceProfile::HighPerformance,
            PerformanceProfile::InMemory,
        ] {
            let config = profile_config(profile, &HashOptions::default());
            assert_eq!(
                config.load_required_properties_only,
                profile == PerformanceProfile::HighPerformance,
                "{}",
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
            (0, 0, false, true, false, true, 0, 500, false)
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
            indexed_properties: vec![String::from("BrowserName")],
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
            let err = results
                .values_by_index(index)
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

    /// The C library finds a property name ignoring case, so a name in
    /// another case than the data file's gives the property's index, and the
    /// reads by name give the property's answers.
    #[test]
    fn a_name_in_another_case_than_the_data_files_is_read() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping a name in another case");
            return;
        };
        let manager = Manager::open(&path, PerformanceProfile::Default).expect("data file loads");
        let index = manager
            .required_property_index("IsMobile")
            .expect("IsMobile is in every Hash data file");
        for name in ["ismobile", "ISMOBILE", "isMobile"] {
            assert_eq!(manager.required_property_index(name), Some(index), "{name}");
        }
        let results = detect(&manager, "header.user-agent", CHROME_ON_WINDOWS);
        assert!(results.has_values("ismobile").expect("has values is read"));
        let value = results
            .value_as_string("ISMOBILE", "|")
            .expect("IsMobile is read by name");
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

            // The name must match exactly up to any null, so another case,
            // part of the name, more than the name, or another property's
            // value finds nothing.
            let index_of = |name: &str| {
                properties
                    .iter()
                    .find(|p| p.name == name)
                    .expect("the property is in every Hash data file")
                    .index
            };
            let is_mobile = index_of("IsMobile");
            assert!(find(index_of("BrowserName"), "Safari").is_some());
            for name in ["true", "TRUE", "Tru", "True ", "TrueX", "", "Safari"] {
                assert_eq!(find(is_mobile, name), None, "{name:?} {}", profile.as_str());
            }
            // .NET's GetValue gives True for each, because the C++ search
            // ends at the null.
            for name in ["True\0", "True\0x", "True\0False"] {
                assert_eq!(
                    find(is_mobile, name).map(|value| value.name).as_deref(),
                    Some("True"),
                    "{name:?} {}",
                    profile.as_str()
                );
            }
            assert_eq!(find(is_mobile, "Tru\0e"), None, "{}", profile.as_str());
            let outside = u32::try_from(properties.len()).expect("few properties");
            assert_eq!(find(outside, "True"), None, "{}", profile.as_str());
        }
    }

    /// Places in a Hash data file, from the packed
    /// `fiftyoneDegreesDataSetHashHeader` in the C library's `hash.h`,
    /// `fiftyoneDegreesProperty` in `property.h` and `fiftyoneDegreesValue`
    /// in `value.h`. A collection's header is its start, its length in bytes
    /// and its count, each four bytes, and a string is its length in two bytes,
    /// then its characters.
    const NAME_HEADER: usize = 58;
    const STRINGS_HEADER: usize = 74;
    const MAPS_HEADER: usize = 98;
    const PROPERTIES_HEADER: usize = 110;
    const VALUES_HEADER: usize = 122;
    const PROPERTY_SIZE: usize = 44;
    const DEFAULT_VALUE_INDEX: usize = 8;
    const NAME_OFFSET: usize = 12;
    const DESCRIPTION_OFFSET: usize = 16;
    const CATEGORY_OFFSET: usize = 20;
    const URL_OFFSET: usize = 24;
    const FIRST_VALUE_INDEX: usize = 28;
    const LAST_VALUE_INDEX: usize = 32;
    const MAP_COUNT: usize = 36;
    const FIRST_MAP_INDEX: usize = 40;
    const VALUE_SIZE: usize = 14;
    const VALUE_NAME_OFFSET: usize = 2;

    fn read_u32(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
    }

    fn write_u32(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    /// The header ends after the profile offsets, the sixth collection header
    /// from the properties' own.
    #[test]
    fn the_header_size_is_the_hash_data_file_header() {
        const COLLECTION_HEADER_SIZE: usize = 12;
        assert_eq!(
            data_file_header_size(),
            PROPERTIES_HEADER + 6 * COLLECTION_HEADER_SIZE
        );
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

    /// The default value is the property's own value with the name of the
    /// value at its default value index, wherever that value is, or with an
    /// empty name when it has no index, as the C++
    /// MetaDataHash::getDefaultValueForProperty finds it. Every default in
    /// the Lite file is one of its own property's values and no value there
    /// has an empty name, so a copy is changed to give the other cases.
    #[test]
    fn a_default_value_is_found_by_its_name_among_the_property_values() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping default value names");
            return;
        };
        let mut bytes = std::fs::read(&path).expect("the data file is read");
        let start = read_u32(&bytes, PROPERTIES_HEADER) as usize;
        let count = read_u32(&bytes, PROPERTIES_HEADER + 8);
        let values_start = read_u32(&bytes, VALUES_HEADER) as usize;
        let values_count = read_u32(&bytes, VALUES_HEADER + 8);
        let strings_start = read_u32(&bytes, STRINGS_HEADER) as usize;
        let record = |index: u32| start + index as usize * PROPERTY_SIZE;
        let value_name =
            |index: u32| values_start + index as usize * VALUE_SIZE + VALUE_NAME_OFFSET;

        let manager = Manager::open(&path, PerformanceProfile::InMemory).expect("data file loads");
        let original = manager
            .data_file_properties()
            .expect("the properties are read");
        let named = |name: &str| {
            original
                .iter()
                .find(|p| p.name == name)
                .expect("the property is in every Hash data file")
        };
        let values = |name: &str| {
            manager
                .data_file_property_values(named(name).index)
                .expect("the values are read")
        };
        let is_mobile = values("IsMobile");
        let platform_names = values("PlatformName");
        let widths = values("ScreenPixelsWidth");
        let browser_names = values("BrowserName");
        let artificial = values("IsArtificialIntelligence");
        drop(manager);

        // The index in the values collection of the value named `name` among
        // the property `of`'s values, which are read in the data file's order.
        let index_of = |of: &str, among: &[DataFileValue], name: &str| {
            let place = among
                .iter()
                .position(|value| value.name == name)
                .expect("the property has the value");
            read_u32(&bytes, record(named(of).index) + FIRST_VALUE_INDEX)
                + u32::try_from(place).expect("few values")
        };
        let artificial_true = index_of("IsArtificialIntelligence", &artificial, "True");
        let mobile_false = index_of("IsMobile", &is_mobile, "False");
        let mobile_true = is_mobile
            .iter()
            .find(|value| value.name == "True")
            .expect("IsMobile has True")
            .clone();
        assert!(platform_names.iter().all(|value| value.name != "False"));
        assert!(widths.iter().all(|value| !value.name.is_empty()));

        // BrowserName has no default value index. Its first value's name is
        // made empty, which is the first name in order, so that value becomes
        // its default, as a value with an empty name is in the Enterprise
        // file. No other value or property may use the name's string.
        let browser = named("BrowserName").index;
        assert_eq!(named("BrowserName").default_value, None);
        assert_eq!(
            read_u32(&bytes, record(browser) + DEFAULT_VALUE_INDEX),
            u32::MAX
        );
        let first = read_u32(&bytes, record(browser) + FIRST_VALUE_INDEX);
        let offset = read_u32(&bytes, value_name(first));
        assert_eq!(
            (0..values_count)
                .filter(|&index| read_u32(&bytes, value_name(index)) == offset)
                .count(),
            1
        );
        for index in 0..count {
            for field in [NAME_OFFSET, DESCRIPTION_OFFSET, CATEGORY_OFFSET, URL_OFFSET] {
                assert_ne!(read_u32(&bytes, record(index) + field), offset);
            }
        }
        let empty = DataFileValue {
            name: String::new(),
            ..browser_names[0].clone()
        };

        // IsMobile's default becomes IsArtificialIntelligence's True, so its
        // own True is found by the name. PlatformName's becomes IsMobile's
        // False, a name it has no value with, and ScreenPixelsWidth's none, so
        // neither has one.
        let default_at = |name: &str| record(named(name).index) + DEFAULT_VALUE_INDEX;
        let mobile_at = default_at("IsMobile");
        write_u32(&mut bytes, mobile_at, artificial_true);
        write_u32(&mut bytes, default_at("PlatformName"), mobile_false);
        write_u32(&mut bytes, default_at("ScreenPixelsWidth"), u32::MAX);
        // A string is its size in two bytes, counting the terminator, then its
        // characters, so a size of one then the terminator is empty.
        let at = strings_start + offset as usize;
        bytes[at..at + 3].copy_from_slice(&[1, 0, 0]);

        let dir = std::env::temp_dir().join(format!(
            "fiftyone-native-default-names-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the folder is made");
        let patched = dir.join("51Degrees.hash");
        std::fs::write(&patched, &bytes).expect("the copy is written");
        let changed = [
            "IsMobile",
            "PlatformName",
            "ScreenPixelsWidth",
            "BrowserName",
        ];
        for profile in METADATA_PROFILES {
            let manager = Manager::open(&patched, profile).expect("the copy loads");
            let properties = manager
                .data_file_properties()
                .expect("the properties are read");
            assert_eq!(properties.len(), original.len());
            for (property, before) in properties.iter().zip(&original) {
                let what = format!("{} {}", property.name, profile.as_str());
                if !changed.contains(&property.name.as_str()) {
                    assert_eq!(property, before, "{what}");
                    continue;
                }
                let expected = match property.name.as_str() {
                    "IsMobile" => Some(mobile_true.clone()),
                    "BrowserName" => Some(empty.clone()),
                    _ => None,
                };
                assert_eq!(property.default_value, expected, "{what}");
            }
            assert_eq!(
                manager
                    .data_file_property_value(browser, "")
                    .expect("the values are read"),
                Some(empty.clone()),
                "{}",
                profile.as_str()
            );
        }

        // A default value index past the last value cannot be read, so the
        // property cannot be read, as the C++ builder throws there.
        write_u32(&mut bytes, mobile_at, values_count);
        std::fs::write(&patched, &bytes).expect("the copy is written");
        for profile in METADATA_PROFILES {
            let manager = Manager::open(&patched, profile).expect("the copy loads");
            assert!(
                manager.data_file_properties().is_err(),
                "{}",
                profile.as_str()
            );
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
        let fields = manager
            .read_property_fields(last)
            .expect("the last property is read");
        assert_eq!(fields.available, 1);

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
        assert!(manager.read_property_fields(0).is_some());
        assert!(manager.read_property_fields(last).is_none());
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

    /// A data tier with an empty name is kept in its place, as the C++
    /// `PropertyMetaDataBuilderHash` keeps every name it reads. A copy of the
    /// data file has the first character of the first property's first tier
    /// other than the file's own made a null, so every property in that
    /// tier lists an empty name there.
    #[test]
    fn an_empty_data_tier_name_is_kept() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping an empty tier name");
            return;
        };
        let mut bytes = std::fs::read(&path).expect("the data file is read");
        let strings = read_u32(&bytes, STRINGS_HEADER) as usize;
        let maps = read_u32(&bytes, MAPS_HEADER) as usize;
        let first_property = read_u32(&bytes, PROPERTIES_HEADER) as usize;
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

        let original = Manager::open(&path, PerformanceProfile::LowMemory)
            .expect("data file loads")
            .data_file_properties()
            .expect("the properties are read");
        let mut expected = original.clone();
        for property in &mut expected {
            for listed in &mut property.data_tiers {
                if *listed == name {
                    listed.clear();
                }
            }
        }
        assert_ne!(expected, original, "a property is in the {name} tier");

        let dir =
            std::env::temp_dir().join(format!("fiftyone-native-empty-tier-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the folder is made");
        let copy = dir.join("51Degrees.hash");
        std::fs::write(&copy, &bytes).expect("the copy is written");
        for profile in METADATA_PROFILES {
            let manager = Manager::open(&copy, profile).expect("the copy loads");
            let properties = manager
                .data_file_properties()
                .expect("the properties are read");
            assert_eq!(properties, expected, "{}", profile.as_str());
        }
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
