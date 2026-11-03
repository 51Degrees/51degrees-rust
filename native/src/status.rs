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

//! Mapping native status codes and exceptions onto the pipeline error model.
//!
//! Every native call either succeeds or reports a failure in one of two ways. A
//! `StatusCode` other than `Success` is returned from an initialization call, or
//! an `Exception` structure is set by a processing call. Both are surfaced to
//! the safe layer as [`fiftyone_pipeline_core::Error::Native`], carrying the
//! status name and a human readable message taken from the native library where
//! one is available.
//!
//! Device Detection and IP Intelligence compile separate copies of the shared
//! common C layer (the IP Intelligence build widens the file offset types,
//! see the `fiftyone-ip-intelligence-sys` crate docs). That gives each engine
//! its own distinct, although structurally identical, `StatusCode` and
//! `Exception` types and its own message functions. The [`NativeStatus`] and
//! [`NativeException`] traits let one set of mapping helpers work against either
//! engine's types, with each engine implementing the trait against its own
//! library functions in [`crate::dd`] and [`crate::ipi`].

use std::ffi::CStr;
use std::os::raw::{c_char, c_void};

use fiftyone_pipeline_core::Error;

/// A native status code that can name itself for error reporting.
///
/// Implemented for each engine's `StatusCode` enum. The blanket helpers below
/// use it to decide whether a call succeeded and, when it did not, to produce a
/// stable status name for [`Error::Native`].
pub trait NativeStatus: Copy {
    /// True when the status represents success (the native `SUCCESS` code).
    fn is_success(&self) -> bool;

    /// A stable, human readable name for the status, for example
    /// `"IncorrectVersion"`. Used as the `status` field of [`Error::Native`].
    fn name(&self) -> &'static str;
}

/// A native exception structure that can report whether it is set and render a
/// message.
///
/// Implemented for each engine's `Exception`. The processing wrappers create a
/// cleared exception, hand a pointer to it across the boundary, then call
/// [`NativeException::check`] to convert a set exception into an
/// [`Error::Native`].
pub trait NativeException: Sized {
    /// Create a cleared exception, equivalent to the native `EXCEPTION_CREATE`
    /// and `EXCEPTION_CLEAR` macros.
    fn cleared() -> Self;

    /// True when no exception is set (the native `EXCEPTION_OKAY` macro).
    fn is_okay(&self) -> bool;

    /// Return a freshly allocated, null terminated native message for the set
    /// exception. The pointer must be released with
    /// [`NativeException::free_message`]. The caller only invokes this when the
    /// exception is set.
    ///
    /// # Safety
    /// `self` must point to a set exception.
    unsafe fn message_ptr(&mut self) -> *const std::os::raw::c_char;

    /// Free a message this engine's library allocated, through the engine's own
    /// allocator.
    ///
    /// # Safety
    /// `ptr` must be a message this engine's library allocated, or be null.
    unsafe fn free_message(ptr: *mut c_void);

    /// Read the set exception's message as an owned [`String`], returning
    /// `fallback` when the native library has no message text for it.
    ///
    /// The native message is copied into an owned [`String`] and the native
    /// allocation is released before returning, so no native memory leaks across
    /// the boundary. The caller is responsible for only treating the result as a
    /// real message when the exception is actually set.
    fn message_or(&mut self, fallback: &str) -> String {
        // Safety: when set, the exception has a message, which the engine's
        // library allocated. When no message is available the pointer is null.
        unsafe { take_message::<Self>(self.message_ptr(), fallback) }
    }

    /// If the exception is set, read its message and return it as an
    /// [`Error::Native`]. Returns `Ok(())` when the exception is okay.
    ///
    /// The native message is copied into an owned [`String`] and the native
    /// allocation is released before returning, so no native memory leaks across
    /// the boundary.
    fn check(&mut self) -> Result<(), Error> {
        if self.is_okay() {
            return Ok(());
        }
        let message = self.message_or("native engine reported an exception with no message");
        Err(Error::Native {
            status: String::from("Exception"),
            message,
        })
    }
}

/// Generate the body of an engine's `status_name` function.
///
/// Device Detection and IP Intelligence compile separate, structurally
/// identical `StatusCode` enums (see the module docs). Their name mappings would
/// otherwise be character-identical 41-arm matches, so this macro keeps the
/// single authoritative variant list here and expands it against whichever
/// engine's `StatusCode` path is supplied. Each module invokes it as
/// `status_name!(path::to::StatusCode, status_expr)`.
macro_rules! status_name {
    ($code:path, $status:expr) => {{
        use $code as StatusCode;
        match $status {
            StatusCode::Success => "Success",
            StatusCode::InsufficientMemory => "InsufficientMemory",
            StatusCode::CorruptData => "CorruptData",
            StatusCode::IncorrectVersion => "IncorrectVersion",
            StatusCode::FileNotFound => "FileNotFound",
            StatusCode::FileBusy => "FileBusy",
            StatusCode::FileFailure => "FileFailure",
            StatusCode::NotSet => "NotSet",
            StatusCode::PointerOutOfBounds => "PointerOutOfBounds",
            StatusCode::NullPointer => "NullPointer",
            StatusCode::TooManyOpenFiles => "TooManyOpenFiles",
            StatusCode::ReqPropNotPresent => "ReqPropNotPresent",
            StatusCode::ProfileEmpty => "ProfileEmpty",
            StatusCode::CollectionFailure => "CollectionFailure",
            StatusCode::FileCopyError => "FileCopyError",
            StatusCode::FileExistsError => "FileExistsError",
            StatusCode::FileWriteError => "FileWriteError",
            StatusCode::FileReadError => "FileReadError",
            StatusCode::FilePermissionDenied => "FilePermissionDenied",
            StatusCode::FilePathTooLong => "FilePathTooLong",
            StatusCode::FileEndOfDocument => "FileEndOfDocument",
            StatusCode::FileEndOfDocuments => "FileEndOfDocuments",
            StatusCode::FileEndOfFile => "FileEndOfFile",
            StatusCode::EncodingError => "EncodingError",
            StatusCode::InvalidCollectionConfig => "InvalidCollectionConfig",
            StatusCode::InvalidConfig => "InvalidConfig",
            StatusCode::InsufficientHandles => "InsufficientHandles",
            StatusCode::CollectionIndexOutOfRange => "CollectionIndexOutOfRange",
            StatusCode::CollectionOffsetOutOfRange => "CollectionOffsetOutOfRange",
            StatusCode::CollectionFileSeekFail => "CollectionFileSeekFail",
            StatusCode::CollectionFileReadFail => "CollectionFileReadFail",
            StatusCode::IncorrectIpAddressFormat => "IncorrectIpAddressFormat",
            StatusCode::TempFileError => "TempFileError",
            StatusCode::InsufficientCapacity => "InsufficientCapacity",
            StatusCode::InvalidInput => "InvalidInput",
            StatusCode::UnsupportedStoredValueType => "UnsupportedStoredValueType",
            StatusCode::FileTooLarge => "FileTooLarge",
            StatusCode::UnknownGeometry => "UnknownGeometry",
            StatusCode::ReservedGeometry => "ReservedGeometry",
            StatusCode::NotImplemented => "NotImplemented",
        }
    }};
}

// Make the macro available to the engine modules in this crate without forcing
// a `#[macro_export]` (it is an internal mapping helper, not public surface).
pub(crate) use status_name;

/// Copy a message the native library allocated into an owned [`String`] and
/// release the native allocation, returning `fallback` when `raw` is null.
///
/// # Safety
/// `raw` must be null, or a null terminated message the library of engine `E`
/// allocated. This call takes ownership of it.
pub(crate) unsafe fn take_message<E: NativeException>(
    raw: *const c_char,
    fallback: &str,
) -> String {
    if raw.is_null() {
        return String::from(fallback);
    }
    // Safety: the caller promises a null terminated message from this engine.
    unsafe {
        let owned = CStr::from_ptr(raw).to_string_lossy().into_owned();
        E::free_message(raw as *mut c_void);
        owned
    }
}

/// Convert a native initialization status into a [`Result`].
///
/// A success status yields `Ok(())`. Any other status yields an
/// [`Error::Native`] whose `status` is the code name and whose `message` is the
/// supplied detail, which each engine builds from the status so that it can name
/// the data file.
///
/// Only an enabled product's manager init calls this, so it is compiled when at
/// least one product feature is on. The [`NativeStatus`] and [`NativeException`]
/// traits above stay available unconditionally because they are part of the
/// crate's public surface.
#[cfg(any(feature = "dd", feature = "ipi"))]
pub(crate) fn status_to_result<S: NativeStatus>(
    status: S,
    detail: impl FnOnce() -> String,
) -> Result<(), Error> {
    if status.is_success() {
        Ok(())
    } else {
        Err(Error::Native {
            status: String::from(status.name()),
            message: detail(),
        })
    }
}
