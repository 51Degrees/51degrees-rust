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

//! Maps split into locked shards by key, so callers working on different
//! keys rarely wait for the same lock.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hash};
use std::sync::{Mutex, MutexGuard, PoisonError};

pub(crate) struct Shards<T> {
    shards: Box<[Mutex<T>]>,
    // Fixed for the life of the shards so a key always finds its shard.
    hasher: RandomState,
}

impl<T> Shards<T> {
    /// `count` shards, at least one, each made by `make`.
    pub(crate) fn new(count: usize, make: impl Fn() -> T) -> Self {
        Shards {
            shards: (0..count.max(1)).map(|_| Mutex::new(make())).collect(),
            hasher: RandomState::new(),
        }
    }

    /// The shard that holds `key`.
    pub(crate) fn for_key<K: Hash + ?Sized>(&self, key: &K) -> &Mutex<T> {
        let index = self.hasher.hash_one(key) as usize % self.shards.len();
        &self.shards[index]
    }
}

/// Locks `mutex`, carrying on after a panic in another holder. Every map
/// here is left consistent between statements, so its state is safe to use.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
