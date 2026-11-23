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

//! One load per key across threads. These spawn threads, so they are
//! compiled only off WebAssembly, where the single-threaded tests cover the
//! same logic.
#![cfg(not(target_family = "wasm"))]

mod common;

use std::future::{poll_fn, Future};
use std::pin::pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};
use std::time::{Duration, Instant};

use common::*;
use fiftyone_caching::{from_fn, LoadingCache, LruStore};

/// Wakes a parked thread, and records that it did.
struct Unpark {
    thread: Thread,
    woken: AtomicBool,
}

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.woken.store(true, Ordering::SeqCst);
        self.thread.unpark();
    }
}

/// Runs a future on this thread, parking until it is woken. A future that
/// is never woken would hang, so waiting long without a wake fails the test
/// instead.
fn block_on_parking<F: Future>(future: F) -> F::Output {
    block_on_parking_then(future, || {})
}

/// As [`block_on_parking`], calling `after_first_poll` once the future has
/// been polled once. A call to the cache has joined or started the load by
/// then.
fn block_on_parking_then<F: Future>(future: F, after_first_poll: impl FnOnce()) -> F::Output {
    let mut after_first_poll = Some(after_first_poll);
    let unpark = Arc::new(Unpark {
        thread: thread::current(),
        woken: AtomicBool::new(false),
    });
    let waker = Waker::from(Arc::clone(&unpark));
    let mut cx = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        unpark.woken.store(false, Ordering::SeqCst);
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
        if let Some(after_first_poll) = after_first_poll.take() {
            after_first_poll();
        }
        // Parking can end without a wake, so only the deadline passing
        // without one is a failure.
        let deadline = Instant::now() + Duration::from_secs(30);
        while !unpark.woken.load(Ordering::SeqCst) {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "a waiting caller was never woken");
            thread::park_timeout(left);
        }
    }
}

#[test]
fn threads_share_one_load() {
    const THREADS: usize = 16;
    let clock = TestClock::new();
    let started = Arc::new(AtomicUsize::new(0));
    let loads = Arc::new(AtomicUsize::new(0));
    let source = {
        let started = Arc::clone(&started);
        let loads = Arc::clone(&loads);
        from_fn(move |key: u32| {
            let started = Arc::clone(&started);
            let loads = Arc::clone(&loads);
            async move {
                loads.fetch_add(1, Ordering::SeqCst);
                // The load cannot finish until every thread has asked, so
                // without the single flight every thread would load.
                poll_fn(|cx| {
                    if started.load(Ordering::SeqCst) == THREADS {
                        Poll::Ready(())
                    } else {
                        cx.waker().wake_by_ref();
                        Poll::Pending
                    }
                })
                .await;
                Ok::<_, String>(format!("{key} loaded"))
            }
        })
    };
    let cache = Arc::new(
        LoadingCache::builder(lru(&clock, 100), source)
            .clock(clock.shared())
            .build(),
    );
    let barrier = Arc::new(Barrier::new(THREADS));

    let threads: Vec<_> = (0..THREADS)
        .map(|_| {
            let cache = Arc::clone(&cache);
            let barrier = Arc::clone(&barrier);
            let started = Arc::clone(&started);
            thread::spawn(move || {
                barrier.wait();
                started.fetch_add(1, Ordering::SeqCst);
                block_on_parking(cache.get(&7))
            })
        })
        .collect();

    for thread in threads {
        assert_eq!(thread.join().unwrap(), Ok("7 loaded".to_owned()));
    }
    assert_eq!(loads.load(Ordering::SeqCst), 1);
}

#[test]
fn threads_share_a_failure() {
    const THREADS: usize = 16;
    let clock = TestClock::new();
    let source = Source::gated();
    source.fail(true);
    let cache = Arc::new(
        LoadingCache::builder(lru(&clock, 100), source.clone())
            .clock(clock.shared())
            .build(),
    );
    let barrier = Arc::new(Barrier::new(THREADS));
    let joined = Arc::new(AtomicUsize::new(0));

    let threads: Vec<_> = (0..THREADS)
        .map(|_| {
            let cache = Arc::clone(&cache);
            let barrier = Arc::clone(&barrier);
            let joined = Arc::clone(&joined);
            thread::spawn(move || {
                barrier.wait();
                block_on_parking_then(cache.get(&7), || {
                    joined.fetch_add(1, Ordering::SeqCst);
                })
            })
        })
        .collect();
    // The load may fail only once every thread has joined it.
    while joined.load(Ordering::SeqCst) < THREADS {
        thread::yield_now();
    }
    source.gate.open();

    for thread in threads {
        assert_eq!(thread.join().unwrap(), Err("load 1 of 7 failed".to_owned()));
    }
    assert_eq!(source.loads(), 1);
}

#[test]
fn the_cache_and_its_futures_can_cross_threads() {
    fn send<T: Send>(_: &T) {}
    fn sync<T: Sync>(_: &T) {}

    let cache = LoadingCache::builder(
        LruStore::<u32, String>::builder().build(),
        from_fn(|key: u32| async move { Ok::<_, String>(key.to_string()) }),
    )
    .build();
    send(&cache);
    sync(&cache);
    send(&cache.get(&1));
    send(&cache.get_loaded(&1));
}

/// A caller on another thread that asks just as a failure lands starts a
/// new load rather than receiving the failure. The waker keeps the leader
/// inside its publication until that caller has its answer, so the timing a
/// busy runtime only sometimes hits is hit every time.
#[test]
fn a_thread_asking_as_a_failure_lands_loads_again() {
    type Cache = LoadingCache<u32, String, LruStore<u32, String>, Source>;

    struct AskFromAnotherThread {
        cache: Arc<Cache>,
        answer: Mutex<Option<Result<String, String>>>,
    }

    impl Wake for AskFromAnotherThread {
        fn wake(self: Arc<Self>) {
            let answer = thread::scope(|scope| {
                scope
                    .spawn(|| block_on_parking(self.cache.get(&1)))
                    .join()
                    .unwrap()
            });
            *self.answer.lock().unwrap() = Some(answer);
        }
    }

    let clock = TestClock::new();
    let source = Source::gated();
    source.fail_first(1);
    let cache: Arc<Cache> = Arc::new(
        LoadingCache::builder(lru(&clock, 100), source.clone())
            .clock(clock.shared())
            .build(),
    );
    let ask = Arc::new(AskFromAnotherThread {
        cache: Arc::clone(&cache),
        answer: Mutex::new(None),
    });
    let asking = Waker::from(Arc::clone(&ask));
    let mut quiet = Context::from_waker(Waker::noop());
    let mut leader = pin!(cache.get(&1));
    let mut waiter = pin!(cache.get(&1));
    assert!(leader.as_mut().poll(&mut quiet).is_pending());
    assert!(waiter
        .as_mut()
        .poll(&mut Context::from_waker(&asking))
        .is_pending());

    source.gate.open();
    let failed = Err("load 1 of 1 failed".to_owned());
    assert_eq!(
        leader.as_mut().poll(&mut quiet),
        Poll::Ready(failed.clone())
    );
    assert_eq!(
        ask.answer.lock().unwrap().take(),
        Some(Ok("1 from load 2".to_owned()))
    );
    assert_eq!(waiter.as_mut().poll(&mut quiet), Poll::Ready(failed));
    assert_eq!(source.loads(), 2);
}
