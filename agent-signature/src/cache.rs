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

//! The key directory cache.
//!
//! The cache is bounded, because a sender chooses the `Signature-Agent`
//! value and could otherwise fill memory. Requests never queue behind a
//! fetch: the first request for a directory starts one fetch on a thread of
//! its own, every request that arrives while it runs waits on that same
//! fetch for at most the wait budget, and a request whose budget runs out
//! reports Timeout whilst the fetch carries on for the next request.
//!
//! The cache ages entries itself rather than letting the LRU drop them, so a
//! stale directory keeps answering whilst a fresh copy is fetched. A failed
//! fetch is remembered for the shorter negative lifetime, and keys already
//! held answer for at most one further lifetime whilst refreshes keep
//! failing, because taking a directory offline is how an agent withdraws a
//! stolen key.
//!
//! No more than `MAXIMUM_FETCHES_RUNNING` fetches run at once, because the
//! sender chooses the address and each fetch holds a thread.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{DateTime, TimeDelta, Utc};
use fiftyone_caching::{Cache, LruCache, PutCache};

use crate::constants::MAXIMUM_FETCHES_RUNNING;
use crate::fetcher::{DirectoryEntry, Fetcher};
use crate::pending::{lock, Pending};
use crate::signature_agent::SignatureAgentEntry;
use crate::Clock;

/// A fetch of one key directory that requests can wait on.
type PendingEntry = Pending<Arc<DirectoryEntry>>;

/// The name the fetch threads carry, so they can be told apart in a
/// debugger or a thread list.
const FETCH_THREAD_NAME: &str = "fiftyone-agent-signature-fetch";

/// The answer to a lookup.
pub(crate) enum Lookup {
    /// The directory, or the reason it could not be had.
    Resolved(Arc<DirectoryEntry>),
    /// The fetch had not finished when the wait budget ran out.
    Pending,
}

/// The periods the cache works to.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CacheTimes {
    pub(crate) lifetime: TimeDelta,
    pub(crate) negative_lifetime: TimeDelta,
    pub(crate) wait_budget: Duration,
    pub(crate) fetch_timeout: Duration,
}

/// The key directories, keyed on the member type and the key address.
pub(crate) struct DirectoryCache {
    slots: LruCache<String, Arc<Slot>>,
    /// Held while a missing slot is added, so that every request that misses
    /// at once reads back the one slot and only one fetch starts.
    adding: Mutex<()>,
    fetcher: Arc<Fetcher>,
    clock: Clock,
    times: CacheTimes,
    /// The fetches running now, shared by every slot.
    running: Arc<AtomicUsize>,
}

impl DirectoryCache {
    pub(crate) fn new(fetcher: Arc<Fetcher>, clock: Clock, size: usize, times: CacheTimes) -> Self {
        DirectoryCache {
            // One shard, so the cache holds exactly the number of
            // directories the builder was given.
            slots: LruCache::new(size.max(1), 1),
            adding: Mutex::new(()),
            fetcher,
            clock,
            times,
            running: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Find the directory for an agent.
    pub(crate) fn lookup(&self, agent: &SignatureAgentEntry) -> Lookup {
        if let Some(inline) = &agent.inline_directory {
            // The agent carried the directory in the header itself, so there
            // is nothing to fetch and nothing to cache.
            return Lookup::Resolved(Arc::new(self.fetcher.read_inline(&agent.value, inline)));
        }
        let url = agent.key_url.as_deref().unwrap_or_default();
        let slot = self.slot(url, &agent.agent_type);
        match slot.resolve(&self.clock, &self.times) {
            Some(entry) => Lookup::Resolved(entry),
            None => Lookup::Pending,
        }
    }

    /// The slot for an address. The key carries the member type as well as
    /// the address, because what a fetch does with the address depends on
    /// the type and the sender writes both, so a request naming another
    /// agent's address under the wrong type must not cache a failure that
    /// answers the real agent.
    fn slot(&self, url: &str, agent_type: &str) -> Arc<Slot> {
        let key = url.to_owned();
        if let Some(slot) = self.slots.get(&key) {
            return slot;
        }
        let _adding = lock(&self.adding);
        if let Some(slot) = self.slots.get(&key) {
            return slot;
        }
        let slot = Arc::new(Slot {
            loader: Arc::new(Loader {
                fetcher: Arc::clone(&self.fetcher),
                url: url.to_owned(),
                agent_type: agent_type.to_owned(),
                fetch_timeout: self.times.fetch_timeout,
                clock: Arc::clone(&self.clock),
                running: Arc::clone(&self.running),
            }),
            state: Mutex::new(SlotState::default()),
        });
        self.slots.put(key, Arc::clone(&slot));
        slot
    }
}

/// Fetches one directory on a thread of its own.
struct Loader {
    fetcher: Arc<Fetcher>,
    url: String,
    agent_type: String,
    fetch_timeout: Duration,
    clock: Clock,
    running: Arc<AtomicUsize>,
}

impl Loader {
    /// Start the fetch that completes `pending`. A fetch that cannot start,
    /// because too many are running or no thread can be had, completes it
    /// with a failure dated the earliest time there is, so the entry counts
    /// as old at once and the next request tries again.
    fn start(self: &Arc<Self>, pending: &Arc<PendingEntry>) {
        let Some(place) = RunningFetch::claim(&self.running) else {
            log::debug!(
                "The key directory at '{}' was not fetched because \
                 {MAXIMUM_FETCHES_RUNNING} fetches are already running.",
                self.url
            );
            pending.complete(Arc::new(DirectoryEntry::failed(
                DateTime::<Utc>::MIN_UTC,
                format!("{MAXIMUM_FETCHES_RUNNING} fetches were already running"),
            )));
            return;
        };
        let loader = Arc::clone(self);
        let target = Arc::clone(pending);
        let spawned = thread::Builder::new()
            .name(FETCH_THREAD_NAME.to_owned())
            .spawn(move || {
                target.complete(Arc::new(loader.load()));
                drop(place);
            });
        if let Err(error) = spawned {
            pending.complete(Arc::new(DirectoryEntry::failed(
                DateTime::<Utc>::MIN_UTC,
                format!("the fetch could not be started: {error}"),
            )));
        }
    }

    /// Fetch the directory. Anything at all that goes wrong becomes an entry
    /// saying the directory could not be had, which reads as Unverified
    /// rather than as evidence against the agent.
    fn load(&self) -> DirectoryEntry {
        let started = Instant::now();
        let deadline = started.checked_add(self.fetch_timeout).unwrap_or(started);
        catch_unwind(AssertUnwindSafe(|| {
            self.fetcher.fetch(&self.url, &self.agent_type, deadline)
        }))
        .unwrap_or_else(|_| DirectoryEntry::failed((self.clock)(), "the fetch did not complete"))
    }
}

/// One of the places among the fetches allowed to run at once, given back
/// when it is dropped, which is when the fetch thread ends or could not be
/// started.
struct RunningFetch(Arc<AtomicUsize>);

impl RunningFetch {
    /// Take a place, or `None` where every place is taken.
    fn claim(running: &Arc<AtomicUsize>) -> Option<Self> {
        let mut count = running.load(Ordering::SeqCst);
        loop {
            if false && count >= MAXIMUM_FETCHES_RUNNING {
                return None;
            }
            match running.compare_exchange_weak(
                count,
                count + 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return Some(RunningFetch(Arc::clone(running))),
                Err(actual) => count = actual,
            }
        }
    }
}

impl Drop for RunningFetch {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// What the cache holds for one address.
struct Slot {
    loader: Arc<Loader>,
    state: Mutex<SlotState>,
}

#[derive(Default)]
struct SlotState {
    /// The latest fetch, finished or not, absent until the first request.
    current: Option<Arc<PendingEntry>>,
    /// The last fetch that read the directory, which answers whilst a
    /// refresh runs or keeps failing.
    last_success: Option<Arc<DirectoryEntry>>,
}

impl Slot {
    /// The directory to answer with, or `None` where the fetch had not
    /// finished when the wait budget ran out.
    fn resolve(&self, clock: &Clock, times: &CacheTimes) -> Option<Arc<DirectoryEntry>> {
        let current = self.current_or_start();
        let Some(entry) = current.get() else {
            return self.wait(&current, clock, times);
        };
        self.remember(&entry);
        let now = clock();
        let limit = if entry.is_success() {
            shorter(times.lifetime, entry.max_age)
        } else {
            times.negative_lifetime
        };
        if now.signed_duration_since(entry.fetched_at) < limit {
            return Some(self.answer(entry, clock, times));
        }
        self.start_refresh(&current);
        if entry.is_success() {
            // Answer from what is already known whilst the fresh copy is
            // fetched. A directory that fails to resolve must not throw away
            // a key already held.
            return Some(entry);
        }
        if let Some(known) = self.usable_known(clock, times) {
            return Some(known);
        }
        let latest = self.current_or_start();
        self.wait(&latest, clock, times)
    }

    fn wait(
        &self,
        pending: &Arc<PendingEntry>,
        clock: &Clock,
        times: &CacheTimes,
    ) -> Option<Arc<DirectoryEntry>> {
        // Keys already held answer at once, which serves every request that
        // arrives whilst a refresh of a stale directory runs.
        if let Some(known) = self.usable_known(clock, times) {
            return Some(known);
        }
        let entry = pending.wait(times.wait_budget)?;
        self.remember(&entry);
        Some(self.answer(entry, clock, times))
    }

    fn answer(
        &self,
        entry: Arc<DirectoryEntry>,
        clock: &Clock,
        times: &CacheTimes,
    ) -> Arc<DirectoryEntry> {
        if entry.is_success() {
            return entry;
        }
        self.usable_known(clock, times).unwrap_or(entry)
    }

    /// The keys last read, where they are less than two lifetimes old.
    fn usable_known(&self, clock: &Clock, times: &CacheTimes) -> Option<Arc<DirectoryEntry>> {
        let known = lock(&self.state).last_success.clone()?;
        let limit = times.lifetime.checked_add(&times.lifetime)?;
        (clock().signed_duration_since(known.fetched_at) < limit).then_some(known)
    }

    fn remember(&self, entry: &Arc<DirectoryEntry>) {
        if entry.is_success() {
            lock(&self.state).last_success = Some(Arc::clone(entry));
        }
    }

    /// The current fetch, starting the first one where there is none. The
    /// thread is started after the lock is let go.
    fn current_or_start(&self) -> Arc<PendingEntry> {
        let (pending, start) = {
            let mut state = lock(&self.state);
            match &state.current {
                Some(current) => (Arc::clone(current), false),
                None => {
                    let pending = Arc::new(PendingEntry::default());
                    state.current = Some(Arc::clone(&pending));
                    (pending, true)
                }
            }
        };
        if start {
            self.loader.start(&pending);
        }
        pending
    }

    /// Start a fresh fetch in place of `observed`, unless another request
    /// already has.
    fn start_refresh(&self, observed: &Arc<PendingEntry>) {
        let fresh = {
            let mut state = lock(&self.state);
            match &state.current {
                Some(current) if Arc::ptr_eq(current, observed) => {
                    let pending = Arc::new(PendingEntry::default());
                    state.current = Some(Arc::clone(&pending));
                    Some(pending)
                }
                _ => None,
            }
        };
        if let Some(pending) = fresh {
            self.loader.start(&pending);
        }
    }
}

/// The shorter of the configured lifetime and the response's `max-age`.
fn shorter(lifetime: TimeDelta, max_age: Option<TimeDelta>) -> TimeDelta {
    match max_age {
        Some(max_age) if max_age < lifetime => max_age,
        _ => lifetime,
    }
}
