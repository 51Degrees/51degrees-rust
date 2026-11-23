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

//! Caches stacked by composition. A brief copy in memory loads from a cache
//! over a shared store, standing in for a platform key-value store, which
//! loads from the source.

mod common;

use std::sync::atomic::Ordering;
use std::sync::Arc;

use common::*;
use fiftyone_caching::{Entry, LoadingCache, Lookup, LruStore, Store};

type Memory = Arc<LruStore<u32, String>>;
type Shared = LoadingCache<u32, String, Memory, Source>;

/// A cache over the shared store, loading from the source.
fn shared(clock: &TestClock, store: &Memory, source: &Source) -> Shared {
    LoadingCache::builder(Arc::clone(store), source.clone())
        .time_to_live(secs(24 * 60 * 60))
        .time_to_idle(secs(60 * 60))
        .renewal_window(secs(15 * 60))
        .clock(clock.shared())
        .build()
}

fn entry_in(store: &Memory, key: u32) -> Option<Entry<String>> {
    match block_on(store.get(&key)) {
        Lookup::Hit(entry) => Some(entry),
        _ => None,
    }
}

#[test]
fn memory_over_store_over_source_loads_once_for_concurrent_misses() {
    let clock = TestClock::new();
    let source = Source::gated();
    let shared_store = Arc::new(lru(&clock, 1000));
    let (below, calls_below) = Counted::new(shared(&clock, &shared_store, &source));
    let memory_store = Arc::new(lru(&clock, 100));
    let cache = LoadingCache::builder(Arc::clone(&memory_store), below)
        .time_to_live(secs(5))
        .clock(clock.shared())
        .build();

    let callers = (0..8).map(|_| boxed(cache.get(&1))).collect();
    let results = run_rounds(callers, |round, _| {
        if round == 1 {
            source.gate.open();
        }
    });

    for result in results {
        assert_eq!(result, Some(Ok("1 from load 1".to_owned())));
    }
    assert_eq!(calls_below.load(Ordering::SeqCst), 1);
    assert_eq!(source.loads(), 1);
    assert!(entry_in(&shared_store, 1).is_some());
    assert!(entry_in(&memory_store, 1).is_some());
}

#[test]
fn memory_over_memory_over_store_loads_once_and_fills_every_layer() {
    let clock = TestClock::new();
    let source = Source::gated();
    let shared_store = Arc::new(lru(&clock, 1000));
    let (to_shared, calls_to_shared) = Counted::new(shared(&clock, &shared_store, &source));
    let middle_store = Arc::new(lru(&clock, 500));
    let middle = LoadingCache::builder(Arc::clone(&middle_store), to_shared)
        .time_to_live(secs(60))
        .clock(clock.shared())
        .build();
    let (to_middle, calls_to_middle) = Counted::new(middle);
    let top_store = Arc::new(lru(&clock, 100));
    let top = LoadingCache::builder(Arc::clone(&top_store), to_middle)
        .time_to_live(secs(5))
        .clock(clock.shared())
        .build();

    let callers = (0..8).map(|_| boxed(top.get(&1))).collect();
    let results = run_rounds(callers, |round, _| {
        if round == 1 {
            source.gate.open();
        }
    });

    for result in results {
        assert_eq!(result, Some(Ok("1 from load 1".to_owned())));
    }
    assert_eq!(calls_to_middle.load(Ordering::SeqCst), 1);
    assert_eq!(calls_to_shared.load(Ordering::SeqCst), 1);
    assert_eq!(source.loads(), 1);
    for store in [&shared_store, &middle_store, &top_store] {
        assert!(entry_in(store, 1).is_some());
    }

    // The top copy ends first. The middle copy still holds the value.
    clock.advance_secs(6);
    assert_eq!(block_on(top.get(&1)), Ok("1 from load 1".to_owned()));
    assert_eq!(calls_to_middle.load(Ordering::SeqCst), 2);
    assert_eq!(calls_to_shared.load(Ordering::SeqCst), 1);
}

#[test]
fn a_hit_below_fills_the_layer_above_with_the_original_write_time() {
    let clock = TestClock::new();
    let source = Source::new();
    let shared_store = Arc::new(lru(&clock, 1000));
    let below = Arc::new(shared(&clock, &shared_store, &source));
    let written = clock.now();
    block_on(below.get(&1)).unwrap();

    clock.advance_secs(100);
    let memory_store = Arc::new(lru(&clock, 100));
    let cache = LoadingCache::builder(Arc::clone(&memory_store), Arc::clone(&below))
        .time_to_live(secs(5))
        .clock(clock.shared())
        .build();
    let loaded = block_on(cache.get_loaded(&1)).unwrap();

    assert_eq!(source.loads(), 1);
    assert_eq!(loaded.written, Some(written));
    let copy = entry_in(&memory_store, 1).unwrap();
    assert_eq!(copy.written, written);
    assert_eq!(copy.expires, Some(clock.now() + secs(5)));
}

#[test]
fn a_copy_never_outlives_the_copy_below() {
    let clock = TestClock::new();
    let source = Source::new();
    let below = LoadingCache::builder(lru(&clock, 1000), source.clone())
        .time_to_live(secs(10))
        .clock(clock.shared())
        .build();
    let memory_store = Arc::new(lru(&clock, 100));
    let cache = LoadingCache::builder(Arc::clone(&memory_store), below)
        .time_to_live(secs(60))
        .clock(clock.shared())
        .build();

    block_on(cache.get(&1)).unwrap();
    let copy = entry_in(&memory_store, 1).unwrap();
    assert_eq!(copy.expires, Some(clock.now() + secs(10)));

    clock.advance_secs(11);
    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 2".to_owned()));
}

#[test]
fn a_copy_comes_back_below_before_the_copy_below_needs_renewing() {
    let clock = TestClock::new();
    let source = Source::new();
    let below = LoadingCache::builder(lru(&clock, 1000), source.clone())
        .time_to_idle(secs(60))
        .renewal_window(secs(15))
        .clock(clock.shared())
        .build();
    let memory_store = Arc::new(lru(&clock, 100));
    // No lifetime of its own, so only the copy below limits it.
    let cache = LoadingCache::builder(Arc::clone(&memory_store), below)
        .clock(clock.shared())
        .build();

    block_on(cache.get(&1)).unwrap();
    let copy = entry_in(&memory_store, 1).unwrap();
    assert_eq!(copy.expires, Some(clock.now() + secs(45)));

    // Used every second for ten minutes, the key keeps coming back to the
    // copy below in time to renew it, so the source is asked once.
    for _ in 0..600 {
        clock.advance_secs(1);
        assert_eq!(block_on(cache.get(&1)), Ok("1 from load 1".to_owned()));
    }
    assert_eq!(source.loads(), 1);
}

#[test]
fn a_busy_key_keeps_its_shared_copy_and_an_idle_one_loses_it() {
    let clock = TestClock::new();
    let source = Source::new();
    let shared_store = Arc::new(lru(&clock, 1000));
    let cache = LoadingCache::builder(lru(&clock, 100), shared(&clock, &shared_store, &source))
        .time_to_live(secs(5))
        .clock(clock.shared())
        .build();

    // Four hours of use every few seconds, far beyond the shared copy's one
    // hour idle time.
    for _ in 0..(4 * 60 * 60 / 4) {
        clock.advance_secs(4);
        assert_eq!(block_on(cache.get(&1)), Ok("1 from load 1".to_owned()));
    }
    assert_eq!(source.loads(), 1);

    // Unused for longer than the idle time, the key leaves the shared store.
    clock.advance_secs(60 * 60 + 1);
    assert!(entry_in(&shared_store, 1).is_none());
    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 2".to_owned()));
}

#[test]
fn a_changed_store_value_shows_once_the_memory_copy_expires() {
    let clock = TestClock::new();
    let source = Source::new();
    let shared_store = Arc::new(lru(&clock, 1000));
    let cache = LoadingCache::builder(lru(&clock, 100), shared(&clock, &shared_store, &source))
        .time_to_live(secs(5))
        .clock(clock.shared())
        .build();
    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 1".to_owned()));

    // Another process writes a new value to the shared store.
    let changed = Entry {
        value: "changed elsewhere".to_owned(),
        written: clock.now(),
        expires: Some(clock.now() + secs(60 * 60)),
        renewed: clock.now(),
    };
    block_on(shared_store.put(&1, &changed, Some(secs(60 * 60))));

    clock.advance_secs(4);
    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 1".to_owned()));
    clock.advance_secs(2);
    assert_eq!(block_on(cache.get(&1)), Ok("changed elsewhere".to_owned()));
    assert_eq!(source.loads(), 1);
}

#[test]
fn a_removed_store_value_is_reloaded_once_the_memory_copy_expires() {
    let clock = TestClock::new();
    let source = Source::new();
    let shared_store = Arc::new(lru(&clock, 1000));
    let cache = LoadingCache::builder(lru(&clock, 100), shared(&clock, &shared_store, &source))
        .time_to_live(secs(5))
        .clock(clock.shared())
        .build();
    block_on(cache.get(&1)).unwrap();

    // Another process purges the key from the shared store.
    block_on(shared_store.remove(&1));

    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 1".to_owned()));
    clock.advance_secs(6);
    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 2".to_owned()));
}

#[test]
fn a_failure_below_reaches_every_caller_above_and_is_not_kept() {
    let clock = TestClock::new();
    let source = Source::gated();
    source.fail(true);
    let shared_store = Arc::new(lru(&clock, 1000));
    let memory_store = Arc::new(lru(&clock, 100));
    let cache = LoadingCache::builder(
        Arc::clone(&memory_store),
        shared(&clock, &shared_store, &source),
    )
    .time_to_live(secs(5))
    .clock(clock.shared())
    .build();

    let callers = (0..8).map(|_| boxed(cache.get(&1))).collect();
    let results = run_rounds(callers, |round, _| {
        if round == 1 {
            source.gate.open();
        }
    });

    for result in results {
        assert_eq!(result, Some(Err("load 1 of 1 failed".to_owned())));
    }
    assert_eq!(source.loads(), 1);
    assert!(memory_store.is_empty());
    assert!(shared_store.is_empty());

    source.fail(false);
    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 2".to_owned()));
}
