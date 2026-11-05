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

//! The threads that run the elements of parallel groups.
//!
//! Starting a thread for each element of each flow data costs far more than
//! most elements take, so the elements run on threads shared by every
//! pipeline in the process. A thread is started only when none is idle, and a
//! thread that has had no work for [`IDLE_SECONDS`] ends.
//!
//! The `group_load` bench in this crate measures the two, as its `threads`
//! and `group` patterns (`benches/group_load.rs`).
//!
//! The thread that offers work takes it back if no pool thread has started
//! it, so a group always finishes, even with every pool thread busy or on a
//! platform that cannot start threads. The first time work finds all
//! [`MOST_THREADS`] pool threads in use, a warning is logged.

use std::collections::VecDeque;
use std::io;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread;
use std::time::Duration;

/// The most pool threads at once. Past it, work waits for a pool thread or is
/// taken back by the thread that offered it, and the first time that happens
/// a warning naming this limit is logged.
const MOST_THREADS: usize = 64;

/// How long a pool thread waits for work before it ends.
const IDLE_SECONDS: u64 = 60;

/// The name every pool thread carries.
const THREAD_NAME: &str = "fiftyone-group";

type Work<T> = Box<dyn FnOnce() -> T + Send>;

/// Work that a pool thread or the thread that offered it runs, whichever
/// takes it first.
pub(crate) struct Job<T> {
    work: Mutex<Option<Work<T>>>,
    /// What the work gave, or what it panicked with, when a pool thread ran
    /// it.
    outcome: Mutex<Option<thread::Result<T>>>,
    finished: Condvar,
}

impl<T: Send + 'static> Job<T> {
    /// Offer `work` to the pool threads.
    pub(crate) fn offer(work: impl FnOnce() -> T + Send + 'static) -> Arc<Self> {
        let job = Arc::new(Job {
            work: Mutex::new(Some(Box::new(work))),
            outcome: Mutex::new(None),
            finished: Condvar::new(),
        });
        Pool::shared().offer(Arc::clone(&job) as Arc<dyn Runnable>);
        job
    }

    /// Run the work on this thread if no pool thread has started it, or wait
    /// for the pool thread that has, and return what the work gave or what
    /// it panicked with.
    pub(crate) fn finish(self: &Arc<Self>) -> thread::Result<T> {
        if let Some(work) = self.take() {
            Pool::shared().withdraw(&(Arc::clone(self) as Arc<dyn Runnable>));
            return panic::catch_unwind(AssertUnwindSafe(work));
        }
        let mut outcome = lock(&self.outcome);
        loop {
            if let Some(outcome) = outcome.take() {
                return outcome;
            }
            outcome = self
                .finished
                .wait(outcome)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn take(&self) -> Option<Work<T>> {
        lock(&self.work).take()
    }
}

/// What the pool threads take from the queue, whatever the work gives.
trait Runnable: Send + Sync {
    /// Run the work unless another thread has taken it.
    fn run_if_waiting(&self);
}

impl<T: Send + 'static> Runnable for Job<T> {
    fn run_if_waiting(&self) {
        if let Some(work) = self.take() {
            let outcome = panic::catch_unwind(AssertUnwindSafe(work));
            *lock(&self.outcome) = Some(outcome);
            self.finished.notify_all();
        }
    }
}

/// The pool threads and the work waiting for them.
struct Pool {
    state: Mutex<State>,
    work_waiting: Condvar,
}

struct State {
    /// Work offered and not yet taken.
    queue: VecDeque<Arc<dyn Runnable>>,
    /// Pool threads running or starting.
    threads: usize,
    /// Pool threads waiting for work.
    idle: usize,
    /// True once the platform has said it cannot start threads.
    unsupported: bool,
    /// True once work has found every pool thread in use, which is logged
    /// only the first time.
    limit_reached: bool,
}

static POOL: OnceLock<Arc<Pool>> = OnceLock::new();

impl Pool {
    fn shared() -> &'static Arc<Pool> {
        POOL.get_or_init(|| {
            Arc::new(Pool {
                state: Mutex::new(State {
                    queue: VecDeque::new(),
                    threads: 0,
                    idle: 0,
                    unsupported: false,
                    limit_reached: false,
                }),
                work_waiting: Condvar::new(),
            })
        })
    }

    /// Queue `job`, then wake an idle pool thread for it or start one. The
    /// first time every pool thread is in use, log a warning.
    fn offer(self: &Arc<Self>, job: Arc<dyn Runnable>) {
        let (start, first_at_limit) = {
            let mut state = lock(&self.state);
            if state.unsupported {
                return;
            }
            state.queue.push_back(job);
            if state.idle >= state.queue.len() {
                self.work_waiting.notify_one();
                (false, false)
            } else if state.threads < MOST_THREADS {
                state.threads += 1;
                (true, false)
            } else {
                (false, !std::mem::replace(&mut state.limit_reached, true))
            }
        };
        if first_at_limit {
            log::warn!(
                "All {MOST_THREADS} threads shared by parallel groups are in use, so an \
                 element of a group that none of them has started runs on the thread \
                 processing the flow data once that thread reaches it. This is logged \
                 only the first time."
            );
        }
        if start {
            let pool = Arc::clone(self);
            let started = thread::Builder::new()
                .name(THREAD_NAME.to_owned())
                .spawn(move || pool.work());
            if let Err(error) = started {
                let mut state = lock(&self.state);
                state.threads -= 1;
                if state.threads == 0 {
                    // Nothing would take the queued work, so the threads
                    // that offered it run it themselves.
                    state.queue.clear();
                    state.unsupported = error.kind() == io::ErrorKind::Unsupported;
                }
            }
        }
    }

    /// Take back work the thread that offered it is running itself, so no
    /// pool thread is woken or started for it.
    fn withdraw(&self, job: &Arc<dyn Runnable>) {
        let mut state = lock(&self.state);
        if let Some(index) = state
            .queue
            .iter()
            .position(|queued| Arc::ptr_eq(queued, job))
        {
            state.queue.remove(index);
        }
    }

    /// A pool thread's loop, which ends once the thread has been idle for
    /// [`IDLE_SECONDS`].
    fn work(&self) {
        let mut state = lock(&self.state);
        loop {
            if let Some(job) = state.queue.pop_front() {
                drop(state);
                job.run_if_waiting();
                state = lock(&self.state);
                continue;
            }
            state.idle += 1;
            let (next, waited) = self
                .work_waiting
                .wait_timeout(state, Duration::from_secs(IDLE_SECONDS))
                .unwrap_or_else(PoisonError::into_inner);
            state = next;
            state.idle -= 1;
            if waited.timed_out() && state.queue.is_empty() {
                state.threads -= 1;
                return;
            }
        }
    }
}

/// Lock a mutex. No thread panics while holding one of these, because work
/// runs outside them and its panics are caught, so a poisoned lock still
/// guards a consistent value.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
