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
//!
//! A cache given a weigher also holds no more than a number of bytes. Each
//! shard holds its share of them and pops entries from the front until it is
//! back within that share, so a cache can be bounded by memory as well as by
//! entries.

use std::hash::Hash;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ahash::RandomState;
use hashlink::LinkedHashMap;

use crate::cache::{Cache, PutCache};
use crate::config::CacheBuilder;

/// The most entries a shard reserves room for when it is made. A larger
/// shard grows as entries arrive, so a very large size, such as one read
/// from a configuration, takes memory only for the entries held.
const MOST_ENTRIES_RESERVED_PER_SHARD: usize = 4096;

/// Gives the bytes an entry takes, for a cache bounded by bytes.
pub type Weigher<K, V> = Arc<dyn Fn(&K, &V) -> usize + Send + Sync>;

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
    /// The bytes each entry takes, when the cache is bounded by bytes.
    weigher: Option<Weigher<K, V>>,
    /// The most bytes a single shard holds. `usize::MAX` when unbounded.
    max_bytes_per_shard: AtomicUsize,
}

/// One shard of the cache: an insertion-ordered map behind its own lock.
struct Shard<K, V> {
    map: Mutex<ShardEntries<K, V>>,
}

/// A shard's entries and the bytes they take.
struct ShardEntries<K, V> {
    entries: LinkedHashMap<K, V, RandomState>,
    /// The sum of the weigher over the entries, or zero with no weigher.
    bytes: usize,
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
    /// capacity is never below the requested size. Each shard reserves room
    /// for at most 4,096 entries when it is made and grows beyond that as
    /// entries arrive.
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
                map: Mutex::new(ShardEntries {
                    entries: LinkedHashMap::with_capacity_and_hasher(
                        capacity_per_shard.min(MOST_ENTRIES_RESERVED_PER_SHARD),
                        RandomState::new(),
                    ),
                    bytes: 0,
                }),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        LruCache {
            shards,
            capacity_per_shard,
            hash_builder,
            weigher: None,
            max_bytes_per_shard: AtomicUsize::new(usize::MAX),
        }
    }

    /// Bounds the cache by bytes as well as entries, with `weigher` giving
    /// the bytes each entry takes. The bound is set by
    /// [`set_max_bytes`](LruCache::set_max_bytes). Until it is set the cache
    /// counts the bytes and holds any number of them.
    ///
    /// # Example
    ///
    /// ```
    /// use std::sync::Arc;
    /// use fiftyone_caching::{Cache, LruCache, PutCache};
    ///
    /// let cache: LruCache<u32, Vec<u8>> =
    ///     LruCache::new(100, 1).with_weigher(Arc::new(|_, value: &Vec<u8>| value.len()));
    /// cache.set_max_bytes(10);
    /// cache.put(1, vec![0; 6]);
    /// cache.put(2, vec![0; 6]);
    /// // Twelve bytes is over the bound, so the least recently used entry went.
    /// assert_eq!(cache.get(&1), None);
    /// assert_eq!(cache.bytes(), 6);
    /// ```
    pub fn with_weigher(mut self, weigher: Weigher<K, V>) -> Self {
        self.weigher = Some(weigher);
        self
    }

    /// Sets the most bytes the cache holds, each shard holding an equal share,
    /// and evicts the least recently used entries of any shard over its share.
    /// An entry larger than a shard's share is not kept. Has no effect on a
    /// cache with no [weigher](LruCache::with_weigher).
    pub fn set_max_bytes(&self, max_bytes: usize) {
        let Some(weigher) = &self.weigher else {
            return;
        };
        let per_shard = (max_bytes / self.shards.len()).max(1);
        self.max_bytes_per_shard.store(per_shard, Ordering::Relaxed);
        for shard in self.shards.iter() {
            let mut shard = shard.lock();
            Self::evict(&mut shard, usize::MAX, per_shard, Some(weigher));
        }
    }

    /// The most bytes the cache holds, or `None` when it is not bounded by
    /// bytes.
    pub fn max_bytes(&self) -> Option<usize> {
        let per_shard = self.max_bytes_per_shard.load(Ordering::Relaxed);
        (self.weigher.is_some() && per_shard != usize::MAX)
            .then(|| per_shard.saturating_mul(self.shards.len()))
    }

    /// The bytes the entries take, by the [weigher](LruCache::with_weigher),
    /// or zero with none.
    pub fn bytes(&self) -> usize {
        self.shards.iter().map(|shard| shard.lock().bytes).sum()
    }

    /// Pops entries from the least recently used end until the shard holds
    /// no more than `capacity` entries and `max_bytes` bytes.
    fn evict(
        shard: &mut ShardEntries<K, V>,
        capacity: usize,
        max_bytes: usize,
        weigher: Option<&Weigher<K, V>>,
    ) {
        while shard.entries.len() > capacity || shard.bytes > max_bytes {
            let Some((key, value)) = shard.entries.pop_front() else {
                shard.bytes = 0;
                break;
            };
            if let Some(weigher) = weigher {
                shard.bytes = shard.bytes.saturating_sub(weigher(&key, &value));
            }
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
        let mut shard = self.shard_for(key).lock();
        let value = shard.entries.remove(key)?;
        if let Some(weigher) = &self.weigher {
            shard.bytes = shard.bytes.saturating_sub(weigher(key, &value));
        }
        Some(value)
    }

    /// Drop every entry from every shard. Shared by the inherent
    /// [`LruCache::clear`] and the [`PutCache::clear`] trait method so the two
    /// cannot drift apart.
    fn clear_entries(&self) {
        for shard in self.shards.iter() {
            let mut shard = shard.lock();
            shard.entries.clear();
            shard.bytes = 0;
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
    fn lock(&self) -> std::sync::MutexGuard<'_, ShardEntries<K, V>> {
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
        let mut shard = self.shard_for(key).lock();
        // `to_back` both fetches the value and promotes the entry to
        // most-recently-used in one pass, which is what makes this an LRU. It
        // returns `None` for a key that is absent, so a miss reports a miss
        // rather than returning whatever happens to be most-recent.
        shard.entries.to_back(key).map(|value| value.clone())
    }

    fn len(&self) -> usize {
        self.shards
            .iter()
            .map(|shard| shard.lock().entries.len())
            .sum()
    }
}

impl<K, V> PutCache<K, V> for LruCache<K, V>
where
    K: Hash + Eq + Clone + Send + Sync,
    V: Clone + Send + Sync,
{
    fn put(&self, key: K, value: V) {
        let weight = self
            .weigher
            .as_ref()
            .map_or(0, |weigher| weigher(&key, &value));
        let mut shard = self.shard_for(&key).lock();
        // `insert` adds (or replaces) at the back, the most-recently-used end,
        // so re-writing an existing key also promotes it.
        let replaced = shard.entries.insert(key, value);
        if let Some(weigher) = &self.weigher {
            // The replaced value is weighed with its key, now at the back.
            let replaced = match (&replaced, shard.entries.back()) {
                (Some(old), Some((key, _))) => weigher(key, old),
                _ => 0,
            };
            shard.bytes = shard.bytes.saturating_sub(replaced).saturating_add(weight);
        }
        // Enforce the per-shard capacity and byte bound by dropping from the
        // front, the least-recently-used end.
        Self::evict(
            &mut shard,
            self.capacity_per_shard,
            self.max_bytes_per_shard.load(Ordering::Relaxed),
            self.weigher.as_ref(),
        );
    }

    fn clear(&self) {
        self.clear_entries();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The room a shard reserves stays near the limit however large the
    /// cache, and the shard grows past it as entries arrive.
    #[test]
    fn a_large_cache_reserves_room_as_entries_arrive() {
        const SIZE: usize = 10_000_000;
        const ADDED: u64 = 10_000;
        let cache: LruCache<u64, u64> = LruCache::new(SIZE, 1);
        assert_eq!(cache.capacity(), SIZE);
        let reserved = cache.shards[0].lock().entries.capacity();
        assert!(
            (MOST_ENTRIES_RESERVED_PER_SHARD..2 * MOST_ENTRIES_RESERVED_PER_SHARD)
                .contains(&reserved),
            "{reserved}"
        );
        for key in 0..ADDED {
            cache.put(key, key);
        }
        assert_eq!(cache.len(), ADDED as usize);
        assert_eq!(cache.get(&0), Some(0));
    }

    fn by_length() -> LruCache<u32, Vec<u8>> {
        LruCache::new(100, 1).with_weigher(Arc::new(|_, value: &Vec<u8>| value.len()))
    }

    #[test]
    fn the_bytes_follow_puts_replacements_and_removals() {
        let cache = by_length();
        cache.put(1, vec![0; 10]);
        cache.put(2, vec![0; 20]);
        assert_eq!(cache.bytes(), 30);
        cache.put(1, vec![0; 5]);
        assert_eq!(cache.bytes(), 25);
        assert_eq!(cache.remove(&2), Some(vec![0; 20]));
        assert_eq!(cache.bytes(), 5);
        cache.clear();
        assert_eq!(cache.bytes(), 0);
        assert_eq!(cache.max_bytes(), None);
    }

    #[test]
    fn lowering_the_bound_evicts_the_least_recently_used() {
        let cache = by_length();
        for key in 1..=4 {
            cache.put(key, vec![0; 10]);
        }
        assert_eq!(cache.get(&1), Some(vec![0; 10]));
        cache.set_max_bytes(25);
        assert_eq!(cache.max_bytes(), Some(25));
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get(&1), Some(vec![0; 10]));
        assert_eq!(cache.get(&4), Some(vec![0; 10]));
        assert_eq!(cache.get(&2), None);
    }

    #[test]
    fn an_entry_larger_than_the_bound_is_not_kept() {
        let cache = by_length();
        cache.set_max_bytes(8);
        cache.put(1, vec![0; 4]);
        cache.put(2, vec![0; 9]);
        assert_eq!(cache.get(&2), None);
        assert_eq!(cache.bytes(), 0);
    }

    #[test]
    fn a_cache_with_no_weigher_ignores_a_bound() {
        let cache: LruCache<u32, u32> = LruCache::new(10, 1);
        cache.set_max_bytes(1);
        cache.put(1, 1);
        assert_eq!(cache.get(&1), Some(1));
        assert_eq!(cache.max_bytes(), None);
    }
}
