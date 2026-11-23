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

//! The routes test.mjs calls. Each answers `ok` or says what went wrong,
//! apart from `/cached`, which answers the value it read.
//!
//! - `/kv` writes, reads and removes bytes.
//! - `/expired` reads an entry past its drop time, which must miss and be
//!   removed, since Spin's store keeps no lifetimes.
//! - `/sweep` sweeps a namespace holding an expired entry and two live ones.
//! - `/cached?key=` reads a key through a cache over the store, whose source
//!   answers with the time it loaded. Spin gives each request a new
//!   instance, so a second request reads the store rather than loading.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fiftyone_caching::spin::KvStore;
use fiftyone_caching::{
    from_fn, ByteLookup, ByteStore, Clock, EncodedStore, Entry, ListKeys, LoadingCache, Lookup,
    Store, Utf8,
};
use spin_sdk::http::{IntoResponse, Request};
use spin_sdk::http_service;

async fn store() -> KvStore {
    KvStore::open_default()
        .await
        .expect("the manifest gives the component the default store")
}

/// A store over the default store that reads the time as `at`.
async fn encoded(namespace: &str, at: SystemTime) -> EncodedStore<KvStore, Utf8> {
    let clock: Arc<dyn Clock> = Arc::new(move || at);
    EncodedStore::builder(store().await, Utf8)
        .namespace(namespace)
        .clock(clock)
        .build()
}

fn entry(value: &str, at: SystemTime) -> Entry<String> {
    Entry {
        value: value.to_string(),
        written: at,
        expires: None,
        renewed: at,
    }
}

async fn round_trip() -> String {
    let store = store().await;
    store
        .put("kv", b"bytes".to_vec(), Some(Duration::from_secs(60)))
        .await;
    match store.get("kv").await {
        ByteLookup::Hit(bytes) if bytes == b"bytes" => {}
        ByteLookup::Hit(bytes) => return format!("read back {bytes:?}"),
        _ => return "missed after the write".to_string(),
    }
    store.remove("kv").await;
    match store.get("kv").await {
        ByteLookup::Miss => "ok".to_string(),
        _ => "still there after the remove".to_string(),
    }
}

async fn expired() -> String {
    let now = SystemTime::now();
    let early = encoded("expired", now).await;
    early
        .put(&1u32, &entry("value", now), Some(Duration::from_secs(60)))
        .await;
    let late = encoded("expired", now + Duration::from_secs(61)).await;
    if !matches!(Store::<u32, String>::get(&late, &1).await, Lookup::Miss) {
        return "read an entry past its drop time".to_string();
    }
    match late.bytes().get("expired/1").await {
        ByteLookup::Miss => "ok".to_string(),
        _ => "left an expired entry in the store".to_string(),
    }
}

async fn sweep() -> String {
    let now = SystemTime::now();
    let early = encoded("sweep", now).await;
    early
        .put(&1u32, &entry("one", now), Some(Duration::from_secs(60)))
        .await;
    early
        .put(&2u32, &entry("two", now), Some(Duration::from_secs(600)))
        .await;
    early.put(&3u32, &entry("three", now), None).await;
    let late = encoded("sweep", now + Duration::from_secs(61)).await;
    let removed = late.sweep().await;
    let mut kept = late.bytes().keys("sweep/").await.unwrap_or_default();
    kept.sort();
    if removed == 1 && kept == ["sweep/2", "sweep/3"] {
        "ok".to_string()
    } else {
        format!("removed {removed}, kept {kept:?}")
    }
}

async fn cached(key: String) -> String {
    let store = EncodedStore::new(store().await, Utf8);
    let loader = from_fn(|key: String| async move {
        let at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Ok::<_, String>(format!("{key} loaded at {at}"))
    });
    let cache = LoadingCache::builder(store, loader)
        .time_to_live(Duration::from_secs(3600))
        .build();
    cache.get(&key).await.unwrap_or_else(|error| error)
}

#[http_service]
async fn handle(req: Request) -> impl IntoResponse {
    let key = req
        .uri()
        .query()
        .and_then(|query| query.strip_prefix("key="))
        .unwrap_or_default()
        .to_string();
    match req.uri().path() {
        "/kv" => round_trip().await,
        "/expired" => expired().await,
        "/sweep" => sweep().await,
        "/cached" => cached(key).await,
        _ => "not found".to_string(),
    }
}
