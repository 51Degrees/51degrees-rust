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

//! What a manager reads from the data set in use and lends by reference,
//! replaced after a reload.

use std::os::raw::c_int;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

/// Each value a manager has used, kept until the manager is dropped so that a
/// reference lent from one stays valid through a reload, with the newest in
/// use. Reading the value in use takes no lock and writes no shared memory.
pub(crate) struct Kept<T> {
    /// The value in use, being the last of `all`.
    current: AtomicPtr<T>,
    /// Every value used. A reload adds one only when its value differs from
    /// the one in use, so a reload of an identical data file adds none.
    all: Mutex<Vec<Arc<T>>>,
}

impl<T> Kept<T> {
    /// `value`, in use.
    pub(crate) fn new(value: T) -> Self {
        let kept = Kept {
            current: AtomicPtr::new(std::ptr::null_mut()),
            all: Mutex::new(Vec::new()),
        };
        kept.replace(value, |_, _| false);
        kept
    }

    /// The value in use.
    pub(crate) fn current(&self) -> &T {
        // Safety: `current` points to a value that `all` holds from before the
        // pointer is stored until `self` is dropped, so the value outlives the
        // borrow of `self`.
        unsafe { &*self.current.load(Ordering::Acquire) }
    }

    /// Use `value`, unless `same` finds it the same as the value in use.
    pub(crate) fn replace(&self, value: T, same: impl FnOnce(&T, &T) -> bool) {
        let mut all = self.all.lock().unwrap_or_else(PoisonError::into_inner);
        if all.last().is_some_and(|last| same(last, &value)) {
            return;
        }
        let value = Arc::new(value);
        all.push(Arc::clone(&value));
        self.current
            .store(Arc::as_ptr(&value).cast_mut(), Ordering::Release);
    }

    /// The number of values used.
    #[cfg(test)]
    pub(crate) fn count(&self) -> usize {
        self.all
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

/// A slot of [`NameIndexes`] that holds no name.
const EMPTY: u32 = u32::MAX;

/// The required property index of each name that has one in a data set,
/// found ignoring the case of ASCII letters, as the C library's
/// `fiftyoneDegreesStringCompare` compares names, so that a name is found as
/// `fiftyoneDegreesPropertiesGetRequiredPropertyIndexFromName` finds it, with
/// no lock and no write to shared memory.
///
/// The names are in a hash table with open addressing, being a power of two
/// slots, at least twice as many as the names, each the place of a name in
/// `names` or [`EMPTY`].
#[derive(PartialEq, Eq)]
pub(crate) struct NameIndexes {
    names: Vec<(Box<str>, c_int)>,
    slots: Box<[u32]>,
}

impl NameIndexes {
    /// The table for `found`, each name with the index the C library gives
    /// it, leaving out a name with none and a name given again in any case.
    pub(crate) fn new(found: impl IntoIterator<Item = (String, c_int)>) -> Self {
        let found: Vec<(String, c_int)> =
            found.into_iter().filter(|(_, index)| *index >= 0).collect();
        let mut slots = vec![EMPTY; (found.len() * 2).next_power_of_two()].into_boxed_slice();
        let mut names: Vec<(Box<str>, c_int)> = Vec::with_capacity(found.len());
        for (name, index) in found {
            let mask = slots.len() - 1;
            let mut slot = hash_ignoring_case(&name) & mask;
            loop {
                let place = slots[slot];
                if place == EMPTY {
                    slots[slot] = names.len() as u32;
                    names.push((name.into_boxed_str(), index));
                    break;
                }
                if names[place as usize].0.eq_ignore_ascii_case(&name) {
                    break;
                }
                slot = (slot + 1) & mask;
            }
        }
        NameIndexes { names, slots }
    }

    /// The index of the property named, ignoring case, or [`None`] when it has
    /// none. A name holding a nul byte, which ends a C string, is never one of
    /// the names, so it has none.
    pub(crate) fn find(&self, name: &str) -> Option<c_int> {
        let mask = self.slots.len() - 1;
        let mut slot = hash_ignoring_case(name) & mask;
        loop {
            let place = self.slots[slot];
            if place == EMPTY {
                return None;
            }
            let (key, index) = &self.names[place as usize];
            if key.eq_ignore_ascii_case(name) {
                return Some(*index);
            }
            slot = (slot + 1) & mask;
        }
    }
}

/// The FNV-1a hash of `name` with its ASCII letters in lower case, so two
/// names that differ only in that case hash the same.
fn hash_ignoring_case(name: &str) -> usize {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name.bytes() {
        hash ^= u64::from(byte.to_ascii_lowercase());
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A name is found ignoring case, a name with no index or a nul byte is
    /// not, and two names that differ only in case give the first's index,
    /// as the C library gives it.
    #[test]
    fn a_name_is_found_as_the_c_library_finds_it() {
        let names = NameIndexes::new([
            (String::from("IsMobile"), 0),
            (String::from("BrowserName"), 1),
            (String::from("Missing"), -1),
            (String::from("ismobile"), 0),
        ]);
        assert_eq!(names.find("ISMOBILE"), Some(0));
        assert_eq!(names.find("browsername"), Some(1));
        assert_eq!(names.find("Missing"), None);
        assert_eq!(names.find("IsMobile\0"), None);
        assert_eq!(names.find("NoSuchProperty"), None);
        assert_eq!(names.find(""), None);
        assert_eq!(NameIndexes::new([]).find("IsMobile"), None);
    }

    /// A value lent before a replace stays as it was, a replace by the same
    /// value keeps the one in use, and another value is then in use.
    #[test]
    fn a_value_lent_stays_valid_through_a_replace() {
        let kept = Kept::new(vec![1, 2]);
        let lent = kept.current();
        kept.replace(vec![1, 2], |a, b| a == b);
        assert_eq!(kept.count(), 1);
        kept.replace(vec![3], |a, b| a == b);
        assert_eq!(kept.count(), 2);
        assert_eq!(lent, &[1, 2]);
        assert_eq!(kept.current(), &[3]);
    }
}
