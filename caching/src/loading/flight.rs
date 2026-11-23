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

//! One load per key at a time.
//!
//! The first caller for a key becomes the leader. It owns the key's slot
//! until the work ends, either doing the work itself or handing its hold to
//! a task that does. Callers that arrive meanwhile wait on the slot and are
//! woken with the result. If the hold is dropped before a result is
//! published, the waiters are woken to try again, and one of them becomes
//! the new leader. They are told whether a caller or a task was holding it,
//! because a caller dropped is one fewer to take over, while a task dropped
//! may be dropped every time it is started.
//!
//! A slot leaves the map before it is given a result, so no caller ever
//! joins a finished slot. A caller that comes later, a woken waiter asking
//! again among them, starts new work rather than receiving an old result.

use std::collections::HashMap;
use std::future::Future;
use std::hash::Hash;
use std::mem;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use super::shards::{lock, Shards};

type Slots<K, T> = HashMap<K, Arc<Slot<T>>>;

/// The keys being worked on, each with the slot its waiters watch.
pub(crate) struct Flights<K, T> {
    slots: Shards<Slots<K, T>>,
}

struct Slot<T> {
    state: Mutex<State<T>>,
}

enum State<T> {
    /// The leader is working. Each waiter keeps its waker at its own index.
    Working(Vec<Option<Waker>>),
    /// The leader's result, given to every waiter.
    Done(T),
    /// The caller leading was dropped before it had a result.
    Abandoned,
    /// The task doing the work was dropped before it had a result.
    Lost,
}

/// What a caller does for a key.
pub(crate) enum Role<K: Hash + Eq, T> {
    /// Do the work, then publish the result.
    Lead(Lead<K, T>),
    /// Wait for the leader.
    Wait(Wait<T>),
}

/// How a wait ended.
pub enum Outcome<T> {
    /// The leader's result.
    Done(T),
    /// The caller leading was dropped, so ask again.
    Abandoned,
    /// The task doing the work was dropped, by a panic in it or by the
    /// spawner that started it.
    Lost,
}

impl<K: Hash + Eq + Clone, T: Clone> Flights<K, T> {
    pub(crate) fn new(shards: usize) -> Self {
        Flights {
            slots: Shards::new(shards, HashMap::new),
        }
    }

    /// Joins the work in progress for `key`, or starts it.
    pub(crate) fn join_or_lead(self: &Arc<Self>, key: &K) -> Role<K, T> {
        let mut slots = lock(self.slots.for_key(key));
        if let Some(slot) = slots.get(key) {
            return Role::Wait(Wait {
                slot: Arc::clone(slot),
                index: None,
            });
        }
        let slot = Arc::new(Slot {
            state: Mutex::new(State::Working(Vec::new())),
        });
        slots.insert(key.clone(), Arc::clone(&slot));
        Role::Lead(Lead {
            flights: Arc::clone(self),
            key: key.clone(),
            slot,
            in_task: false,
        })
    }
}

/// The leader's hold on a key. It owns what it needs, so it can move into a
/// task of its own. Dropping it ends the work, and wakes the waiters to try
/// again if no result was published.
pub(crate) struct Lead<K: Hash + Eq, T> {
    flights: Arc<Flights<K, T>>,
    key: K,
    slot: Arc<Slot<T>>,
    /// Whether a task holds this rather than a caller.
    in_task: bool,
}

impl<K: Hash + Eq, T> Lead<K, T> {
    /// A wait on this load, for the caller that handed the load to a task.
    pub(crate) fn wait(&self) -> Wait<T> {
        Wait {
            slot: Arc::clone(&self.slot),
            index: None,
        }
    }

    /// Marks the hold as a task's, before it is given to a spawner. Dropped
    /// from here on without a result, it tells the waiters the work was
    /// lost, even if the spawner never starts the task.
    pub(crate) fn hand_to_task(&mut self) {
        self.in_task = true;
    }

    /// Gives `result` to every caller waiting on the slot, as the last step
    /// of the work. A caller that arrives from now on starts new work, so a
    /// failure reaches only the callers that were waiting for it.
    pub(crate) fn publish(&self, result: T) {
        self.leave();
        let mut state = lock(&self.slot.state);
        if let State::Working(wakers) = mem::replace(&mut *state, State::Done(result)) {
            drop(state);
            wakers.into_iter().flatten().for_each(Waker::wake);
        }
    }

    /// Takes the slot out of the map, unless a new leader's slot has
    /// replaced it. Done before waking anyone, so a woken waiter that asks
    /// again starts new work rather than joining this slot.
    fn leave(&self) {
        let mut slots = lock(self.flights.slots.for_key(&self.key));
        if slots
            .get(&self.key)
            .is_some_and(|slot| Arc::ptr_eq(slot, &self.slot))
        {
            slots.remove(&self.key);
        }
    }
}

impl<K: Hash + Eq, T> Drop for Lead<K, T> {
    fn drop(&mut self) {
        self.leave();
        let mut state = lock(&self.slot.state);
        let State::Working(wakers) = &mut *state else {
            return;
        };
        let wakers = mem::take(wakers);
        *state = if self.in_task {
            State::Lost
        } else {
            State::Abandoned
        };
        drop(state);
        wakers.into_iter().flatten().for_each(Waker::wake);
    }
}

/// A waiter's future, ready with the leader's outcome.
pub(crate) struct Wait<T> {
    slot: Arc<Slot<T>>,
    index: Option<usize>,
}

impl<T: Clone> Future for Wait<T> {
    type Output = Outcome<T>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Outcome<T>> {
        let this = &mut *self;
        let mut state = lock(&this.slot.state);
        match &mut *state {
            State::Done(result) => Poll::Ready(Outcome::Done(result.clone())),
            State::Abandoned => Poll::Ready(Outcome::Abandoned),
            State::Lost => Poll::Ready(Outcome::Lost),
            State::Working(wakers) => {
                match this.index {
                    Some(index) => match &mut wakers[index] {
                        Some(waker) => waker.clone_from(cx.waker()),
                        empty => *empty = Some(cx.waker().clone()),
                    },
                    None => {
                        this.index = Some(wakers.len());
                        wakers.push(Some(cx.waker().clone()));
                    }
                }
                Poll::Pending
            }
        }
    }
}

impl<T> Drop for Wait<T> {
    fn drop(&mut self) {
        if let Some(index) = self.index {
            if let State::Working(wakers) = &mut *lock(&self.slot.state) {
                wakers[index] = None;
            }
        }
    }
}
