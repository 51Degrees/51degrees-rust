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

//! A store that makes callers in other processes wait for one load. Two
//! caches over one store stand for two processes, since each cache only
//! collapses the callers in its own process.

mod common;

use std::sync::Arc;

use common::*;
use fiftyone_caching::{LoadingCache, Store};

/// A cache standing for one process, over the store all processes share.
fn process<S: Store<u32, String>>(
    store: &Arc<S>,
    source: &Source,
    clock: &TestClock,
) -> LoadingCache<u32, String, Arc<S>, Source> {
    LoadingCache::builder(Arc::clone(store), source.clone())
        .clock(clock.shared())
        .build()
}

#[test]
fn a_store_that_waits_shares_one_load_across_processes() {
    let clock = TestClock::new();
    let source = Source::gated();
    let store = Arc::new(WaitingStore::new(&clock));
    let first = process(&store, &source, &clock);
    let second = process(&store, &source, &clock);

    let callers = (0..4)
        .map(|_| boxed(first.get(&1)))
        .chain((0..4).map(|_| boxed(second.get(&1))))
        .collect();
    let results = run_rounds(callers, |round, _| {
        if round == 1 {
            source.gate.open();
        }
    });

    for result in results {
        assert_eq!(result, Some(Ok("1 from load 1".to_owned())));
    }
    assert_eq!(source.loads(), 1);
    // Only the second process's loading caller waited in the store. Its
    // other callers waited for it inside the process.
    assert_eq!(store.waited(), 1);
}

#[test]
fn when_the_reserved_load_fails_the_waiting_process_loads_itself() {
    let clock = TestClock::new();
    let source = Source::gated();
    source.fail_first(1);
    let store = Arc::new(WaitingStore::new(&clock));
    let first = process(&store, &source, &clock);
    let second = process(&store, &source, &clock);

    let callers = (0..4)
        .map(|_| boxed(first.get(&1)))
        .chain((0..4).map(|_| boxed(second.get(&1))))
        .collect();
    let results = run_rounds(callers, |round, _| {
        if round == 1 {
            source.gate.open();
        }
    });

    for result in &results[..4] {
        assert_eq!(result, &Some(Err("load 1 of 1 failed".to_owned())));
    }
    for result in &results[4..] {
        assert_eq!(result, &Some(Ok("1 from load 2".to_owned())));
    }
    assert_eq!(source.loads(), 2);
}

#[test]
fn a_cancelled_reserved_load_releases_the_waiting_process() {
    let clock = TestClock::new();
    let source = Source::gated();
    let store = Arc::new(WaitingStore::new(&clock));
    let first = process(&store, &source, &clock);
    let second = process(&store, &source, &clock);

    let callers = vec![boxed(first.get(&1)), boxed(second.get(&1))];
    let results = run_rounds(callers, |round, callers| {
        if round == 1 {
            callers[0] = None;
            source.gate.open();
        }
    });

    assert_eq!(results[0], None);
    assert_eq!(results[1], Some(Ok("1 from load 2".to_owned())));
    assert_eq!(source.loads(), 2);
}

#[test]
fn a_store_that_cannot_wait_still_works() {
    let clock = TestClock::new();
    let source = Source::gated();
    let store = Arc::new(lru(&clock, 100));
    let first = process(&store, &source, &clock);
    let second = process(&store, &source, &clock);

    let callers = (0..4)
        .map(|_| boxed(first.get(&1)))
        .chain((0..4).map(|_| boxed(second.get(&1))))
        .collect();
    let results = run_rounds(callers, |round, _| {
        if round == 1 {
            source.gate.open();
        }
    });

    // Each process collapses its own callers into one load.
    assert_eq!(source.loads(), 2);
    for result in &results[..4] {
        assert_eq!(result, &results[0]);
    }
    for result in &results[4..] {
        assert_eq!(result, &results[4]);
    }
    assert!(results.iter().all(|result| matches!(result, Some(Ok(_)))));
}
