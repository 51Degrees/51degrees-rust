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

//! The data a data set loaded from memory reads in place.
//!
//! The C library keeps a pointer to the data for as long as the data set
//! lives. The data is held as the pointer `Box::into_raw` gives rather than
//! as a `Box`, because Rust treats a `Box` as the unique owner of its memory,
//! and whether moving one ends the pointers taken from it earlier is not
//! settled in Rust's aliasing rules
//! (<https://github.com/rust-lang/unsafe-code-guidelines/issues/326>). The
//! `Box` is rebuilt only to free the data.

use std::os::raw::c_void;
use std::ptr::NonNull;

/// The bytes of a data file that a data set reads in place, freed when this
/// is dropped. Its owner drops it only after freeing the data set.
pub(crate) struct HeldMemory {
    data: NonNull<[u8]>,
}

impl HeldMemory {
    /// Take `data`, keeping its bytes where they are until this is dropped.
    pub(crate) fn new(data: Vec<u8>) -> HeldMemory {
        let data = NonNull::new(Box::into_raw(data.into_boxed_slice()))
            .expect("Box::into_raw gives a pointer that is not null");
        HeldMemory { data }
    }

    /// The first byte, as the C library takes it.
    pub(crate) fn start(&self) -> *mut c_void {
        self.data.cast::<c_void>().as_ptr()
    }

    /// The number of bytes.
    pub(crate) fn length(&self) -> usize {
        self.data.len()
    }
}

impl Drop for HeldMemory {
    fn drop(&mut self) {
        // Safety: `data` came from `Box::into_raw` in `new` and is freed only
        // here, after the data set reading it has been freed.
        unsafe { drop(Box::from_raw(self.data.as_ptr())) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bytes held are the data given, where the start points, and data
    /// with no bytes is held too.
    #[test]
    fn the_bytes_held_are_the_data_given() {
        let held = HeldMemory::new(vec![5, 1, 68]);
        assert_eq!(held.length(), 3);
        // Safety: `start` is the first of `length` bytes that `held` keeps.
        let bytes = unsafe { std::slice::from_raw_parts(held.start().cast::<u8>(), held.length()) };
        assert_eq!(bytes, [5, 1, 68]);

        let empty = HeldMemory::new(Vec::new());
        assert_eq!(empty.length(), 0);
        assert!(!empty.start().is_null());
    }
}
