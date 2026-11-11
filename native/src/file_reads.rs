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

//! Keeps the calls that can read a data file within the handles its pool
//! holds.
//!
//! A data set that reads from its file after it is loaded holds a fixed pool
//! of file handles, one for each concurrent operation it was opened to
//! expect, to at most 64. A read that finds every handle in use waits in the
//! native library until another read releases one. A permit taken before each
//! such call makes a caller beyond the size of the pool wait here instead,
//! before it reaches the native library.

use std::sync::{Condvar, Mutex, PoisonError};

/// The permits for the calls that can read a data file, one for each handle
/// in its pool.
pub(crate) struct FileReads {
    available: Mutex<usize>,
    returned: Condvar,
}

impl FileReads {
    /// Permits for a pool of `handles` file handles, or [`None`] when the data
    /// set reads nothing from its file once it is loaded.
    pub(crate) fn for_handles(handles: u16) -> Option<FileReads> {
        (handles > 0).then(|| FileReads {
            available: Mutex::new(usize::from(handles)),
            returned: Condvar::new(),
        })
    }

    /// Wait for a permit. It is returned when the permit is dropped.
    pub(crate) fn permit(&self) -> FileReadPermit<'_> {
        let mut available = self
            .available
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        while *available == 0 {
            available = self
                .returned
                .wait(available)
                .unwrap_or_else(PoisonError::into_inner);
        }
        *available -= 1;
        FileReadPermit(self)
    }
}

/// A permit to make one call that can read a data file.
pub(crate) struct FileReadPermit<'a>(&'a FileReads);

impl Drop for FileReadPermit<'_> {
    fn drop(&mut self) {
        *self
            .0
            .available
            .lock()
            .unwrap_or_else(PoisonError::into_inner) += 1;
        self.0.returned.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn a_data_set_held_in_memory_needs_no_permits() {
        assert!(FileReads::for_handles(0).is_none());
    }

    /// No more callers hold a permit at once than there are handles.
    #[test]
    fn no_more_callers_read_at_once_than_there_are_handles() {
        const HANDLES: u16 = 3;
        let reads = Arc::new(FileReads::for_handles(HANDLES).expect("permits"));
        let reading = Arc::new(AtomicUsize::new(0));
        let most = Arc::new(AtomicUsize::new(0));
        let threads: Vec<_> = (0..12)
            .map(|_| {
                let (reads, reading, most) =
                    (Arc::clone(&reads), Arc::clone(&reading), Arc::clone(&most));
                std::thread::spawn(move || {
                    for _ in 0..200 {
                        let _permit = reads.permit();
                        let now = reading.fetch_add(1, Ordering::SeqCst) + 1;
                        most.fetch_max(now, Ordering::SeqCst);
                        std::thread::yield_now();
                        reading.fetch_sub(1, Ordering::SeqCst);
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().expect("a thread does not panic");
        }
        let most = most.load(Ordering::SeqCst);
        assert!(most <= usize::from(HANDLES), "{most} callers read at once");
        assert_eq!(*reads.available.lock().unwrap(), usize::from(HANDLES));
    }
}
