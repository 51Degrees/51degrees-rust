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

//! Lifetimes, renewal and eviction, on a clock the tests move rather than
//! by sleeping.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use common::*;
use fiftyone_caching::{from_fn, Entry, Loaded, LoadingCache, Lookup, Store};

#[test]
fn time_to_live_reloads_once_it_passes() {
    let clock = TestClock::new();
    let source = Source::new();
    let cache = LoadingCache::builder(lru(&clock, 100), source.clone())
        .time_to_live(secs(60))
        .clock(clock.shared())
        .build();

    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 1".to_owned()));
    clock.advance_secs(59);
    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 1".to_owned()));
    clock.advance_secs(2);
    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 2".to_owned()));
}

#[test]
fn time_to_idle_keeps_a_used_entry_and_drops_an_unused_one() {
    let clock = TestClock::new();
    let source = Source::new();
    let cache = LoadingCache::builder(lru(&clock, 100), source.clone())
        .time_to_idle(secs(60))
        .renewal_window(secs(15))
        .clock(clock.shared())
        .build();

    block_on(cache.get(&1)).unwrap();
    // Each use is within the idle time of the last, so the entry stays,
    // although it has now lived longer than the idle time.
    for _ in 0..4 {
        clock.advance_secs(50);
        assert_eq!(block_on(cache.get(&1)), Ok("1 from load 1".to_owned()));
    }
    clock.advance_secs(61);
    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 2".to_owned()));
}

#[test]
fn a_used_entry_is_renewed_at_most_once_per_window() {
    let clock = TestClock::new();
    let store = Arc::new(Recorded::new(lru(&clock, 100)));
    let cache = LoadingCache::builder(Arc::clone(&store), Source::new())
        .time_to_idle(secs(60))
        .renewal_window(secs(15))
        .clock(clock.shared())
        .build();

    block_on(cache.get(&1)).unwrap();
    assert_eq!(store.take_writes(), [(1, Some(secs(60)))]);

    // Forty uses a second apart renew the entry when 15 and 30 seconds
    // have passed since it was last written, and at no other use.
    for _ in 0..40 {
        clock.advance_secs(1);
        block_on(cache.get(&1)).unwrap();
    }
    assert_eq!(
        store.take_writes(),
        [(1, Some(secs(60))), (1, Some(secs(60)))]
    );
}

#[test]
fn renewal_never_takes_an_entry_past_its_time_to_live() {
    let clock = TestClock::new();
    let source = Source::new();
    let store = Arc::new(Recorded::new(lru(&clock, 100)));
    let cache = LoadingCache::builder(Arc::clone(&store), source.clone())
        .time_to_live(secs(100))
        .time_to_idle(secs(60))
        .renewal_window(secs(15))
        .clock(clock.shared())
        .build();

    block_on(cache.get(&1)).unwrap();
    clock.advance_secs(50);
    block_on(cache.get(&1)).unwrap();
    // Written for the idle time, then renewed for what is left of the time
    // to live, since that ends before the idle time would.
    assert_eq!(
        store.take_writes(),
        [(1, Some(secs(60))), (1, Some(secs(50)))]
    );

    // Renewing now could not keep the entry any longer, so nothing is
    // written.
    clock.advance_secs(45);
    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 1".to_owned()));
    assert_eq!(store.take_writes(), []);

    clock.advance_secs(6);
    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 2".to_owned()));
}

#[test]
fn an_unused_entry_leaves_the_store_by_itself() {
    let clock = TestClock::new();
    let store = Arc::new(lru(&clock, 100));
    let cache = LoadingCache::builder(Arc::clone(&store), Source::new())
        .time_to_idle(secs(60))
        .clock(clock.shared())
        .build();

    block_on(cache.get(&1)).unwrap();
    clock.advance_secs(59);
    assert!(matches!(block_on(store.get(&1)), Lookup::Hit(_)));
    // The store drops the entry once the time it was written with runs
    // out, without the cache asking.
    clock.advance_secs(2);
    assert!(matches!(block_on(store.get(&1)), Lookup::Miss));
    assert!(store.is_empty());
}

#[test]
fn the_lru_store_evicts_the_least_recently_used_entry() {
    let clock = TestClock::new();
    let store = lru(&clock, 2);
    let entry = |value: &str| Entry {
        value: value.to_owned(),
        written: clock.now(),
        expires: None,
        renewed: clock.now(),
    };

    block_on(store.put(&1, &entry("one"), None));
    block_on(store.put(&2, &entry("two"), None));
    // Using 1 leaves 2 as the least recently used.
    assert!(matches!(block_on(store.get(&1)), Lookup::Hit(_)));
    block_on(store.put(&3, &entry("three"), None));

    assert!(matches!(block_on(store.get(&2)), Lookup::Miss));
    assert!(matches!(block_on(store.get(&1)), Lookup::Hit(_)));
    assert!(matches!(block_on(store.get(&3)), Lookup::Hit(_)));
    assert_eq!(store.len(), 2);
}

#[test]
fn a_full_cache_reloads_the_least_recently_used_key() {
    let clock = TestClock::new();
    let source = Source::new();
    let cache = LoadingCache::builder(lru(&clock, 2), source.clone())
        .clock(clock.shared())
        .build();

    for key in [1, 2, 1, 3] {
        block_on(cache.get(&key)).unwrap();
    }
    assert_eq!(source.loads(), 3);
    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 1".to_owned()));
    assert_eq!(block_on(cache.get(&2)), Ok("2 from load 4".to_owned()));
}

#[test]
fn a_loaded_expiry_shortens_the_copy() {
    let clock = TestClock::new();
    let loads = Arc::new(AtomicUsize::new(0));
    let source = {
        let clock = clock.clone();
        let loads = Arc::clone(&loads);
        from_fn(move |key: u32| {
            let load = loads.fetch_add(1, Ordering::SeqCst) + 1;
            let expires = clock.now() + secs(10);
            async move {
                Ok::<_, String>(Loaded::new(format!("{key} from load {load}")).expires_at(expires))
            }
        })
    };
    let cache = LoadingCache::builder(lru(&clock, 100), source)
        .time_to_live(secs(60))
        .clock(clock.shared())
        .build();

    block_on(cache.get(&1)).unwrap();
    clock.advance_secs(9);
    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 1".to_owned()));
    clock.advance_secs(2);
    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 2".to_owned()));
}

#[test]
fn a_value_already_past_its_end_is_returned_but_not_stored() {
    let clock = TestClock::new();
    let loads = Arc::new(AtomicUsize::new(0));
    let source = {
        let clock = clock.clone();
        let loads = Arc::clone(&loads);
        from_fn(move |key: u32| {
            loads.fetch_add(1, Ordering::SeqCst);
            let expired = clock.now() - secs(1);
            async move { Ok::<_, String>(Loaded::new(key.to_string()).expires_at(expired)) }
        })
    };
    let store = Arc::new(lru(&clock, 100));
    let cache = LoadingCache::builder(Arc::clone(&store), source)
        .clock(clock.shared())
        .build();

    assert_eq!(block_on(cache.get(&1)), Ok("1".to_owned()));
    assert_eq!(block_on(cache.get(&1)), Ok("1".to_owned()));
    assert_eq!(loads.load(Ordering::SeqCst), 2);
    assert!(store.is_empty());
}

#[test]
fn a_store_that_never_drops_entries_still_serves_nothing_stale() {
    let clock = TestClock::new();
    let source = Source::new();
    let cache = LoadingCache::builder(KeepsEverything::default(), source.clone())
        .time_to_live(secs(10))
        .clock(clock.shared())
        .build();

    block_on(cache.get(&1)).unwrap();
    clock.advance_secs(11);
    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 2".to_owned()));
}

#[test]
fn remove_takes_a_key_out_of_the_store() {
    let clock = TestClock::new();
    let source = Source::new();
    let cache = LoadingCache::builder(lru(&clock, 100), source.clone())
        .clock(clock.shared())
        .build();

    block_on(cache.get(&1)).unwrap();
    block_on(cache.remove(&1));
    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 2".to_owned()));
}

#[test]
#[should_panic(expected = "renewal window")]
fn a_renewal_window_over_half_the_idle_time_is_refused() {
    let clock = TestClock::new();
    let _ = LoadingCache::builder(lru(&clock, 100), Source::new())
        .time_to_idle(secs(60))
        .renewal_window(secs(31))
        .clock(clock.shared())
        .build();
}
