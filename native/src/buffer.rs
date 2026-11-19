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

//! A reusable, growable byte buffer used to read native string values.
//!
//! The native value getters write into a caller supplied buffer and return the
//! number of characters that were available. When that exceeds the buffer size
//! the buffer was too small and the call must be retried with a larger buffer.
//! Allocating a fresh buffer for every property of every request would dominate
//! the cost of a fast on-premise lookup, so this module keeps one buffer per
//! thread and grows it only when a value does not fit.
//!
//! The buffer is keyed by thread and never shared, matching the per-thread,
//! single `FlowData` processing model the pipeline uses, so no locking is
//! needed on the hot path.

use std::cell::RefCell;
use std::os::raw::c_char;

/// The longest name, in bytes, that [`with_c_str`] copies to the stack. A
/// longer one is copied to the heap.
const STACK_NAME_LENGTH: usize = 128;

/// Call `f` with `text` as a null terminated string, copied to the stack when
/// it is short, so a lookup by name allocates nothing. Returns [`None`] when
/// `text` holds a nul byte, as no native name can.
pub(crate) fn with_c_str<R>(text: &str, f: impl FnOnce(*const c_char) -> R) -> Option<R> {
    let bytes = text.as_bytes();
    if bytes.contains(&0) {
        return None;
    }
    if bytes.len() < STACK_NAME_LENGTH {
        let mut name = [0u8; STACK_NAME_LENGTH];
        name[..bytes.len()].copy_from_slice(bytes);
        Some(f(name.as_ptr() as *const c_char))
    } else {
        let name = std::ffi::CString::new(bytes).ok()?;
        Some(f(name.as_ptr()))
    }
}

/// The starting capacity of the value buffer. Most property values are short, so
/// this is enough to satisfy the common case in a single native call.
const INITIAL_CAPACITY: usize = 256;

/// The largest buffer this helper will grow to. A native getter that keeps
/// reporting a larger required size than this is treated as misbehaving and the
/// read is abandoned rather than allowed to allocate without bound.
const MAX_CAPACITY: usize = 16 * 1024 * 1024;

thread_local! {
    /// The per-thread value buffer, grown on demand and reused across calls.
    static VALUE_BUFFER: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(INITIAL_CAPACITY));
}

/// Read a string value into the thread-local buffer using a native getter.
///
/// `writer` is invoked with a writable pointer and the buffer length in bytes.
/// It must write up to that many bytes and return the number of characters that
/// were available (the convention every native `GetValuesString` function uses).
/// When the returned count exceeds the buffer length the buffer is grown to fit
/// and `writer` is invoked again, repeating until the value fits or the maximum
/// capacity is reached.
///
/// On success the decoded value is handed to `consume`, which borrows it for the
/// duration of the call. The value is not allocated as an owned [`String`], so a
/// caller that only needs to inspect or copy part of it pays no extra
/// allocation. Returns [`None`] when the writer reports a zero length value or
/// when the value will not fit within [`MAX_CAPACITY`].
///
/// # Safety
/// `writer` must only write within the `length` bytes it is given and must
/// return the available character count using the native convention.
pub(crate) unsafe fn with_value_string<R>(
    mut writer: impl FnMut(*mut c_char, usize) -> usize,
    consume: impl FnOnce(&str) -> R,
) -> Option<R> {
    VALUE_BUFFER.with(|cell| {
        let mut buffer = cell.borrow_mut();
        if buffer.capacity() < INITIAL_CAPACITY {
            buffer.reserve(INITIAL_CAPACITY);
        }

        loop {
            let capacity = buffer.capacity().max(INITIAL_CAPACITY);
            // Present the full capacity to the native writer as writable space.
            buffer.resize(capacity, 0);

            let available = writer(buffer.as_mut_ptr() as *mut c_char, buffer.len());

            if available == 0 {
                // No value was written for this property.
                return None;
            }

            if available <= buffer.len() {
                // The value fit. Find the written extent. The native getters
                // null terminate, so trim at the first nul within the reported
                // length, falling back to the reported length when none is
                // present.
                let written = &buffer[..available];
                let end = written.iter().position(|&b| b == 0).unwrap_or(available);
                let text = String::from_utf8_lossy(&buffer[..end]);
                return Some(consume(text.as_ref()));
            }

            // The value did not fit. Grow to the reported size (plus one for a
            // terminator) and retry, unless that would exceed the cap.
            let needed = available.saturating_add(1);
            if needed > MAX_CAPACITY {
                return None;
            }
            let additional = needed.saturating_sub(buffer.len());
            buffer.reserve(additional);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    /// A name reaches C whole and null terminated, whether it is copied to
    /// the stack or, when it is too long for that, to the heap.
    #[test]
    fn a_name_reaches_c_whole_at_every_length() {
        let names = [
            String::new(),
            String::from("IsMobile"),
            "a".repeat(STACK_NAME_LENGTH - 1),
            "b".repeat(STACK_NAME_LENGTH),
            "c".repeat(STACK_NAME_LENGTH * 8),
        ];
        for name in &names {
            // Safety: `with_c_str` passes a null terminated string.
            let read = with_c_str(name, |c_name| {
                unsafe { CStr::from_ptr(c_name) }
                    .to_str()
                    .map(str::to_owned)
            });
            assert_eq!(read, Some(Ok(name.clone())), "{} bytes", name.len());
        }
    }

    /// A name holding a nul byte is refused, as C would read only the part
    /// before it.
    #[test]
    fn a_name_holding_a_nul_byte_is_refused() {
        assert_eq!(with_c_str("Is\0Mobile", |_| ()), None);
        assert_eq!(with_c_str(&"d\0".repeat(STACK_NAME_LENGTH), |_| ()), None);
    }
}
