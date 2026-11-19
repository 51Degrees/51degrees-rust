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

//! Stores of bytes, such as a platform's key-value store or cache, and the
//! adapter that keeps a cache's entries in one.

use std::fmt::{Display, Write};
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::clock::{self, Clock};
use super::codec::{decode_entry, encode_entry_published, read_header, Codec};
use super::entry::Entry;
use super::store::{Lookup, Store};

/// What a [`ByteStore`] found for a key.
#[derive(Debug)]
pub enum ByteLookup<R> {
    /// The bytes held for the key.
    Hit(Vec<u8>),
    /// Nothing held, and this caller is the one to fill the key. The store
    /// makes other callers wait until the reservation is filled or dropped.
    Reserved(R),
    /// Nothing held, and no reservation.
    Miss,
}

/// A platform store that keeps bytes under string keys, such as a
/// key-value store or a cache. [`EncodedStore`] makes one the
/// [`Store`] of a [`LoadingCache`](crate::LoadingCache), writing each entry
/// in the [entry format](crate::encode_entry).
///
/// The keys it is given are made by [`EncodedStore`] and hold only ASCII
/// letters, digits, `-`, `.`, `_`, `~` and `%`, after the namespace and a
/// `/` when one is set. A store that fails reports a miss or drops the
/// write, as a [`Store`] does.
pub trait ByteStore {
    /// Held by the caller the store chose to fill a missing key, as
    /// [`Store::Reservation`] is. A store that never makes callers wait uses
    /// [`std::convert::Infallible`].
    type Reservation;

    /// Whether the platform drops each entry by itself once the lifetime it
    /// was written with has passed, even if late. When it does not,
    /// [`EncodedStore`] removes an entry it reads past its drop time.
    const DROPS_EXPIRED: bool = true;

    /// The bytes held for `key`.
    fn get(&self, key: &str) -> impl Future<Output = ByteLookup<Self::Reservation>>;

    /// Writes `bytes` for `key`, to be kept for `keep_for`, or until evicted
    /// when `None`. A platform may round the lifetime up to the shortest it
    /// supports, because the entry's own drop time is checked on every read.
    fn put(
        &self,
        key: &str,
        bytes: Vec<u8>,
        keep_for: Option<Duration>,
    ) -> impl Future<Output = ()>;

    /// Writes the bytes a reservation was held for and releases the callers
    /// waiting on it. A store that never reserves can keep this default.
    fn fill(
        &self,
        key: &str,
        reservation: Self::Reservation,
        bytes: Vec<u8>,
        keep_for: Option<Duration>,
    ) -> impl Future<Output = ()> {
        drop(reservation);
        self.put(key, bytes, keep_for)
    }

    /// Removes the bytes held for `key`.
    fn remove(&self, key: &str) -> impl Future<Output = ()>;
}

/// A [`ByteStore`] that can list its keys, so [`EncodedStore::sweep`] can
/// remove the entries no one reads again.
pub trait ListKeys {
    /// Every key that starts with `prefix`, or `None` when the keys cannot
    /// be listed.
    fn keys(&self, prefix: &str) -> impl Future<Output = Option<Vec<String>>>;
}

/// A [`Store`] that keeps a cache's entries in a [`ByteStore`].
///
/// Each entry is written in the [entry format](crate::encode_entry), with
/// its value turned into bytes by a [`Codec`] and the time the store must
/// drop it alongside. That drop time is checked on every read, so an entry
/// is never returned after its `keep_for` even on a platform that deletes
/// late, keeps a lifetime no shorter than a minute, or has no lifetimes at
/// all. Bytes that are not an entry in a format this crate reads are a miss,
/// and the next write replaces them.
///
/// Each entry also holds the published date of the data its source used,
/// from [`EncodedStoreBuilder::published`], and the version of its codec's
/// format, from [`Codec::version`]. An entry whose date or version differs
/// from the current ones is a miss, and is removed, so a data update or an
/// upgrade never serves a value from older data or in an older format.
///
/// A key is written as its [`Display`] text with every byte other than an
/// ASCII letter, digit, `-`, `.`, `_` or `~` written as `%` and two
/// uppercase hex digits, behind the namespace and a `/` when one is set. Two
/// different keys therefore never share a platform key. A key longer than
/// the platform allows is a miss on every read and a dropped write, so key
/// by a digest when keys can be long.
pub struct EncodedStore<B, C> {
    bytes: B,
    codec: C,
    namespace: Option<String>,
    clock: Arc<dyn Clock>,
    published: Published,
}

/// Gives the published date of the data the source uses now.
type Published = Arc<dyn Fn() -> SystemTime + Send + Sync>;

impl<B, C> EncodedStore<B, C> {
    /// A store over `bytes` with values turned into bytes by `codec`, no
    /// namespace and the system clock.
    ///
    /// # Panics
    ///
    /// On `wasm32-unknown-unknown`, which has no system clock. Use
    /// [`EncodedStore::builder`] and give it a [`Clock`].
    pub fn new(bytes: B, codec: C) -> Self {
        Self::builder(bytes, codec).build()
    }

    /// Starts building a store over `bytes` with values turned into bytes by
    /// `codec`.
    pub fn builder(bytes: B, codec: C) -> EncodedStoreBuilder<B, C> {
        EncodedStoreBuilder {
            bytes,
            codec,
            namespace: None,
            clock: None,
            published: None,
        }
    }

    /// The store of bytes the entries are kept in.
    pub fn bytes(&self) -> &B {
        &self.bytes
    }

    /// The platform key `key` is kept under.
    pub fn key_for<K: Display>(&self, key: &K) -> String {
        let mut text = String::new();
        // Writing to a String does not fail.
        let _ = write!(text, "{key}");
        let mut out = String::with_capacity(
            self.namespace.as_ref().map_or(0, |ns| ns.len() + 1) + text.len(),
        );
        if let Some(namespace) = &self.namespace {
            out.push_str(namespace);
            out.push('/');
        }
        for byte in text.bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                out.push(char::from(byte));
            } else {
                let _ = write!(out, "%{byte:02X}");
            }
        }
        out
    }

    fn is_past(&self, drop_at: Option<SystemTime>) -> bool {
        drop_at.is_some_and(|drop_at| self.clock.now() >= drop_at)
    }
}

impl<B: ByteStore + ListKeys, C> EncodedStore<B, C> {
    /// Removes every entry under this store's namespace that is past its
    /// drop time, and returns how many were removed. For a platform with no
    /// lifetimes, an entry that is never read again stays until a sweep.
    ///
    /// Bytes that are not an entry in a format this crate reads are left
    /// alone, because they may belong to a newer release or to other code.
    pub async fn sweep(&self) -> usize {
        let prefix = self
            .namespace
            .as_ref()
            .map_or_else(String::new, |ns| format!("{ns}/"));
        let Some(keys) = self.bytes.keys(&prefix).await else {
            return 0;
        };
        let mut removed = 0;
        for key in keys {
            let ByteLookup::Hit(bytes) = self.bytes.get(&key).await else {
                continue;
            };
            let past = match read_header(&bytes) {
                Ok((header, _)) => self.is_past(header.drop_at),
                Err(_) => false,
            };
            if past {
                self.bytes.remove(&key).await;
                removed += 1;
            }
        }
        removed
    }
}

impl<K, V, B, C> Store<K, V> for EncodedStore<B, C>
where
    K: Display,
    B: ByteStore,
    C: Codec<V>,
{
    type Reservation = B::Reservation;

    async fn get(&self, key: &K) -> Lookup<V, B::Reservation> {
        let key = self.key_for(key);
        let bytes = match self.bytes.get(&key).await {
            ByteLookup::Hit(bytes) => bytes,
            ByteLookup::Reserved(reservation) => return Lookup::Reserved(reservation),
            ByteLookup::Miss => return Lookup::Miss,
        };
        // An entry from other data or in another value format is removed
        // before its value is decoded, which the codec may not accept.
        if read_header(&bytes).is_ok_and(|(header, _)| {
            header.published != (self.published)() || header.value_version != self.codec.version()
        }) {
            self.bytes.remove(&key).await;
            return Lookup::Miss;
        }
        match decode_entry(&self.codec, &bytes) {
            Ok(stored) if !self.is_past(stored.drop_at) => Lookup::Hit(stored.entry),
            Ok(_) => {
                if !B::DROPS_EXPIRED {
                    self.bytes.remove(&key).await;
                }
                Lookup::Miss
            }
            Err(_) => Lookup::Miss,
        }
    }

    async fn put(&self, key: &K, entry: &Entry<V>, keep_for: Option<Duration>) {
        let key = self.key_for(key);
        if let Some(bytes) = self.encode(entry, keep_for) {
            self.bytes.put(&key, bytes, keep_for).await;
        }
    }

    async fn fill(
        &self,
        key: &K,
        reservation: B::Reservation,
        entry: &Entry<V>,
        keep_for: Option<Duration>,
    ) {
        let key = self.key_for(key);
        match self.encode(entry, keep_for) {
            Some(bytes) => self.bytes.fill(&key, reservation, bytes, keep_for).await,
            // Dropping the reservation releases the callers waiting on it.
            None => drop(reservation),
        }
    }

    async fn remove(&self, key: &K) {
        self.bytes.remove(&self.key_for(key)).await;
    }
}

impl<B, C> EncodedStore<B, C> {
    fn encode<V>(&self, entry: &Entry<V>, keep_for: Option<Duration>) -> Option<Vec<u8>>
    where
        C: Codec<V>,
    {
        let drop_at = match keep_for {
            Some(keep_for) => Some(self.clock.now().checked_add(keep_for)?),
            None => None,
        };
        encode_entry_published(&self.codec, entry, drop_at, (self.published)())
    }
}

/// Builds an [`EncodedStore`].
pub struct EncodedStoreBuilder<B, C> {
    bytes: B,
    codec: C,
    namespace: Option<String>,
    clock: Option<Arc<dyn Clock>>,
    published: Option<Published>,
}

impl<B, C> EncodedStoreBuilder<B, C> {
    /// Puts every key behind `namespace` and a `/`, so several caches can
    /// share one platform store. The namespace is written as given, so keep
    /// to characters the platform accepts in a key.
    pub fn namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    /// The clock that sets and checks drop times. Defaults to the system
    /// clock, which `wasm32-unknown-unknown` does not have.
    pub fn clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = Some(clock);
        self
    }

    /// Gives the published date of the data the source uses now, such as a
    /// data file's, which each write keeps with the entry. A read answers
    /// absent for an entry kept with another date, and removes it. Called on
    /// every read and write, so keep it cheap. Defaults to the epoch, for a
    /// source with no published date.
    pub fn published(mut self, published: impl Fn() -> SystemTime + Send + Sync + 'static) -> Self {
        self.published = Some(Arc::new(published));
        self
    }

    /// Builds the store.
    ///
    /// # Panics
    ///
    /// On `wasm32-unknown-unknown` when no [clock](Self::clock) was given.
    pub fn build(self) -> EncodedStore<B, C> {
        EncodedStore {
            bytes: self.bytes,
            codec: self.codec,
            namespace: self.namespace,
            clock: clock::given_or_system(self.clock),
            published: self.published.unwrap_or_else(|| Arc::new(|| UNIX_EPOCH)),
        }
    }
}

/// A shared store of bytes, so several caches can use one.
impl<B: ByteStore> ByteStore for Arc<B> {
    type Reservation = B::Reservation;
    const DROPS_EXPIRED: bool = B::DROPS_EXPIRED;

    fn get(&self, key: &str) -> impl Future<Output = ByteLookup<Self::Reservation>> {
        (**self).get(key)
    }

    fn put(
        &self,
        key: &str,
        bytes: Vec<u8>,
        keep_for: Option<Duration>,
    ) -> impl Future<Output = ()> {
        (**self).put(key, bytes, keep_for)
    }

    fn fill(
        &self,
        key: &str,
        reservation: Self::Reservation,
        bytes: Vec<u8>,
        keep_for: Option<Duration>,
    ) -> impl Future<Output = ()> {
        (**self).fill(key, reservation, bytes, keep_for)
    }

    fn remove(&self, key: &str) -> impl Future<Output = ()> {
        (**self).remove(key)
    }
}

impl<B: ListKeys> ListKeys for Arc<B> {
    fn keys(&self, prefix: &str) -> impl Future<Output = Option<Vec<String>>> {
        (**self).keys(prefix)
    }
}
