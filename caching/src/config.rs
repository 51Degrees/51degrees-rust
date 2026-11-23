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

//! Cache configuration and the builder that applies it.
//!
//! The two tunables match the
//! [caching specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/caching.md#cache-implementation).
//! They are the total `size` (default 1000) and the `concurrency`, which is the
//! number of shards (default the number of CPU cores).

use std::hash::Hash;

use crate::lru::LruCache;

/// The default total cache size, matching the specification.
pub const DEFAULT_SIZE: usize = 1000;

/// The number of logical CPUs, the default shard count.
///
/// Falls back to `1` on the rare platform where the count cannot be queried,
/// which still yields a correct (single-shard) cache.
pub fn default_concurrency() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

/// Builds an [`LruCache`] from a [`size`](CacheBuilder::size) and a
/// [`concurrency`](CacheBuilder::concurrency) (shard count).
///
/// The builder is value-type agnostic, so one builder can produce caches of
/// different key and value types via the generic [`CacheBuilder::build`].
///
/// # Example
///
/// ```
/// use fiftyone_caching::{CacheBuilder, LruCache};
///
/// let cache: LruCache<String, u64> = CacheBuilder::new()
///     .size(500)
///     .concurrency(4)
///     .build();
/// assert_eq!(cache.shard_count(), 4);
/// ```
#[derive(Debug, Clone)]
pub struct CacheBuilder {
    size: usize,
    concurrency: usize,
}

impl CacheBuilder {
    /// Create a builder with the specification defaults: size 1000, one shard
    /// per CPU core.
    pub fn new() -> Self {
        CacheBuilder {
            size: DEFAULT_SIZE,
            concurrency: default_concurrency(),
        }
    }

    /// Set the total number of entries the cache holds before it evicts the
    /// least-recently-used entry. Values below one are treated as one.
    pub fn size(mut self, size: usize) -> Self {
        self.size = size;
        self
    }

    /// Set the number of shards (the concurrency level). More shards reduce
    /// lock contention between threads at the cost of slightly looser global
    /// LRU behavior. Values below one are treated as one.
    pub fn concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency;
        self
    }

    /// The configured total size.
    pub fn configured_size(&self) -> usize {
        self.size
    }

    /// The configured shard count.
    pub fn configured_concurrency(&self) -> usize {
        self.concurrency
    }

    /// Build a sharded [`LruCache`] for the given key and value types.
    pub fn build<K, V>(&self) -> LruCache<K, V>
    where
        K: Hash + Eq + Clone + Send + Sync,
        V: Clone + Send + Sync,
    {
        LruCache::new(self.size, self.concurrency)
    }
}

impl Default for CacheBuilder {
    fn default() -> Self {
        CacheBuilder::new()
    }
}
