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

//! Stores over Fastly Compute's KV store and core cache, for
//! `wasm32-wasip1`, turned on by the `fastly` feature.
//!
//! [`KvStore`] is global and durable, so it suits the cache of record.
//! [`CoreCache`] is the cache of the point of presence serving the request.
//! It makes concurrent callers there wait for one load, so it suits a layer
//! between process memory and the KV store. Each is a
//! [`ByteStore`](crate::ByteStore), made the store of a
//! [`LoadingCache`](crate::LoadingCache) by
//! [`EncodedStore`](crate::EncodedStore).
//!
//! ```no_run
//! use std::time::Duration;
//! use fiftyone_caching::{fastly, from_fn, EncodedStore, LoadingCache, Utf8};
//!
//! let kv = fastly::KvStore::open("cache")
//!     .ok()
//!     .flatten()
//!     .expect("the service links a KV store named cache");
//! let loader = from_fn(|key: String| async move { Ok::<_, String>(format!("value of {key}")) });
//! let cache = LoadingCache::builder(EncodedStore::new(kv, Utf8), loader)
//!     .time_to_live(Duration::from_secs(3600))
//!     .build();
//! # drop(cache);
//! ```

use std::time::Duration;

/// The lifetime given to an entry written with no lifetime, where the
/// platform needs one.
pub(crate) const LONGEST: Duration = Duration::from_secs(365 * 24 * 60 * 60);

/// The KV store time to live for an entry kept for `keep_for`. The platform
/// takes whole seconds, rounding down, and deletes up to a day late, so the
/// time is rounded up to at least a second and the entry's drop time,
/// checked on every read, holds the exact lifetime.
pub(crate) fn kv_time_to_live(keep_for: Option<Duration>) -> Option<Duration> {
    let keep_for = keep_for?;
    let secs = keep_for
        .as_secs()
        .saturating_add(u64::from(keep_for.subsec_nanos() > 0))
        .clamp(1, u64::from(u32::MAX));
    Some(Duration::from_secs(secs))
}

/// The core cache max age for an entry kept for `keep_for`.
pub(crate) fn max_age(keep_for: Option<Duration>) -> Duration {
    keep_for.unwrap_or(LONGEST)
}

/// What a core cache lookup gives the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Answer {
    /// The caller is obliged to fill the key, so it gets a reservation.
    Fill,
    /// A fresh item to read.
    Read,
    /// Nothing to read and nothing to fill.
    Miss,
}

/// Decides a core cache lookup. `found` is `None` when nothing was found,
/// and otherwise whether the item is fresh and usable. An obligation always
/// means fill, including when a stale item came with it, so the one caller
/// the platform chose loads the value and the others wait for it.
pub(crate) fn answer(obliged: bool, found: Option<bool>) -> Answer {
    match (obliged, found) {
        (true, _) => Answer::Fill,
        (false, Some(true)) => Answer::Read,
        (false, _) => Answer::Miss,
    }
}

/// The surrogate key each item is written with, so removing the key can
/// purge it. A 64-bit FNV-1a hash of the key, in hex. Two keys sharing a
/// hash only means removing one also purges the other.
pub(crate) fn surrogate_key(key: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in key.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(all(feature = "fastly", target_os = "wasi", target_env = "p1"))]
pub use binding::{CoreCache, KvStore, Obligation};

#[cfg(all(feature = "fastly", target_os = "wasi", target_env = "p1"))]
mod binding {
    use std::convert::Infallible;
    use std::io::Write;
    use std::time::Duration;

    use ::fastly::cache::core::{self, CacheError, CacheKey, Transaction};
    use ::fastly::http::body::StreamingBody;
    use ::fastly::kv_store::{KVStore, KVStoreError};

    use super::{answer, kv_time_to_live, max_age, surrogate_key, Answer};
    use crate::{ByteLookup, ByteStore};

    /// A store over a Fastly KV store, global and durable.
    ///
    /// Each entry is written with a time to live of its lifetime in whole
    /// seconds, rounded up. Fastly deletes an item up to a day after that,
    /// so [`EncodedStore`](crate::EncodedStore) checks each entry's own drop
    /// time when it reads it. A key takes at most one write a second, and a
    /// write over that limit is dropped. Keys are up to 1,024 bytes.
    pub struct KvStore {
        store: KVStore,
    }

    impl KvStore {
        /// Opens the KV store linked to the service under `name`, or `None`
        /// when no store has that name.
        pub fn open(name: &str) -> Result<Option<Self>, KVStoreError> {
            Ok(KVStore::open(name)?.map(Self::from))
        }
    }

    impl From<KVStore> for KvStore {
        fn from(store: KVStore) -> Self {
            KvStore { store }
        }
    }

    impl ByteStore for KvStore {
        type Reservation = Infallible;

        async fn get(&self, key: &str) -> ByteLookup<Infallible> {
            match self.store.lookup(key) {
                Ok(mut found) => ByteLookup::Hit(found.take_body_bytes()),
                Err(_) => ByteLookup::Miss,
            }
        }

        async fn put(&self, key: &str, bytes: Vec<u8>, keep_for: Option<Duration>) {
            let mut insert = self.store.build_insert();
            if let Some(time_to_live) = kv_time_to_live(keep_for) {
                insert = insert.time_to_live(time_to_live);
            }
            let _ = insert.execute(key, bytes);
        }

        async fn remove(&self, key: &str) {
            let _: Result<(), KVStoreError> = self.store.delete(key);
        }
    }

    /// A store over the core cache of the Fastly point of presence serving
    /// the request.
    ///
    /// A lookup that finds nothing obliges one caller in the point of
    /// presence to fill the key, and that caller gets an [`Obligation`] as
    /// its reservation. Other callers wait inside the lookup until it is
    /// filled, or dropped, when one of them is obliged instead. A stale item
    /// that comes with an obligation is treated the same way, so the cache
    /// above loads a fresh value rather than reading the stale one.
    ///
    /// A lookup blocks the instance while it waits, so share one
    /// [`LoadingCache`](crate::LoadingCache) over this store within an
    /// instance. Its single flight then keeps a second lookup of the same
    /// key from waiting on a fill the same instance owes.
    ///
    /// Each item is written with its lifetime as its max age, or a year when
    /// it has none, and a surrogate key from a hash of its key, which
    /// [`remove`](ByteStore::remove) purges.
    #[derive(Debug, Clone, Copy, Default)]
    pub struct CoreCache {
        wait_at_most: Option<Duration>,
    }

    impl CoreCache {
        /// A store over the core cache, whose lookups wait as long as the
        /// platform lets them for another caller's fill.
        pub fn new() -> Self {
            CoreCache::default()
        }

        /// Stops a lookup waiting for another caller's fill after `timeout`,
        /// when it reports a miss and the cache above loads the value
        /// itself.
        pub fn wait_at_most(mut self, timeout: Duration) -> Self {
            self.wait_at_most = Some(timeout);
            self
        }
    }

    /// The duty to fill a key in the core cache. Dropping it unfilled
    /// passes the duty to one of the callers waiting for the key.
    pub struct Obligation {
        transaction: Transaction,
    }

    impl ByteStore for CoreCache {
        type Reservation = Obligation;

        async fn get(&self, key: &str) -> ByteLookup<Obligation> {
            let mut lookup = Transaction::lookup(cache_key(key));
            if let Some(timeout) = self.wait_at_most {
                lookup = lookup.timeout(timeout);
            }
            let Ok(transaction) = lookup.execute() else {
                return ByteLookup::Miss;
            };
            let found = transaction.found();
            let fresh = found
                .as_ref()
                .map(|found| found.is_usable() && !found.is_stale());
            match answer(transaction.must_insert_or_update(), fresh) {
                Answer::Fill => ByteLookup::Reserved(Obligation { transaction }),
                Answer::Read => match found.map(|found| found.to_stream()) {
                    Some(Ok(body)) => ByteLookup::Hit(body.into_bytes()),
                    _ => ByteLookup::Miss,
                },
                Answer::Miss => ByteLookup::Miss,
            }
        }

        async fn put(&self, key: &str, bytes: Vec<u8>, keep_for: Option<Duration>) {
            let surrogate = surrogate_key(key);
            let writer = core::insert(cache_key(key), max_age(keep_for))
                .surrogate_keys([surrogate.as_str()])
                .known_length(bytes.len() as u64)
                .execute();
            write_all(writer, &bytes);
        }

        async fn fill(
            &self,
            key: &str,
            obligation: Obligation,
            bytes: Vec<u8>,
            keep_for: Option<Duration>,
        ) {
            let surrogate = surrogate_key(key);
            let writer = obligation
                .transaction
                .insert(max_age(keep_for))
                .surrogate_keys([surrogate.as_str()])
                .known_length(bytes.len() as u64)
                .execute();
            write_all(writer, &bytes);
        }

        async fn remove(&self, key: &str) {
            let _ = ::fastly::http::purge::purge_surrogate_key(&surrogate_key(key));
        }
    }

    fn cache_key(key: &str) -> CacheKey {
        CacheKey::copy_from_slice(key.as_bytes())
    }

    /// Writes the item's bytes. On any failure the body is dropped
    /// unfinished, which abandons the item.
    fn write_all(writer: Result<StreamingBody, CacheError>, bytes: &[u8]) {
        if let Ok(mut body) = writer {
            if body.write_all(bytes).is_ok() {
                let _ = body.finish();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_the_kv_time_to_live_up_to_whole_seconds() {
        let ms = Duration::from_millis;
        assert_eq!(kv_time_to_live(None), None);
        assert_eq!(kv_time_to_live(Some(ms(1))), Some(Duration::from_secs(1)));
        assert_eq!(
            kv_time_to_live(Some(ms(1000))),
            Some(Duration::from_secs(1))
        );
        assert_eq!(
            kv_time_to_live(Some(ms(1001))),
            Some(Duration::from_secs(2))
        );
        assert_eq!(
            kv_time_to_live(Some(Duration::from_secs(u64::MAX))),
            Some(Duration::from_secs(u64::from(u32::MAX)))
        );
    }

    #[test]
    fn gives_an_entry_with_no_lifetime_a_year() {
        assert_eq!(max_age(None), LONGEST);
        assert_eq!(
            max_age(Some(Duration::from_secs(5))),
            Duration::from_secs(5)
        );
    }

    #[test]
    fn fills_whenever_obliged_even_over_a_stale_item() {
        assert_eq!(answer(true, None), Answer::Fill);
        assert_eq!(answer(true, Some(false)), Answer::Fill);
        assert_eq!(answer(true, Some(true)), Answer::Fill);
    }

    #[test]
    fn reads_only_a_fresh_item_when_not_obliged() {
        assert_eq!(answer(false, Some(true)), Answer::Read);
        assert_eq!(answer(false, Some(false)), Answer::Miss);
        assert_eq!(answer(false, None), Answer::Miss);
    }

    #[test]
    fn hashes_keys_to_stable_surrogate_keys() {
        // Published FNV-1a 64-bit test vectors.
        assert_eq!(surrogate_key(""), "cbf29ce484222325");
        assert_eq!(surrogate_key("a"), "af63dc4c8601ec8c");
        assert_eq!(surrogate_key("foobar"), "85944171f73967e8");
        assert_ne!(surrogate_key("ns/1"), surrogate_key("ns/2"));
    }
}
