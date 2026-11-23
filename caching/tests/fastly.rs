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

//! The Fastly stores on a Fastly runtime. These run under Viceroy, Fastly's
//! local Compute runtime, with the settings in `tests/fastly.toml`:
//!
//! ```text
//! CARGO_TARGET_WASM32_WASIP1_RUNNER="viceroy run -C caching/tests/fastly.toml" \
//!     cargo test -p fiftyone-caching --target wasm32-wasip1 --features fastly
//! ```
//!
//! Every test uses its own keys, because the tests in a run share the
//! runtime's KV store and cache.

#![cfg(all(feature = "fastly", target_os = "wasi", target_env = "p1"))]

mod common;

use common::{block_on, lru, secs, Source, TestClock};
use fiftyone_caching::fastly::{CoreCache, KvStore};
use fiftyone_caching::{ByteLookup, ByteStore, EncodedStore, LoadingCache, Lookup, Store, Utf8};

fn kv() -> KvStore {
    KvStore::open("cache")
        .expect("the KV store opens")
        .expect("tests/fastly.toml declares the cache store")
}

fn hit(lookup: ByteLookup<impl Sized>) -> Option<Vec<u8>> {
    match lookup {
        ByteLookup::Hit(bytes) => Some(bytes),
        _ => None,
    }
}

#[test]
fn kv_store_writes_reads_and_removes_bytes() {
    let store = kv();
    block_on(store.put("kv-bytes", b"bytes".to_vec(), Some(secs(60))));
    assert_eq!(
        hit(block_on(store.get("kv-bytes"))),
        Some(b"bytes".to_vec())
    );
    block_on(store.remove("kv-bytes"));
    assert!(matches!(block_on(store.get("kv-bytes")), ByteLookup::Miss));
}

#[test]
fn kv_store_reports_a_missing_key_as_a_miss() {
    assert!(matches!(
        block_on(kv().get("kv-never-written")),
        ByteLookup::Miss
    ));
}

#[test]
fn kv_store_keeps_keys_with_characters_fastly_refuses() {
    let clock = TestClock::new();
    let store = EncodedStore::builder(kv(), Utf8)
        .namespace("kv-escaped")
        .clock(clock.shared())
        .build();
    let key = "a#b?c;d^e|f\ng".to_string();
    let entry = fiftyone_caching::Entry {
        value: "value".to_string(),
        written: clock.now(),
        expires: None,
        renewed: clock.now(),
    };
    block_on(store.put(&key, &entry, None));
    match block_on(Store::<String, String>::get(&store, &key)) {
        Lookup::Hit(read) => assert_eq!(read, entry),
        _ => panic!("the escaped key reads back"),
    }
}

#[test]
fn kv_store_misses_from_the_drop_time_before_fastly_deletes() {
    let clock = TestClock::new();
    let store = EncodedStore::builder(kv(), Utf8)
        .namespace("kv-drop")
        .clock(clock.shared())
        .build();
    let entry = fiftyone_caching::Entry {
        value: "value".to_string(),
        written: clock.now(),
        expires: None,
        renewed: clock.now(),
    };
    block_on(store.put(&1u32, &entry, Some(secs(60))));
    clock.advance_secs(60);
    assert!(matches!(
        block_on(Store::<u32, String>::get(&store, &1)),
        Lookup::Miss
    ));
    assert!(
        hit(block_on(store.bytes().get("kv-drop/1"))).is_some(),
        "Fastly still holds the bytes"
    );
}

#[test]
fn kv_store_serves_a_second_cache_without_a_load() {
    let clock = TestClock::new();
    let source = Source::new();
    let cache = || {
        let store = EncodedStore::builder(kv(), Utf8)
            .namespace("kv-shared")
            .clock(clock.shared())
            .build();
        LoadingCache::builder(store, source.clone())
            .time_to_live(secs(60))
            .clock(clock.shared())
            .build()
    };
    assert_eq!(block_on(cache().get(&1)), Ok("1 from load 1".to_string()));
    assert_eq!(block_on(cache().get(&1)), Ok("1 from load 1".to_string()));
    assert_eq!(source.loads(), 1);
    clock.advance_secs(60);
    assert_eq!(block_on(cache().get(&1)), Ok("1 from load 2".to_string()));
}

#[test]
fn core_cache_obliges_the_first_caller_and_serves_its_fill() {
    let cache = CoreCache::new();
    let ByteLookup::Reserved(obligation) = block_on(cache.get("core-fill")) else {
        panic!("a missing key obliges the caller to fill it");
    };
    block_on(cache.fill("core-fill", obligation, b"bytes".to_vec(), Some(secs(60))));
    assert_eq!(
        hit(block_on(cache.get("core-fill"))),
        Some(b"bytes".to_vec())
    );
}

#[test]
fn core_cache_passes_on_a_dropped_obligation() {
    let cache = CoreCache::new();
    let ByteLookup::Reserved(obligation) = block_on(cache.get("core-dropped")) else {
        panic!("a missing key obliges the caller to fill it");
    };
    drop(obligation);
    assert!(
        matches!(block_on(cache.get("core-dropped")), ByteLookup::Reserved(_)),
        "the next caller is obliged rather than left waiting"
    );
}

#[test]
fn core_cache_takes_a_write_with_no_obligation() {
    let cache = CoreCache::new();
    block_on(cache.put("core-put", b"bytes".to_vec(), Some(secs(60))));
    assert_eq!(
        hit(block_on(cache.get("core-put"))),
        Some(b"bytes".to_vec())
    );
}

#[test]
fn core_cache_purges_a_removed_key() {
    let cache = CoreCache::new();
    block_on(cache.put("core-removed", b"bytes".to_vec(), Some(secs(60))));
    block_on(cache.remove("core-removed"));
    assert_eq!(hit(block_on(cache.get("core-removed"))), None);
}

#[test]
fn caches_layered_over_both_stores_load_once() {
    let clock = TestClock::new();
    let source = Source::new();
    let layers = || {
        let kv_store = EncodedStore::builder(kv(), Utf8)
            .namespace("layered")
            .clock(clock.shared())
            .build();
        let kv_layer = LoadingCache::builder(kv_store, source.clone())
            .time_to_live(secs(600))
            .clock(clock.shared())
            .build();
        let core_store = EncodedStore::builder(CoreCache::new(), Utf8)
            .namespace("layered")
            .clock(clock.shared())
            .build();
        let core_layer = LoadingCache::builder(core_store, kv_layer)
            .time_to_live(secs(60))
            .clock(clock.shared())
            .build();
        LoadingCache::builder(lru(&clock, 100), core_layer)
            .time_to_live(secs(5))
            .clock(clock.shared())
            .build()
    };

    assert_eq!(block_on(layers().get(&1)), Ok("1 from load 1".to_string()));
    // A new instance has empty memory and reads the point of presence's
    // cache.
    assert_eq!(block_on(layers().get(&1)), Ok("1 from load 1".to_string()));
    assert_eq!(source.loads(), 1);
}
