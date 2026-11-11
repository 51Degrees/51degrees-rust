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

/// The required property index of each name that has one in a data set,
/// sorted ignoring case as the C library compares names, so that a name is
/// found as `fiftyoneDegreesPropertiesGetRequiredPropertyIndexFromName` finds
/// it, with no lock and no write to shared memory.
#[derive(PartialEq, Eq)]
pub(crate) struct NameIndexes(Vec<(Box<str>, c_int)>);

impl NameIndexes {
    /// The table for `found`, each name with the index the C library gives
    /// it, leaving out a name with none.
    pub(crate) fn new(found: impl IntoIterator<Item = (String, c_int)>) -> Self {
        let mut names: Vec<(Box<str>, c_int)> = found
            .into_iter()
            .filter(|(_, index)| *index >= 0)
            .map(|(name, index)| (name.into_boxed_str(), index))
            .collect();
        names.sort_by(|(a, _), (b, _)| compare_ignoring_case(a, b));
        NameIndexes(names)
    }

    /// The index of the property named, ignoring case, or [`None`] when it has
    /// none or the name holds a nul byte, which ends a C string.
    pub(crate) fn find(&self, name: &str) -> Option<c_int> {
        if name.contains('\0') {
            return None;
        }
        self.0
            .binary_search_by(|(key, _)| compare_ignoring_case(key, name))
            .ok()
            .map(|at| self.0[at].1)
    }
}

/// `a` against `b`, ignoring the case of ASCII letters, as the C library's
/// `fiftyoneDegreesStringCompare` compares them.
fn compare_ignoring_case(a: &str, b: &str) -> std::cmp::Ordering {
    a.bytes()
        .map(|byte| byte.to_ascii_lowercase())
        .cmp(b.bytes().map(|byte| byte.to_ascii_lowercase()))
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
