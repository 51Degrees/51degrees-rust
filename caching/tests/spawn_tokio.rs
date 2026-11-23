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

//! Loads run as tasks of their own on a multi-threaded tokio runtime, and
//! on the tokio feature's local pool. Both need threads, so these are
//! compiled only off WebAssembly.
#![cfg(not(target_family = "wasm"))]

mod common;

use std::future::Future;
use std::pin::pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};
use std::time::{Duration, Instant};

use common::*;
use fiftyone_caching::tokio::LocalPool;
use fiftyone_caching::{Loaded, LoadingCache, Spawn, StartLoad, ValueLoader};
use tokio::runtime::{Builder, Handle};

/// Starts each load on one of the runtime's blocking threads and runs it
/// there, so the load's future never moves between threads.
fn tokio_spawner(start: StartLoad) {
    tokio::task::spawn_blocking(move || Handle::current().block_on(start()));
}

#[test]
fn a_dropped_loading_caller_neither_stops_the_load_nor_starts_another() {
    dropped_caller_keeps_one_load(tokio_spawner);
}

#[test]
fn the_local_pool_keeps_one_load_when_its_caller_is_dropped() {
    dropped_caller_keeps_one_load(LocalPool::new(2));
}

/// Runs a load on the spawner, drops the caller that started it while it
/// runs, and checks the callers that came after get its value.
fn dropped_caller_keeps_one_load(spawner: impl fiftyone_caching::Spawn + Send + Sync + 'static) {
    let runtime = Builder::new_multi_thread()
        .worker_threads(4)
        .build()
        .unwrap();
    runtime.block_on(async {
        let clock = TestClock::new();
        let source = Source::gated();
        let cache = Arc::new(
            LoadingCache::builder(lru(&clock, 100), source.clone())
                .clock(clock.shared())
                .spawner(spawner)
                .build(),
        );

        let first = tokio::spawn({
            let cache = Arc::clone(&cache);
            async move { cache.get(&1).await }
        });
        // The load cannot finish until the gate opens, so once it has started
        // the first caller is only waiting for it.
        while source.loads() == 0 {
            tokio::task::yield_now().await;
        }
        let others: Vec<_> = (0..8)
            .map(|_| {
                let cache = Arc::clone(&cache);
                tokio::spawn(async move { cache.get(&1).await })
            })
            .collect();
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());
        source.gate.open();

        for other in others {
            assert_eq!(other.await.unwrap(), Ok("1 from load 1".to_owned()));
        }
        assert_eq!(source.loads(), 1);
    });
}

#[test]
fn the_local_pool_needs_no_runtime_where_loads_start() {
    let clock = TestClock::new();
    let source = Source::new();
    let cache = LoadingCache::builder(lru(&clock, 100), source.clone())
        .clock(clock.shared())
        .spawner(LocalPool::new(1))
        .build();
    assert!(Handle::try_current().is_err(), "no runtime on this thread");
    assert_eq!(park_on(cache.get(&1)), Ok("1 from load 1".to_owned()));
    assert_eq!(source.loads(), 1);
}

/// Runs a future on this thread, parking between polls until it is woken.
///
/// # Panics
///
/// If the future has not finished within ten seconds.
fn park_on<F: Future>(future: F) -> F::Output {
    struct Unpark(Thread);

    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = pin!(future);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
        assert!(Instant::now() < deadline, "the load never finished");
        // A deadline check rather than one long park, because a park can
        // return before it is woken.
        thread::park_timeout(Duration::from_millis(50));
    }
}

/// A source whose every load panics, counting the loads.
#[derive(Clone, Default)]
struct Panics(Arc<AtomicUsize>);

impl ValueLoader<u32, String> for Panics {
    type Error = String;

    async fn load(&self, key: &u32) -> Result<Loaded<String>, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("the load of {key} panicked")
    }
}

#[test]
fn a_load_that_panics_runs_twice_then_its_caller_panics() {
    panicking_load_ends_its_caller(tokio_spawner);
}

#[test]
fn the_local_pool_ends_a_caller_whose_load_panics() {
    panicking_load_ends_its_caller(LocalPool::new(2));
}

/// Runs a load that panics on the spawner, and checks the caller ends by
/// panicking once the load has been asked for twice.
fn panicking_load_ends_its_caller(spawner: impl Spawn + Send + Sync + 'static) {
    let runtime = Builder::new_multi_thread()
        .worker_threads(2)
        .build()
        .unwrap();
    let clock = TestClock::new();
    let source = Panics::default();
    let cache = Arc::new(
        LoadingCache::builder(lru(&clock, 100), source.clone())
            .clock(clock.shared())
            .spawner(spawner)
            .build(),
    );

    let (ended, end) = mpsc::channel();
    runtime.spawn(async move {
        let caller = tokio::spawn(async move { cache.get(&1).await });
        let _ = ended.send(caller.await);
    });

    // The caller ends, where asking again each time would never end.
    let caller = end
        .recv_timeout(Duration::from_secs(10))
        .expect("the caller should end");
    assert!(caller.unwrap_err().is_panic());
    assert_eq!(source.0.load(Ordering::SeqCst), 2);
}
