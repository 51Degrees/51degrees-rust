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

//! Loads run as tasks of their own, started by a local spawner whose tasks
//! the test runs on its one thread, so these run on every target. A dropped
//! caller neither stops a load nor starts another, and every caller, the
//! first included, waits for its result. A load lost with its task is asked
//! for once more, and a second loss panics the caller, which is tested
//! where a panic unwinds.

mod common;

use std::cell::{Cell, RefCell};
#[cfg(panic = "unwind")]
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use common::*;
use fiftyone_caching::{LoadTask, LoadingCache, Lookup, LruStore, SpawnLocal, SpawnedLocal, Store};
#[cfg(panic = "unwind")]
use fiftyone_caching::{Loaded, ValueLoader};

/// A local spawner that keeps its tasks for the test to run.
#[derive(Clone, Default)]
struct Tasks {
    queued: Rc<RefCell<Vec<LoadTask>>>,
    started: Rc<Cell<usize>>,
}

impl SpawnLocal for Tasks {
    fn spawn_local(&self, task: LoadTask) {
        self.started.set(self.started.get() + 1);
        self.queued.borrow_mut().push(task);
    }
}

type Cache = LoadingCache<u32, String, LruStore<u32, String>, Source, SpawnedLocal<Tasks>>;

fn cache(clock: &TestClock, source: &Source, tasks: &Tasks) -> Cache {
    LoadingCache::builder(lru(clock, 100), source.clone())
        .clock(clock.shared())
        .local_spawner(tasks.clone())
        .build()
}

/// Polls the callers and the spawned tasks in turn until all have finished,
/// calling `before_round` first, which may drop a caller.
fn run_with_tasks<'a, T>(
    tasks: &Tasks,
    callers: Vec<Boxed<'a, T>>,
    mut before_round: impl FnMut(usize, &mut [Option<Boxed<'a, T>>]),
) -> Vec<Option<T>> {
    let mut callers: Vec<Option<Boxed<'a, T>>> = callers.into_iter().map(Some).collect();
    let mut results: Vec<Option<T>> = callers.iter().map(|_| None).collect();
    let mut running: Vec<LoadTask> = Vec::new();
    let mut cx = Context::from_waker(Waker::noop());
    for round in 0..10_000 {
        before_round(round, &mut callers);
        for (slot, result) in callers.iter_mut().zip(results.iter_mut()) {
            if let Some(caller) = slot {
                if let Poll::Ready(value) = caller.as_mut().poll(&mut cx) {
                    *result = Some(value);
                    *slot = None;
                }
            }
        }
        running.extend(tasks.queued.borrow_mut().drain(..));
        running.retain_mut(|task| task.as_mut().poll(&mut cx).is_pending());
        let callers_done = callers.iter().all(Option::is_none);
        if callers_done && running.is_empty() && tasks.queued.borrow().is_empty() {
            return results;
        }
    }
    panic!("callers or tasks stopped making progress");
}

#[test]
fn a_dropped_loading_caller_neither_stops_the_load_nor_starts_another() {
    let clock = TestClock::new();
    let source = Source::gated();
    let tasks = Tasks::default();
    let cache = cache(&clock, &source, &tasks);

    // The first caller starts the load, and is dropped while it runs.
    let callers = (0..3).map(|_| boxed(cache.get(&1))).collect();
    let results = run_with_tasks(&tasks, callers, |round, callers| {
        if round == 1 {
            callers[0] = None;
            source.gate.open();
        }
    });

    assert_eq!(source.loads(), 1);
    assert_eq!(tasks.started.get(), 1);
    assert_eq!(results[0], None);
    assert_eq!(results[1], Some(Ok("1 from load 1".to_owned())));
    assert_eq!(results[2], Some(Ok("1 from load 1".to_owned())));
}

#[test]
fn a_load_runs_to_its_end_with_no_caller_left() {
    let clock = TestClock::new();
    let source = Source::gated();
    let tasks = Tasks::default();
    let cache = cache(&clock, &source, &tasks);

    let results = run_with_tasks(&tasks, vec![boxed(cache.get(&1))], |round, callers| {
        if round == 1 {
            callers[0] = None;
            source.gate.open();
        }
    });
    assert_eq!(results[0], None);

    // The load stored its value, so the next caller is served from the
    // store, with no second load and no task.
    let results = run_with_tasks(&tasks, vec![boxed(cache.get(&1))], |_, _| {});
    assert_eq!(results[0], Some(Ok("1 from load 1".to_owned())));
    assert_eq!(source.loads(), 1);
    assert_eq!(tasks.started.get(), 1);
}

#[test]
fn a_failed_load_reaches_every_caller_and_is_not_kept() {
    let clock = TestClock::new();
    let source = Source::gated();
    source.fail(true);
    let tasks = Tasks::default();
    let cache = cache(&clock, &source, &tasks);

    let callers = (0..3).map(|_| boxed(cache.get(&1))).collect();
    let results = run_with_tasks(&tasks, callers, |round, _| {
        if round == 1 {
            source.gate.open();
        }
    });

    assert_eq!(source.loads(), 1);
    for result in results {
        assert_eq!(result, Some(Err("load 1 of 1 failed".to_owned())));
    }

    source.fail(false);
    let results = run_with_tasks(&tasks, vec![boxed(cache.get(&1))], |_, _| {});
    assert_eq!(results[0], Some(Ok("1 from load 2".to_owned())));
}

#[test]
fn callers_dropped_while_they_lead_are_not_counted_as_lost_loads() {
    let clock = TestClock::new();
    let source = Source::new();
    let tasks = Tasks::default();
    let store = Arc::new(WaitingStore::new(&clock));
    let cache = LoadingCache::builder(Arc::clone(&store), source.clone())
        .clock(clock.shared())
        .local_spawner(tasks.clone())
        .build();

    // Another process holds the key's reservation, so a caller leading the
    // key here waits in the store, where it can be dropped.
    let Lookup::Reserved(reservation) = block_on(store.get(&1)) else {
        panic!("the first lookup of a key reserves it");
    };
    let mut reservation = Some(reservation);

    // Three leaders in a row are dropped, then the other process gives up
    // its load, and the last caller loads the value itself.
    let callers = (0..4).map(|_| boxed(cache.get(&1))).collect();
    let results = run_with_tasks(&tasks, callers, |round, callers| match round {
        1..=3 => callers[round - 1] = None,
        4 => drop(reservation.take()),
        _ => {}
    });

    assert_eq!(results[3], Some(Ok("1 from load 1".to_owned())));
    assert_eq!(source.loads(), 1);
    assert_eq!(tasks.started.get(), 1);
}

/// The start of what a caller panics with when it has lost two loads.
#[cfg(panic = "unwind")]
const LOST_TWICE: &str = "a load was lost twice";

/// Runs `work`, which must panic, and returns what it panicked with.
#[cfg(panic = "unwind")]
fn panic_message(work: impl FnOnce()) -> String {
    let payload = catch_unwind(AssertUnwindSafe(work)).expect_err("the work should have panicked");
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => payload
            .downcast_ref::<&str>()
            .map(ToString::to_string)
            .unwrap_or_default(),
    }
}

#[test]
#[cfg(panic = "unwind")]
fn a_spawner_that_drops_its_tasks_is_asked_twice_then_the_caller_panics() {
    let clock = TestClock::new();
    let source = Source::new();
    let asked = Rc::new(Cell::new(0));
    let spawner = {
        let asked = Rc::clone(&asked);
        move |task: LoadTask| {
            asked.set(asked.get() + 1);
            drop(task);
        }
    };
    let cache = LoadingCache::builder(lru(&clock, 100), source.clone())
        .clock(clock.shared())
        .local_spawner(spawner)
        .build();

    // The caller ends, where asking again each time would never end.
    let message = panic_message(|| {
        let _ = block_on(cache.get(&1));
    });

    assert!(message.starts_with(LOST_TWICE), "{message}");
    assert_eq!(asked.get(), 2);
    assert_eq!(source.loads(), 0);
}

/// A source whose every load panics, counting the loads.
#[cfg(panic = "unwind")]
#[derive(Clone, Default)]
struct Panics(Rc<Cell<usize>>);

#[cfg(panic = "unwind")]
impl ValueLoader<u32, String> for Panics {
    type Error = String;

    async fn load(&self, key: &u32) -> Result<Loaded<String>, String> {
        self.0.set(self.0.get() + 1);
        panic!("the load of {key} panicked")
    }
}

#[test]
#[cfg(panic = "unwind")]
fn a_load_that_panics_in_its_task_runs_twice_then_its_caller_panics() {
    let clock = TestClock::new();
    let source = Panics::default();
    let tasks = Tasks::default();
    let cache = LoadingCache::builder(lru(&clock, 100), source.clone())
        .clock(clock.shared())
        .local_spawner(tasks.clone())
        .build();

    let message = panic_message(|| {
        let mut caller = boxed(cache.get(&1));
        let mut cx = Context::from_waker(Waker::noop());
        for _ in 0..10 {
            if caller.as_mut().poll(&mut cx).is_ready() {
                return;
            }
            // A task that panics is dropped by what runs it, as a runtime
            // does, and the panic goes no further.
            let started: Vec<LoadTask> = tasks.queued.borrow_mut().drain(..).collect();
            for mut task in started {
                let _ = catch_unwind(AssertUnwindSafe(|| task.as_mut().poll(&mut cx)));
            }
        }
    });

    assert!(message.starts_with(LOST_TWICE), "{message}");
    assert_eq!(source.0.get(), 2);
    assert_eq!(tasks.started.get(), 2);
}
