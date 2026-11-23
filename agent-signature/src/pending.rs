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

//! A result worked out on a thread of its own, which any number of requests
//! can wait on for a limited time.

use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// A result that is being worked out, or has been.
pub(crate) struct Pending<T: Clone> {
    result: Mutex<Option<T>>,
    finished: Condvar,
}

impl<T: Clone> Default for Pending<T> {
    fn default() -> Self {
        Pending {
            result: Mutex::new(None),
            finished: Condvar::new(),
        }
    }
}

impl<T: Clone> Pending<T> {
    /// Record the result and wake every waiter. Only the first result is
    /// kept.
    pub(crate) fn complete(&self, value: T) {
        let mut result = lock(&self.result);
        if result.is_none() {
            *result = Some(value);
        }
        drop(result);
        self.finished.notify_all();
    }

    /// The result, where it has been worked out.
    pub(crate) fn get(&self) -> Option<T> {
        lock(&self.result).clone()
    }

    /// Wait for the result for at most `budget`, and return `None` where it
    /// is still being worked out after that. The work carries on either way.
    pub(crate) fn wait(&self, budget: Duration) -> Option<T> {
        let deadline = Instant::now().checked_add(budget);
        let mut result = lock(&self.result);
        loop {
            if let Some(value) = result.as_ref() {
                return Some(value.clone());
            }
            let remaining = match deadline {
                Some(deadline) => deadline.saturating_duration_since(Instant::now()),
                None => budget,
            };
            if remaining.is_zero() {
                return None;
            }
            result = match self.finished.wait_timeout(result, remaining) {
                Ok((guard, _)) => guard,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
    }
}

/// Lock a mutex, carrying on with what it holds where a holder panicked,
/// because everything this crate keeps behind a lock is a complete value at
/// every point it can be seen.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;

    use super::*;

    #[test]
    fn a_waiter_gives_up_at_the_budget_and_the_result_arrives_later() {
        let pending = Arc::new(Pending::<u32>::default());
        let started = Instant::now();
        assert_eq!(pending.wait(Duration::from_millis(50)), None);
        assert!(started.elapsed() >= Duration::from_millis(50));
        let worker = Arc::clone(&pending);
        thread::spawn(move || worker.complete(7)).join().unwrap();
        assert_eq!(pending.get(), Some(7));
        assert_eq!(pending.wait(Duration::ZERO), Some(7));
    }

    #[test]
    fn a_waiter_is_woken_by_the_result() {
        let pending = Arc::new(Pending::<u32>::default());
        let worker = Arc::clone(&pending);
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            worker.complete(3);
        });
        assert_eq!(pending.wait(Duration::from_secs(30)), Some(3));
        handle.join().unwrap();
    }
}
