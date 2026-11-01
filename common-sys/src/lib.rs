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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-common-sys-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-common-sys-lib.rs&utm_term=logo)
//!
//! Raw FFI bindings to the common C library shared by the on-premise
//! Device Detection and IP Intelligence engines.
//!
//! This crate is the native foundation that both products build on. It owns the
//! compilation and static linking of the common C sources (as the
//! `fiftyone-common-c` library) and exposes hand written `extern "C"`
//! declarations and `#[repr(C)]` structures for the portion of the public ABI
//! that is genuinely shared between the products.
//!
//! The bound surface is deliberately the common subset only:
//!
//! - **Status** ([`StatusCode`] and [`fiftyoneDegreesStatusGetMessage`]).
//! - **Exception** ([`Exception`], [`fiftyoneDegreesExceptionGetMessage`] and
//!   the okay/clear helpers).
//! - **Resource manager** ([`ResourceManager`] with init, free, handle
//!   increment, decrement and use-count helpers).
//! - **Evidence** ([`EvidenceKeyValuePairArray`], create, free, add and iterate,
//!   plus the [`EvidencePrefix`] enum and prefix mapping).
//! - **Properties** ([`PropertiesRequired`]).
//! - **Memory** ([`fiftyoneDegreesFree`]) so that strings allocated by the
//!   library can be released through the matching allocator.
//!
//! Internals such as collections, data sets and headers are exposed as opaque
//! pointer types. Only the structures that actually cross the boundary in this
//! shared surface are given a concrete `#[repr(C)]` layout. Engine specific
//! surfaces (the Hash device detection results and the IP Intelligence results)
//! live in the `device-detection-sys` and `ip-intelligence-sys` crates which
//! link this one.
//!
//! # Safety
//!
//! Every item in this crate is an unchecked binding to a C function or
//! structure. Callers are responsible for upholding the contracts described in
//! the upstream headers (`exceptions.h`, `status.h`, `resource.h`,
//! `evidence.h`, `properties.h` and `memory.h`). The names mirror the C names
//! exactly so the headers remain the authoritative reference.

#![warn(missing_docs)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use std::os::raw::{c_char, c_int, c_void};

// ---------------------------------------------------------------------------
// Status codes (status.h)
// ---------------------------------------------------------------------------

/// Status returned from the initialization of a resource.
///
/// Mirrors `fiftyoneDegreesStatusCode` from `status.h`. The discriminants are
/// the enum's natural ordinal values, which is the ABI the C library uses, so
/// the variants must stay in the same order as the header.
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
    /// The requested functionality is not implemented, usually due to a compile
    /// flag.
    NotImplemented,
}

// ---------------------------------------------------------------------------
// Exception (exceptions.h)
// ---------------------------------------------------------------------------

/// Structure used to represent a 51Degrees exception, passed into methods that
/// might generate exceptions.
///
/// Mirrors `fiftyoneDegreesException` from `exceptions.h`. The `file` and `func`
/// pointers reference static strings owned by the C library, so they are never
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

    /// Returns `true` when no exception is set, matching the `EXCEPTION_OKAY`
    /// macro (status equals [`StatusCode::NotSet`]).
    pub fn is_okay(&self) -> bool {
        self.status == StatusCode::NotSet
    }
}

// ---------------------------------------------------------------------------
// Resource manager (resource.h)
// ---------------------------------------------------------------------------

/// Opaque handle for a shared resource managed by a [`ResourceManager`].
///
/// The internal layout (a double width interlocked counter followed by resource
/// and manager pointers) is intentionally hidden. Callers only ever hold a
/// pointer to it, obtained from [`fiftyoneDegreesResourceHandleIncUse`].
#[repr(C)]
pub struct ResourceHandle {
    _private: [u8; 0],
}

/// Manager structure used to provide access to a shared and changing resource.
///
/// Mirrors `fiftyoneDegreesResourceManager` from `resource.h`, which is a single
/// (volatile) pointer to the active handle. The struct is allocated by the
/// caller and passed by pointer to [`fiftyoneDegreesResourceManagerInit`].
#[repr(C)]
pub struct ResourceManager {
    /// Current handle for the resource used by the manager.
    pub active: *mut ResourceHandle,
}

impl ResourceManager {
    /// Create a zeroed manager suitable for passing to
    /// [`fiftyoneDegreesResourceManagerInit`].
    pub fn zeroed() -> Self {
        ResourceManager {
            active: std::ptr::null_mut(),
        }
    }
}

/// Function pointer matching `freeResource` passed to
/// [`fiftyoneDegreesResourceManagerInit`].
pub type FreeResourceMethod = unsafe extern "C" fn(*mut c_void);

// ---------------------------------------------------------------------------
// Evidence (evidence.h, pair.h)
// ---------------------------------------------------------------------------

/// Evidence prefixes used to determine the category a piece of evidence belongs
/// to, which in turn determines how the value is parsed.
///
/// Mirrors `fiftyoneDegreesEvidencePrefix`. The values are bit flags so several
/// prefixes can be combined when iterating.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EvidencePrefix {
    /// An HTTP header value.
    HttpHeaderString = 1 << 0,
    /// A list of IP addresses as a string to be parsed.
    HttpHeaderIpAddresses = 1 << 1,
    /// A server value, for example client IP.
    Server = 1 << 2,
    /// A query string parameter.
    Query = 1 << 3,
    /// A cookie value.
    Cookie = 1 << 4,
    /// The evidence is invalid and should be ignored.
    Ignore = 1 << 7,
}

/// Map of a prefix string to its prefix enum value.
///
/// Mirrors `fiftyoneDegreesEvidencePrefixMap`. Returned by
/// [`fiftyoneDegreesEvidenceMapPrefix`].
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EvidencePrefixMap {
    /// Name of the prefix.
    pub prefix: *const c_char,
    /// Length of the prefix string.
    pub prefix_length: usize,
    /// Enum value of the prefix name.
    pub prefix_enum: EvidencePrefix,
}

/// A key value pair with the key and value lengths cached alongside the
/// pointers.
///
/// Mirrors `fiftyoneDegreesKeyValuePair` from `pair.h`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KeyValuePair {
    /// Pointer to the key string.
    pub key: *const c_char,
    /// Number of characters in the key.
    pub key_length: usize,
    /// Pointer to the value string.
    pub value: *const c_char,
    /// Number of characters in the value.
    pub value_length: usize,
}

/// Opaque header reference within a data set, referenced by an evidence pair.
///
/// Mirrors the forward declared `fiftyoneDegreesHeader` from `headers.h`.
#[repr(C)]
pub struct Header {
    _private: [u8; 0],
}

/// Evidence key value pair structure combining the prefix, key and value.
///
/// Mirrors `fiftyoneDegreesEvidenceKeyValuePair`. The `parsed_value` pointer is
/// the value after prefix specific parsing and may not be a string.
#[repr(C)]
pub struct EvidenceKeyValuePair {
    /// Category of the evidence, for example [`EvidencePrefix::HttpHeaderString`].
    pub prefix: EvidencePrefix,
    /// The original field key and value.
    pub item: KeyValuePair,
    /// Parsed value which may not be a string.
    pub parsed_value: *const c_void,
    /// Length of the parsed value.
    pub parsed_length: usize,
    /// Unique header in the data set, or null if not related to a header.
    pub header: *mut Header,
}

/// Array of evidence key value pairs.
///
/// Mirrors the `fiftyoneDegreesEvidenceKeyValuePairArray` produced by the
/// `FIFTYONE_DEGREES_ARRAY_TYPE` macro in `array.h`, including the linked list
/// `next`/`prev` members added for evidence. `items` points at the first of
/// `capacity` contiguous pairs.
#[repr(C)]
pub struct EvidenceKeyValuePairArray {
    /// Number of used items.
    pub count: u32,
    /// Number of available items.
    pub capacity: u32,
    /// Pointer to the first item.
    pub items: *mut EvidenceKeyValuePair,
    /// Next array in the chain, or null.
    pub next: *mut EvidenceKeyValuePairArray,
    /// Previous array in the chain, or null.
    pub prev: *mut EvidenceKeyValuePairArray,
}

/// Callback used to iterate evidence key value pairs.
///
/// Mirrors `fiftyoneDegreesEvidenceIterateMethod`. Return `true` to continue
/// iterating, `false` to stop.
pub type EvidenceIterateMethod =
    unsafe extern "C" fn(state: *mut c_void, pair: *mut EvidenceKeyValuePair) -> bool;

// ---------------------------------------------------------------------------
// Properties (properties.h)
// ---------------------------------------------------------------------------

/// Opaque set of properties that are available in a data set.
///
/// Mirrors `fiftyoneDegreesPropertiesAvailable`. It is produced and consumed by
/// the engine specific crates, so only an opaque pointer is exposed here.
#[repr(C)]
pub struct PropertiesAvailable {
    _private: [u8; 0],
}

/// Defines the set of properties required by a caller, typically passed to a
/// data set creation method.
///
/// Mirrors `fiftyoneDegreesPropertiesRequired`. Exactly one of `existing`,
/// `array` or `string` is used, evaluated in that order, with the first set
/// field winning. A fully zeroed value requests all properties.
#[repr(C)]
pub struct PropertiesRequired {
    /// Array of required property names, or null when all are required.
    pub array: *const *const c_char,
    /// Number of properties in `array`.
    pub count: c_int,
    /// Separated list of required property names, or null when all are required.
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

// ---------------------------------------------------------------------------
// extern "C" function declarations
// ---------------------------------------------------------------------------

extern "C" {
    // -- status.h --

    /// Returns a newly allocated English message for the status code. The caller
    /// must free the returned pointer with [`fiftyoneDegreesFree`].
    ///
    /// # Safety
    /// `file_name` must be a valid null terminated string or null.
    pub fn fiftyoneDegreesStatusGetMessage(
        status: StatusCode,
        file_name: *const c_char,
    ) -> *const c_char;

    // -- exceptions.h --

    /// Returns a newly allocated English message describing the exception. The
    /// caller must free the returned pointer with [`fiftyoneDegreesFree`].
    ///
    /// # Safety
    /// `exception` must point to a valid, set [`Exception`].
    pub fn fiftyoneDegreesExceptionGetMessage(exception: *mut Exception) -> *const c_char;

    /// If the exception is set, prints a message to stderr and exits the
    /// process. Provided for completeness, mirroring `EXCEPTION_THROW` in C.
    ///
    /// # Safety
    /// `exception` must point to a valid [`Exception`].
    pub fn fiftyoneDegreesExceptionCheckAndExit(exception: *mut Exception);

    // -- resource.h --

    /// Initialize a preallocated [`ResourceManager`] with a resource to manage.
    ///
    /// # Safety
    /// `manager` must point to a writable [`ResourceManager`]. `resource_handle`
    /// must point to the handle pointer located inside `resource`.
    pub fn fiftyoneDegreesResourceManagerInit(
        manager: *mut ResourceManager,
        resource: *mut c_void,
        resource_handle: *mut *mut ResourceHandle,
        free_resource: FreeResourceMethod,
    );

    /// Frees the manager and, once no handles remain in use, its resource.
    ///
    /// # Safety
    /// `manager` must point to a manager previously initialized with
    /// [`fiftyoneDegreesResourceManagerInit`].
    pub fn fiftyoneDegreesResourceManagerFree(manager: *mut ResourceManager);

    /// Increments the resource use count and returns a handle. The handle must
    /// be released with [`fiftyoneDegreesResourceHandleDecUse`].
    ///
    /// # Safety
    /// `manager` must point to an initialized manager.
    pub fn fiftyoneDegreesResourceHandleIncUse(
        manager: *mut ResourceManager,
    ) -> *mut ResourceHandle;

    /// Decrements the use count for a handle, allowing the resource to be freed
    /// once the count reaches zero.
    ///
    /// # Safety
    /// `handle` must be a handle obtained from
    /// [`fiftyoneDegreesResourceHandleIncUse`].
    pub fn fiftyoneDegreesResourceHandleDecUse(handle: *mut ResourceHandle);

    /// Returns the current use count. Not thread safe, intended for testing.
    ///
    /// # Safety
    /// `handle` must be a valid handle.
    pub fn fiftyoneDegreesResourceHandleGetUse(handle: *mut ResourceHandle) -> i32;

    /// Replaces the managed resource with a new one, freeing the old resource
    /// once it is no longer in use.
    ///
    /// # Safety
    /// `manager` must be initialized and `new_resource_handle` must point to the
    /// handle pointer inside `new_resource`.
    pub fn fiftyoneDegreesResourceReplace(
        manager: *mut ResourceManager,
        new_resource: *mut c_void,
        new_resource_handle: *mut *mut ResourceHandle,
    );

    // -- evidence.h --

    /// Creates a new evidence array with the requested capacity.
    ///
    /// # Safety
    /// The returned pointer must be released with
    /// [`fiftyoneDegreesEvidenceFree`].
    pub fn fiftyoneDegreesEvidenceCreate(capacity: u32) -> *mut EvidenceKeyValuePairArray;

    /// Frees an evidence array and any chained arrays. Does not free the
    /// referenced key and value strings.
    ///
    /// # Safety
    /// `evidence` must come from [`fiftyoneDegreesEvidenceCreate`].
    pub fn fiftyoneDegreesEvidenceFree(evidence: *mut EvidenceKeyValuePairArray);

    /// Adds a string entry to the evidence. The key and value memory must
    /// outlive the evidence array, as the values are not copied.
    ///
    /// # Safety
    /// `evidence` must be a valid array and `key`/`value` valid null terminated
    /// strings that outlive it.
    pub fn fiftyoneDegreesEvidenceAddString(
        evidence: *mut EvidenceKeyValuePairArray,
        prefix: EvidencePrefix,
        key: *const c_char,
        value: *const c_char,
    ) -> *mut EvidenceKeyValuePair;

    /// Determines the evidence prefix map entry from a key, or null if none
    /// exists.
    ///
    /// # Safety
    /// `key` must be a valid null terminated string.
    pub fn fiftyoneDegreesEvidenceMapPrefix(key: *const c_char) -> *mut EvidencePrefixMap;

    /// Returns the null terminated prefix string (including the dot separator)
    /// for an evidence prefix.
    pub fn fiftyoneDegreesEvidencePrefixString(prefix: EvidencePrefix) -> *const c_char;

    /// Iterates over evidence matching the prefix flags, calling `callback` for
    /// each match, and returns the number of matches iterated.
    ///
    /// # Safety
    /// `evidence` must be valid and `callback` must be sound for the supplied
    /// `state`.
    pub fn fiftyoneDegreesEvidenceIterate(
        evidence: *mut EvidenceKeyValuePairArray,
        prefixes: c_int,
        state: *mut c_void,
        callback: EvidenceIterateMethod,
    ) -> u32;

    // -- memory.h --

    /// Frees memory through the C runtime's `free`, which is how the library
    /// frees memory unless its allocator has been replaced. Release what the
    /// library allocates with [`fiftyoneDegreesFree`] instead, which follows a
    /// replaced allocator.
    ///
    /// # Safety
    /// `ptr` must be a pointer the C runtime's `malloc` allocated, or null.
    pub fn fiftyoneDegreesMemoryStandardFree(ptr: *mut c_void);

    /// The function the library frees memory with, the counterpart of the
    /// allocator it allocates with. Release the strings
    /// [`fiftyoneDegreesStatusGetMessage`] and
    /// [`fiftyoneDegreesExceptionGetMessage`] return with it. It is
    /// [`fiftyoneDegreesMemoryStandardFree`] unless the allocator has been
    /// replaced, as the library's memory tracking replaces it.
    ///
    /// # Safety
    /// Call it only with a pointer the library allocated, or null.
    pub static mut fiftyoneDegreesFree: unsafe extern "C" fn(ptr: *mut c_void);

    // -- threading.h --

    /// Returns `true` if the library was compiled thread safe.
    pub fn fiftyoneDegreesThreadingGetIsThreadSafe() -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{CStr, CString};

    /// The static library links and a trivial status round-trip succeeds. This
    /// proves the native build linked on the host toolchain.
    #[test]
    fn status_message_round_trip() {
        unsafe {
            let raw = fiftyoneDegreesStatusGetMessage(StatusCode::FileNotFound, std::ptr::null());
            assert!(!raw.is_null(), "status message should be allocated");
            let message = CStr::from_ptr(raw).to_string_lossy().into_owned();
            // The library produces a human readable message for the code.
            assert!(!message.is_empty(), "status message should not be empty");
            // Free through the matching allocator.
            fiftyoneDegreesFree(raw as *mut std::os::raw::c_void);
        }
    }

    /// A cleared exception reads as okay, and once a status is set the library
    /// produces a non-empty message which we free.
    #[test]
    fn exception_create_and_message() {
        let mut exception = Exception::cleared();
        assert!(exception.is_okay(), "fresh exception should be okay");

        exception.status = StatusCode::NullPointer;
        // Provide a source location so the formatted message has all fields.
        let file = CString::new("lib.rs").unwrap();
        let func = CString::new("exception_create_and_message").unwrap();
        exception.file = file.as_ptr();
        exception.func = func.as_ptr();
        exception.line = 42;
        assert!(!exception.is_okay(), "set exception should not be okay");

        unsafe {
            let raw = fiftyoneDegreesExceptionGetMessage(&mut exception);
            assert!(!raw.is_null(), "exception message should be allocated");
            let message = CStr::from_ptr(raw).to_string_lossy().into_owned();
            assert!(!message.is_empty(), "exception message should not be empty");
            fiftyoneDegreesFree(raw as *mut std::os::raw::c_void);
        }
    }

    /// A resource manager round-trip: initialize with a heap resource, take and
    /// release a handle, then free. This exercises the resource and threading
    /// parts of the C library through the FFI boundary.
    #[test]
    fn resource_manager_round_trip() {
        // A trivial resource that carries its own handle pointer, matching the
        // pattern in resource.h where the handle lives inside the resource.
        #[repr(C)]
        struct DemoResource {
            handle: *mut ResourceHandle,
            value: i32,
        }

        unsafe extern "C" fn free_demo(resource: *mut c_void) {
            // The resource was produced by Box::into_raw below.
            drop(Box::from_raw(resource as *mut DemoResource));
        }

        unsafe {
            let resource = Box::into_raw(Box::new(DemoResource {
                handle: std::ptr::null_mut(),
                value: 7,
            }));

            let mut manager = ResourceManager::zeroed();
            fiftyoneDegreesResourceManagerInit(
                &mut manager,
                resource as *mut c_void,
                &mut (*resource).handle,
                free_demo,
            );
            assert!(
                !(*resource).handle.is_null(),
                "init should assign a handle to the resource"
            );

            let handle = fiftyoneDegreesResourceHandleIncUse(&mut manager);
            assert!(!handle.is_null(), "incrementing use should return a handle");
            assert!(
                fiftyoneDegreesResourceHandleGetUse(handle) >= 1,
                "use count should be at least one while held"
            );

            fiftyoneDegreesResourceHandleDecUse(handle);

            // Freeing the manager runs free_demo once the last handle is gone.
            fiftyoneDegreesResourceManagerFree(&mut manager);
        }
    }

    /// Evidence create, add a string, iterate and free. Exercises the evidence
    /// surface end to end through the FFI boundary.
    #[test]
    fn evidence_add_and_iterate() {
        unsafe extern "C" fn count_cb(state: *mut c_void, pair: *mut EvidenceKeyValuePair) -> bool {
            let counter = &mut *(state as *mut u32);
            *counter += 1;
            // Touch the pair to confirm the layout is sound.
            assert!(!(*pair).item.key.is_null());
            true
        }

        unsafe {
            let evidence = fiftyoneDegreesEvidenceCreate(4);
            assert!(!evidence.is_null(), "evidence array should be allocated");

            // The strings must outlive the evidence array because they are not
            // copied. Holding the CStrings in scope guarantees that here.
            let key = CString::new("user-agent").unwrap();
            let value = CString::new("Example/1.0").unwrap();

            let pair = fiftyoneDegreesEvidenceAddString(
                evidence,
                EvidencePrefix::HttpHeaderString,
                key.as_ptr(),
                value.as_ptr(),
            );
            assert!(
                !pair.is_null(),
                "adding evidence should return the new pair"
            );
            assert_eq!((*evidence).count, 1, "one item should be recorded");

            let mut counter: u32 = 0;
            let iterated = fiftyoneDegreesEvidenceIterate(
                evidence,
                EvidencePrefix::HttpHeaderString as c_int,
                &mut counter as *mut u32 as *mut c_void,
                count_cb,
            );
            assert_eq!(iterated, 1, "exactly one matching item should be iterated");
            assert_eq!(counter, 1, "callback should have run once");

            fiftyoneDegreesEvidenceFree(evidence);
        }
    }

    /// The evidence prefix mapping resolves a known prefix string to its enum.
    #[test]
    fn evidence_map_prefix() {
        unsafe {
            let key = CString::new("header.user-agent").unwrap();
            let map = fiftyoneDegreesEvidenceMapPrefix(key.as_ptr());
            assert!(!map.is_null(), "header prefix should map to an entry");
            assert_eq!(
                (*map).prefix_enum,
                EvidencePrefix::HttpHeaderString,
                "header maps to the HTTP header string prefix"
            );
        }
    }
}
