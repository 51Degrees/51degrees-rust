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

//! The cache trait surface.
//!
//! These traits are deliberately tiny so that an engine can swap in an
//! alternative cache (for example a distributed one) without depending on the
//! concrete [`crate::LruCache`].

use std::hash::Hash;

/// A read-only key-value cache.
///
/// This is the minimal surface an engine needs to look a value up. The default
/// implementation in this crate ([`crate::LruCache`]) is a sharded LRU cache,
/// but any type that can answer "is there a value for this key" can stand in.
///
/// The value type `V` is returned by value (cloned), not by reference, because
/// a sharded cache cannot safely hand out a borrow that outlives the per-shard
/// lock. This also matches the use-case: engines cache cheap-to-clone handles
/// such as an [`std::sync::Arc`] around their aspect data, or owned JSON.
pub trait Cache<K, V>: Send + Sync
where
    K: Hash + Eq,
    V: Clone,
{
    /// Look up the value stored under `key`, returning a clone if present.
    ///
    /// A hit on an LRU cache also marks the entry as most-recently-used, so
    /// reading takes `&self` rather than `&mut self` (the cache locks
    /// internally).
    fn get(&self, key: &K) -> Option<V>;

    /// The number of entries currently held across the whole cache.
    fn len(&self) -> usize;

    /// True if the cache holds no entries.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A cache that values can also be written into.
///
/// This extends [`Cache`] with [`PutCache::put`]. An engine writes the result
/// of a cache miss back through this so the next equivalent request is a hit.
pub trait PutCache<K, V>: Cache<K, V>
where
    K: Hash + Eq,
    V: Clone,
{
    /// Store `value` under `key`.
    ///
    /// If the key is already present its value is replaced and the entry is
    /// promoted to most-recently-used. Inserting a new entry may evict the
    /// least-recently-used entry from the relevant shard once it is full.
    fn put(&self, key: K, value: V);

    /// Remove every entry from the cache.
    ///
    /// The default implementation does nothing. That suits a backing whose
    /// entries are not owned by this process, for example a shared external
    /// key/value store where clearing from here is either unsupported or
    /// undesirable. The in-process [`crate::LruCache`] overrides this to drop
    /// all of its entries.
    fn clear(&self) {}
}
