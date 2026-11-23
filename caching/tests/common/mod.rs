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

//! Helpers shared by the tests. A clock the test moves, executors that need
//! no runtime, a gate a load waits on, a counting source, and stores that
//! stand in for platform stores, of entries and of bytes.

// Each test file uses its own subset of these helpers.
#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};
use std::convert::Infallible;
use std::future::{poll_fn, Future};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, SystemTime};

use fiftyone_caching::{
    ByteLookup, ByteStore, Clock, Entry, ListKeys, Loaded, Lookup, LruStore, Store, ValueLoader,
};

/// A clock that moves only when the test moves it.
#[derive(Clone)]
pub struct TestClock(Arc<Mutex<SystemTime>>);

impl TestClock {
    pub fn new() -> Self {
        let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        TestClock(Arc::new(Mutex::new(start)))
    }

    pub fn now(&self) -> SystemTime {
        *self.0.lock().unwrap()
    }

    pub fn advance(&self, by: Duration) {
        *self.0.lock().unwrap() += by;
    }

    pub fn advance_secs(&self, secs: u64) {
        self.advance(Duration::from_secs(secs));
    }

    /// The clock in the form builders take.
    pub fn shared(&self) -> Arc<dyn Clock> {
        let clock = self.clone();
        Arc::new(move || clock.now())
    }
}

pub fn secs(secs: u64) -> Duration {
    Duration::from_secs(secs)
}

/// A store in memory on the test clock, with one shard so eviction order
/// is exact.
pub fn lru(clock: &TestClock, size: usize) -> LruStore<u32, String> {
    LruStore::builder()
        .size(size)
        .concurrency(1)
        .clock(clock.shared())
        .build()
}

pub type Boxed<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

pub fn boxed<'a, T>(future: impl Future<Output = T> + 'a) -> Boxed<'a, T> {
    Box::pin(future)
}

/// Polls every unfinished future once per round until all finish, calling
/// `before_round` first, which may drop a future to cancel it. The waker
/// does nothing, so progress comes only from polling again, as when many
/// requests interleave on one thread. Returns `None` for a dropped future.
///
/// # Panics
///
/// If no future finishes for many rounds.
pub fn run_rounds<'a, T>(
    futures: Vec<Boxed<'a, T>>,
    mut before_round: impl FnMut(usize, &mut [Option<Boxed<'a, T>>]),
) -> Vec<Option<T>> {
    let mut pending: Vec<Option<Boxed<'a, T>>> = futures.into_iter().map(Some).collect();
    let mut results: Vec<Option<T>> = pending.iter().map(|_| None).collect();
    let mut cx = Context::from_waker(Waker::noop());
    let mut idle_rounds = 0;
    for round in 0.. {
        before_round(round, &mut pending);
        let mut finished = false;
        for (slot, result) in pending.iter_mut().zip(results.iter_mut()) {
            if let Some(future) = slot {
                if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
                    *result = Some(value);
                    *slot = None;
                    finished = true;
                }
            }
        }
        if pending.iter().all(Option::is_none) {
            break;
        }
        idle_rounds = if finished { 0 } else { idle_rounds + 1 };
        assert!(idle_rounds < 10_000, "futures stopped making progress");
    }
    results
}

/// Runs the futures together, polled in turn on one thread.
pub fn run_all<'a, T>(futures: Vec<Boxed<'a, T>>) -> Vec<T> {
    run_rounds(futures, |_, _| {})
        .into_iter()
        .map(|result| result.expect("no future was dropped"))
        .collect()
}

/// Runs one future to completion on this thread.
pub fn block_on<'a, T>(future: impl Future<Output = T> + 'a) -> T {
    run_all(vec![boxed(future)]).pop().unwrap()
}

/// A gate a load waits at until the test opens it.
#[derive(Clone, Default)]
pub struct Gate(Arc<AtomicBool>);

impl Gate {
    pub fn open(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn opened() -> Self {
        let gate = Gate::default();
        gate.open();
        gate
    }

    pub async fn pass(&self) {
        poll_fn(|cx| {
            if self.0.load(Ordering::SeqCst) {
                Poll::Ready(())
            } else {
                // Asks to be polled again, for executors that wait to be
                // woken.
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        })
        .await
    }
}

/// A source that counts its loads, waits at its gate, then answers. The
/// value names the key and the load that produced it, so a test can tell
/// whether a value came from a new load.
#[derive(Clone)]
pub struct Source {
    loads: Arc<AtomicUsize>,
    pub gate: Gate,
    failing: Arc<AtomicBool>,
    failing_loads: Arc<AtomicUsize>,
}

impl Source {
    /// A source whose gate is open.
    pub fn new() -> Self {
        Source {
            loads: Arc::default(),
            gate: Gate::opened(),
            failing: Arc::default(),
            failing_loads: Arc::default(),
        }
    }

    /// A source whose loads wait until the test opens the gate.
    pub fn gated() -> Self {
        Source {
            gate: Gate::default(),
            ..Source::new()
        }
    }

    pub fn loads(&self) -> usize {
        self.loads.load(Ordering::SeqCst)
    }

    pub fn fail(&self, failing: bool) {
        self.failing.store(failing, Ordering::SeqCst);
    }

    /// Fails the first `loads` loads and answers the rest.
    pub fn fail_first(&self, loads: usize) {
        self.failing_loads.store(loads, Ordering::SeqCst);
    }
}

impl ValueLoader<u32, String> for Source {
    type Error = String;

    async fn load(&self, key: &u32) -> Result<Loaded<String>, String> {
        let load = self.loads.fetch_add(1, Ordering::SeqCst) + 1;
        self.gate.pass().await;
        if self.failing.load(Ordering::SeqCst) || load <= self.failing_loads.load(Ordering::SeqCst)
        {
            Err(format!("load {load} of {key} failed"))
        } else {
            Ok(Loaded::new(format!("{key} from load {load}")))
        }
    }
}

/// Counts the calls made through a loader, such as a cache above calling
/// the cache below.
pub struct Counted<L> {
    pub inner: L,
    calls: Arc<AtomicUsize>,
}

impl<L> Counted<L> {
    pub fn new(inner: L) -> (Self, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = Counted {
            inner,
            calls: Arc::clone(&calls),
        };
        (counted, calls)
    }
}

impl<L: ValueLoader<u32, String>> ValueLoader<u32, String> for Counted<L> {
    type Error = L::Error;

    async fn load(&self, key: &u32) -> Result<Loaded<String>, L::Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.load(key).await
    }
}

/// Records the writes made to a store.
pub struct Recorded<S> {
    pub inner: S,
    writes: Mutex<Vec<(u32, Option<Duration>)>>,
}

impl<S> Recorded<S> {
    pub fn new(inner: S) -> Self {
        Recorded {
            inner,
            writes: Mutex::default(),
        }
    }

    /// The writes since the last call, as key and `keep_for`.
    pub fn take_writes(&self) -> Vec<(u32, Option<Duration>)> {
        std::mem::take(&mut *self.writes.lock().unwrap())
    }
}

impl<S: Store<u32, String>> Store<u32, String> for Recorded<S> {
    type Reservation = S::Reservation;

    async fn get(&self, key: &u32) -> Lookup<String, S::Reservation> {
        self.inner.get(key).await
    }

    async fn put(&self, key: &u32, entry: &Entry<String>, keep_for: Option<Duration>) {
        self.writes.lock().unwrap().push((*key, keep_for));
        self.inner.put(key, entry, keep_for).await
    }

    async fn fill(
        &self,
        key: &u32,
        reservation: S::Reservation,
        entry: &Entry<String>,
        keep_for: Option<Duration>,
    ) {
        self.writes.lock().unwrap().push((*key, keep_for));
        self.inner.fill(key, reservation, entry, keep_for).await
    }

    async fn remove(&self, key: &u32) {
        self.inner.remove(key).await
    }
}

/// A store that keeps every entry for ever, whatever it is told, to show
/// the cache never serves an entry past its time.
#[derive(Default)]
pub struct KeepsEverything(Mutex<HashMap<u32, Entry<String>>>);

impl Store<u32, String> for KeepsEverything {
    type Reservation = Infallible;

    async fn get(&self, key: &u32) -> Lookup<String, Infallible> {
        match self.0.lock().unwrap().get(key) {
            Some(entry) => Lookup::Hit(entry.clone()),
            None => Lookup::Miss,
        }
    }

    async fn put(&self, key: &u32, entry: &Entry<String>, _: Option<Duration>) {
        self.0.lock().unwrap().insert(*key, entry.clone());
    }

    async fn remove(&self, key: &u32) {
        self.0.lock().unwrap().remove(key);
    }
}

/// Stands in for a platform store whose lookup makes concurrent callers, in
/// any process, wait for one load. The first caller to miss gets a
/// reservation. Later callers wait until it is filled, and then get the
/// entry, or dropped, and then get a miss.
pub struct WaitingStore {
    inner: LruStore<u32, String>,
    loads_in_progress: Arc<Mutex<HashMap<u32, Arc<Pending>>>>,
    waited: AtomicUsize,
}

/// A load in progress that callers wait on.
#[derive(Default)]
struct Pending {
    state: Mutex<PendingState>,
}

#[derive(Default)]
struct PendingState {
    /// `Some(true)` once filled, `Some(false)` once dropped unfilled.
    outcome: Option<bool>,
    wakers: Vec<Waker>,
}

/// A reservation in a [`WaitingStore`]. Dropping it releases the callers
/// waiting on it.
pub struct Reservation {
    key: u32,
    pending: Arc<Pending>,
    loads_in_progress: Arc<Mutex<HashMap<u32, Arc<Pending>>>>,
    filled: bool,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.loads_in_progress.lock().unwrap().remove(&self.key);
        let wakers = {
            let mut state = self.pending.state.lock().unwrap();
            state.outcome = Some(self.filled);
            std::mem::take(&mut state.wakers)
        };
        wakers.into_iter().for_each(Waker::wake);
    }
}

impl WaitingStore {
    pub fn new(clock: &TestClock) -> Self {
        WaitingStore {
            inner: lru(clock, 1000),
            loads_in_progress: Arc::default(),
            waited: AtomicUsize::new(0),
        }
    }

    /// How many lookups waited for another caller's load.
    pub fn waited(&self) -> usize {
        self.waited.load(Ordering::SeqCst)
    }

    /// Reserves the load of `key`, or returns the load already in progress.
    fn reserve(&self, key: u32) -> Result<Reservation, Arc<Pending>> {
        let mut loads = self.loads_in_progress.lock().unwrap();
        if let Some(pending) = loads.get(&key) {
            return Err(Arc::clone(pending));
        }
        let pending = Arc::new(Pending::default());
        loads.insert(key, Arc::clone(&pending));
        Ok(Reservation {
            key,
            pending,
            loads_in_progress: Arc::clone(&self.loads_in_progress),
            filled: false,
        })
    }
}

impl Store<u32, String> for WaitingStore {
    type Reservation = Reservation;

    async fn get(&self, key: &u32) -> Lookup<String, Reservation> {
        if let Lookup::Hit(entry) = self.inner.get(key).await {
            return Lookup::Hit(entry);
        }
        let pending = match self.reserve(*key) {
            Ok(reservation) => return Lookup::Reserved(reservation),
            Err(pending) => pending,
        };
        self.waited.fetch_add(1, Ordering::SeqCst);
        let filled = poll_fn(|cx| {
            let mut state = pending.state.lock().unwrap();
            match state.outcome {
                Some(filled) => Poll::Ready(filled),
                None => {
                    state.wakers.push(cx.waker().clone());
                    Poll::Pending
                }
            }
        })
        .await;
        if filled {
            match self.inner.get(key).await {
                Lookup::Hit(entry) => Lookup::Hit(entry),
                _ => Lookup::Miss,
            }
        } else {
            Lookup::Miss
        }
    }

    async fn put(&self, key: &u32, entry: &Entry<String>, keep_for: Option<Duration>) {
        self.inner.put(key, entry, keep_for).await
    }

    async fn fill(
        &self,
        key: &u32,
        mut reservation: Reservation,
        entry: &Entry<String>,
        keep_for: Option<Duration>,
    ) {
        self.inner.put(key, entry, keep_for).await;
        reservation.filled = true;
    }

    async fn remove(&self, key: &u32) {
        self.inner.remove(key).await
    }
}

/// Stands in for a platform store of bytes. `DROPS` is what the store
/// claims about dropping entries past their lifetime, which this stand-in
/// never does itself, so a test sees what the adapter removes.
#[derive(Default)]
pub struct MemoryBytes<const DROPS: bool = true> {
    items: Mutex<BTreeMap<String, Vec<u8>>>,
    puts: Mutex<Vec<(String, Option<Duration>)>>,
    removed: Mutex<Vec<String>>,
}

impl<const DROPS: bool> MemoryBytes<DROPS> {
    pub fn new() -> Self {
        MemoryBytes {
            items: Mutex::default(),
            puts: Mutex::default(),
            removed: Mutex::default(),
        }
    }

    /// The bytes held for `key`.
    pub fn raw(&self, key: &str) -> Option<Vec<u8>> {
        self.items.lock().unwrap().get(key).cloned()
    }

    /// Holds `bytes` for `key` without going through the store.
    pub fn insert_raw(&self, key: &str, bytes: Vec<u8>) {
        self.items.lock().unwrap().insert(key.to_string(), bytes);
    }

    /// The keys held, in order.
    pub fn held(&self) -> Vec<String> {
        self.items.lock().unwrap().keys().cloned().collect()
    }

    /// The writes made through the store since the last call, with the
    /// lifetime each was given.
    pub fn take_puts(&self) -> Vec<(String, Option<Duration>)> {
        std::mem::take(&mut *self.puts.lock().unwrap())
    }

    /// The keys removed through the store.
    pub fn removed(&self) -> Vec<String> {
        self.removed.lock().unwrap().clone()
    }
}

impl<const DROPS: bool> ByteStore for MemoryBytes<DROPS> {
    type Reservation = Infallible;
    const DROPS_EXPIRED: bool = DROPS;

    async fn get(&self, key: &str) -> ByteLookup<Infallible> {
        match self.raw(key) {
            Some(bytes) => ByteLookup::Hit(bytes),
            None => ByteLookup::Miss,
        }
    }

    async fn put(&self, key: &str, bytes: Vec<u8>, keep_for: Option<Duration>) {
        self.insert_raw(key, bytes);
        self.puts.lock().unwrap().push((key.to_string(), keep_for));
    }

    async fn remove(&self, key: &str) {
        self.items.lock().unwrap().remove(key);
        self.removed.lock().unwrap().push(key.to_string());
    }
}

impl<const DROPS: bool> ListKeys for MemoryBytes<DROPS> {
    async fn keys(&self, prefix: &str) -> Option<Vec<String>> {
        Some(
            self.held()
                .into_iter()
                .filter(|key| key.starts_with(prefix))
                .collect(),
        )
    }
}

/// Stands in for a platform store of bytes that reserves a missing key for
/// the caller that should fill it, as a cache with request collapsing does.
#[derive(Default)]
pub struct ReservingBytes {
    pub bytes: MemoryBytes,
    released: Arc<AtomicUsize>,
    fills: Mutex<Vec<(String, Option<Duration>)>>,
}

/// A reservation in a [`ReservingBytes`], which counts its release.
pub struct BytesReservation {
    key: String,
    released: Arc<AtomicUsize>,
}

impl Drop for BytesReservation {
    fn drop(&mut self) {
        self.released.fetch_add(1, Ordering::SeqCst);
    }
}

impl ReservingBytes {
    /// How many reservations have been released, filled or not.
    pub fn released(&self) -> usize {
        self.released.load(Ordering::SeqCst)
    }

    /// The keys filled through a reservation, with the lifetime each was
    /// given.
    pub fn fills(&self) -> Vec<(String, Option<Duration>)> {
        self.fills.lock().unwrap().clone()
    }
}

impl ByteStore for ReservingBytes {
    type Reservation = BytesReservation;

    async fn get(&self, key: &str) -> ByteLookup<BytesReservation> {
        match self.bytes.raw(key) {
            Some(bytes) => ByteLookup::Hit(bytes),
            None => ByteLookup::Reserved(BytesReservation {
                key: key.to_string(),
                released: Arc::clone(&self.released),
            }),
        }
    }

    async fn put(&self, key: &str, bytes: Vec<u8>, keep_for: Option<Duration>) {
        self.bytes.put(key, bytes, keep_for).await
    }

    async fn fill(
        &self,
        key: &str,
        reservation: BytesReservation,
        bytes: Vec<u8>,
        keep_for: Option<Duration>,
    ) {
        assert_eq!(
            reservation.key, key,
            "filled under the key it was reserved for"
        );
        self.fills.lock().unwrap().push((key.to_string(), keep_for));
        self.bytes.put(key, bytes, keep_for).await;
    }

    async fn remove(&self, key: &str) {
        self.bytes.remove(key).await
    }
}
