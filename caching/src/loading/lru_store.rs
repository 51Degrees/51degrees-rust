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

//! A store in process memory over the sharded LRU cache.

use std::convert::Infallible;
use std::hash::Hash;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use super::clock::{self, Clock};
use super::entry::Entry;
use super::store::{Lookup, Store};
use crate::cache::{Cache, PutCache};
use crate::config::CacheBuilder;
use crate::lru::{LruCache, Weigher};
#[cfg(not(target_family = "wasm"))]
use crate::memory_budget::MemoryBudget;

/// The bytes an entry takes beyond its value: the key, the times and the
/// cache's own bookkeeping.
fn entry_overhead<K, V>() -> usize {
    std::mem::size_of::<K>() + std::mem::size_of::<Kept<V>>() + 32
}

/// A [`Store`] in process memory, the sharded [`LruCache`] with lifetimes.
///
/// When full it evicts the least recently used entry in the entry's shard,
/// and it drops an entry once the `keep_for` it was written with runs out. It
/// is the store of an [`LruLoadingCache`](crate::LruLoadingCache), and stands
/// in for a platform store in tests. It never makes callers wait.
///
/// A store can also be bounded by bytes, with a fixed
/// [limit](LruStoreBuilder::max_bytes) or, on native targets, a
/// [`MemoryBudget`](crate::MemoryBudget) that follows the machine's memory,
/// so that copies in memory are evicted when memory comes under pressure.
/// Below a store on disk, an evicted entry is read back from there rather
/// than loaded from the source.
pub struct LruStore<K, V>
where
    K: Hash + Eq + Send + Sync,
    V: Clone + Send + Sync,
{
    cache: LruCache<K, Kept<V>>,
    clock: Arc<dyn Clock>,
    /// The fixed limit on bytes, which a budget never exceeds.
    #[cfg(not(target_family = "wasm"))]
    max_bytes: Option<usize>,
    #[cfg(not(target_family = "wasm"))]
    budget: Option<MemoryBudget>,
}

/// An entry and the time the store drops it.
#[derive(Clone)]
struct Kept<V> {
    entry: Entry<V>,
    until: Option<SystemTime>,
}

impl<K, V> LruStore<K, V>
where
    K: Hash + Eq + Clone + Send + Sync,
    V: Clone + Send + Sync,
{
    /// Starts building a store, with the [`CacheBuilder`] defaults.
    pub fn builder() -> LruStoreBuilder<K, V> {
        LruStoreBuilder {
            cache: CacheBuilder::new(),
            clock: None,
            weigher: None,
            max_bytes: None,
            #[cfg(not(target_family = "wasm"))]
            budget: None,
            types: PhantomData,
        }
    }

    /// The number of entries held, including any past their time that have
    /// not been read since.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// True when no entries are held.
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// The most entries the store holds.
    pub fn capacity(&self) -> usize {
        self.cache.capacity()
    }

    /// The bytes the entries take, or zero for a store not bounded by bytes.
    pub fn bytes(&self) -> usize {
        self.cache.bytes()
    }

    /// The most bytes the store now holds, or `None` for a store not bounded
    /// by bytes.
    pub fn max_bytes(&self) -> Option<usize> {
        self.cache.max_bytes()
    }

    /// Reads the memory budget again when its period has passed, and evicts
    /// entries when it has fallen.
    #[cfg(not(target_family = "wasm"))]
    fn follow_budget(&self) {
        let Some(budget) = &self.budget else {
            return;
        };
        if let Some(bytes) = budget.bytes_if_due(self.cache.bytes()) {
            self.cache
                .set_max_bytes(self.max_bytes.map_or(bytes, |most| most.min(bytes)));
        }
    }
}

impl<K, V> Store<K, V> for LruStore<K, V>
where
    K: Hash + Eq + Clone + Send + Sync,
    V: Clone + Send + Sync,
{
    type Reservation = Infallible;

    async fn get(&self, key: &K) -> Lookup<V, Infallible> {
        // Reading marks the entry most recently used. The clock is read
        // only for an entry with a time to be dropped at.
        match self.cache.get(key) {
            Some(kept) if kept.until.is_none_or(|until| self.clock.now() < until) => {
                Lookup::Hit(kept.entry)
            }
            Some(_) => {
                self.cache.remove(key);
                Lookup::Miss
            }
            None => Lookup::Miss,
        }
    }

    async fn try_get(&self, key: &K) -> Option<Entry<V>> {
        // The cache checks the entry is still fresh, so the time it would
        // be dropped at is left to `get`, and the clock is not read here.
        self.cache.get(key).map(|kept| kept.entry)
    }

    async fn put(&self, key: &K, entry: &Entry<V>, keep_for: Option<Duration>) {
        let now = self.clock.now();
        let kept = Kept {
            entry: entry.clone(),
            until: keep_for.and_then(|keep_for| now.checked_add(keep_for)),
        };
        self.cache.put(key.clone(), kept);
        #[cfg(not(target_family = "wasm"))]
        self.follow_budget();
    }

    async fn remove(&self, key: &K) {
        self.cache.remove(key);
    }
}

/// Builds an [`LruStore`].
pub struct LruStoreBuilder<K, V> {
    cache: CacheBuilder,
    clock: Option<Arc<dyn Clock>>,
    weigher: Option<Weigher<K, Kept<V>>>,
    max_bytes: Option<usize>,
    #[cfg(not(target_family = "wasm"))]
    budget: Option<MemoryBudget>,
    types: PhantomData<fn(K) -> V>,
}

impl<K, V> LruStoreBuilder<K, V>
where
    K: Hash + Eq + Clone + Send + Sync,
    V: Clone + Send + Sync,
{
    /// The most entries the store holds. Defaults to
    /// [`DEFAULT_SIZE`](crate::DEFAULT_SIZE).
    pub fn size(mut self, size: usize) -> Self {
        self.cache = self.cache.size(size);
        self
    }

    /// How many shards the entries are split into, each with its own lock.
    /// Defaults to the number of processors.
    pub fn concurrency(mut self, concurrency: usize) -> Self {
        self.cache = self.cache.concurrency(concurrency);
        self
    }

    /// The clock that decides when entries are dropped. Defaults to the
    /// system clock, which `wasm32-unknown-unknown` does not have.
    pub fn clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = Some(clock);
        self
    }

    /// Builds the store.
    ///
    /// # Panics
    ///
    /// On `wasm32-unknown-unknown` when no [clock](Self::clock) was given.
    pub fn build(self) -> LruStore<K, V> {
        let mut cache = self.cache.build();
        if let Some(weigher) = self.weigher {
            cache = cache.with_weigher(weigher);
        }
        #[allow(unused_mut)]
        let mut most = self.max_bytes;
        #[cfg(not(target_family = "wasm"))]
        if let Some(budget) = &self.budget {
            let bytes = budget.bytes_for(0);
            most = Some(most.map_or(bytes, |most| most.min(bytes)));
        }
        if let Some(most) = most {
            cache.set_max_bytes(most);
        }
        LruStore {
            cache,
            clock: clock::given_or_system(self.clock),
            #[cfg(not(target_family = "wasm"))]
            max_bytes: self.max_bytes,
            #[cfg(not(target_family = "wasm"))]
            budget: self.budget,
        }
    }
}

impl<K, V> LruStoreBuilder<K, V>
where
    K: Hash + Eq + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// How many bytes a value takes, for a store bounded by bytes. The store
    /// adds what each entry takes beyond its value. Without it, a store
    /// bounded by bytes counts only that, which suits values of one fixed
    /// size.
    ///
    /// ```
    /// use fiftyone_caching::LruStore;
    ///
    /// let store = LruStore::<String, Vec<u8>>::builder()
    ///     .weigh(|value: &Vec<u8>| value.len())
    ///     .max_bytes(64 * 1024 * 1024)
    ///     .build();
    /// assert!(store.max_bytes().is_some());
    /// ```
    pub fn weigh(mut self, weigh: impl Fn(&V) -> usize + Send + Sync + 'static) -> Self {
        let overhead = entry_overhead::<K, V>();
        self.weigher = Some(Arc::new(move |_: &K, kept: &Kept<V>| {
            weigh(&kept.entry.value).saturating_add(overhead)
        }));
        self
    }

    /// The most bytes the entries take. Once over it, the least recently
    /// used entries are evicted.
    pub fn max_bytes(mut self, max_bytes: usize) -> Self {
        self.max_bytes = Some(max_bytes);
        self.counting_bytes()
    }

    /// Bounds the entries by a budget that follows the machine's memory, so
    /// the store evicts entries when memory comes under pressure. A
    /// [`max_bytes`](Self::max_bytes) given too is never exceeded. Native
    /// targets only.
    #[cfg(not(target_family = "wasm"))]
    pub fn memory_budget(mut self, budget: MemoryBudget) -> Self {
        self.budget = Some(budget);
        self.counting_bytes()
    }

    /// Counts what each entry takes beyond its value when no
    /// [weigher](Self::weigh) was given.
    fn counting_bytes(mut self) -> Self {
        if self.weigher.is_none() {
            let overhead = entry_overhead::<K, V>();
            self.weigher = Some(Arc::new(move |_: &K, _: &Kept<V>| overhead));
        }
        self
    }
}
