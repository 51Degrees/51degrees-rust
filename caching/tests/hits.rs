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

//! A copy the store can give without waiting is served by that lookup
//! alone, with no part in the key's load.

mod common;

use std::convert::Infallible;
use std::future::poll_fn;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use common::*;
use fiftyone_caching::{Entry, LoadingCache, Lookup, LruStore, Store};

/// A store in memory that counts its lookups of each kind and its writes.
/// Its waiting lookup takes two polls, so callers polled in turn overlap.
struct Counting {
    inner: LruStore<u32, String>,
    quick: AtomicUsize,
    waiting: AtomicUsize,
    writes: AtomicUsize,
}

impl Counting {
    fn new(clock: &TestClock) -> Arc<Self> {
        Arc::new(Counting {
            inner: lru(clock, 100),
            quick: AtomicUsize::new(0),
            waiting: AtomicUsize::new(0),
            writes: AtomicUsize::new(0),
        })
    }

    /// The lookups that never wait, the lookups that may, and the writes.
    fn counts(&self) -> (usize, usize, usize) {
        (
            self.quick.load(Ordering::SeqCst),
            self.waiting.load(Ordering::SeqCst),
            self.writes.load(Ordering::SeqCst),
        )
    }
}

impl Store<u32, String> for Counting {
    type Reservation = Infallible;

    async fn get(&self, key: &u32) -> Lookup<String, Infallible> {
        self.waiting.fetch_add(1, Ordering::SeqCst);
        let mut polled = false;
        poll_fn(|cx| {
            if polled {
                Poll::Ready(())
            } else {
                polled = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        })
        .await;
        self.inner.get(key).await
    }

    async fn try_get(&self, key: &u32) -> Option<Entry<String>> {
        self.quick.fetch_add(1, Ordering::SeqCst);
        self.inner.try_get(key).await
    }

    async fn put(&self, key: &u32, entry: &Entry<String>, keep_for: Option<Duration>) {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.inner.put(key, entry, keep_for).await
    }

    async fn remove(&self, key: &u32) {
        self.inner.remove(key).await
    }
}

/// Four callers for key 1, polled in turn.
fn four_callers<'a, S, L>(
    cache: &'a LoadingCache<u32, String, S, L>,
) -> Vec<Boxed<'a, Result<String, L::Error>>>
where
    S: Store<u32, String>,
    L: fiftyone_caching::ValueLoader<u32, String>,
{
    (0..4).map(|_| boxed(cache.get(&1))).collect()
}

#[test]
fn a_hit_is_served_by_the_lookup_that_never_waits() {
    let clock = TestClock::new();
    let source = Source::new();
    let store = Counting::new(&clock);
    let cache = LoadingCache::builder(Arc::clone(&store), source.clone())
        .time_to_live(secs(60))
        .clock(clock.shared())
        .build();

    // The miss looks once before joining the key's load and once leading it.
    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 1".to_owned()));
    assert_eq!(store.counts(), (1, 1, 1));

    for _ in 0..5 {
        assert_eq!(block_on(cache.get(&1)), Ok("1 from load 1".to_owned()));
    }
    assert_eq!(store.counts(), (6, 1, 1));
    assert_eq!(source.loads(), 1);
}

#[test]
fn a_copy_written_meanwhile_is_served_while_the_key_is_still_loading() {
    let clock = TestClock::new();
    let source = Source::gated();
    let store = Arc::new(lru(&clock, 100));
    let cache = LoadingCache::builder(Arc::clone(&store), source.clone())
        .clock(clock.shared())
        .build();

    // The first caller misses and starts a load, which waits at the gate.
    let mut loading = boxed(cache.get(&1));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(loading.as_mut().poll(&mut cx).is_pending());
    assert_eq!(source.loads(), 1);

    // Another process writes the key to the store they share.
    let written = Entry {
        value: "1 written elsewhere".to_owned(),
        written: clock.now(),
        expires: None,
        renewed: clock.now(),
    };
    block_on(store.put(&1, &written, None));

    // The next caller is served that copy at once. Nothing polls the load
    // here, so a caller that waited for it would never finish.
    assert_eq!(
        block_on(cache.get(&1)),
        Ok("1 written elsewhere".to_owned())
    );

    source.gate.open();
    assert_eq!(block_on(loading), Ok("1 from load 1".to_owned()));
    assert_eq!(source.loads(), 1);
}

#[test]
fn a_copy_due_to_renew_is_renewed_once_however_many_callers_use_it() {
    let clock = TestClock::new();
    let source = Source::new();
    let store = Counting::new(&clock);
    let cache = LoadingCache::builder(Arc::clone(&store), source.clone())
        .time_to_idle(secs(60))
        .renewal_window(secs(15))
        .clock(clock.shared())
        .build();
    block_on(cache.get(&1)).unwrap();
    assert_eq!(store.counts(), (1, 1, 1));

    // Inside the renewal window a use writes nothing and joins no load.
    clock.advance_secs(10);
    for result in run_all(four_callers(&cache)) {
        assert_eq!(result, Ok("1 from load 1".to_owned()));
    }
    assert_eq!(store.counts(), (5, 1, 1));

    // Past it, the callers join the key's load, and the one leading it
    // writes the renewal for all of them.
    clock.advance_secs(10);
    for result in run_all(four_callers(&cache)) {
        assert_eq!(result, Ok("1 from load 1".to_owned()));
    }
    assert_eq!(store.counts(), (9, 2, 2));

    // The renewed copy is a plain hit again.
    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 1".to_owned()));
    assert_eq!(store.counts(), (10, 2, 2));
    assert_eq!(source.loads(), 1);
}

#[test]
fn a_copy_past_its_time_is_loaded_again_once_for_every_caller() {
    let clock = TestClock::new();
    let source = Source::new();
    let store = Counting::new(&clock);
    let cache = LoadingCache::builder(Arc::clone(&store), source.clone())
        .time_to_live(secs(60))
        .clock(clock.shared())
        .build();
    block_on(cache.get(&1)).unwrap();

    clock.advance_secs(61);
    for result in run_all(four_callers(&cache)) {
        assert_eq!(result, Ok("1 from load 2".to_owned()));
    }
    assert_eq!(source.loads(), 2);
    // Each caller looked without waiting, and the one that led looked again.
    assert_eq!(store.counts(), (5, 2, 2));
}

#[test]
fn a_store_that_only_answers_the_waiting_lookup_serves_hits_through_it() {
    let clock = TestClock::new();
    let source = Source::new();
    let store = Arc::new(WaitingStore::new(&clock));
    let cache = LoadingCache::builder(Arc::clone(&store), source.clone())
        .clock(clock.shared())
        .build();

    for _ in 0..3 {
        assert_eq!(block_on(cache.get(&1)), Ok("1 from load 1".to_owned()));
    }
    assert_eq!(source.loads(), 1);
}
