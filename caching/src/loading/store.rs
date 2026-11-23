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

//! Where a cache keeps its entries.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use super::entry::Entry;

/// What a [`Store`] found for a key.
#[derive(Debug)]
pub enum Lookup<V, R> {
    /// An entry. The cache still checks it has not expired.
    Hit(Entry<V>),
    /// No entry, and this caller is the one to load it. The store makes other
    /// callers for the key wait until the reservation is filled or dropped.
    Reserved(R),
    /// No entry and no reservation. Either the store cannot make callers
    /// wait, or it waited for another caller's load and that load left no
    /// entry.
    Miss,
}

/// Storage for a [`crate::LoadingCache`], such as process memory or a
/// platform's key-value store or cache.
///
/// A store only keeps entries. It drops each one once the `keep_for` it was
/// written with runs out, or sooner when it needs the room. The cache does
/// everything else: one load per key at a time in its process, lifetimes,
/// renewal on use, and checking every entry it reads is still fresh, so a
/// store that drops entries late, or never, still gives correct results.
///
/// A store that can make callers in other processes wait for one load, as a
/// platform cache with request collapsing can, answers
/// [`Lookup::Reserved`] to the caller that should load and makes the others
/// wait inside [`Store::get`]. Any other store uses
/// [`std::convert::Infallible`] as its reservation and never answers
/// `Reserved`.
///
/// A store that fails reports a miss or drops the write, so a store that is
/// down slows requests rather than failing them. Log failures inside the
/// implementation.
pub trait Store<K, V> {
    /// Held by the caller a waiting store chose to load a value. Dropping it
    /// without filling must release the callers waiting on it, because a
    /// cancelled or failed load drops it.
    type Reservation;

    /// Looks up the entry for `key`.
    fn get(&self, key: &K) -> impl Future<Output = Lookup<V, Self::Reservation>>;

    /// The entry for `key` if the store can give it without waiting and
    /// without reserving the key's load. `None` when it holds no entry, or
    /// cannot tell without waiting.
    ///
    /// The cache asks this before a caller joins the key's load, and serves
    /// a fresh entry from it alone. It checks the entry is still fresh
    /// itself, so the store may answer with one it was due to drop. Answer
    /// it when a lookup costs little next to joining a load, as one in
    /// process memory does. The default answers `None`, so the callers for
    /// one key share one [`get`](Store::get), which suits a store whose
    /// lookup crosses a network or may wait.
    fn try_get(&self, key: &K) -> impl Future<Output = Option<Entry<V>>> {
        let _ = key;
        async { None }
    }

    /// Writes `entry`, keeping it for `keep_for`, or until evicted when
    /// `None`.
    fn put(
        &self,
        key: &K,
        entry: &Entry<V>,
        keep_for: Option<Duration>,
    ) -> impl Future<Output = ()>;

    /// Writes the entry a reservation was held for and releases the callers
    /// waiting on it. A store that never reserves can keep this default.
    fn fill(
        &self,
        key: &K,
        reservation: Self::Reservation,
        entry: &Entry<V>,
        keep_for: Option<Duration>,
    ) -> impl Future<Output = ()> {
        drop(reservation);
        self.put(key, entry, keep_for)
    }

    /// Removes the entry for `key`.
    fn remove(&self, key: &K) -> impl Future<Output = ()>;
}

/// A shared store, so several caches, or a test, can use one store.
impl<K, V, S: Store<K, V>> Store<K, V> for Arc<S> {
    type Reservation = S::Reservation;

    fn get(&self, key: &K) -> impl Future<Output = Lookup<V, Self::Reservation>> {
        (**self).get(key)
    }

    fn try_get(&self, key: &K) -> impl Future<Output = Option<Entry<V>>> {
        (**self).try_get(key)
    }

    fn put(
        &self,
        key: &K,
        entry: &Entry<V>,
        keep_for: Option<Duration>,
    ) -> impl Future<Output = ()> {
        (**self).put(key, entry, keep_for)
    }

    fn fill(
        &self,
        key: &K,
        reservation: Self::Reservation,
        entry: &Entry<V>,
        keep_for: Option<Duration>,
    ) -> impl Future<Output = ()> {
        (**self).fill(key, reservation, entry, keep_for)
    }

    fn remove(&self, key: &K) -> impl Future<Output = ()> {
        (**self).remove(key)
    }
}
