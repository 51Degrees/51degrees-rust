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

//! A store over Spin's key-value store, for `wasm32-wasip2` on Spin 4,
//! turned on by the `spin` feature. It uses spin-sdk 7, whose key-value
//! interface is asynchronous over WASI 0.3.
//!
//! [`KvStore`] is a [`ByteStore`], made the store of a
//! [`LoadingCache`](crate::LoadingCache) by
//! [`EncodedStore`](crate::EncodedStore).
//!
//! ```no_run
//! # async fn run() -> Result<(), spin_sdk::key_value::Error> {
//! use std::time::Duration;
//! use fiftyone_caching::{from_fn, spin::KvStore, EncodedStore, LoadingCache, Utf8};
//!
//! let store = EncodedStore::new(KvStore::open_default().await?, Utf8);
//! let loader = from_fn(|key: String| async move { Ok::<_, String>(format!("value of {key}")) });
//! let cache = LoadingCache::builder(store, loader)
//!     .time_to_live(Duration::from_secs(3600))
//!     .build();
//! # drop(cache);
//! # Ok(())
//! # }
//! ```

use std::convert::Infallible;
use std::time::Duration;

use ::spin_sdk::key_value::{Error, Store};

use crate::{ByteLookup, ByteStore, ListKeys};

/// A store over a Spin key-value store.
///
/// Spin's store keeps no lifetimes, so an entry stays until it is replaced
/// or removed. [`EncodedStore`](crate::EncodedStore) keeps each entry's
/// drop time beside its value, treats an entry past it as a miss and
/// removes it when read, and [`EncodedStore::sweep`](crate::EncodedStore::sweep)
/// removes the expired entries no one reads again. Key and value sizes
/// depend on the store the Spin application is configured with.
pub struct KvStore {
    store: Store,
}

impl KvStore {
    /// Opens the store with the given label, which the component must be
    /// allowed in the application manifest.
    pub async fn open(label: &str) -> Result<Self, Error> {
        Ok(Store::open(label).await?.into())
    }

    /// Opens the store labelled `default`.
    pub async fn open_default() -> Result<Self, Error> {
        Ok(Store::open_default().await?.into())
    }
}

impl From<Store> for KvStore {
    fn from(store: Store) -> Self {
        KvStore { store }
    }
}

impl ByteStore for KvStore {
    type Reservation = Infallible;
    const DROPS_EXPIRED: bool = false;

    async fn get(&self, key: &str) -> ByteLookup<Infallible> {
        match self.store.get(key).await {
            Ok(Some(bytes)) => ByteLookup::Hit(bytes),
            _ => ByteLookup::Miss,
        }
    }

    async fn put(&self, key: &str, bytes: Vec<u8>, _keep_for: Option<Duration>) {
        let _ = self.store.set(key, bytes).await;
    }

    async fn remove(&self, key: &str) {
        let _ = self.store.delete(key).await;
    }
}

impl ListKeys for KvStore {
    async fn keys(&self, prefix: &str) -> Option<Vec<String>> {
        let keys = self.store.get_keys().await.collect().await.ok()?;
        Some(
            keys.into_iter()
                .filter(|key| key.starts_with(prefix))
                .collect(),
        )
    }
}
