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

//! Concurrency tests for the sharded LRU cache: that concurrent writers and
//! readers do not lose data, and that contention on a small shared key space
//! stays consistent without panicking or deadlocking.
//!
//! These spawn OS threads, so the whole file is compiled only off wasm. The
//! `wasm32-wasip1` target is single-threaded and `std::thread::spawn` traps
//! there, which would abort the entire test binary. The single-threaded
//! behavioural suite in `lru_cache.rs` is what verifies the cache logic on
//! wasm; this file adds the thread-safety coverage that only makes sense, and
//! can only run, on a threaded target.
#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;
use std::thread;

use fiftyone_caching::{Cache, CacheBuilder, LruCache, PutCache};

#[test]
fn concurrent_writers_and_readers_do_not_lose_data() {
    let cache: Arc<LruCache<u64, u64>> =
        Arc::new(CacheBuilder::new().size(100_000).concurrency(8).build());

    let mut handles = Vec::new();
    for t in 0..8u64 {
        let cache = Arc::clone(&cache);
        handles.push(thread::spawn(move || {
            let base = t * 10_000;
            for i in 0..10_000u64 {
                cache.put(base + i, base + i);
            }
            // Each thread reads back its own keys, which no other thread wrote.
            for i in 0..10_000u64 {
                assert_eq!(cache.get(&(base + i)), Some(base + i));
            }
        }));
    }
    for h in handles {
        h.join().expect("worker thread panicked");
    }
    // Total written (80_000) is under capacity (100_000) so nothing evicted.
    assert_eq!(cache.len(), 80_000);
}

#[test]
fn concurrent_contention_on_same_keys_stays_consistent() {
    // Every thread hammers the same small key space. The cache must not panic
    // or deadlock, and afterwards every key still maps to one of the written
    // values.
    let cache: Arc<LruCache<u64, u64>> =
        Arc::new(CacheBuilder::new().size(64).concurrency(4).build());
    let mut handles = Vec::new();
    for t in 0..8u64 {
        let cache = Arc::clone(&cache);
        handles.push(thread::spawn(move || {
            for _ in 0..20_000u64 {
                for k in 0..16u64 {
                    cache.put(k, t);
                    let _ = cache.get(&k);
                }
            }
        }));
    }
    for h in handles {
        h.join().expect("worker thread panicked");
    }
    for k in 0..16u64 {
        if let Some(v) = cache.get(&k) {
            assert!(v < 8, "value for key {k} was never written");
        }
    }
}
