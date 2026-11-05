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

//! The default sharded LRU cache.
//!
//! This implements the custom sharded LRU cache described in the
//! [caching specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/caching.md#cache-implementation).
//! Third-party general-purpose caches were found to be too slow for the
//! pipeline's hot path, so this keeps the design simple. A request only touches
//! one shard, so threads contend on a lock only when they hash to the same
//! shard.
//!
//! Each shard is a [`std::sync::Mutex`] around an insertion-ordered map. On a
//! hit the entry is moved to the back (most-recently-used). When a shard is
//! full an insert pops the entry at the front (least-recently-used).

use std::hash::Hash;
use std::sync::Mutex;

use ahash::RandomState;
use hashlink::LinkedHashMap;

use crate::cache::{Cache, PutCache};
use crate::config::CacheBuilder;

/// A sharded, least-recently-used cache.
///
/// `LruCache` is generic over a key `K` and a value `V`. Both must be `Send +
/// Sync` so the cache itself is `Send + Sync` and can live behind an
/// [`std::sync::Arc`] shared across worker threads. `V` must be `Clone` because
/// [`Cache::get`] returns the value by clone rather than by reference (a borrow
/// cannot outlive the per-shard lock).
///
/// This is the reason the cache is generic over a value type and never over
/// `dyn ElementData`: element data is not `Clone`. Engines instead cache their
/// own concrete `Send + Sync + Clone` data, typically an [`std::sync::Arc`]
/// around an aspect-data struct, or the raw cloud JSON.
///
/// Build one with [`CacheBuilder`] (via [`LruCache::builder`]) or use
/// [`LruCache::with_defaults`] for the specification defaults (size 1000, one
/// shard per CPU).
///
/// # Example
///
/// ```
/// use fiftyone_caching::{Cache, CacheBuilder, LruCache, PutCache};
///
/// let cache: LruCache<String, u32> = CacheBuilder::new().size(2).concurrency(1).build();
/// cache.put("a".to_owned(), 1);
/// cache.put("b".to_owned(), 2);
/// assert_eq!(cache.get(&"a".to_owned()), Some(1));
///
/// // The cache holds two entries, so adding a third evicts the least recently
/// // used. "a" was just read so "b" is now the eviction candidate.
/// cache.put("c".to_owned(), 3);
/// assert_eq!(cache.get(&"b".to_owned()), None);
/// assert_eq!(cache.get(&"a".to_owned()), Some(1));
/// assert_eq!(cache.get(&"c".to_owned()), Some(3));
/// ```
pub struct LruCache<K, V>
where
    K: Hash + Eq + Send + Sync,
    V: Clone + Send + Sync,
{
    shards: Box<[Shard<K, V>]>,
    /// The per-shard capacity. The total capacity is this multiplied by the
    /// number of shards.
    capacity_per_shard: usize,
    /// The fixed hasher used to pick a shard. It must be the same for the life
    /// of the cache so a key always lands on the same shard.
    hash_builder: RandomState,
}

/// One shard of the cache: an insertion-ordered map behind its own lock.
struct Shard<K, V> {
    map: Mutex<LinkedHashMap<K, V, RandomState>>,
}

impl<K, V> LruCache<K, V>
where
    K: Hash + Eq + Clone + Send + Sync,
    V: Clone + Send + Sync,
{
    /// Start building a cache with the specification defaults, which the
    /// returned [`CacheBuilder`] can override.
    pub fn builder() -> CacheBuilder {
        CacheBuilder::new()
    }

    /// Build a cache with the specification defaults: a total size of 1000 and
    /// one shard per logical CPU.
    pub fn with_defaults() -> Self {
        CacheBuilder::new().build()
    }

    /// Build a cache directly from a total `size` and `shard_count`.
    ///
    /// This is the constructor [`CacheBuilder::build`] calls. `size` is the
    /// total number of entries across every shard, and `shard_count` is the
    /// number of shards. Both are clamped to at least one. The per-shard
    /// capacity is `size` divided by `shard_count`, rounded up so the total
    /// capacity is never below the requested size.
    pub fn new(size: usize, shard_count: usize) -> Self {
        let shard_count = shard_count.max(1);
        let size = size.max(1);
        // Round up so size / shard_count shards never under-provision. For
        // example size 1000 over 3 shards gives 334 per shard (1002 total),
        // which is the conservative choice.
        let capacity_per_shard = size.div_ceil(shard_count);
        let hash_builder = RandomState::new();
        let shards = (0..shard_count)
            .map(|_| Shard {
                map: Mutex::new(LinkedHashMap::with_capacity_and_hasher(
                    capacity_per_shard,
                    RandomState::new(),
                )),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        LruCache {
            shards,
            capacity_per_shard,
            hash_builder,
        }
    }

    /// The number of shards in this cache.
    pub fn shard_count(&self) -> usize {
        self.shards.len()
    }

    /// The maximum number of entries a single shard will hold before evicting.
    pub fn capacity_per_shard(&self) -> usize {
        self.capacity_per_shard
    }

    /// The total capacity across every shard.
    pub fn capacity(&self) -> usize {
        self.capacity_per_shard * self.shards.len()
    }

    /// Remove every entry from the cache.
    pub fn clear(&self) {
        self.clear_entries();
    }

    /// Remove the entry for `key`, returning its value if there was one.
    pub fn remove(&self, key: &K) -> Option<V> {
        self.shard_for(key).lock().remove(key)
    }

    /// Drop every entry from every shard. Shared by the inherent
    /// [`LruCache::clear`] and the [`PutCache::clear`] trait method so the two
    /// cannot drift apart.
    fn clear_entries(&self) {
        for shard in self.shards.iter() {
            shard.lock().clear();
        }
    }

    /// Pick the shard a key belongs to. A key always maps to the same shard
    /// because `hash_builder` is fixed for the life of the cache.
    fn shard_for(&self, key: &K) -> &Shard<K, V> {
        let index = (self.hash_builder.hash_one(key) as usize) % self.shards.len();
        &self.shards[index]
    }
}

impl<K, V> Shard<K, V>
where
    K: Hash + Eq,
{
    /// Lock the shard, recovering from a poisoned lock.
    ///
    /// A poisoned lock means a previous holder panicked while mutating the map.
    /// The map is a plain key-value store with no cross-entry invariant to
    /// uphold, so it is safe to carry on with whatever state it was left in
    /// rather than propagating the panic to every later request.
    fn lock(&self) -> std::sync::MutexGuard<'_, LinkedHashMap<K, V, RandomState>> {
        self.map
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl<K, V> Cache<K, V> for LruCache<K, V>
where
    K: Hash + Eq + Clone + Send + Sync,
    V: Clone + Send + Sync,
{
    fn get(&self, key: &K) -> Option<V> {
        let mut map = self.shard_for(key).lock();
        // `to_back` both fetches the value and promotes the entry to
        // most-recently-used in one pass, which is what makes this an LRU. It
        // returns `None` for a key that is absent, so a miss reports a miss
        // rather than returning whatever happens to be most-recent.
        map.to_back(key).map(|value| value.clone())
    }

    fn len(&self) -> usize {
        self.shards.iter().map(|shard| shard.lock().len()).sum()
    }
}

impl<K, V> PutCache<K, V> for LruCache<K, V>
where
    K: Hash + Eq + Clone + Send + Sync,
    V: Clone + Send + Sync,
{
    fn put(&self, key: K, value: V) {
        let mut map = self.shard_for(&key).lock();
        // `insert` adds (or replaces) at the back, the most-recently-used end,
        // so re-writing an existing key also promotes it.
        map.insert(key, value);
        // Enforce the per-shard capacity by dropping from the front, the
        // least-recently-used end. A single insert can only ever take the shard
        // one over capacity, but pop in a loop to stay correct even if the
        // capacity were ever reduced.
        while map.len() > self.capacity_per_shard {
            map.pop_front();
        }
    }

    fn clear(&self) {
        self.clear_entries();
    }
}
