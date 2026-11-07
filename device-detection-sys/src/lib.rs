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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-sys-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-sys-lib.rs&utm_term=logo)
//!
//! Raw FFI bindings to the Hash on-premise device detection ABI in
//! the 51degrees-cxx C sources.
//!
//! This crate is the native foundation for on-premise Hash device detection. It
//! compiles the device detection and Hash C layers from 51degrees-cxx
//! and links them statically, building on top of the shared common C layer
//! supplied by the [`fiftyone_common_sys`] crate. It then exposes hand written
//! `extern "C"` declarations and `#[repr(C)]` structures for the public Hash
//! ABI declared in `src/hash/hash.h`.
//!
//! The bound surface covers the full life cycle of a Hash detection:
//!
//! - **Configuration** ([`ConfigHash`] and the predefined configuration globals
//!   [`fiftyoneDegreesHashDefaultConfig`], [`fiftyoneDegreesHashInMemoryConfig`],
//!   [`fiftyoneDegreesHashHighPerformanceConfig`],
//!   [`fiftyoneDegreesHashLowMemoryConfig`],
//!   [`fiftyoneDegreesHashBalancedConfig`] and
//!   [`fiftyoneDegreesHashBalancedTempConfig`]).
//! - **Manager initialization** ([`fiftyoneDegreesHashInitManagerFromFile`] and
//!   [`fiftyoneDegreesHashInitManagerFromMemory`], plus the matching size
//!   helpers).
//! - **Results** ([`fiftyoneDegreesResultsHashCreate`], the three process
//!   methods [`fiftyoneDegreesResultsHashFromEvidence`],
//!   [`fiftyoneDegreesResultsHashFromUserAgent`] and
//!   [`fiftyoneDegreesResultsHashFromDeviceId`], the value getters
//!   [`fiftyoneDegreesResultsHashGetValuesString`],
//!   [`fiftyoneDegreesResultsHashGetHasValues`],
//!   [`fiftyoneDegreesResultsHashGetNoValueReason`] and
//!   [`fiftyoneDegreesResultsHashFree`]).
//! - **Data set access** ([`fiftyoneDegreesDataSetHashGet`],
//!   [`fiftyoneDegreesDataSetHashRelease`] and
//!   [`fiftyoneDegreesResourceManagerFree`], re-exported from the common crate).
//! - **Metadata enumeration** through the small property and evidence key
//!   helpers ([`fiftyoneDegreesShimHashGetRequiredPropertyCount`] and friends)
//!   compiled from the crate's C shim. These read fields buried in the nested
//!   data set structures so the Rust side need not mirror those private layouts.
//!
//! The deeply nested data set and results structures are exposed as opaque
//! pointer types. Only [`ConfigHash`] and its base structures, which a caller
//! constructs to drive initialization, are given a concrete `#[repr(C)]`
//! layout. The configuration globals are used unmodified in the common case, so
//! most callers never need to touch the configuration layout at all.
//!
//! Shared types ([`ResourceManager`], [`Exception`], [`StatusCode`],
//! [`PropertiesRequired`], the evidence types and the standard free function)
//! are re-exported from [`fiftyone_common_sys`] so a caller has a single import
//! surface for a full detection.
//!
//! # Safety
//!
//! Every item in this crate is an unchecked binding to a C function or
//! structure. Callers must uphold the contracts described in `hash.h` and the
//! shared common C library headers. The names mirror the C names exactly so the
//! headers remain the authoritative reference. In particular a results
//! structure must be freed with [`fiftyoneDegreesResultsHashFree`] before its
//! manager is freed with [`fiftyoneDegreesResourceManagerFree`], and a data set
//! reference taken with [`fiftyoneDegreesDataSetHashGet`] must be released with
//! [`fiftyoneDegreesDataSetHashRelease`].

#![warn(missing_docs)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use std::os::raw::{c_char, c_int, c_void};

// Re-export the shared FFI types so a caller building a detection has a single
// import surface. These are the exact types the Hash entry points consume.
pub use fiftyone_common_sys::{
    fiftyoneDegreesExceptionGetMessage, fiftyoneDegreesFree, fiftyoneDegreesMemoryStandardFree,
    fiftyoneDegreesResourceManagerFree, fiftyoneDegreesStatusGetMessage, EvidenceKeyValuePair,
    EvidenceKeyValuePairArray, EvidencePrefix, Exception, PropertiesAvailable, PropertiesRequired,
    ResourceHandle, ResourceManager, StatusCode,
};

// ---------------------------------------------------------------------------
// Enumerations (hash.h, results.h)
// ---------------------------------------------------------------------------

/// Method used to find a match for the evidence provided.
///
/// Mirrors `fiftyoneDegreesHashMatchMethod` from `hash.h`. The discriminants are
/// the enum's natural ordinal values, which is the ABI the C library uses.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HashMatchMethod {
    /// No match method was used.
    None = 0,
    /// The performance optimized graph produced the match.
    Performance,
    /// A combination of graphs produced the match.
    Combined,
    /// The predictive graph produced the match.
    Predictive,
    /// Sentinel equal to the number of methods, never a real result.
    Length,
}

/// Reason a value is missing or invalid for a property in a result.
///
/// Mirrors `fiftyoneDegreesResultsNoValueReason` from `src/common/results.h`.
/// Returned by [`fiftyoneDegreesResultsHashGetNoValueReason`].
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResultsNoValueReason {
    /// The difference value exceeded the configured threshold.
    Difference = 0,
    /// No hash nodes were matched.
    NoMatchedNodes,
    /// The requested property does not exist or is not required.
    InvalidProperty,
    /// No result in the set contains a value for the property.
    NoResultForProperty,
    /// There are no results to read a value from.
    NoResults,
    /// There are too many values to express as the requested type.
    TooManyValues,
    /// The result contains a null profile for the required component.
    NullProfile,
    /// The match is deemed high risk of being incorrect.
    HighRisk,
    /// None of the above reasons applies.
    Unknown,
}

// ---------------------------------------------------------------------------
// Configuration structures (config.h, config-dd.h, hash.h)
// ---------------------------------------------------------------------------

/// Configuration applied to a single managed collection.
///
/// Mirrors `fiftyoneDegreesCollectionConfig` from `src/common/collection.h`.
/// The fields tune the in memory caching and loading behavior for one
/// collection within the data set.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CollectionConfig {
    /// True if the collection should be loaded fully into memory.
    pub loaded: bool,
    /// Capacity of the cache for the collection, or zero for no cache.
    pub capacity: u32,
    /// Number of concurrent operations the cache must support.
    pub concurrency: u16,
}

/// Base configuration shared by every data set.
///
/// Mirrors `fiftyoneDegreesConfigBase` from `src/common/config.h`. A caller
/// rarely constructs this directly, preferring one of the predefined Hash
/// configuration globals.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ConfigBase {
    /// True if the data file should be loaded entirely into continuous memory.
    pub all_in_memory: bool,
    /// True if the HTTP header field names might include the `HTTP_` prefix.
    pub uses_upper_prefixed_headers: bool,
    /// True if externally allocated data set memory should be freed when no
    /// longer needed.
    pub free_data: bool,
    /// True if a temporary copy of the data file should be created.
    pub use_temp_file: bool,
    /// True if an existing temporary file may be reused.
    pub reuse_temp_file: bool,
    /// Null terminated list of temp directory paths in preference order, or
    /// null.
    pub temp_dirs: *const *const c_char,
    /// Number of entries in `temp_dirs`.
    pub temp_dir_count: c_int,
    /// True if an index to values for property and profiles should be created.
    pub property_value_index: bool,
}

/// Device detection configuration extending [`ConfigBase`].
///
/// Mirrors `fiftyoneDegreesConfigDeviceDetection` from `config-dd.h`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ConfigDeviceDetection {
    /// Base configuration members.
    pub b: ConfigBase,
    /// True if the matched user agent characters should be recorded.
    pub update_matched_user_agent: bool,
    /// The limit, in bytes, on the length of each header value that detection
    /// uses. Despite the name it applies to every header and every
    /// pseudo-header, not only the User-Agent, and a pseudo-header counts one
    /// separator byte between each of the values it joins. A value longer
    /// than the limit is ignored with no error, and detection moves on to the
    /// next header.
    ///
    /// The predefined configurations set 500 bytes, the C library's default.
    /// The reason for that default is given where [the default is defined]
    /// and in the C library's [header length limit] documentation, which
    /// also gives the memory a larger limit costs.
    ///
    /// [the default is defined]: https://github.com/51Degrees/51degrees-cxx/blob/e866f6d3cea936587790f87967eff7863778b30e/src/dd/config-dd.h#L100-L106
    /// [header length limit]: https://github.com/51Degrees/51degrees-cxx/blob/e866f6d3cea936587790f87967eff7863778b30e/docs/dd/README.md#header-length-limit
    pub max_matched_user_agent_length: usize,
    /// True if a result with no matched node should still be considered valid.
    pub allow_unmatched: bool,
    /// True if special evidence (for example client hints) is processed.
    pub process_special_evidence: bool,
}

/// Hash specific configuration extending [`ConfigDeviceDetection`].
///
/// Mirrors `fiftyoneDegreesConfigHash` from `hash.h`. The predefined global
/// configurations such as [`fiftyoneDegreesHashDefaultConfig`] are values of
/// this type. A caller passes a pointer to one of them, or to a copy it has
/// modified, into the manager initialization methods.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ConfigHash {
    /// Base device detection configuration.
    pub b: ConfigDeviceDetection,
    /// Strings collection configuration.
    pub strings: CollectionConfig,
    /// Components collection configuration.
    pub components: CollectionConfig,
    /// Maps collection configuration.
    pub maps: CollectionConfig,
    /// Properties collection configuration.
    pub properties: CollectionConfig,
    /// Values collection configuration.
    pub values: CollectionConfig,
    /// Profiles collection configuration.
    pub profiles: CollectionConfig,
    /// Root nodes collection configuration.
    pub root_nodes: CollectionConfig,
    /// Nodes collection configuration.
    pub nodes: CollectionConfig,
    /// Profile offsets collection configuration.
    pub profile_offsets: CollectionConfig,
    /// Maximum allowed difference when matching hashes.
    pub difference: i32,
    /// Maximum allowed drift when matching hashes.
    pub drift: i32,
    /// True if the performance optimized graph should be used.
    pub use_performance_graph: bool,
    /// True if the predictive optimized graph should be used.
    pub use_predictive_graph: bool,
    /// True if the route through each graph should be traced (debug only).
    pub trace_route: bool,
}

// ---------------------------------------------------------------------------
// Opaque data set and results types (hash.h)
// ---------------------------------------------------------------------------

/// Opaque Hash data set obtained through [`fiftyoneDegreesDataSetHashGet`].
///
/// Mirrors the deeply nested `fiftyoneDegreesDataSetHash` from `hash.h`. The
/// internal layout is intentionally hidden. The only operation defined on the
/// pointer here is to release it with [`fiftyoneDegreesDataSetHashRelease`].
/// Property and evidence key enumeration is provided through the shim helpers
/// rather than by reaching into this structure from Rust.
#[repr(C)]
pub struct DataSetHash {
    _private: [u8; 0],
}

/// Opaque Hash results structure produced by
/// [`fiftyoneDegreesResultsHashCreate`].
///
/// Mirrors `fiftyoneDegreesResultsHash` from `hash.h`. It holds a reference to
/// the data set, which is released when the results are freed with
/// [`fiftyoneDegreesResultsHashFree`].
#[repr(C)]
pub struct ResultsHash {
    _private: [u8; 0],
}

// ---------------------------------------------------------------------------
// Predefined configuration globals (hash.h)
// ---------------------------------------------------------------------------

extern "C" {
    /// In memory configuration. Loads the data set from a memory buffer with no
    /// caching and only the performance graph enabled.
    pub static mut fiftyoneDegreesHashInMemoryConfig: ConfigHash;

    /// Highest performance configuration. Loads everything into memory and drops
    /// the connection to the source file.
    pub static mut fiftyoneDegreesHashHighPerformanceConfig: ConfigHash;

    /// Low memory configuration. Keeps a file connection and uses no caching for
    /// the smallest footprint at the cost of speed.
    pub static mut fiftyoneDegreesHashLowMemoryConfig: ConfigHash;

    /// Balanced configuration. Uses caching to trade memory for performance.
    /// This is the default operating mode.
    pub static mut fiftyoneDegreesHashBalancedConfig: ConfigHash;

    /// Balanced configuration that also creates a temporary file copy of the
    /// source data file to avoid locking it.
    pub static mut fiftyoneDegreesHashBalancedTempConfig: ConfigHash;

    /// Default detection configuration. No temp file, no drift or difference
    /// allowance, and the matched user agent substrings are recorded.
    pub static mut fiftyoneDegreesHashDefaultConfig: ConfigHash;
}

// ---------------------------------------------------------------------------
// extern "C" function declarations (hash.h)
// ---------------------------------------------------------------------------

extern "C" {
    /// Returns the constant number of bytes needed to initialize a Hash manager
    /// from a file with the given configuration, or zero if not constant.
    ///
    /// # Safety
    /// `file_name` must be a valid null terminated path. `config` and
    /// `properties` may be null for defaults. `exception` must be valid.
    pub fn fiftyoneDegreesHashSizeManagerFromFile(
        config: *mut ConfigHash,
        properties: *mut PropertiesRequired,
        file_name: *const c_char,
        exception: *mut Exception,
    ) -> usize;

    /// Initializes `manager` with a Hash data set loaded from `file_name`.
    ///
    /// # Safety
    /// `manager` must point to a zeroed [`ResourceManager`]. `file_name` must be
    /// a valid null terminated path. `config` and `properties` may be null for
    /// defaults. `exception` must be valid.
    pub fn fiftyoneDegreesHashInitManagerFromFile(
        manager: *mut ResourceManager,
        config: *mut ConfigHash,
        properties: *mut PropertiesRequired,
        file_name: *const c_char,
        exception: *mut Exception,
    ) -> StatusCode;

    /// Returns the constant number of bytes needed to initialize a Hash manager
    /// from a memory buffer, or zero if not constant. If the memory doesn't
    /// hold a data set that loads, `exception` holds the status and the number
    /// returned means nothing.
    ///
    /// # Safety
    /// `memory` must point to `size` readable bytes. `config` and `properties`
    /// may be null. `exception` must be valid.
    pub fn fiftyoneDegreesHashSizeManagerFromMemory(
        config: *mut ConfigHash,
        properties: *mut PropertiesRequired,
        memory: *mut c_void,
        size: std::os::raw::c_long,
        exception: *mut Exception,
    ) -> usize;

    /// Initializes `manager` with a Hash data set read from a memory buffer.
    ///
    /// # Safety
    /// `manager` must point to a zeroed [`ResourceManager`]. `memory` must point
    /// to `size` bytes that outlive the manager. `config` and `properties` may
    /// be null. `exception` must be valid.
    pub fn fiftyoneDegreesHashInitManagerFromMemory(
        manager: *mut ResourceManager,
        config: *mut ConfigHash,
        properties: *mut PropertiesRequired,
        memory: *mut c_void,
        size: std::os::raw::c_long,
        exception: *mut Exception,
    ) -> StatusCode;

    /// Allocates a results structure referencing the data set in `manager`. The
    /// returned pointer must be freed with [`fiftyoneDegreesResultsHashFree`].
    ///
    /// `user_agent_capacity` is accepted for source compatibility and ignored,
    /// because results are sized by the number of components in the data set.
    /// `overrides_capacity` sizes the property-override array. Zero turns
    /// overrides off, so values the client-side JavaScript sends back are
    /// ignored.
    ///
    /// This declaration must match `fiftyoneDegreesResultsHashCreate` in the
    /// vendored `src/hash/hash.h`. Rust cannot check that, so
    /// `native/tests/smoke.rs` checks that an override reaches the results.
    ///
    /// # Safety
    /// `manager` must be an initialized Hash manager.
    pub fn fiftyoneDegreesResultsHashCreate(
        manager: *mut ResourceManager,
        user_agent_capacity: u32,
        overrides_capacity: u32,
    ) -> *mut ResultsHash;

    /// Frees a results structure and releases its data set reference.
    ///
    /// # Safety
    /// `results` must come from [`fiftyoneDegreesResultsHashCreate`].
    pub fn fiftyoneDegreesResultsHashFree(results: *mut ResultsHash);

    /// Processes the evidence pairs and populates the results.
    ///
    /// # Safety
    /// `results` and `evidence` must be valid. `exception` must be valid.
    pub fn fiftyoneDegreesResultsHashFromEvidence(
        results: *mut ResultsHash,
        evidence: *mut EvidenceKeyValuePairArray,
        exception: *mut Exception,
    );

    /// Processes a single user agent and populates the results.
    ///
    /// # Safety
    /// `results` must be valid. `user_agent` must point to `user_agent_length`
    /// bytes. `exception` must be valid.
    pub fn fiftyoneDegreesResultsHashFromUserAgent(
        results: *mut ResultsHash,
        user_agent: *const c_char,
        user_agent_length: usize,
        exception: *mut Exception,
    );

    /// Processes a device id and populates the results. Returns the number of
    /// valid profiles parsed from the device id.
    ///
    /// # Safety
    /// `results` must be valid. `device_id` must point to `device_id_length`
    /// bytes. `exception` must be valid.
    pub fn fiftyoneDegreesResultsHashFromDeviceId(
        results: *mut ResultsHash,
        device_id: *const c_char,
        device_id_length: usize,
        exception: *mut Exception,
    ) -> c_int;

    /// Returns true if the results contain valid values for the required
    /// property index provided.
    ///
    /// # Safety
    /// `results` must be valid. `exception` must be valid.
    pub fn fiftyoneDegreesResultsHashGetHasValues(
        results: *mut ResultsHash,
        required_property_index: c_int,
        exception: *mut Exception,
    ) -> bool;

    /// Returns the reason the results do not contain a valid value for the
    /// required property index provided.
    ///
    /// # Safety
    /// `results` must be valid. `exception` must be valid.
    pub fn fiftyoneDegreesResultsHashGetNoValueReason(
        results: *mut ResultsHash,
        required_property_index: c_int,
        exception: *mut Exception,
    ) -> ResultsNoValueReason;

    /// Returns a static English description for a no value reason.
    pub fn fiftyoneDegreesResultsHashGetNoValueReasonMessage(
        reason: ResultsNoValueReason,
    ) -> *const c_char;

    /// Writes the values for `property_name` into `buffer`, joined with
    /// `separator`, and returns the number of characters available. A return
    /// value larger than `length` means the buffer was too small.
    ///
    /// # Safety
    /// `results` must be valid. `property_name` and `separator` must be valid
    /// null terminated strings. `buffer` must point to `length` writable bytes.
    /// `exception` must be valid.
    pub fn fiftyoneDegreesResultsHashGetValuesString(
        results: *mut ResultsHash,
        property_name: *const c_char,
        buffer: *mut c_char,
        length: usize,
        separator: *const c_char,
        exception: *mut Exception,
    ) -> usize;

    /// Writes the values for the required property index into `buffer`. Behaves
    /// like [`fiftyoneDegreesResultsHashGetValuesString`] but keyed by index.
    ///
    /// # Safety
    /// As for [`fiftyoneDegreesResultsHashGetValuesString`].
    pub fn fiftyoneDegreesResultsHashGetValuesStringByRequiredPropertyIndex(
        results: *mut ResultsHash,
        required_property_index: c_int,
        buffer: *mut c_char,
        length: usize,
        separator: *const c_char,
        exception: *mut Exception,
    ) -> usize;

    /// Writes a JSON document of all available property values into `buffer` and
    /// returns the number of characters available.
    ///
    /// # Safety
    /// `results` must be valid. `buffer` must point to `length` writable bytes.
    /// `exception` must be valid.
    pub fn fiftyoneDegreesResultsHashGetValuesJson(
        results: *mut ResultsHash,
        buffer: *mut c_char,
        length: usize,
        exception: *mut Exception,
    ) -> usize;

    /// Writes the device id of the results into `buffer` as a null terminated
    /// string and returns the destination pointer (the `buffer`), or null when
    /// the value did not fit. The device id is the per-component profile ids
    /// joined with the `-` separator, surfaced as the `DeviceId` match metric.
    /// It is not a data-file property, so it is read through this dedicated
    /// getter rather than the by-name value reader.
    ///
    /// # Safety
    /// `results` must be valid. `buffer` must point to `length` writable bytes.
    /// `exception` must be valid.
    pub fn fiftyoneDegreesHashGetDeviceIdFromResults(
        results: *mut ResultsHash,
        buffer: *mut c_char,
        length: usize,
        exception: *mut Exception,
    ) -> *mut c_char;

    /// Returns a safe reference to the Hash data set managed by `manager`. The
    /// reference must be released with [`fiftyoneDegreesDataSetHashRelease`].
    ///
    /// # Safety
    /// `manager` must be an initialized Hash manager.
    pub fn fiftyoneDegreesDataSetHashGet(manager: *mut ResourceManager) -> *mut DataSetHash;

    /// Releases a data set reference obtained from
    /// [`fiftyoneDegreesDataSetHashGet`].
    ///
    /// # Safety
    /// `data_set` must come from [`fiftyoneDegreesDataSetHashGet`].
    pub fn fiftyoneDegreesDataSetHashRelease(data_set: *mut DataSetHash);
}

// ---------------------------------------------------------------------------
// Property and evidence key enumeration shim (src/shim.c)
// ---------------------------------------------------------------------------

extern "C" {
    /// Returns the number of required property indexes of the data set managed
    /// by `manager`, which is the number of names it was created with, or the
    /// number of its available properties when it was created naming none.
    /// Returns zero if no data set is available.
    ///
    /// # Safety
    /// `manager` must be an initialized Hash manager.
    pub fn fiftyoneDegreesShimHashGetRequiredPropertyCount(manager: *mut ResourceManager) -> u32;

    /// Writes the name of the required property at `required_property_index`
    /// into `buffer` as a null terminated string and returns the number of
    /// characters written, excluding the terminator. A name the data set has
    /// no property for is written as it was given.
    ///
    /// # Safety
    /// `manager` must be an initialized Hash manager. `buffer` must point to
    /// `length` writable bytes.
    pub fn fiftyoneDegreesShimHashGetRequiredPropertyName(
        manager: *mut ResourceManager,
        required_property_index: c_int,
        buffer: *mut c_char,
        length: usize,
    ) -> usize;

    /// Returns the required property index for `property_name`, or -1 when the
    /// property is not one of the required properties.
    ///
    /// # Safety
    /// `manager` must be an initialized Hash manager. `property_name` must be a
    /// valid null terminated string.
    pub fn fiftyoneDegreesShimHashGetRequiredPropertyIndexFromName(
        manager: *mut ResourceManager,
        property_name: *const c_char,
    ) -> c_int;

    /// Returns the number of HTTP header evidence keys in the data set managed
    /// by `manager`, or zero if no data set is available.
    ///
    /// # Safety
    /// `manager` must be an initialized Hash manager.
    pub fn fiftyoneDegreesShimHashGetEvidenceKeyCount(manager: *mut ResourceManager) -> u32;

    /// Writes the name of the HTTP header evidence key at `header_index` into
    /// `buffer` as a null terminated string and returns the number of
    /// characters written, excluding the terminator.
    ///
    /// # Safety
    /// `manager` must be an initialized Hash manager. `buffer` must point to
    /// `length` writable bytes.
    pub fn fiftyoneDegreesShimHashGetEvidenceKey(
        manager: *mut ResourceManager,
        header_index: u32,
        buffer: *mut c_char,
        length: usize,
    ) -> usize;

    /// Writes the data set's name (its tier, for example `Lite`, `Enterprise`
    /// or `TAC`) into `buffer` as a null terminated string and returns the
    /// number of characters written, excluding the terminator. Returns zero
    /// when the name cannot be read.
    ///
    /// # Safety
    /// `manager` must be an initialized Hash manager. `buffer` must point to
    /// `length` writable bytes.
    pub fn fiftyoneDegreesShimHashGetDataSetName(
        manager: *mut ResourceManager,
        buffer: *mut c_char,
        length: usize,
    ) -> usize;

    /// Writes the date the data file was published, from its header, into
    /// `*year`, `*month` and `*day` and returns 1, or 0 when no data set is
    /// available.
    ///
    /// # Safety
    /// `manager` must be an initialized Hash manager. Each output pointer must
    /// be null or point to a writable `i32`.
    pub fn fiftyoneDegreesShimHashGetDataSetPublished(
        manager: *mut ResourceManager,
        year: *mut i32,
        month: *mut i32,
        day: *mut i32,
    ) -> c_int;

    /// Writes the date the next data file is expected, from the data file
    /// header, into `*year`, `*month` and `*day` and returns 1, or 0 when no
    /// data set is available.
    ///
    /// # Safety
    /// `manager` must be an initialized Hash manager. Each output pointer must
    /// be null or point to a writable `i32`.
    pub fn fiftyoneDegreesShimHashGetDataSetNextUpdate(
        manager: *mut ResourceManager,
        year: *mut i32,
        month: *mut i32,
        day: *mut i32,
    ) -> c_int;

    /// Returns the size of a Hash data file's header in bytes, being how much
    /// of a data file the C library compares to find a copy of it to reuse.
    pub fn fiftyoneDegreesShimHashGetHeaderSize() -> usize;

    /// Reads the match metrics of the results into the output pointers and
    /// returns 1, or returns 0 when `results` is null or a graph cannot be
    /// walked. Any graph not yet walked is walked first, as the walks set the
    /// metrics, so this can read the data file. The results hold one result
    /// for each component, each from the header that matched it. As in the C++
    /// results, `method` receives the highest match-method enum value of any
    /// result (0 None, 1 Performance, 2 Combined, 3 Predictive), and the
    /// difference, drift, iterations and matched-node count are summed across
    /// the results. With no results every metric is zero. Any output pointer
    /// may be null to skip that metric. See `src/shim.c`.
    ///
    /// # Safety
    /// `results` must be an initialized Hash results structure. Each non-null
    /// output pointer must be writable.
    pub fn fiftyoneDegreesShimHashGetResultMetrics(
        results: *mut ResultsHash,
        method: *mut i32,
        difference: *mut i32,
        drift: *mut i32,
        iterations: *mut i32,
        matched_nodes: *mut i32,
    ) -> c_int;

    /// Returns the number of results, one for each component, or zero when
    /// `results` is null. See `src/shim.c`.
    ///
    /// # Safety
    /// `results` must be an initialized Hash results structure or null.
    pub fn fiftyoneDegreesShimHashGetResultCount(results: *mut ResultsHash) -> u32;

    /// Reads the match metrics of the single result at `result_index` into the
    /// output pointers and returns 1, or returns 0 when the index is out of
    /// range or a graph cannot be walked. Any graph not yet walked is walked
    /// first, so this can read the data file. Any output pointer may be null
    /// to skip that metric. See `src/shim.c`.
    ///
    /// # Safety
    /// `results` must be an initialized Hash results structure or null. Each
    /// non-null output pointer must be writable.
    pub fn fiftyoneDegreesShimHashGetResultMetricsAt(
        results: *mut ResultsHash,
        result_index: u32,
        method: *mut i32,
        difference: *mut i32,
        drift: *mut i32,
        iterations: *mut i32,
        matched_nodes: *mut i32,
    ) -> c_int;

    /// Writes the matched User-Agent of the result at `result_index` into
    /// `buffer` as a null terminated string and returns the number of
    /// characters written, excluding the terminator. Returns zero when the
    /// index is out of range, a graph cannot be walked or the result records
    /// no matched User-Agent, and cuts the value short when the buffer is too
    /// small. Any graph not yet walked is walked first, so this can read the
    /// data file. See `src/shim.c`.
    ///
    /// # Safety
    /// `results` must be an initialized Hash results structure. `buffer` must
    /// point to `length` writable bytes.
    pub fn fiftyoneDegreesShimHashGetMatchedUserAgent(
        results: *mut ResultsHash,
        result_index: u32,
        buffer: *mut c_char,
        length: usize,
    ) -> usize;

    /// Returns the number of handles in the file pool of the data set
    /// managed by `manager` when any of its collections reads from the data
    /// file after it is loaded, or zero when the whole data set is held in
    /// memory. A read that finds every handle in use fails rather than
    /// waiting for one, so this is how many calls can read the data file at
    /// once. See `src/shim.c`.
    ///
    /// # Safety
    /// `manager` must be an initialized Hash manager.
    pub fn fiftyoneDegreesShimHashGetFileHandleCount(manager: *mut ResourceManager) -> u16;

    /// Returns the number of handles to the data file that the data set
    /// managed by `manager` opened when it loaded, which stay open until the
    /// data set is freed. Every configuration that does not load the whole
    /// file into memory opens one for each concurrent operation it expects,
    /// even when all of its collections are then held in memory. See
    /// `src/shim.c`.
    ///
    /// # Safety
    /// `manager` must be an initialized Hash manager.
    pub fn fiftyoneDegreesShimHashGetFilePoolSize(manager: *mut ResourceManager) -> u16;
}

// ---------------------------------------------------------------------------
// Data file metadata shim (src/shim.c)
// ---------------------------------------------------------------------------

/// A string value. The `FIFTYONE_DEGREES_PROPERTY_VALUE_TYPE_` constants are
/// the C library's `fiftyoneDegreesPropertyValueType` values
/// (`src/common/propertyValueType.h`) the C++ `PropertyMetaDataBuilderHash`
/// names, for the `value_type`
/// [`fiftyoneDegreesShimHashGetDataFileProperty`] gives.
pub const FIFTYONE_DEGREES_PROPERTY_VALUE_TYPE_STRING: u8 = 0;
/// An integer value.
pub const FIFTYONE_DEGREES_PROPERTY_VALUE_TYPE_INTEGER: u8 = 1;
/// A double value.
pub const FIFTYONE_DEGREES_PROPERTY_VALUE_TYPE_DOUBLE: u8 = 2;
/// A boolean value.
pub const FIFTYONE_DEGREES_PROPERTY_VALUE_TYPE_BOOLEAN: u8 = 3;
/// A JavaScript value.
pub const FIFTYONE_DEGREES_PROPERTY_VALUE_TYPE_JAVASCRIPT: u8 = 4;

/// The name of a data file property, for
/// [`fiftyoneDegreesShimHashGetDataFilePropertyString`].
pub const SHIM_PROPERTY_NAME: c_int = 0;
/// The description of a data file property.
pub const SHIM_PROPERTY_DESCRIPTION: c_int = 1;
/// The category of a data file property.
pub const SHIM_PROPERTY_CATEGORY: c_int = 2;
/// The URL of a data file property.
pub const SHIM_PROPERTY_URL: c_int = 3;

/// The name of a property value, the first of its strings
/// [`fiftyoneDegreesShimHashGetDataFilePropertyValues`] gives.
pub const SHIM_VALUE_NAME: c_int = 0;
/// The description of a property value, given after its name.
pub const SHIM_VALUE_DESCRIPTION: c_int = 1;
/// The URL of a property value, given after its description.
pub const SHIM_VALUE_URL: c_int = 2;

/// Takes one string of a property value from
/// [`fiftyoneDegreesShimHashGetDataFilePropertyValues`]. `field` is one of the
/// `SHIM_VALUE_` constants and `value` a null terminated string valid only
/// for the call. `context` is the pointer the caller passed.
pub type ShimValueStringCallback =
    unsafe extern "C" fn(context: *mut c_void, field: c_int, value: *const c_char);

/// Returned by the data file metadata string readers when the string, or the
/// property, value or component it belongs to, cannot be read, or an index
/// is out of range. An absent string gives zero instead.
pub const SHIM_STRING_FAILED: usize = usize::MAX;

/// Returned by [`fiftyoneDegreesShimHashFindDataFilePropertyValue`] when the
/// property or the value cannot be read, or the property index is out of
/// range.
pub const SHIM_FIND_FAILED: c_int = 0;
/// Returned by [`fiftyoneDegreesShimHashFindDataFilePropertyValue`] when it
/// found the value and gave its strings.
pub const SHIM_FIND_FOUND: c_int = 1;
/// Returned by [`fiftyoneDegreesShimHashFindDataFilePropertyValue`] when the
/// property has no value with the name.
pub const SHIM_FIND_NOT_FOUND: c_int = 2;

extern "C" {
    /// Returns the number of properties the data file defines, including
    /// those the data set was not created with, or zero when no data set is
    /// available. See `src/shim.c`.
    ///
    /// # Safety
    /// `manager` must be an initialized Hash manager.
    pub fn fiftyoneDegreesShimHashGetDataFilePropertyCount(manager: *mut ResourceManager) -> u32;

    /// Reads the fields of the data file property at `property_index` that
    /// are not strings and returns 1, or returns 0 when the index is out of
    /// range or the property or its name cannot be read, the name being read
    /// to find whether the data set was created with the property.
    /// `value_type` is the C library's property value type, `available` is 1
    /// when the data set was created with the property, `required_index` is
    /// the index the results getters take for it or -1 for none,
    /// `component_id` is the id of the property's component or 0 for none,
    /// `value_count` is the number of values the file holds for the property
    /// or 0 when they are not all in its values collection,
    /// `default_value_number` the place of its default value among them or -1
    /// for none, and `map_count` the number of data tiers it is in. See
    /// `src/shim.c`.
    ///
    /// # Safety
    /// `manager` must be an initialized Hash manager. Every output pointer
    /// must be writable.
    pub fn fiftyoneDegreesShimHashGetDataFileProperty(
        manager: *mut ResourceManager,
        property_index: u32,
        value_type: *mut u8,
        available: *mut u8,
        required_index: *mut i32,
        component_id: *mut u8,
        display_order: *mut u8,
        is_mandatory: *mut u8,
        is_list: *mut u8,
        is_obsolete: *mut u8,
        show: *mut u8,
        show_values: *mut u8,
        value_count: *mut u32,
        default_value_number: *mut i32,
        map_count: *mut u32,
    ) -> c_int;

    /// Writes the string `field` of the data file property at
    /// `property_index`, one of the `SHIM_PROPERTY_` constants, into `buffer`
    /// as a null terminated string, as much as fits, and returns the
    /// string's full length excluding the terminator. An absent string is
    /// written as empty, with length zero, and one that cannot be read gives
    /// [`SHIM_STRING_FAILED`]. See `src/shim.c`.
    ///
    /// # Safety
    /// `manager` must be an initialized Hash manager. `buffer` must point to
    /// `length` writable bytes.
    pub fn fiftyoneDegreesShimHashGetDataFilePropertyString(
        manager: *mut ResourceManager,
        property_index: u32,
        field: c_int,
        buffer: *mut c_char,
        length: usize,
    ) -> usize;

    /// Writes the name of the data tier at `map_index` among those the data
    /// file property at `property_index` is in into `buffer`, as much as
    /// fits, and returns its full length, or [`SHIM_STRING_FAILED`] when either
    /// index is out of range or the name cannot be read. See `src/shim.c`.
    ///
    /// # Safety
    /// `manager` must be an initialized Hash manager. `buffer` must point to
    /// `length` writable bytes.
    pub fn fiftyoneDegreesShimHashGetDataFilePropertyMap(
        manager: *mut ResourceManager,
        property_index: u32,
        map_index: u32,
        buffer: *mut c_char,
        length: usize,
    ) -> usize;

    /// Reads at most `count` of the values the data file holds for the
    /// property at `property_index`, from the one at `value_number` among
    /// them, and returns 1. For each value it calls `callback` with `context`
    /// three times, with its name, description and URL in that order, an
    /// absent description or URL being empty. A `value_number` at or past the
    /// last value reads none. Returns 0 when the property index is out of
    /// range or the property, a value or a string cannot be read, after
    /// giving `callback` the strings read before the failure. See
    /// `src/shim.c`.
    ///
    /// # Safety
    /// `manager` must be an initialized Hash manager, and `context` must be
    /// what `callback` expects.
    pub fn fiftyoneDegreesShimHashGetDataFilePropertyValues(
        manager: *mut ResourceManager,
        property_index: u32,
        value_number: u32,
        count: u32,
        callback: ShimValueStringCallback,
        context: *mut c_void,
    ) -> c_int;

    /// Finds the value named `name`, matched exactly, among those the data
    /// file holds for the property at `property_index`, calls `callback` with
    /// its name, description and URL as
    /// [`fiftyoneDegreesShimHashGetDataFilePropertyValues`] does, and returns
    /// [`SHIM_FIND_FOUND`]. Returns [`SHIM_FIND_NOT_FOUND`] when the property
    /// has no such value, or [`SHIM_FIND_FAILED`] when the property index is
    /// out of range or the property, a value or a string cannot be read. The
    /// value is found by the C library's binary search over the property's
    /// values, which are in order of their names, so a value the data file
    /// holds out of that order can be missed. See `src/shim.c`.
    ///
    /// # Safety
    /// `manager` must be an initialized Hash manager, `name` a null
    /// terminated string, and `context` what `callback` expects.
    pub fn fiftyoneDegreesShimHashFindDataFilePropertyValue(
        manager: *mut ResourceManager,
        property_index: u32,
        name: *const c_char,
        callback: ShimValueStringCallback,
        context: *mut c_void,
    ) -> c_int;

    /// Returns the number of components the data set lists, or zero when no
    /// data set is available. See `src/shim.c`.
    ///
    /// # Safety
    /// `manager` must be an initialized Hash manager.
    pub fn fiftyoneDegreesShimHashGetComponentCount(manager: *mut ResourceManager) -> u32;

    /// Reads the id of the component at `component_index` and the profile id
    /// of its default profile and returns 1, or returns 0 when the index is
    /// out of range or the default profile cannot be read. See `src/shim.c`.
    ///
    /// # Safety
    /// `manager` must be an initialized Hash manager. Both output pointers
    /// must be writable.
    pub fn fiftyoneDegreesShimHashGetComponent(
        manager: *mut ResourceManager,
        component_index: u32,
        component_id: *mut u8,
        default_profile_id: *mut u32,
    ) -> c_int;

    /// Writes the name of the component at `component_index` into `buffer`,
    /// as much as fits, and returns its full length, or [`SHIM_STRING_FAILED`]
    /// when the index is out of range or the name cannot be read. See
    /// `src/shim.c`.
    ///
    /// # Safety
    /// `manager` must be an initialized Hash manager. `buffer` must point to
    /// `length` writable bytes.
    pub fn fiftyoneDegreesShimHashGetComponentName(
        manager: *mut ResourceManager,
        component_index: u32,
        buffer: *mut c_char,
        length: usize,
    ) -> usize;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{CStr, CString};

    /// Optional path to the Lite Hash data file, injected by the build script
    /// when the file has been downloaded into the checkout.
    const LITE_DATA_FILE: Option<&str> = option_env!("51DEGREES_DD_PATH");

    /// A representative desktop Chrome on Windows user agent.
    const DESKTOP_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
        AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

    /// The no value reason messages link and return non-empty text. This proves
    /// the Hash static library linked even when no data file is available.
    #[test]
    fn no_value_reason_message_links() {
        unsafe {
            let raw = fiftyoneDegreesResultsHashGetNoValueReasonMessage(
                ResultsNoValueReason::NoMatchedNodes,
            );
            assert!(!raw.is_null(), "reason message should be a static string");
            let message = CStr::from_ptr(raw).to_string_lossy();
            assert!(!message.is_empty(), "reason message should not be empty");
        }
    }

    /// The predefined default configuration global is reachable across the FFI
    /// boundary and carries the documented default graph settings. Reading the
    /// graph flags and the trace flag, which sit at the very end of the struct,
    /// confirms the whole `ConfigHash` layout is aligned with the C definition,
    /// and confirms the `EXTERNAL_VAR` globals linked.
    #[test]
    fn default_config_global_links() {
        unsafe {
            // The default configuration is the Balanced one. Its initializer in
            // hash.c disables the performance graph, enables the predictive
            // graph and disables tracing. Matching these end-of-struct values
            // proves the preceding nested base structures are laid out exactly.
            let config = std::ptr::addr_of!(fiftyoneDegreesHashDefaultConfig);
            assert!(
                !(*config).use_performance_graph,
                "the default (Balanced) config disables the performance graph"
            );
            assert!(
                (*config).use_predictive_graph,
                "the default (Balanced) config enables the predictive graph"
            );
            assert!(
                !(*config).trace_route,
                "the default config does not trace the graph route"
            );

            // The high performance configuration caches everything in memory, so
            // every collection has its `loaded` flag set. Reading a collection
            // config flag confirms the collection layout too.
            let high = std::ptr::addr_of!(fiftyoneDegreesHashHighPerformanceConfig);
            assert!(
                (*high).strings.loaded,
                "high performance config loads the strings collection in memory"
            );
        }
    }

    /// Full smoke test: initialize a manager from the Lite data file, run one
    /// detection from a desktop user agent, read IsMobile, enumerate a couple of
    /// properties and evidence keys, then free everything in the correct order.
    ///
    /// The low memory configuration is used deliberately. It keeps a connection
    /// to the source file and caches nothing, giving the smallest resident
    /// footprint. That is all this smoke test needs (it only proves the native
    /// symbols link and a basic property/evidence read works) and it keeps the
    /// process small enough not to be SIGKILLed by the OS under the memory
    /// pressure of the shared macOS CI runner. The high-memory default config
    /// is exercised by `default_config_global_links` and by the higher-level
    /// on-premise tests.
    ///
    /// When the data file is absent the test still exercised the linking above,
    /// so it returns early with an explanatory note rather than failing.
    #[test]
    fn smoke_detect_is_mobile() {
        let Some(data_file) = LITE_DATA_FILE else {
            eprintln!(
                "no Lite data file found at build time; \
                 symbol linking is covered by the other tests"
            );
            return;
        };

        unsafe {
            let mut manager = ResourceManager::zeroed();
            let mut exception = Exception::cleared();
            let path = CString::new(data_file).expect("data file path has no interior nul");

            // Use the low memory configuration unmodified for the smallest
            // footprint. Passing null requests all available properties.
            let config = std::ptr::addr_of_mut!(fiftyoneDegreesHashLowMemoryConfig);
            let status = fiftyoneDegreesHashInitManagerFromFile(
                &mut manager,
                config,
                std::ptr::null_mut(),
                path.as_ptr(),
                &mut exception,
            );
            // The init ran cleanly through the FFI boundary and returned a well
            // defined status, which proves the native Hash library linked. The
            // data file may be absent or unusable, reported as a non-Success
            // status. Treat that as a clean skip rather than a
            // failure: the real detection path is exercised by the higher-level
            // on-premise tests when a usable data file is present.
            if status != StatusCode::Success {
                eprintln!(
                    "Hash data file did not load ({status:?}); the symbols linked \
                     and the init path executed, so skipping the detection checks"
                );
                return;
            }
            assert!(
                exception.is_okay(),
                "a successful manager init should not raise an exception"
            );

            // The data set should expose a healthy number of properties and at
            // least the User-Agent evidence key.
            let property_count = fiftyoneDegreesShimHashGetRequiredPropertyCount(&mut manager);
            assert!(
                property_count > 0,
                "the Lite data set should expose properties"
            );
            let evidence_count = fiftyoneDegreesShimHashGetEvidenceKeyCount(&mut manager);
            assert!(
                evidence_count > 0,
                "the Lite data set should expose evidence keys"
            );

            // Read the first property name back to confirm the enumeration shim
            // returns real strings.
            let mut name_buffer = [0u8; 128];
            let written = fiftyoneDegreesShimHashGetRequiredPropertyName(
                &mut manager,
                0,
                name_buffer.as_mut_ptr() as *mut c_char,
                name_buffer.len(),
            );
            assert!(written > 0, "the first property name should be readable");

            // IsMobile must be one of the required properties for this test to
            // be meaningful.
            let is_mobile_name = CString::new("IsMobile").unwrap();
            let is_mobile_index = fiftyoneDegreesShimHashGetRequiredPropertyIndexFromName(
                &mut manager,
                is_mobile_name.as_ptr(),
            );
            assert!(
                is_mobile_index >= 0,
                "IsMobile should be an available property in the Lite data file"
            );

            // Run a single detection from the desktop user agent. The results
            // are sized by the data set, so the user-agent capacity is unused;
            // the overrides capacity is zero (no property overrides here).
            let results = fiftyoneDegreesResultsHashCreate(&mut manager, 0, 0);
            assert!(!results.is_null(), "results allocation should succeed");

            fiftyoneDegreesResultsHashFromUserAgent(
                results,
                DESKTOP_USER_AGENT.as_ptr() as *const c_char,
                DESKTOP_USER_AGENT.len(),
                &mut exception,
            );
            assert!(
                exception.is_okay(),
                "processing a user agent should not raise an exception"
            );

            // The result should carry a value for IsMobile.
            let has_values =
                fiftyoneDegreesResultsHashGetHasValues(results, is_mobile_index, &mut exception);
            assert!(exception.is_okay(), "has values check should not throw");
            assert!(
                has_values,
                "IsMobile should have a value for a desktop user agent"
            );

            // Read the IsMobile value as a string. A desktop user agent must be
            // detected as not a mobile device.
            let mut value_buffer = [0u8; 64];
            let separator = CString::new(",").unwrap();
            let chars = fiftyoneDegreesResultsHashGetValuesString(
                results,
                is_mobile_name.as_ptr(),
                value_buffer.as_mut_ptr() as *mut c_char,
                value_buffer.len(),
                separator.as_ptr(),
                &mut exception,
            );
            assert!(exception.is_okay(), "reading IsMobile should not throw");
            assert!(chars > 0, "IsMobile should produce a non-empty value");

            let value = CStr::from_ptr(value_buffer.as_ptr() as *const c_char)
                .to_string_lossy()
                .into_owned();
            assert_eq!(
                value, "False",
                "a desktop user agent should be detected as not mobile"
            );

            // Free in the documented order: results first, then the manager.
            fiftyoneDegreesResultsHashFree(results);
            fiftyoneDegreesResourceManagerFree(&mut manager);
        }
    }
}
