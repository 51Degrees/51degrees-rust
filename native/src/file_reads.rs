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
//! expect. A read that finds every handle in use does not wait for one. It
//! fails with `InsufficientHandles`, and in the IP Intelligence library a
//! failed read of a property's values ends the process. A permit taken before
//! each such call makes a caller beyond the size of the pool wait instead.

use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

/// The permits for the calls that can read a data file, one for each handle
/// in its pool.
pub(crate) struct FileReads {
    state: Mutex<Permits>,
    returned: Condvar,
}

/// The permits not taken, and the callers waiting for one.
struct Permits {
    available: usize,
    waiting: usize,
}

impl FileReads {
    /// Permits for a pool of `handles` file handles, or [`None`] when the data
    /// set reads nothing from its file once it is loaded.
    pub(crate) fn for_handles(handles: u16) -> Option<FileReads> {
        (handles > 0).then(|| FileReads {
            state: Mutex::new(Permits {
                available: usize::from(handles),
                waiting: 0,
            }),
            returned: Condvar::new(),
        })
    }

    /// Wait for a permit. It is returned when the permit is dropped.
    pub(crate) fn permit(&self) -> FileReadPermit<'_> {
        let mut state = self.lock();
        while state.available == 0 {
            state.waiting += 1;
            state = self
                .returned
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
            state.waiting -= 1;
        }
        state.available -= 1;
        FileReadPermit(self)
    }

    fn lock(&self) -> MutexGuard<'_, Permits> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A permit to make one call that can read a data file.
pub(crate) struct FileReadPermit<'a>(&'a FileReads);

/// Returns the permit, and wakes a caller only when one is waiting. A lookup
/// takes a permit for every property it reads, and on Linux `notify_one`
/// makes a system call even when no thread waits, so waking on every return
/// would add one to every read.
impl Drop for FileReadPermit<'_> {
    fn drop(&mut self) {
        let wake = {
            let mut state = self.0.lock();
            state.available += 1;
            state.waiting > 0
        };
        if wake {
            self.0.returned.notify_one();
        }
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
        let state = reads.lock();
        assert_eq!(state.available, usize::from(HANDLES));
        assert_eq!(state.waiting, 0, "every waiting caller was counted out");
    }

    /// A caller waiting for the only permit gets it when the holder returns
    /// it, so a return that sees a waiter wakes it.
    #[test]
    fn a_returned_permit_wakes_the_caller_waiting_for_it() {
        const WAKE_TIMEOUT_SECONDS: u64 = 10;
        let reads = Arc::new(FileReads::for_handles(1).expect("permits"));
        let held = reads.permit();
        let (taken, took) = std::sync::mpsc::channel();
        let waiter = {
            let reads = Arc::clone(&reads);
            std::thread::spawn(move || {
                let _permit = reads.permit();
                taken.send(()).expect("the test is listening");
            })
        };
        while reads.lock().waiting == 0 {
            std::thread::yield_now();
        }
        drop(held);
        took.recv_timeout(std::time::Duration::from_secs(WAKE_TIMEOUT_SECONDS))
            .expect("the waiting caller is woken when the permit is returned");
        waiter.join().expect("the waiter does not panic");
        assert_eq!(reads.lock().waiting, 0);
    }
}
