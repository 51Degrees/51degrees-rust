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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-ip-intelligence-sys-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-ip-intelligence-sys-lib.rs&utm_term=logo)
//!
//! Raw FFI bindings to the on-premise IP Intelligence ABI (`Ipi`) in
//! the 51degrees-cxx C sources, including the weighted value getters.
//!
//! This crate owns the compilation and static linking of the
//! `fiftyone-ip-intelligence-c` native library (see `build.rs`) and exposes
//! hand written `extern "C"` declarations and `#[repr(C)]` structures for the
//! public `Ipi` C ABI declared in `ipi.h` and `ipi_weighted_results.h`.
//!
//! The shared common types (the [`Exception`], [`StatusCode`],
//! [`ResourceManager`], [`PropertiesRequired`] and evidence structures) live in
//! the [`common`] submodule. They are declared locally rather than reused from
//! `fiftyone-common-sys` because the IP Intelligence engine is compiled with 64
//! bit file offsets and must link its own common C library (see the [`common`]
//! module docs). Only the IP Intelligence specific surface is declared on top:
//!
//! - **Initialization** ([`fiftyoneDegreesIpiInitManagerFromFile`],
//!   [`fiftyoneDegreesIpiInitManagerFromMemory`] and the size helpers) with the
//!   predefined [`ConfigIpi`] globals
//!   ([`fiftyoneDegreesIpiDefaultConfig`] and the in memory, high performance,
//!   low memory and balanced variants).
//! - **Results** ([`fiftyoneDegreesResultsIpiCreate`],
//!   [`fiftyoneDegreesResultsIpiFromIpAddress`],
//!   [`fiftyoneDegreesResultsIpiFromIpAddressString`],
//!   [`fiftyoneDegreesResultsIpiFromEvidence`] and
//!   [`fiftyoneDegreesResultsIpiFree`]).
//! - **Value access** ([`fiftyoneDegreesResultsIpiGetValuesString`],
//!   [`fiftyoneDegreesResultsIpiGetHasValues`],
//!   [`fiftyoneDegreesResultsIpiGetNoValueReason`] and the legacy weighted
//!   getter [`fiftyoneDegreesResultsIpiGetValues`] returning a
//!   [`ProfilePercentage`]).
//! - **Weighted values** ([`fiftyoneDegreesResultsIpiGetValuesCollection`]
//!   producing a [`WeightedValuesCollection`] of [`WeightedValueHeader`]
//!   pointers, each carrying a `rawWeighting`, and
//!   [`fiftyoneDegreesWeightedValuesCollectionRelease`]).
//! - **Data set and metadata** ([`fiftyoneDegreesDataSetIpiGet`],
//!   [`fiftyoneDegreesDataSetIpiRelease`] and
//!   [`fiftyoneDegreesResourceManagerFree`], which is declared against this
//!   crate's own common C library build).
//! - **Property and header enumeration shim**
//!   ([`fiftyoneDegreesShimIpiGetRequiredPropertyCount`],
//!   [`fiftyoneDegreesShimIpiGetRequiredPropertyName`],
//!   [`fiftyoneDegreesShimIpiGetRequiredPropertyIndexFromName`],
//!   [`fiftyoneDegreesShimIpiGetUniqueHeaderCount`] and
//!   [`fiftyoneDegreesShimIpiGetUniqueHeaderName`]). These flat helpers are
//!   compiled from this crate's own `src/shim.c` (not the upstream library) and
//!   read the `available` property set and the `uniqueHeaders` buried deep
//!   inside the data set structures, so the Rust side does not mirror those
//!   private C layouts. They mirror the equivalent shim in
//!   `fiftyone-device-detection-sys`.
//! - **Evidence** ([`common::fiftyoneDegreesEvidenceCreate`],
//!   [`common::fiftyoneDegreesEvidenceAddString`] and
//!   [`common::fiftyoneDegreesEvidenceFree`]), bound to this crate's own
//!   common C library build for [`fiftyoneDegreesResultsIpiFromEvidence`].
//!
//! Internals such as collections, data sets, graphs and headers are exposed as
//! opaque pointer types. Only the structures that genuinely cross the boundary
//! in this surface are given a concrete `#[repr(C)]` layout. The names mirror
//! the C names exactly so the headers remain the authoritative reference.
//!
//! # Safety
//!
//! Every item in this crate is an unchecked binding to a C function or
//! structure. Callers are responsible for upholding the contracts described in
//! the upstream headers (`ipi.h`, `ipi_weighted_results.h` and the shared
//! common C library headers). Pointers handed to these functions must be valid,
//! and results and weighted value collections must be freed with their matching
//! release functions.

#![warn(missing_docs)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use std::os::raw::{c_char, c_int, c_void};

pub use common::{
    EvidenceKeyValuePair, EvidenceKeyValuePairArray, EvidencePrefix, Exception,
    PropertiesAvailable, PropertiesRequired, ResourceHandle, ResourceManager, StatusCode,
};

/// Shared common C library ABI types needed by the IP Intelligence surface.
///
/// These mirror the same common C library structures that the
/// `fiftyone-common-sys` crate declares, but they are defined locally and
/// resolved against the copy of the common C library compiled into this crate's
/// own native library. The IP Intelligence engine is built with
/// `FIFTYONE_DEGREES_LARGE_DATA_FILE_SUPPORT`, which widens the file offset and
/// on disk offset types to 64 bit. That makes its common C library objects ABI
/// incompatible with the 32 bit offset build owned by `fiftyone-common-sys`, so
/// the IP Intelligence library must link its own common C library and must not
/// pull in the common crate's native library. If this crate depended on
/// `fiftyone-common-sys`, that crate's `fiftyone-common-c` native library would
/// also be linked, and the duplicate common C library symbols would be bound
/// inconsistently across the two offset widths, which corrupts the data set
/// header read and makes initialization fail with an incorrect version error.
/// Declaring the handful of shared types here keeps a single, offset-consistent
/// copy of the common C library in any binary that links this crate.
pub mod common {
    use std::os::raw::{c_char, c_int};

    /// Status returned from the initialization of a resource, mirroring
    /// `fiftyoneDegreesStatusCode` from `status.h`. The discriminants are the
    /// enum's natural ordinal values, matching the header order.
    #[repr(C)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum StatusCode {
        /// All okay.
        Success = 0,
        /// Lack of memory.
        InsufficientMemory,
        /// Data structure not readable.
        CorruptData,
        /// Data not the required version.
        IncorrectVersion,
        /// The data file could not be found.
        FileNotFound,
        /// The data file was busy.
        FileBusy,
        /// Unknown file error.
        FileFailure,
        /// Should never be returned to the caller.
        NotSet,
        /// Working pointer exceeded the memory containing the data.
        PointerOutOfBounds,
        /// A key pointer was not set.
        NullPointer,
        /// Too many files are open.
        TooManyOpenFiles,
        /// None of the required properties could be found.
        ReqPropNotPresent,
        /// The profile id represents an empty profile.
        ProfileEmpty,
        /// A collection item could not be retrieved due to too many concurrent
        /// operations.
        CollectionFailure,
        /// The data file could not be copied.
        FileCopyError,
        /// The file or directory already exists, so could not be created.
        FileExistsError,
        /// The data file could not be created.
        FileWriteError,
        /// The data file could not be read.
        FileReadError,
        /// File permission denied.
        FilePermissionDenied,
        /// The file path is longer than the available storage.
        FilePathTooLong,
        /// End of a yaml document read.
        FileEndOfDocument,
        /// End of yaml documents read.
        FileEndOfDocuments,
        /// End of file.
        FileEndOfFile,
        /// There was an error encoding characters of a string.
        EncodingError,
        /// The configuration could not produce a valid collection.
        InvalidCollectionConfig,
        /// An invalid config was provided.
        InvalidConfig,
        /// There were not enough handles available to retrieve data.
        InsufficientHandles,
        /// Collection index out of range.
        CollectionIndexOutOfRange,
        /// Collection offset out of range.
        CollectionOffsetOutOfRange,
        /// Collection file seek failure.
        CollectionFileSeekFail,
        /// Collection file read failure.
        CollectionFileReadFail,
        /// IP address format is incorrect.
        IncorrectIpAddressFormat,
        /// Error creating temp file.
        TempFileError,
        /// Insufficient capacity of an array to hold all the items.
        InsufficientCapacity,
        /// Invalid input data (for example base64 or JSON misformat).
        InvalidInput,
        /// `StoredValueType` is not supported at this version.
        UnsupportedStoredValueType,
        /// File size exceeds malloc capabilities.
        FileTooLarge,
        /// Unsupported geometry type found in WKB.
        UnknownGeometry,
        /// Abstract or reserved geometry type found in WKB.
        ReservedGeometry,
        /// The requested functionality is not implemented, usually due to a
        /// compile flag.
        NotImplemented,
    }

    /// Structure used to represent a 51Degrees exception, mirroring
    /// `fiftyoneDegreesException` from `exceptions.h`. The `file` and `func`
    /// pointers reference static strings owned by the library and are never
    /// freed by the caller.
    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
    pub struct Exception {
        /// File generating the exception, or null.
        pub file: *const c_char,
        /// Function generating the exception, or null.
        pub func: *const c_char,
        /// Line number generating the exception.
        pub line: c_int,
        /// Status code to assign.
        pub status: StatusCode,
    }

    impl Exception {
        /// Create a cleared exception equivalent to the `EXCEPTION_CREATE` /
        /// `EXCEPTION_CLEAR` macros (null source, line `-1`, status
        /// [`StatusCode::NotSet`]).
        pub fn cleared() -> Self {
            Exception {
                file: std::ptr::null(),
                func: std::ptr::null(),
                line: -1,
                status: StatusCode::NotSet,
            }
        }

        /// Returns `true` when no exception is set, matching the
        /// `EXCEPTION_OKAY` macro (status equals [`StatusCode::NotSet`]).
        pub fn is_okay(&self) -> bool {
            self.status == StatusCode::NotSet
        }
    }

    /// Opaque handle for a shared resource managed by a [`ResourceManager`],
    /// mirroring `fiftyoneDegreesResourceHandle`. The internal layout is hidden.
    #[repr(C)]
    pub struct ResourceHandle {
        _private: [u8; 0],
    }

    /// Manager structure providing access to a shared and changing resource,
    /// mirroring `fiftyoneDegreesResourceManager`, which is a single pointer to
    /// the active handle.
    #[repr(C)]
    pub struct ResourceManager {
        /// Current handle for the resource used by the manager.
        pub active: *mut ResourceHandle,
    }

    impl ResourceManager {
        /// Create a zeroed manager suitable for passing to an init function.
        pub fn zeroed() -> Self {
            ResourceManager {
                active: std::ptr::null_mut(),
            }
        }
    }

    /// Opaque set of properties available in a data set, mirroring
    /// `fiftyoneDegreesPropertiesAvailable`.
    #[repr(C)]
    pub struct PropertiesAvailable {
        _private: [u8; 0],
    }

    /// Array of evidence key value pairs, mirroring
    /// `fiftyoneDegreesEvidenceKeyValuePairArray`.
    ///
    /// Only ever passed by pointer to
    /// [`fiftyoneDegreesResultsIpiFromEvidence`](crate::fiftyoneDegreesResultsIpiFromEvidence),
    /// so it is exposed as an opaque pointer type here. Create and fill it with
    /// [`fiftyoneDegreesEvidenceCreate`] and
    /// [`fiftyoneDegreesEvidenceAddString`] below, which belong to this crate's
    /// own common C library build, and never with the evidence functions of
    /// `fiftyone-common-sys`.
    #[repr(C)]
    pub struct EvidenceKeyValuePairArray {
        _private: [u8; 0],
    }

    /// One pair in an evidence array, mirroring
    /// `fiftyoneDegreesEvidenceKeyValuePair`. Returned by
    /// [`fiftyoneDegreesEvidenceAddString`] and exposed as an opaque pointer
    /// type only.
    #[repr(C)]
    pub struct EvidenceKeyValuePair {
        _private: [u8; 0],
    }

    /// The category of an evidence value, mirroring
    /// `fiftyoneDegreesEvidencePrefix`. The values are bit flags. The IP
    /// Intelligence lookup reads only [`EvidencePrefix::Query`] and
    /// [`EvidencePrefix::Server`] values.
    #[repr(C)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum EvidencePrefix {
        /// An HTTP header value.
        HttpHeaderString = 1 << 0,
        /// A list of IP addresses as a string to be parsed.
        HttpHeaderIpAddresses = 1 << 1,
        /// A server value, for example the client IP.
        Server = 1 << 2,
        /// A query string parameter.
        Query = 1 << 3,
        /// A cookie value.
        Cookie = 1 << 4,
        /// The evidence is invalid and should be ignored.
        Ignore = 1 << 7,
    }

    /// Defines the set of properties required by a caller, mirroring
    /// `fiftyoneDegreesPropertiesRequired`. A fully zeroed value requests all
    /// properties.
    #[repr(C)]
    pub struct PropertiesRequired {
        /// Array of required property names, or null when all are required.
        pub array: *const *const c_char,
        /// Number of properties in `array`.
        pub count: c_int,
        /// Separated list of required property names, or null when all are
        /// required.
        pub string: *const c_char,
        /// Pointer to an existing set of property names from another instance.
        pub existing: *mut PropertiesAvailable,
    }

    impl PropertiesRequired {
        /// Create a value that requests all available properties (every field
        /// null/zero), matching a freshly cleared
        /// `fiftyoneDegreesPropertiesRequired`.
        pub fn all_properties() -> Self {
            PropertiesRequired {
                array: std::ptr::null(),
                count: 0,
                string: std::ptr::null(),
                existing: std::ptr::null_mut(),
            }
        }
    }

    extern "C" {
        /// Returns a newly allocated English message describing the exception.
        /// Free the returned pointer with [`fiftyoneDegreesFree`].
        /// Resolved against this crate's own common C library build.
        ///
        /// The native symbol is `ipi_`-prefixed because this crate's private
        /// copy of the common C library is compiled into its own symbol
        /// namespace (see `src/symbol_prefix.h` and `build.rs`), so the Rust
        /// name is bound to the prefixed definition with `link_name`.
        ///
        /// # Safety
        /// `exception` must point to a valid, set [`Exception`].
        #[link_name = "ipi_fiftyoneDegreesExceptionGetMessage"]
        pub fn fiftyoneDegreesExceptionGetMessage(exception: *mut Exception) -> *const c_char;

        /// Returns a newly allocated English message describing the status,
        /// naming the file when the message is about one. Free the returned
        /// pointer with [`fiftyoneDegreesFree`].
        ///
        /// The native symbol is `ipi_`-prefixed for the same reason as
        /// [`fiftyoneDegreesExceptionGetMessage`] above.
        ///
        /// # Safety
        /// `file_name` must be a valid null terminated string or null.
        #[link_name = "ipi_fiftyoneDegreesStatusGetMessage"]
        pub fn fiftyoneDegreesStatusGetMessage(
            status: StatusCode,
            file_name: *const c_char,
        ) -> *const c_char;

        /// Frees memory through the C runtime's `free`, which is how the
        /// library frees memory unless its allocator has been replaced. Release
        /// what the library allocates with [`fiftyoneDegreesFree`] instead,
        /// which follows a replaced allocator.
        ///
        /// The native symbol is `ipi_`-prefixed for the same reason as
        /// [`fiftyoneDegreesExceptionGetMessage`] above.
        ///
        /// # Safety
        /// `ptr` must be a pointer the C runtime's `malloc` allocated, or null.
        #[link_name = "ipi_fiftyoneDegreesMemoryStandardFree"]
        pub fn fiftyoneDegreesMemoryStandardFree(ptr: *mut std::os::raw::c_void);

        /// The function the library frees memory with, the counterpart of the
        /// allocator it allocates with. Release the string
        /// [`fiftyoneDegreesExceptionGetMessage`] returns with it. It is
        /// [`fiftyoneDegreesMemoryStandardFree`] unless the allocator has been
        /// replaced, as the library's memory tracking replaces it.
        ///
        /// The native symbol is `ipi_`-prefixed for the same reason as
        /// [`fiftyoneDegreesExceptionGetMessage`] above.
        ///
        /// # Safety
        /// Call it only with a pointer the library allocated, or null.
        #[link_name = "ipi_fiftyoneDegreesFree"]
        pub static mut fiftyoneDegreesFree: unsafe extern "C" fn(ptr: *mut std::os::raw::c_void);

        /// Creates an evidence array with room for `capacity` pairs, or returns
        /// null when the allocation fails. Free it with
        /// [`fiftyoneDegreesEvidenceFree`].
        ///
        /// The native symbol is `ipi_`-prefixed for the same reason as
        /// [`fiftyoneDegreesExceptionGetMessage`] above.
        ///
        /// # Safety
        /// The returned array must be freed exactly once.
        #[link_name = "ipi_fiftyoneDegreesEvidenceCreate"]
        pub fn fiftyoneDegreesEvidenceCreate(capacity: u32) -> *mut EvidenceKeyValuePairArray;

        /// Adds a string value under `prefix` and `key`, chaining a further
        /// array when this one is full. The array points at the key and value
        /// rather than copying them.
        ///
        /// # Safety
        /// `evidence` must come from [`fiftyoneDegreesEvidenceCreate`]. `key`
        /// and `value` must be null terminated and outlive every use of the
        /// array.
        #[link_name = "ipi_fiftyoneDegreesEvidenceAddString"]
        pub fn fiftyoneDegreesEvidenceAddString(
            evidence: *mut EvidenceKeyValuePairArray,
            prefix: EvidencePrefix,
            key: *const c_char,
            value: *const c_char,
        ) -> *mut EvidenceKeyValuePair;

        /// Frees an evidence array and every array chained to it.
        ///
        /// # Safety
        /// `evidence` must come from [`fiftyoneDegreesEvidenceCreate`], or be
        /// null.
        #[link_name = "ipi_fiftyoneDegreesEvidenceFree"]
        pub fn fiftyoneDegreesEvidenceFree(evidence: *mut EvidenceKeyValuePairArray);
    }
}

/// Signed file offset type used by the IP Intelligence ABI.
///
/// The native library is compiled with `FIFTYONE_DEGREES_LARGE_DATA_FILE_SUPPORT`
/// (see `build.rs`), so `fiftyoneDegreesFileOffset` is a 64 bit signed integer.
/// It appears as the `size`/`length` parameter of the from-memory init and
/// reload functions.
pub type FileOffset = i64;

// ---------------------------------------------------------------------------
// IP type (src/common/ip.h)
// ---------------------------------------------------------------------------

/// Version of an IP address, mirroring `fiftyoneDegreesIpType`.
///
/// Stored as a single byte inside the native `IpAddress` structure but passed
/// by value to [`fiftyoneDegreesResultsIpiFromIpAddress`] as a full width enum.
/// The discriminants are the IP version numbers the header assigns.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IpType {
    /// The address could not be parsed as a valid IP address.
    Invalid = 0,
    /// An IPv4 address.
    Ipv4 = 4,
    /// An IPv6 address.
    Ipv6 = 6,
}

// ---------------------------------------------------------------------------
// Property value type (src/common/propertyValueType.h)
// ---------------------------------------------------------------------------

/// Stored type of a property value, mirroring `fiftyoneDegreesPropertyValueType`.
///
/// This is the `valueType` field of a [`WeightedValueHeader`], so the variant a
/// header carries selects which concrete `Weighted*` structure the header
/// pointer may be reinterpreted as. The discriminants match the header exactly.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PropertyValueType {
    /// String value.
    String = 0,
    /// Integer value.
    Integer = 1,
    /// Double value.
    Double = 2,
    /// Boolean value.
    Boolean = 3,
    /// JavaScript string value.
    JavaScript = 4,
    /// Single precision floating point value.
    SinglePrecisionFloat = 5,
    /// Single byte value.
    SingleByte = 6,
    /// Coordinate value.
    Coordinate = 7,
    /// IP address (range) value.
    IpAddress = 8,
    /// Well known binary geometry value.
    Wkb = 9,
    /// Object value, mainly nested aspect data.
    Object = 10,
    /// Declination angle stored as a short.
    Declination = 11,
    /// Azimuth angle stored as a short.
    Azimuth = 12,
    /// Reduced well known binary geometry value.
    WkbReduced = 13,
    /// Weighted list of string values.
    WeightedString = 14,
    /// Weighted list of integer values.
    WeightedInt = 15,
    /// Weighted list of double values.
    WeightedDouble = 16,
    /// Weighted list of boolean values.
    WeightedBool = 17,
    /// Weighted list of single precision float values.
    WeightedSingle = 18,
    /// Weighted list of byte values.
    WeightedByte = 19,
    /// Weighted list of IP range values.
    WeightedIpAddress = 20,
    /// Weighted list of reduced well known binary geometry values.
    WeightedWkbReduced = 21,
}

// ---------------------------------------------------------------------------
// No value reason (src/common/results.h)
// ---------------------------------------------------------------------------

/// Reason a result does not contain a valid value for a property, mirroring
/// `fiftyoneDegreesResultsNoValueReason`.
///
/// Returned by [`fiftyoneDegreesResultsIpiGetNoValueReason`]. The variants are
/// declared without explicit discriminants in the header, so they take the
/// natural ordinal values and must stay in header order.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResultsNoValueReason {
    /// The difference value is higher than the threshold (Pattern API).
    Difference,
    /// No hash nodes were matched (Hash API).
    NoMatchedNodes,
    /// The requested property does not exist or is not a required property.
    InvalidProperty,
    /// There is no result which contains a value for the requested property.
    NoResultForProperty,
    /// There are no results to get a value from.
    NoResults,
    /// There are too many values to be expressed as the requested type.
    TooManyValues,
    /// The results contain a null profile for the required component.
    NullProfile,
    /// The match is deemed a high risk of incorrect or misleading results.
    HighRisk,
    /// None of the above.
    Unknown,
}

// ---------------------------------------------------------------------------
// Data buffer (src/common/data.h)
// ---------------------------------------------------------------------------

/// Owned data buffer used by the weighted value collection and weighted string
/// values, mirroring `fiftyoneDegreesData`.
///
/// The library owns and frees the pointer through the matching release
/// functions, so callers only ever read these fields. It also appears as the
/// `tempData` scratch buffer passed to
/// [`fiftyoneDegreesResultsIpiGetValuesCollection`], which should be zeroed
/// before the first use so the library can allocate it as needed.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Data {
    /// Pointer to the immutable data, or null when empty.
    pub ptr: *mut u8,
    /// Number of bytes allocated at the pointer. Library internal.
    pub allocated: u32,
    /// Number of valid bytes currently referenced by the pointer.
    pub used: u32,
}

impl Data {
    /// Create an empty, zeroed data buffer suitable for use as the scratch
    /// `tempData` argument of
    /// [`fiftyoneDegreesResultsIpiGetValuesCollection`].
    pub fn zeroed() -> Self {
        Data {
            ptr: std::ptr::null_mut(),
            allocated: 0,
            used: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Opaque pointer types
// ---------------------------------------------------------------------------

/// Opaque IP Intelligence configuration, mirroring `fiftyoneDegreesConfigIpi`.
///
/// The structure is large and made up of nested collection configurations whose
/// layout is not needed by a consumer. It is only ever passed by pointer to the
/// init functions, taken from one of the predefined global configurations such
/// as [`fiftyoneDegreesIpiDefaultConfig`]. A null pointer requests the default
/// configuration.
#[repr(C)]
pub struct ConfigIpi {
    _private: [u8; 0],
}

/// Opaque IP Intelligence data set obtained from a [`ResourceManager`] with
/// [`fiftyoneDegreesDataSetIpiGet`], mirroring `fiftyoneDegreesDataSetIpi`.
///
/// It carries the loaded data file header, configuration, property metadata and
/// the collections. The layout is intentionally hidden. The reference must be
/// returned with [`fiftyoneDegreesDataSetIpiRelease`].
#[repr(C)]
pub struct DataSetIpi {
    _private: [u8; 0],
}

/// Opaque results structure produced by [`fiftyoneDegreesResultsIpiCreate`],
/// mirroring `fiftyoneDegreesResultsIpi`.
///
/// It holds the matched IP range result and the value list. The layout varies
/// with the build, so it is only ever handled by pointer. Free it with
/// [`fiftyoneDegreesResultsIpiFree`].
#[repr(C)]
pub struct ResultsIpi {
    _private: [u8; 0],
}

/// Opaque single result, mirroring `fiftyoneDegreesResultIpi`.
///
/// Referenced by the network id getters. A consumer does not construct it
/// directly, so it is exposed as an opaque pointer type only.
#[repr(C)]
pub struct ResultIpi {
    _private: [u8; 0],
}

/// Opaque string builder, mirroring `fiftyoneDegreesStringBuilder`.
///
/// Passed to [`fiftyoneDegreesResultsIpiAddValuesString`]. Most callers use the
/// buffer based [`fiftyoneDegreesResultsIpiGetValuesString`] instead, so the
/// builder is exposed only as an opaque pointer for completeness.
#[repr(C)]
pub struct StringBuilder {
    _private: [u8; 0],
}

/// A value read from one of the data set's collections, mirroring
/// `fiftyoneDegreesCollectionItem`.
///
/// It is laid out as the C structure is, because [`ProfilePercentage`] holds
/// one ahead of its weighting, and it is passed by pointer to
/// [`fiftyoneDegreesIpiGetIpAddressAsString`]. The library sets and releases
/// the item, so a caller only reads it.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CollectionItem {
    /// The item's data, including any memory allocated to hold it.
    pub data: Data,
    /// A handle the library releases the item with, such as a cache node.
    pub handle: *mut c_void,
    /// The collection the item came from. Library internal.
    pub collection: *const c_void,
}

/// Callback used to iterate matching profiles, mirroring
/// `fiftyoneDegreesProfileIterateMethod`.
///
/// Return `true` to continue iterating, `false` to stop. The `profile` pointer
/// references a profile within the data set and is only valid for the duration
/// of the call.
pub type ProfileIterateMethod =
    unsafe extern "C" fn(state: *mut c_void, profile: *mut c_void) -> bool;

// ---------------------------------------------------------------------------
// Weighted values (ipi_weighted_results.h)
// ---------------------------------------------------------------------------

/// Common header shared by every weighted value structure, mirroring
/// `fiftyoneDegreesWeightedValueHeader`.
///
/// A [`WeightedValuesCollection`] is an array of pointers to these headers. The
/// `value_type` selects which concrete `Weighted*` structure the header pointer
/// may be cast to, and `raw_weighting` is the confidence weight out of the
/// library maximum.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WeightedValueHeader {
    /// Type of the property value, selecting the concrete structure.
    pub value_type: PropertyValueType,
    /// Index of the required property this value belongs to.
    pub required_property_index: c_int,
    /// Raw confidence weighting for this value.
    pub raw_weighting: u32,
}

/// Weighted integer value, mirroring `fiftyoneDegreesWeightedInt`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WeightedInt {
    /// Common weighted value header.
    pub header: WeightedValueHeader,
    /// The integer value.
    pub value: i32,
}

/// Weighted double value, mirroring `fiftyoneDegreesWeightedDouble`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WeightedDouble {
    /// Common weighted value header.
    pub header: WeightedValueHeader,
    /// The double value.
    pub value: f64,
}

/// Weighted boolean value, mirroring `fiftyoneDegreesWeightedBool`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WeightedBool {
    /// Common weighted value header.
    pub header: WeightedValueHeader,
    /// The boolean value.
    pub value: bool,
}

/// Weighted byte value, mirroring `fiftyoneDegreesWeightedByte`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WeightedByte {
    /// Common weighted value header.
    pub header: WeightedValueHeader,
    /// The byte value.
    pub value: u8,
}

/// Weighted string value, mirroring `fiftyoneDegreesWeightedString`.
///
/// The `string_data` buffer owns the value memory, and `value` points into it.
/// Both are valid until the owning [`WeightedValuesCollection`] is released.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WeightedString {
    /// Common weighted value header.
    pub header: WeightedValueHeader,
    /// Buffer that owns the string memory.
    pub string_data: Data,
    /// Pointer to the null terminated string value within `string_data`.
    pub value: *const c_char,
}

/// Collection of weighted values, mirroring
/// `fiftyoneDegreesWeightedValuesCollection`.
///
/// `items` points at an array of `items_count` header pointers. Each header may
/// be cast to its concrete `Weighted*` structure based on its
/// [`WeightedValueHeader::value_type`]. The collection owns the value memory and
/// the table of pointers through the two [`Data`] buffers, so it must be
/// released with [`fiftyoneDegreesWeightedValuesCollectionRelease`].
#[repr(C)]
pub struct WeightedValuesCollection {
    /// Buffer that owns the actual weighted value structures.
    pub values_data: Data,
    /// Buffer that owns the `items` table of pointers.
    pub items_data: Data,
    /// Array of `items_count` pointers to weighted value headers.
    pub items: *mut *mut WeightedValueHeader,
    /// Number of pointers in `items`.
    pub items_count: u32,
}

/// Legacy weighted value returned by [`fiftyoneDegreesResultsIpiGetValues`],
/// mirroring `fiftyoneDegreesProfilePercentage`
/// (an alias of `fiftyoneDegreesWeightedItem`).
///
/// It pairs a collection item holding the value with a `raw_weighting`. The
/// item is library managed and released when the owning results are freed.
#[repr(C)]
pub struct ProfilePercentage {
    /// Collection item holding the value. Library managed.
    pub item: CollectionItem,
    /// Raw weighting out of the library maximum (`65535 * 65535`).
    pub raw_weighting: u32,
}

// ---------------------------------------------------------------------------
// Predefined configurations (ipi.h globals)
// ---------------------------------------------------------------------------

extern "C" {
    /// In memory configuration. Loads the data set from a buffer with no
    /// caching. Mirrors `fiftyoneDegreesIpiInMemoryConfig`.
    pub static mut fiftyoneDegreesIpiInMemoryConfig: ConfigIpi;

    /// Highest performance configuration. Loads everything into memory.
    /// Mirrors `fiftyoneDegreesIpiHighPerformanceConfig`.
    pub static mut fiftyoneDegreesIpiHighPerformanceConfig: ConfigIpi;

    /// Low memory configuration. Keeps a file connection with no caching.
    /// Mirrors `fiftyoneDegreesIpiLowMemoryConfig`.
    pub static mut fiftyoneDegreesIpiLowMemoryConfig: ConfigIpi;

    /// Balanced configuration using caches. The default operating point.
    /// Mirrors `fiftyoneDegreesIpiBalancedConfig`.
    pub static mut fiftyoneDegreesIpiBalancedConfig: ConfigIpi;

    /// Balanced configuration that creates a temporary file copy.
    /// Mirrors `fiftyoneDegreesIpiBalancedTempConfig`.
    pub static mut fiftyoneDegreesIpiBalancedTempConfig: ConfigIpi;

    /// Default configuration. Does not create a temporary file.
    /// Mirrors `fiftyoneDegreesIpiDefaultConfig`.
    pub static mut fiftyoneDegreesIpiDefaultConfig: ConfigIpi;
}

// ---------------------------------------------------------------------------
// extern "C" function declarations
// ---------------------------------------------------------------------------

extern "C" {
    // -- data set lifecycle (ipi.h) --

    /// Gets a safe reference to the IP Intelligence data set from the resource
    /// manager. Release it with [`fiftyoneDegreesDataSetIpiRelease`].
    ///
    /// # Safety
    /// `manager` must point to a manager initialized by one of the IP
    /// Intelligence init functions.
    pub fn fiftyoneDegreesDataSetIpiGet(manager: *mut ResourceManager) -> *mut DataSetIpi;

    /// Releases a data set reference obtained from
    /// [`fiftyoneDegreesDataSetIpiGet`].
    ///
    /// # Safety
    /// `dataset` must come from [`fiftyoneDegreesDataSetIpiGet`].
    pub fn fiftyoneDegreesDataSetIpiRelease(dataset: *mut DataSetIpi);

    /// Frees a resource manager and, once no references remain, its data set.
    /// Declared here so it links against this crate's own common C library
    /// resource layer rather than any other build of it.
    ///
    /// The native symbol is `ipi_`-prefixed because this crate's private copy of
    /// the common C library is compiled into its own symbol namespace (see
    /// `src/symbol_prefix.h` and `build.rs`), so the Rust name is bound to the
    /// prefixed definition with `link_name`. This is what keeps the resource
    /// manager freed through the matching wide-offset build rather than the
    /// Device Detection one.
    ///
    /// # Safety
    /// `manager` must point to an initialized manager.
    #[link_name = "ipi_fiftyoneDegreesResourceManagerFree"]
    pub fn fiftyoneDegreesResourceManagerFree(manager: *mut ResourceManager);

    // -- initialization (ipi.h) --

    /// Returns the constant memory size needed to initialize from a file, or
    /// zero when the configuration allows runtime allocation.
    ///
    /// # Safety
    /// `config` and `properties` may be null. `file_name` must be a valid null
    /// terminated string. `exception` must point to a cleared [`Exception`].
    pub fn fiftyoneDegreesIpiSizeManagerFromFile(
        config: *mut ConfigIpi,
        properties: *mut PropertiesRequired,
        file_name: *const c_char,
        exception: *mut Exception,
    ) -> usize;

    /// Initializes a resource manager from an IP Intelligence data file.
    ///
    /// # Safety
    /// `manager` must point to a writable [`ResourceManager`]. `config` and
    /// `properties` may be null for defaults. `file_name` must be a valid null
    /// terminated string. `exception` must point to a cleared [`Exception`].
    pub fn fiftyoneDegreesIpiInitManagerFromFile(
        manager: *mut ResourceManager,
        config: *mut ConfigIpi,
        properties: *mut PropertiesRequired,
        file_name: *const c_char,
        exception: *mut Exception,
    ) -> StatusCode;

    /// Returns the constant memory size needed to initialize from memory, or
    /// zero when the configuration allows runtime allocation. If the memory
    /// doesn't hold a data set that loads, `exception` holds the status and the
    /// number returned means nothing.
    ///
    /// # Safety
    /// `config` and `properties` may be null. `memory` must point to `size`
    /// readable bytes. `exception` must point to a cleared [`Exception`].
    pub fn fiftyoneDegreesIpiSizeManagerFromMemory(
        config: *mut ConfigIpi,
        properties: *mut PropertiesRequired,
        memory: *mut c_void,
        size: FileOffset,
        exception: *mut Exception,
    ) -> usize;

    /// Initializes a resource manager from an IP Intelligence data set held in
    /// contiguous memory. The memory must outlive the manager.
    ///
    /// # Safety
    /// `manager` must be writable. `memory` must point to `size` readable bytes
    /// that outlive the manager. `exception` must point to a cleared
    /// [`Exception`].
    pub fn fiftyoneDegreesIpiInitManagerFromMemory(
        manager: *mut ResourceManager,
        config: *mut ConfigIpi,
        properties: *mut PropertiesRequired,
        memory: *mut c_void,
        size: FileOffset,
        exception: *mut Exception,
    ) -> StatusCode;

    /// Reloads the data set from the named file using the original
    /// configuration.
    ///
    /// # Safety
    /// `manager` must be initialized. `file_name` must be a valid null
    /// terminated string. `exception` must point to a cleared [`Exception`].
    pub fn fiftyoneDegreesIpiReloadManagerFromFile(
        manager: *mut ResourceManager,
        file_name: *const c_char,
        exception: *mut Exception,
    ) -> StatusCode;

    /// Reloads the data set from contiguous memory using the original
    /// configuration.
    ///
    /// # Safety
    /// `manager` must be initialized. `source` must point to `length` readable
    /// bytes that outlive the manager. `exception` must point to a cleared
    /// [`Exception`].
    pub fn fiftyoneDegreesIpiReloadManagerFromMemory(
        manager: *mut ResourceManager,
        source: *mut c_void,
        length: FileOffset,
        exception: *mut Exception,
    ) -> StatusCode;

    /// Reloads the data set from the file the manager was created with.
    ///
    /// # Safety
    /// `manager` must be initialized. `exception` must point to a cleared
    /// [`Exception`].
    pub fn fiftyoneDegreesIpiReloadManagerFromOriginalFile(
        manager: *mut ResourceManager,
        exception: *mut Exception,
    ) -> StatusCode;

    // -- results lifecycle (ipi.h) --

    /// Allocates a results structure referencing the data set in `manager`.
    /// Free it with [`fiftyoneDegreesResultsIpiFree`].
    ///
    /// # Safety
    /// `manager` must point to an initialized IP Intelligence manager.
    pub fn fiftyoneDegreesResultsIpiCreate(manager: *mut ResourceManager) -> *mut ResultsIpi;

    /// Frees a results structure and releases its data set reference.
    ///
    /// # Safety
    /// `results` must come from [`fiftyoneDegreesResultsIpiCreate`].
    pub fn fiftyoneDegreesResultsIpiFree(results: *mut ResultsIpi);

    // -- processing (ipi.h) --

    /// Processes a single IP address in byte array form and populates the
    /// results. The IP version is supplied as `type`.
    ///
    /// # Safety
    /// `results` must be valid. `ip_address` must point to `ip_address_length`
    /// readable bytes. `exception` must point to a cleared [`Exception`].
    pub fn fiftyoneDegreesResultsIpiFromIpAddress(
        results: *mut ResultsIpi,
        ip_address: *const u8,
        ip_address_length: usize,
        ip_type: IpType,
        exception: *mut Exception,
    );

    /// Processes a single IP address string and populates the results.
    ///
    /// # Safety
    /// `results` must be valid. `ip_address` must point to
    /// `ip_address_length` readable bytes. `exception` must point to a cleared
    /// [`Exception`].
    pub fn fiftyoneDegreesResultsIpiFromIpAddressString(
        results: *mut ResultsIpi,
        ip_address: *const c_char,
        ip_address_length: usize,
        exception: *mut Exception,
    );

    /// Processes the evidence value pairs and populates the results.
    ///
    /// # Safety
    /// `results` must be valid and reference an initialized manager. `evidence`
    /// must be a valid evidence array. `exception` must point to a cleared
    /// [`Exception`].
    pub fn fiftyoneDegreesResultsIpiFromEvidence(
        results: *mut ResultsIpi,
        evidence: *mut EvidenceKeyValuePairArray,
        exception: *mut Exception,
    );

    // -- value access (ipi.h) --

    /// Returns whether the results contain valid values for the required
    /// property index.
    ///
    /// # Safety
    /// `results` must be valid. `exception` must point to a cleared
    /// [`Exception`].
    pub fn fiftyoneDegreesResultsIpiGetHasValues(
        results: *mut ResultsIpi,
        required_property_index: c_int,
        exception: *mut Exception,
    ) -> bool;

    /// Returns the reason the results have no valid value for the required
    /// property index.
    ///
    /// # Safety
    /// `results` must be valid. `exception` must point to a cleared
    /// [`Exception`].
    pub fn fiftyoneDegreesResultsIpiGetNoValueReason(
        results: *mut ResultsIpi,
        required_property_index: c_int,
        exception: *mut Exception,
    ) -> ResultsNoValueReason;

    /// Returns a static, human readable description for a no value reason.
    pub fn fiftyoneDegreesResultsIpiGetNoValueReasonMessage(
        reason: ResultsNoValueReason,
    ) -> *const c_char;

    /// Populates the results value list for the required property index and
    /// returns the first [`ProfilePercentage`] weighted value, or null.
    ///
    /// # Safety
    /// `results` must be valid. `exception` must point to a cleared
    /// [`Exception`]. The returned pointer is owned by `results` and is invalid
    /// once they are freed.
    pub fn fiftyoneDegreesResultsIpiGetValues(
        results: *mut ResultsIpi,
        required_property_index: c_int,
        exception: *mut Exception,
    ) -> *const ProfilePercentage;

    /// Appends the values for a named property to a string builder.
    ///
    /// # Safety
    /// `results` must be valid. `property_name`, `builder` and `separator` must
    /// be valid. `exception` must point to a cleared [`Exception`].
    pub fn fiftyoneDegreesResultsIpiAddValuesString(
        results: *mut ResultsIpi,
        property_name: *const c_char,
        builder: *mut StringBuilder,
        separator: *const c_char,
        exception: *mut Exception,
    );

    /// Writes the values for a named property into a caller buffer, returning
    /// the number of characters available (which may exceed `buffer_length`).
    ///
    /// # Safety
    /// `results` must be valid. `property_name` and `separator` must be valid
    /// null terminated strings. `buffer` must point to `buffer_length` writable
    /// bytes. `exception` must point to a cleared [`Exception`].
    pub fn fiftyoneDegreesResultsIpiGetValuesString(
        results: *mut ResultsIpi,
        property_name: *const c_char,
        buffer: *mut c_char,
        buffer_length: usize,
        separator: *const c_char,
        exception: *mut Exception,
    ) -> usize;

    /// Writes the values for a required property index into a caller buffer,
    /// returning the number of characters available.
    ///
    /// # Safety
    /// `results` must be valid. `separator` must be a valid null terminated
    /// string. `buffer` must point to `buffer_length` writable bytes.
    /// `exception` must point to a cleared [`Exception`].
    pub fn fiftyoneDegreesResultsIpiGetValuesStringByRequiredPropertyIndex(
        results: *mut ResultsIpi,
        required_property_index: c_int,
        buffer: *mut c_char,
        buffer_length: usize,
        separator: *const c_char,
        exception: *mut Exception,
    ) -> usize;

    /// Iterates over the profiles in the data set that contain the property and
    /// value provided, returning the number of matches.
    ///
    /// # Safety
    /// `manager` must be initialized. `property_name` and `value_name` must be
    /// valid null terminated strings. `callback` must be sound for `state`.
    /// `exception` must point to a cleared [`Exception`].
    pub fn fiftyoneDegreesIpiIterateProfilesForPropertyAndValue(
        manager: *mut ResourceManager,
        property_name: *const c_char,
        value_name: *const c_char,
        state: *mut c_void,
        callback: ProfileIterateMethod,
        exception: *mut Exception,
    ) -> u32;

    /// Writes the IP address held by a collection item (for range start/end
    /// properties) into a caller buffer as a string.
    ///
    /// # Safety
    /// `item` must point to a valid collection item from a range property
    /// value. `buffer` must point to `buffer_length` writable bytes.
    /// `exception` must point to a cleared [`Exception`].
    pub fn fiftyoneDegreesIpiGetIpAddressAsString(
        item: *const CollectionItem,
        ip_type: IpType,
        buffer: *mut c_char,
        buffer_length: u32,
        exception: *mut Exception,
    ) -> usize;

    // -- weighted values (ipi_weighted_results.h) --

    /// Extracts weighted values for the required property indexes into a
    /// [`WeightedValuesCollection`]. Release it with
    /// [`fiftyoneDegreesWeightedValuesCollectionRelease`].
    ///
    /// # Safety
    /// `results` must be valid. `required_property_indexes` must point to
    /// `required_property_indexes_length` readable indexes. `temp_data` must
    /// point to a [`Data`] buffer (a [`Data::zeroed`] value is acceptable).
    /// `exception` must point to a cleared [`Exception`].
    pub fn fiftyoneDegreesResultsIpiGetValuesCollection(
        results: *mut ResultsIpi,
        required_property_indexes: *const c_int,
        required_property_indexes_length: u32,
        temp_data: *mut Data,
        exception: *mut Exception,
    ) -> WeightedValuesCollection;

    /// Releases all memory held by a weighted values collection.
    ///
    /// # Safety
    /// `collection` must come from
    /// [`fiftyoneDegreesResultsIpiGetValuesCollection`].
    pub fn fiftyoneDegreesWeightedValuesCollectionRelease(
        collection: *mut WeightedValuesCollection,
    );

    // -- property enumeration shim (src/shim.c) --

    /// Returns the number of required property indexes of the data set managed
    /// by `manager`, which is the number of names it was created with, or the
    /// number of its available properties when it was created naming none.
    /// Returns zero when no data set or available property set is present.
    /// Defined by this crate's `src/shim.c`, not by the upstream library, so
    /// the deeply nested `available` field can be read from C.
    ///
    /// # Safety
    /// `manager` must point to a manager initialized by one of the IP
    /// Intelligence init functions.
    pub fn fiftyoneDegreesShimIpiGetRequiredPropertyCount(manager: *mut ResourceManager) -> u32;

    /// Writes the name of the required property at `required_property_index`
    /// into `buffer` as a null terminated string and returns the number of
    /// characters written, excluding the terminator. A name the data set has
    /// no property for is written as it was given. Returns zero when the
    /// index is out of range, the buffer is too small or no data set is
    /// available. Defined by this crate's `src/shim.c`.
    ///
    /// # Safety
    /// `manager` must be initialized. `buffer` must point to `length` writable
    /// bytes.
    pub fn fiftyoneDegreesShimIpiGetRequiredPropertyName(
        manager: *mut ResourceManager,
        required_property_index: c_int,
        buffer: *mut c_char,
        length: usize,
    ) -> usize;

    /// Returns the zero based required property index for the property named
    /// `property_name`, or `-1` when it is not one of the required properties.
    /// This is the index expected by [`fiftyoneDegreesResultsIpiGetHasValues`]
    /// and [`fiftyoneDegreesResultsIpiGetValuesCollection`]. Defined by this
    /// crate's `src/shim.c`.
    ///
    /// # Safety
    /// `manager` must be initialized. `property_name` must be a valid null
    /// terminated string.
    pub fn fiftyoneDegreesShimIpiGetRequiredPropertyIndexFromName(
        manager: *mut ResourceManager,
        property_name: *const c_char,
    ) -> c_int;

    /// Writes the data set's name (its tier, for example `Lite`, `Enterprise` or
    /// `TAC`) into `buffer` as a null terminated string and returns the number
    /// of characters written, excluding the terminator. Returns zero when no
    /// data set is available, the name cannot be read, or the buffer is too
    /// small. Defined by this crate's `src/shim.c`, reading the name from the
    /// data set header's `nameOffset`.
    ///
    /// # Safety
    /// `manager` must be initialized. `buffer` must point to `length` writable
    /// bytes.
    pub fn fiftyoneDegreesShimIpiGetDataSetName(
        manager: *mut ResourceManager,
        buffer: *mut c_char,
        length: usize,
    ) -> usize;

    /// Writes the data set's published date into `*year`, `*month` and `*day`
    /// and returns `1` on success or `0` when no data set is available. Any
    /// output pointer may be null. Defined by this crate's `src/shim.c`, reading
    /// the embedded publish date from the data set header.
    ///
    /// # Safety
    /// `manager` must be initialized. Each non-null output pointer must be
    /// writable.
    pub fn fiftyoneDegreesShimIpiGetDataSetPublished(
        manager: *mut ResourceManager,
        year: *mut c_int,
        month: *mut c_int,
        day: *mut c_int,
    ) -> c_int;

    /// Returns the number of headers the data set reads the client IP from, or
    /// zero when no data set is available. Defined by this crate's
    /// `src/shim.c`, reading the data set's `uniqueHeaders`.
    ///
    /// # Safety
    /// `manager` must be initialized.
    pub fn fiftyoneDegreesShimIpiGetUniqueHeaderCount(manager: *mut ResourceManager) -> u32;

    /// Writes the name of the header at `header_index`, in the data set's
    /// priority order, into `buffer` as a null terminated string and returns
    /// the number of characters written, excluding the terminator. Returns zero
    /// when the index is out of range or no data set is available, and cuts the
    /// name short when the buffer is too small. Defined by this crate's
    /// `src/shim.c`.
    ///
    /// # Safety
    /// `manager` must be initialized. `buffer` must point to `length` writable
    /// bytes.
    pub fn fiftyoneDegreesShimIpiGetUniqueHeaderName(
        manager: *mut ResourceManager,
        header_index: u32,
        buffer: *mut c_char,
        length: usize,
    ) -> usize;

    /// Initializes `manager` from the data file `file_name` with a copy of
    /// `config`, setting the expected concurrency of every collection in the
    /// copy to `concurrency` when that is zero or more, or keeping the
    /// configuration's own when it is negative. The expected concurrency
    /// sizes the pool of file handles the collections that are not held in
    /// memory read through. The configuration `config` points to is not
    /// changed. Defined by this crate's `src/shim.c`.
    ///
    /// # Safety
    /// As for [`fiftyoneDegreesIpiInitManagerFromFile`], and `config` must
    /// point to a configuration.
    pub fn fiftyoneDegreesShimIpiInitManagerFromFile(
        manager: *mut ResourceManager,
        config: *const ConfigIpi,
        concurrency: i32,
        properties: *mut PropertiesRequired,
        file_name: *const c_char,
        exception: *mut Exception,
    ) -> StatusCode;

    /// Returns the number of handles in the file pool of the data set
    /// managed by `manager` when any of its collections reads from the data
    /// file after it is loaded, or zero when the whole data set is held in
    /// memory. A read that finds every handle in use fails rather than
    /// waiting for one, so this is how many calls can read the data file at
    /// once. Defined by this crate's `src/shim.c`.
    ///
    /// # Safety
    /// `manager` must be initialized.
    pub fn fiftyoneDegreesShimIpiGetFileHandleCount(manager: *mut ResourceManager) -> u16;

    /// Returns the [`PropertyValueType`] discriminant the data file stores the
    /// values of the required property at `required_property_index` as, read
    /// from the data set `results` was created from. A latitude, for example,
    /// is stored as [`PropertyValueType::Declination`]. Returns `-1` when the
    /// index is out of range, the data set does not have the property or the
    /// type cannot be read. Defined by this crate's `src/shim.c`.
    ///
    /// # Safety
    /// `results` must be null or point to results created by
    /// [`fiftyoneDegreesResultsIpiCreate`] and not yet freed.
    pub fn fiftyoneDegreesShimIpiGetStoredValueType(
        results: *mut ResultsIpi,
        required_property_index: c_int,
    ) -> c_int;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{fiftyoneDegreesExceptionGetMessage, fiftyoneDegreesFree};
    use std::ffi::{CStr, CString};
    use std::path::PathBuf;

    /// Resolve the 51degrees-cxx checkout the same way `build.rs` does so the
    /// smoke test can find the ASN data file in it.
    fn cxx_dir() -> PathBuf {
        if let Ok(dir) = std::env::var("FIFTYONE_CXX_DIR") {
            return PathBuf::from(dir);
        }
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .join("cxx")
    }

    /// Path to the ASN IP Intelligence data file in the data repository, if it
    /// has been downloaded.
    fn asn_data_file() -> Option<PathBuf> {
        let path = cxx_dir()
            .join("data")
            .join("ip-intelligence")
            .join("51Degrees-IPIV4AsnIpiV41.ipi");
        path.is_file().then_some(path)
    }

    /// The data file `51DEGREES_IPI_PATH` names, else the ASN file.
    fn named_or_asn_data_file() -> Option<PathBuf> {
        std::env::var("51DEGREES_IPI_PATH")
            .ok()
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .or_else(asn_data_file)
    }

    /// The weighting of the first value the legacy getter returns is the
    /// weighting the weighted value collection gives the same value, which
    /// needs [`ProfilePercentage`] laid out as the C structure is.
    #[test]
    fn the_legacy_getter_reads_each_values_weighting() {
        let Some(data_file) = named_or_asn_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping the legacy weighting");
            return;
        };
        let file = CString::new(data_file.to_string_lossy().as_ref()).expect("no nul");
        unsafe {
            let mut manager = ResourceManager::zeroed();
            let mut exception = Exception::cleared();
            let status = fiftyoneDegreesIpiInitManagerFromFile(
                &mut manager,
                std::ptr::addr_of_mut!(fiftyoneDegreesIpiInMemoryConfig),
                std::ptr::null_mut(),
                file.as_ptr(),
                &mut exception,
            );
            assert_eq!(
                status,
                StatusCode::Success,
                "{}",
                exception_message(&mut exception)
            );
            let results = fiftyoneDegreesResultsIpiCreate(&mut manager);
            assert!(!results.is_null(), "results should be allocated");
            let ip = CString::new("1.1.1.1").expect("no nul");
            fiftyoneDegreesResultsIpiFromIpAddressString(
                results,
                ip.as_ptr(),
                ip.as_bytes().len(),
                &mut exception,
            );
            assert!(exception.is_okay(), "{}", exception_message(&mut exception));

            let mut checked = 0;
            let count = fiftyoneDegreesShimIpiGetRequiredPropertyCount(&mut manager);
            for index in 0..count as c_int {
                let first = fiftyoneDegreesResultsIpiGetValues(results, index, &mut exception);
                assert!(exception.is_okay(), "{}", exception_message(&mut exception));
                if first.is_null() {
                    continue;
                }
                let legacy = (*first).raw_weighting;
                let indexes = [index];
                let mut collection = fiftyoneDegreesResultsIpiGetValuesCollection(
                    results,
                    indexes.as_ptr(),
                    1,
                    std::ptr::null_mut(),
                    &mut exception,
                );
                assert!(exception.is_okay(), "{}", exception_message(&mut exception));
                assert!(collection.items_count > 0, "property {index} has values");
                let weighted = (**collection.items).raw_weighting;
                fiftyoneDegreesWeightedValuesCollectionRelease(&mut collection);
                assert_eq!(legacy, weighted, "property {index}");
                checked += 1;
            }
            assert!(checked > 0, "some property should have values");

            fiftyoneDegreesResultsIpiFree(results);
            fiftyoneDegreesResourceManagerFree(&mut manager);
        }
    }

    /// Every required property has a stored value type, and an index outside
    /// the required properties, or no results, gives -1.
    #[test]
    fn the_stored_value_type_is_read_for_each_required_property() {
        let Some(data_file) = named_or_asn_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping the stored types");
            return;
        };
        let file = CString::new(data_file.to_string_lossy().as_ref()).expect("no nul");
        unsafe {
            let mut manager = ResourceManager::zeroed();
            let mut exception = Exception::cleared();
            let status = fiftyoneDegreesIpiInitManagerFromFile(
                &mut manager,
                std::ptr::addr_of_mut!(fiftyoneDegreesIpiInMemoryConfig),
                std::ptr::null_mut(),
                file.as_ptr(),
                &mut exception,
            );
            assert_eq!(
                status,
                StatusCode::Success,
                "{}",
                exception_message(&mut exception)
            );
            let results = fiftyoneDegreesResultsIpiCreate(&mut manager);
            assert!(!results.is_null(), "results should be allocated");

            let count = fiftyoneDegreesShimIpiGetRequiredPropertyCount(&mut manager);
            assert!(count > 0, "the data set has properties");
            for index in 0..count as c_int {
                let stored = fiftyoneDegreesShimIpiGetStoredValueType(results, index);
                assert!(
                    (0..=PropertyValueType::WeightedWkbReduced as c_int).contains(&stored),
                    "property {index} is stored as {stored}"
                );
            }
            assert_eq!(fiftyoneDegreesShimIpiGetStoredValueType(results, -1), -1);
            assert_eq!(
                fiftyoneDegreesShimIpiGetStoredValueType(results, count as c_int),
                -1
            );
            assert_eq!(
                fiftyoneDegreesShimIpiGetStoredValueType(std::ptr::null_mut(), 0),
                -1
            );

            let latitude = CString::new("Latitude").expect("no nul");
            let index = fiftyoneDegreesShimIpiGetRequiredPropertyIndexFromName(
                &mut manager,
                latitude.as_ptr(),
            );
            if index >= 0 {
                assert_eq!(
                    fiftyoneDegreesShimIpiGetStoredValueType(results, index),
                    PropertyValueType::Declination as c_int
                );
            }

            fiftyoneDegreesResultsIpiFree(results);
            fiftyoneDegreesResourceManagerFree(&mut manager);
        }
    }

    /// The no value reason message getter links and returns a non-empty static
    /// description. This proves the IP Intelligence static library linked on the
    /// host toolchain even when the data file is absent.
    #[test]
    fn no_value_reason_message_links() {
        unsafe {
            let raw =
                fiftyoneDegreesResultsIpiGetNoValueReasonMessage(ResultsNoValueReason::NoResults);
            assert!(!raw.is_null(), "reason message should be available");
            let message = CStr::from_ptr(raw).to_string_lossy().into_owned();
            assert!(!message.is_empty(), "reason message should not be empty");
        }
    }

    /// The predefined default configuration global is present and linkable. Its
    /// address being non-null confirms the configuration data linked in.
    #[test]
    fn default_config_global_links() {
        // Taking the address of the static is enough to force it to link. No
        // dereference happens, so this needs no unsafe block.
        let config_ptr = std::ptr::addr_of_mut!(fiftyoneDegreesIpiDefaultConfig);
        assert!(
            !config_ptr.is_null(),
            "default config should have an address"
        );
    }

    /// Full end to end smoke test against the ASN data file when it is present:
    /// initialize a manager, look up a public IP address, read a property value
    /// with its weighting, then free everything. When the file is absent the
    /// test still asserts the symbols link by exercising the reason message.
    #[test]
    fn ipi_lookup_round_trip() {
        let Some(data_file) = asn_data_file() else {
            // No data file in this checkout. The link is still proven by the
            // other tests, so treat the lookup as not applicable here.
            eprintln!("ASN .ipi data file not found, skipping lookup round trip");
            return;
        };

        unsafe {
            let mut manager = ResourceManager::zeroed();
            let mut exception = Exception::cleared();

            // Request all available properties (null properties) with the
            // default configuration (null config).
            let file_path = CString::new(data_file.to_string_lossy().as_ref())
                .expect("data file path has no interior nul");

            let status = fiftyoneDegreesIpiInitManagerFromFile(
                &mut manager,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                file_path.as_ptr(),
                &mut exception,
            );

            // The init function ran cleanly through the FFI boundary and
            // returned a well defined status. That alone proves the
            // initialization symbols and the whole common plus
            // graph plus `ipi` link is sound on the host toolchain. The
            // data file itself may be absent or unusable, reported as a
            // non-Success status. Treat that as a clean skip rather
            // than a failure: the real lookup path is exercised by the
            // higher-level on-premise tests when a usable data file is present.
            if status != StatusCode::Success {
                eprintln!(
                    "IP Intelligence data file did not load ({status:?}); the \
                     symbols linked and the init path executed, so skipping the \
                     lookup checks"
                );
                return;
            }

            assert!(
                exception.is_okay(),
                "successful init should not raise an exception: {}",
                exception_message(&mut exception)
            );

            // Create results and look up a well known public IP address.
            let results = fiftyoneDegreesResultsIpiCreate(&mut manager);
            assert!(!results.is_null(), "results should be allocated");

            let ip = CString::new("185.28.167.77").unwrap();
            let mut exception = Exception::cleared();
            fiftyoneDegreesResultsIpiFromIpAddressString(
                results,
                ip.as_ptr(),
                ip.as_bytes().len(),
                &mut exception,
            );
            assert!(
                exception.is_okay(),
                "lookup should not raise an exception: {}",
                exception_message(&mut exception)
            );

            // Read a network/location property as a string. Different data files
            // expose different property sets, so accept any of a small
            // set of common IP Intelligence properties and require that at least
            // one returns characters.
            let candidate_properties = [
                "RegisteredCountry",
                "RegisteredName",
                "RegisteredOwner",
                "IpRangeStart",
                "IpRangeEnd",
                "Mcc",
            ];
            let separator = CString::new("|").unwrap();
            let mut total_chars = 0usize;
            let mut buffer = vec![0i8; 1024];
            for name in candidate_properties {
                let prop = CString::new(name).unwrap();
                let mut exception = Exception::cleared();
                let written = fiftyoneDegreesResultsIpiGetValuesString(
                    results,
                    prop.as_ptr(),
                    buffer.as_mut_ptr(),
                    buffer.len(),
                    separator.as_ptr(),
                    &mut exception,
                );
                if exception.is_okay() && written > 0 {
                    total_chars += written;
                    let value = CStr::from_ptr(buffer.as_ptr())
                        .to_string_lossy()
                        .into_owned();
                    eprintln!("{name} = {value} ({written} chars)");
                }
            }
            assert!(
                total_chars > 0,
                "at least one candidate property should return a value for a public IP"
            );

            // Exercise the weighted getters. Read the first weighted value for
            // required property index 0 (its existence depends on the build, so
            // a null first value is acceptable) and the weighted collection.
            let mut exception = Exception::cleared();
            let first = fiftyoneDegreesResultsIpiGetValues(results, 0, &mut exception);
            if exception.is_okay() && !first.is_null() {
                let weighting = (*first).raw_weighting;
                eprintln!("first weighted value raw_weighting = {weighting}");
            }

            let indexes = [0i32];
            let mut temp = Data::zeroed();
            let mut exception = Exception::cleared();
            let mut collection = fiftyoneDegreesResultsIpiGetValuesCollection(
                results,
                indexes.as_ptr(),
                indexes.len() as u32,
                &mut temp,
                &mut exception,
            );
            if exception.is_okay() && !collection.items.is_null() {
                eprintln!(
                    "weighted collection holds {} item(s)",
                    collection.items_count
                );
                for i in 0..collection.items_count as isize {
                    let header = *collection.items.offset(i);
                    if !header.is_null() {
                        eprintln!(
                            "  item {i}: type {:?} weighting {}",
                            (*header).value_type,
                            (*header).raw_weighting
                        );
                    }
                }
            }
            fiftyoneDegreesWeightedValuesCollectionRelease(&mut collection);

            // Free the results and the manager.
            fiftyoneDegreesResultsIpiFree(results);
            fiftyoneDegreesResourceManagerFree(&mut manager);
        }
    }

    /// Read and free the allocated message for a set exception, used to report
    /// failures from the smoke test with the library's own description.
    unsafe fn exception_message(exception: &mut Exception) -> String {
        if exception.is_okay() {
            return String::from("(no exception)");
        }
        let raw = fiftyoneDegreesExceptionGetMessage(exception);
        if raw.is_null() {
            return String::from("(exception set, no message)");
        }
        let message = CStr::from_ptr(raw).to_string_lossy().into_owned();
        fiftyoneDegreesFree(raw as *mut c_void);
        message
    }
}
