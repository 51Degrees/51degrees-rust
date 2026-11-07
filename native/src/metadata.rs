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

//! What Device Detection and IP Intelligence share in reading a data file's
//! metadata through their shims: the value type, and the readers for the
//! strings each shim gives.

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};

use fiftyone_pipeline_core::Error;

/// What a shim's metadata string reader returns when it cannot read the
/// string, `(size_t)-1` in both shims.
pub(crate) const STRING_FAILED: usize = usize::MAX;

/// The text of a value, the first string a shim's value reader gives.
pub(crate) const VALUE_NAME: c_int = 0;
/// The description of a value, given after its text.
pub(crate) const VALUE_DESCRIPTION: c_int = 1;
/// The URL of a value, given after its description.
pub(crate) const VALUE_URL: c_int = 2;

/// The length of the buffer a metadata string is first read into. A longer
/// string is read again into a buffer that fits it.
pub(crate) const METADATA_STRING_BUFFER_LENGTH: usize = 256;

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

/// Take one string of a property value from a shim's value reader into the
/// `Vec<DataFileValue>` that `context` points to. The shim gives each value's
/// text first, which starts a new value, then its description and URL.
///
/// # Safety
/// `context` must point to a `Vec<DataFileValue>` that nothing else uses
/// during the call, and `value` must be a null terminated string.
pub(crate) unsafe extern "C" fn take_value_string(
    context: *mut c_void,
    field: c_int,
    value: *const c_char,
) {
    // Safety: the caller's contract above.
    let (values, text) = unsafe {
        (
            &mut *context.cast::<Vec<DataFileValue>>(),
            CStr::from_ptr(value).to_string_lossy().into_owned(),
        )
    };
    match field {
        VALUE_NAME => values.push(DataFileValue {
            name: text,
            description: String::new(),
            url: String::new(),
        }),
        VALUE_DESCRIPTION => {
            if let Some(last) = values.last_mut() {
                last.description = text;
            }
        }
        VALUE_URL => {
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
pub(crate) fn read_metadata_string(
    mut read: impl FnMut(*mut c_char, usize) -> usize,
) -> Option<String> {
    let mut buffer = vec![0u8; METADATA_STRING_BUFFER_LENGTH];
    let mut length = read(buffer.as_mut_ptr() as *mut c_char, buffer.len());
    if length == STRING_FAILED {
        return None;
    }
    if length >= buffer.len() {
        buffer = vec![0u8; length + 1];
        length = read(buffer.as_mut_ptr() as *mut c_char, buffer.len());
        if length == STRING_FAILED {
            return None;
        }
    }
    let end = length.min(buffer.len() - 1);
    Some(String::from_utf8_lossy(&buffer[..end]).into_owned())
}

/// The error for a part of the data file's metadata that could not be read.
pub(crate) fn metadata_read_failed(what: &str) -> Error {
    Error::Native {
        status: String::from("CollectionFailure"),
        message: format!("the data file's metadata could not be read ({what})"),
    }
}
