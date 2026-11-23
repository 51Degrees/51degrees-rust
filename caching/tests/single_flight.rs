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

//! One load per key at a time, with many requests interleaved on one
//! thread. Every check runs once over the LruStore and once over a
//! store that makes callers wait, so the one cache is shown to work with
//! both kinds of store.

mod common;

use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};

use common::*;
use fiftyone_caching::{LoadingCache, Store};

const CALLERS: usize = 8;

fn cache<S: Store<u32, String>>(
    store: S,
    source: &Source,
    clock: &TestClock,
) -> LoadingCache<u32, String, S, Source> {
    LoadingCache::builder(store, source.clone())
        .clock(clock.shared())
        .build()
}

/// Every caller is polled once before the load may finish, so all of them
/// miss together. One load means they were collapsed.
fn concurrent_misses_share_one_load<S: Store<u32, String>>(make: fn(&TestClock) -> S) {
    let clock = TestClock::new();
    let source = Source::gated();
    let cache = cache(make(&clock), &source, &clock);

    let callers = (0..CALLERS).map(|_| boxed(cache.get(&1))).collect();
    let results = run_rounds(callers, |round, _| {
        if round == 1 {
            source.gate.open();
        }
    });

    assert_eq!(source.loads(), 1);
    for result in results {
        assert_eq!(result, Some(Ok("1 from load 1".to_owned())));
    }
}

/// A failed load reaches every waiting caller, is not stored, and the next
/// caller loads again.
fn a_failed_load_reaches_every_waiter_and_is_not_kept<S: Store<u32, String>>(
    make: fn(&TestClock) -> S,
) {
    let clock = TestClock::new();
    let source = Source::gated();
    source.fail(true);
    let cache = cache(make(&clock), &source, &clock);

    let callers = (0..CALLERS).map(|_| boxed(cache.get(&1))).collect();
    let results = run_rounds(callers, |round, _| {
        if round == 1 {
            source.gate.open();
        }
    });

    assert_eq!(source.loads(), 1);
    for result in results {
        assert_eq!(result, Some(Err("load 1 of 1 failed".to_owned())));
    }

    source.fail(false);
    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 2".to_owned()));
    assert_eq!(source.loads(), 2);
}

/// When the caller running the load is dropped, a waiting caller loads
/// instead, and every remaining caller gets that value.
fn a_cancelled_load_passes_to_a_waiting_caller<S: Store<u32, String>>(make: fn(&TestClock) -> S) {
    let clock = TestClock::new();
    let source = Source::gated();
    let cache = cache(make(&clock), &source, &clock);

    let callers = (0..3).map(|_| boxed(cache.get(&1))).collect();
    let results = run_rounds(callers, |round, callers| {
        if round == 1 {
            callers[0] = None;
            source.gate.open();
        }
    });

    assert_eq!(source.loads(), 2);
    assert_eq!(results[0], None);
    assert_eq!(results[1], Some(Ok("1 from load 2".to_owned())));
    assert_eq!(results[2], Some(Ok("1 from load 2".to_owned())));
}

/// Callers for different keys do not wait for each other.
fn different_keys_load_separately<S: Store<u32, String>>(make: fn(&TestClock) -> S) {
    let clock = TestClock::new();
    let source = Source::new();
    let cache = cache(make(&clock), &source, &clock);

    let keys = [1, 2, 3];
    let results = run_all(keys.iter().map(|key| boxed(cache.get(key))).collect());

    assert_eq!(source.loads(), 3);
    let mut results: Vec<String> = results.into_iter().map(Result::unwrap).collect();
    results.sort();
    assert_eq!(
        results,
        ["1 from load 1", "2 from load 2", "3 from load 3"].map(String::from)
    );
}

/// A value once loaded is served from the store.
fn a_stored_value_is_not_loaded_again<S: Store<u32, String>>(make: fn(&TestClock) -> S) {
    let clock = TestClock::new();
    let source = Source::new();
    let cache = cache(make(&clock), &source, &clock);

    for _ in 0..3 {
        assert_eq!(block_on(cache.get(&1)), Ok("1 from load 1".to_owned()));
    }
    assert_eq!(source.loads(), 1);
}

/// A caller that asks just as a failure lands starts a new load, because a
/// failure is not kept. A caller that was waiting before it landed still
/// receives it.
fn a_caller_asking_as_a_failure_lands_loads_again<S>(make: fn(&TestClock) -> S)
where
    S: Store<u32, String> + Send + Sync + 'static,
{
    let clock = TestClock::new();
    let source = Source::gated();
    source.fail_first(1);
    let cache = Arc::new(cache(make(&clock), &source, &clock));

    let [leader, asked, waiter] = ask_as_the_result_lands(&cache, &source);

    let failed = Some(Err("load 1 of 1 failed".to_owned()));
    assert_eq!(leader, failed);
    assert_eq!(asked, Some(Ok("1 from load 2".to_owned())));
    assert_eq!(waiter, failed);
    assert_eq!(source.loads(), 2);
}

/// A caller that asks just as a value lands reads it from the store, because
/// the value is written before it is published, so it is loaded once.
fn a_caller_asking_as_a_value_lands_reads_the_store<S>(make: fn(&TestClock) -> S)
where
    S: Store<u32, String> + Send + Sync + 'static,
{
    let clock = TestClock::new();
    let source = Source::gated();
    let cache = Arc::new(cache(make(&clock), &source, &clock));

    let results = ask_as_the_result_lands(&cache, &source);

    assert_eq!(
        results,
        [(); 3].map(|_| Some(Ok("1 from load 1".to_owned())))
    );
    assert_eq!(source.loads(), 1);
}

/// A waiter's waker that asks the cache for the key again from inside the
/// wake. A leader wakes its waiters as it publishes its result, so the call
/// arrives as the result lands, as a waiter woken on another thread can.
struct AskAgain<S: Store<u32, String>> {
    cache: Arc<LoadingCache<u32, String, S, Source>>,
    answer: Mutex<Option<Poll<Result<String, String>>>>,
}

impl<S> Wake for AskAgain<S>
where
    S: Store<u32, String> + Send + Sync + 'static,
{
    fn wake(self: Arc<Self>) {
        let mut call = boxed(self.cache.get(&1));
        let answer = call.as_mut().poll(&mut Context::from_waker(Waker::noop()));
        *self.answer.lock().unwrap() = Some(answer);
    }
}

/// Starts a load that waits at the source's gate and a waiter whose waker
/// asks again, opens the gate, and returns what the leader, the caller that
/// asked again and the waiter each received.
fn ask_as_the_result_lands<S>(
    cache: &Arc<LoadingCache<u32, String, S, Source>>,
    source: &Source,
) -> [Option<Result<String, String>>; 3]
where
    S: Store<u32, String> + Send + Sync + 'static,
{
    let ask_again = Arc::new(AskAgain {
        cache: Arc::clone(cache),
        answer: Mutex::new(None),
    });
    let asking = Waker::from(Arc::clone(&ask_again));
    let mut quiet = Context::from_waker(Waker::noop());
    let mut leader = boxed(cache.get(&1));
    let mut waiter = boxed(cache.get(&1));
    assert!(leader.as_mut().poll(&mut quiet).is_pending());
    assert!(waiter
        .as_mut()
        .poll(&mut Context::from_waker(&asking))
        .is_pending());

    source.gate.open();
    let leader = ready(leader.as_mut().poll(&mut quiet));
    let asked = ask_again.answer.lock().unwrap().take().and_then(ready);
    let waiter = ready(waiter.as_mut().poll(&mut quiet));
    [leader, asked, waiter]
}

fn ready<T>(poll: Poll<T>) -> Option<T> {
    match poll {
        Poll::Ready(value) => Some(value),
        Poll::Pending => None,
    }
}

fn lru_store(clock: &TestClock) -> fiftyone_caching::LruStore<u32, String> {
    lru(clock, 100)
}

/// Generates one test per store for a check.
macro_rules! for_each_store {
    ($check:ident) => {
        mod $check {
            use super::*;

            #[test]
            fn lru_store() {
                $check(super::lru_store);
            }

            #[test]
            fn waiting_store() {
                $check(WaitingStore::new);
            }
        }
    };
}

for_each_store!(concurrent_misses_share_one_load);
for_each_store!(a_failed_load_reaches_every_waiter_and_is_not_kept);
for_each_store!(a_cancelled_load_passes_to_a_waiting_caller);
for_each_store!(different_keys_load_separately);
for_each_store!(a_stored_value_is_not_loaded_again);
for_each_store!(a_caller_asking_as_a_failure_lands_loads_again);
for_each_store!(a_caller_asking_as_a_value_lands_reads_the_store);
