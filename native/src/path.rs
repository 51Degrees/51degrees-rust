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

//! A data file path in the form the native libraries open it.
//!
//! The native libraries open a data file with the C runtime's narrow file
//! functions. On Windows those read the name in the system's ANSI code page
//! rather than UTF-8, so a path holding a character outside ASCII is passed
//! in that code page, or by its short name when the code page cannot hold it.
//! Elsewhere the path's bytes are passed as they are.

use std::ffi::CString;
use std::path::Path;

use fiftyone_pipeline_core::Error;

/// The path as the native libraries' file functions read it.
pub(crate) fn native_path(path: &Path) -> Result<CString, Error> {
    let bytes = encode(path).ok_or_else(|| Error::Native {
        status: String::from("InvalidInput"),
        message: format!(
            "the data file path cannot be given to the native library: {}",
            path.display()
        ),
    })?;
    CString::new(bytes).map_err(|_| Error::Native {
        status: String::from("InvalidInput"),
        message: String::from("data file path contains an interior nul byte"),
    })
}

#[cfg(unix)]
fn encode(path: &Path) -> Option<Vec<u8>> {
    use std::os::unix::ffi::OsStrExt;
    Some(path.as_os_str().as_bytes().to_vec())
}

#[cfg(not(any(unix, windows)))]
fn encode(path: &Path) -> Option<Vec<u8>> {
    path.to_str().map(|text| text.as_bytes().to_vec())
}

#[cfg(windows)]
fn encode(path: &Path) -> Option<Vec<u8>> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    windows::to_ansi(&wide).or_else(|| windows::to_ansi(&windows::short_name(&wide)?))
}

#[cfg(windows)]
mod windows {
    use std::os::raw::{c_char, c_int};
    use std::ptr;

    const CP_ACP: u32 = 0;
    const CP_UTF8: u32 = 65001;
    const WC_NO_BEST_FIT_CHARS: u32 = 0x400;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetACP() -> u32;
        fn WideCharToMultiByte(
            code_page: u32,
            flags: u32,
            wide: *const u16,
            wide_length: c_int,
            multi_byte: *mut c_char,
            multi_byte_length: c_int,
            default_char: *const c_char,
            used_default_char: *mut c_int,
        ) -> c_int;
        fn GetShortPathNameW(long_path: *const u16, short_path: *mut u16, length: u32) -> u32;
    }

    /// `wide` in the ANSI code page, or [`None`] when a character has no
    /// exact equivalent in it.
    pub(super) fn to_ansi(wide: &[u16]) -> Option<Vec<u8>> {
        if wide.is_empty() {
            return Some(Vec::new());
        }
        let length = c_int::try_from(wide.len()).ok()?;
        // Safety: reads only the constant code page.
        let utf8 = unsafe { GetACP() } == CP_UTF8;
        // A UTF-8 code page takes neither the flag nor the default character
        // test, and holds every character.
        let flags = if utf8 { 0 } else { WC_NO_BEST_FIT_CHARS };
        let mut used_default: c_int = 0;
        let used_default_ptr = if utf8 {
            ptr::null_mut()
        } else {
            &mut used_default as *mut c_int
        };
        // Safety: `wide` holds `length` units, and the first call writes
        // nothing because it is given no output buffer.
        let needed = unsafe {
            WideCharToMultiByte(
                CP_ACP,
                flags,
                wide.as_ptr(),
                length,
                ptr::null_mut(),
                0,
                ptr::null(),
                used_default_ptr,
            )
        };
        if needed <= 0 || used_default != 0 {
            return None;
        }
        let mut bytes = vec![0u8; needed as usize];
        // Safety: `bytes` has room for the `needed` bytes the first call
        // reported.
        let written = unsafe {
            WideCharToMultiByte(
                CP_ACP,
                flags,
                wide.as_ptr(),
                length,
                bytes.as_mut_ptr() as *mut c_char,
                needed,
                ptr::null(),
                used_default_ptr,
            )
        };
        (written == needed && used_default == 0).then_some(bytes)
    }

    /// The short name of the existing path `wide`, or [`None`] when it has
    /// none.
    pub(super) fn short_name(wide: &[u16]) -> Option<Vec<u16>> {
        let mut long: Vec<u16> = wide.to_vec();
        long.push(0);
        // Safety: `long` is null terminated, and a zero length output takes
        // no write.
        let needed = unsafe { GetShortPathNameW(long.as_ptr(), ptr::null_mut(), 0) };
        if needed == 0 {
            return None;
        }
        let mut short = vec![0u16; needed as usize];
        // Safety: `short` has room for the `needed` units reported, the
        // terminator included.
        let written = unsafe { GetShortPathNameW(long.as_ptr(), short.as_mut_ptr(), needed) };
        if written == 0 || written >= needed {
            return None;
        }
        short.truncate(written as usize);
        Some(short)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ascii_path_is_passed_as_it_is() {
        let path = Path::new("data/51Degrees-LiteV4.1.hash");
        assert_eq!(
            native_path(path)
                .expect("an ASCII path converts")
                .as_bytes(),
            b"data/51Degrees-LiteV4.1.hash"
        );
    }

    #[test]
    fn a_nul_byte_is_refused() {
        assert!(native_path(Path::new("data\0.hash")).is_err());
    }
}
