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

//! A spawner for native hosts on tokio, turned on by the `tokio` feature.
//!
//! ```
//! use fiftyone_caching::{from_fn, tokio::LocalPool, LoadingCache, LruStore};
//!
//! let store: LruStore<u32, String> = LruStore::builder().build();
//! let loader = from_fn(|key: u32| async move { Ok::<_, String>(format!("value of {key}")) });
//! let cache = LoadingCache::builder(store, loader)
//!     .spawner(LocalPool::new(4))
//!     .build();
//! # drop(cache);
//! ```

use ::tokio_util::task::LocalPoolHandle;

use crate::{Spawn, StartLoad};

/// Runs each load as a task of its own on a pool of threads, each with a
/// single-threaded tokio runtime, through tokio-util's
/// [`LocalPoolHandle::spawn_pinned`].
///
/// Each load's future is made on the thread that runs it and stays there,
/// so it need not be `Send`, and loads share the pool's threads while they
/// wait rather than holding one each. A load may use tokio's timers and I/O,
/// which belong to the pool thread's runtime. The pool needs no runtime to
/// be running where loads are started. Dropping the last handle on the pool
/// stops its threads, and with them any load still running.
#[derive(Clone)]
pub struct LocalPool {
    pool: LocalPoolHandle,
}

impl LocalPool {
    /// A pool of `threads` threads.
    ///
    /// # Panics
    ///
    /// When `threads` is 0.
    pub fn new(threads: usize) -> Self {
        LocalPool {
            pool: LocalPoolHandle::new(threads),
        }
    }
}

impl From<LocalPoolHandle> for LocalPool {
    fn from(pool: LocalPoolHandle) -> Self {
        LocalPool { pool }
    }
}

impl Spawn for LocalPool {
    fn spawn(&self, start: StartLoad) {
        // Dropping the join handle leaves the load running.
        drop(self.pool.spawn_pinned(start));
    }
}
