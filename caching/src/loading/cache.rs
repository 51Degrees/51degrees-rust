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

//! The loading cache.

use std::future::Future;
use std::hash::Hash;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use super::clock::{self, Clock};
use super::entry::{earliest, Entry, Loaded};
use super::flight::{Flights, Lead, Outcome, Role, Wait};
use super::loader::ValueLoader;
use super::lru_store::LruStore;
use super::spawn::{Inline, LoadRunner, Spawned, SpawnedLocal};
use super::store::{Lookup, Store};
use crate::config::default_concurrency;

/// A cache that loads a missing value once, however many callers ask for it
/// at the same time.
///
/// The first caller for a key that is not in the [`Store`] runs the
/// [`ValueLoader`]. Every caller that asks for the key meanwhile waits for that
/// load, and all of them get its value or its error. A failed load is not
/// stored, so the next caller loads again. If the caller running the load is
/// dropped, a waiting caller takes over.
///
/// A `LoadingCache` is itself a [`ValueLoader`], so a cache over a small store in
/// memory can load from a cache over a shared store, which loads from the
/// source. Each cache has its own lifetimes, and a cache never keeps a copy
/// longer than the copy it loaded it from.
///
/// # Example
///
/// ```
/// use std::time::Duration;
/// use fiftyone_caching::{from_fn, LoadingCache, LruStore};
///
/// // The shared store would be a platform's key-value store. A store in
/// // memory stands in for it here.
/// let shared = LoadingCache::builder(
///     LruStore::builder().size(10_000).build(),
///     from_fn(|url: String| async move { Ok::<_, String>(format!("page {url}")) }),
/// )
/// .time_to_live(Duration::from_secs(24 * 60 * 60))
/// .time_to_idle(Duration::from_secs(60 * 60))
/// .build();
///
/// // A brief copy in process memory, loading from the shared cache.
/// let cache = LoadingCache::builder(LruStore::builder().size(1000).build(), shared)
///     .time_to_live(Duration::from_secs(5))
///     .build();
/// # let _ = &cache;
/// ```
pub struct LoadingCache<K, V, S, L, R = Inline>
where
    L: ValueLoader<K, V>,
{
    inner: Arc<Inner<K, V, S, L>>,
    runner: R,
}

/// The loads in progress for one cache, keyed by what they load.
type InFlight<K, V, E> = Arc<Flights<K, Result<Loaded<V>, E>>>;

/// The parts of a cache a load uses, shared with loads running as tasks.
struct Inner<K, V, S, L>
where
    L: ValueLoader<K, V>,
{
    store: S,
    loader: L,
    lifetimes: Lifetimes,
    clock: Arc<dyn Clock>,
    flights: InFlight<K, V, L::Error>,
}

impl<K, V, S, L> LoadingCache<K, V, S, L>
where
    K: Hash + Eq + Clone,
    V: Clone,
    S: Store<K, V>,
    L: ValueLoader<K, V>,
{
    /// Starts building a cache that keeps its entries in `store` and loads
    /// missing values with `loader`.
    pub fn builder(store: S, loader: L) -> LoadingCacheBuilder<K, V, S, L> {
        LoadingCacheBuilder {
            store,
            loader,
            time_to_live: None,
            time_to_idle: None,
            renewal_window: None,
            clock: None,
            concurrency: default_concurrency(),
            runner: Inline,
            types: PhantomData,
        }
    }
}

impl<K, V, S, L, R> LoadingCache<K, V, S, L, R>
where
    K: Hash + Eq + Clone,
    V: Clone,
    S: Store<K, V>,
    L: ValueLoader<K, V>,
    R: LoadRunner<K, V, S, L>,
{
    /// The value for `key`, from the store or loaded.
    ///
    /// # Panics
    ///
    /// With a spawner, if two loads this caller waits on are lost with their
    /// tasks, as [`get_loaded`](Self::get_loaded) describes.
    pub async fn get(&self, key: &K) -> Result<V, L::Error> {
        self.get_loaded(key).await.map(|loaded| loaded.value)
    }

    /// The value for `key` with the time it was written and the time this
    /// cache's copy stops being usable. A cache above uses these times to
    /// keep its own copy no longer.
    ///
    /// # Panics
    ///
    /// With a spawner, if two loads this caller waits on are lost with their
    /// tasks, each by a panic in the load or by the spawner dropping the
    /// task. A load lost once is asked for once more. A second loss is taken
    /// to mean every load would be lost, so the caller panics rather than
    /// ask without end.
    pub async fn get_loaded(&self, key: &K) -> Result<Loaded<V>, L::Error> {
        // A copy the store gives without waiting is served here, with no
        // part in the key's load.
        if let Some(entry) = self.inner.store.try_get(key).await {
            if let Some(served) = self.inner.usable(entry) {
                return Ok(served);
            }
        }
        let mut lost = false;
        loop {
            let outcome = match self.inner.flights.join_or_lead(key) {
                Role::Wait(wait) => wait.await,
                // The caller looks in the store itself, so a hit never
                // waits for a task. Only a load goes to the runner.
                Role::Lead(lead) => match self.inner.look(key, &lead).await {
                    Found::Fresh(served) => Outcome::Done(Ok(served)),
                    Found::Missing(reservation) => {
                        let job = Job {
                            inner: Arc::clone(&self.inner),
                            key: key.clone(),
                            lead,
                            reservation,
                        };
                        self.runner.run(job).await
                    }
                },
            };
            match outcome {
                Outcome::Done(result) => return result,
                // The caller leading the key was dropped, so this one asks
                // again and may lead. Each time is one caller fewer, so the
                // asking ends.
                Outcome::Abandoned => {}
                // The task doing the load was dropped. A task can be dropped
                // by chance, as when its thread is stopping, so the load is
                // asked for once more. A second loss is more likely a load
                // that panics or a spawner that runs nothing, which would
                // be lost every time.
                Outcome::Lost => {
                    assert!(!lost, "{LOST_TWICE}");
                    lost = true;
                }
            }
        }
    }

    /// Removes `key` from this cache's store. Caches below keep their copies.
    pub async fn remove(&self, key: &K) {
        self.inner.store.remove(key).await;
    }

    /// The store this cache keeps its entries in.
    pub fn store(&self) -> &S {
        &self.inner.store
    }
}

/// What a caller panics with when it has lost two loads.
const LOST_TWICE: &str =
    "a load was lost twice, by a panic in it or by a spawner dropping its task";

/// What the store held for a key a caller is leading.
enum Found<V, R> {
    /// A fresh entry, already given to the waiting callers.
    Fresh(Loaded<V>),
    /// Nothing usable, so the value must be loaded, filling the reservation
    /// when the store gave one.
    Missing(Option<R>),
}

/// One load, owning what it uses so it can run in a task of its own.
pub struct Job<K, V, S, L>
where
    K: Hash + Eq,
    S: Store<K, V>,
    L: ValueLoader<K, V>,
{
    inner: Arc<Inner<K, V, S, L>>,
    key: K,
    lead: Lead<K, Result<Loaded<V>, L::Error>>,
    reservation: Option<S::Reservation>,
}

impl<K, V, S, L> Job<K, V, S, L>
where
    K: Hash + Eq + Clone,
    V: Clone,
    S: Store<K, V>,
    L: ValueLoader<K, V>,
{
    /// A wait on this load's result.
    pub(crate) fn wait(&self) -> Wait<Result<Loaded<V>, L::Error>> {
        self.lead.wait()
    }

    /// This load as a task's, ready to be given to a spawner. Dropped from
    /// here on without a result, it counts as lost.
    pub(crate) fn for_task(mut self) -> Self {
        self.lead.hand_to_task();
        self
    }

    /// Does the load and gives its result to the waiting callers.
    pub(crate) async fn run(self) -> Result<Loaded<V>, L::Error> {
        let Job {
            inner,
            key,
            lead,
            reservation,
        } = self;
        inner.load(&key, lead, reservation).await
    }

    /// Does the load, for a task with no caller to return the result to.
    pub(crate) async fn finish(self) {
        let _ = self.run().await;
    }
}

impl<K, V, S, L> Inner<K, V, S, L>
where
    K: Hash + Eq + Clone,
    V: Clone,
    S: Store<K, V>,
    L: ValueLoader<K, V>,
{
    /// `entry` as it is served, when it is fresh. A copy due to be renewed
    /// is left to the caller leading the key, so one caller writes the
    /// renewal however many use the copy at once.
    fn usable(&self, entry: Entry<V>) -> Option<Loaded<V>> {
        // A copy with no end and no idle time is usable whatever the time,
        // so the clock is read only for a copy that has one.
        if entry.expires.is_some() || self.lifetimes.time_to_idle.is_some() {
            let now = self.clock.now();
            if !self.lifetimes.is_fresh(&entry, now) || self.lifetimes.renewal_due(&entry, now) {
                return None;
            }
        }
        Some(self.lifetimes.serve(entry))
    }

    /// Looks for `key` in the store, for the caller leading it. A fresh
    /// entry is renewed if the use is due to renew it, then given to the
    /// waiting callers and returned.
    async fn look(
        &self,
        key: &K,
        lead: &Lead<K, Result<Loaded<V>, L::Error>>,
    ) -> Found<V, S::Reservation> {
        let found = self.store.get(key).await;
        // Read after the lookup, which may have waited for another process.
        let now = self.clock.now();
        match found {
            Lookup::Hit(entry) if self.lifetimes.is_fresh(&entry, now) => {
                let renewed = self.lifetimes.renewal(&entry, now);
                let served = self.lifetimes.served(renewed.as_ref().unwrap_or(&entry));
                if let Some(renewed) = renewed {
                    self.write(key, None, &renewed, now).await;
                }
                lead.publish(Ok(served.clone()));
                Found::Fresh(served)
            }
            Lookup::Hit(_) | Lookup::Miss => Found::Missing(None),
            Lookup::Reserved(reservation) => Found::Missing(Some(reservation)),
        }
    }

    /// Loads the value for `key`, writes it to the store, then gives it to
    /// the waiting callers, so a caller that arrives after them finds it in
    /// the store rather than loading it again.
    async fn load(
        &self,
        key: &K,
        lead: Lead<K, Result<Loaded<V>, L::Error>>,
        reservation: Option<S::Reservation>,
    ) -> Result<Loaded<V>, L::Error> {
        match self.loader.load(key).await {
            Ok(loaded) => {
                let now = self.clock.now();
                let entry = self.lifetimes.copy(loaded, now);
                let served = self.lifetimes.served(&entry);
                self.write(key, reservation, &entry, now).await;
                lead.publish(Ok(served.clone()));
                Ok(served)
            }
            Err(error) => {
                // Release callers waiting in the store before the ones
                // waiting here.
                drop(reservation);
                lead.publish(Err(error.clone()));
                Err(error)
            }
        }
    }

    /// Writes `entry` to the store, filling `reservation` when there is one.
    /// An entry already past its end at `now` is not written.
    async fn write(
        &self,
        key: &K,
        reservation: Option<S::Reservation>,
        entry: &Entry<V>,
        now: SystemTime,
    ) {
        let Some(keep_for) = self.lifetimes.keep_for(entry, now) else {
            return;
        };
        match reservation {
            Some(reservation) => self.store.fill(key, reservation, entry, keep_for).await,
            None => self.store.put(key, entry, keep_for).await,
        }
    }
}

impl<K, V, S, L, R> ValueLoader<K, V> for LoadingCache<K, V, S, L, R>
where
    K: Hash + Eq + Clone,
    V: Clone,
    S: Store<K, V>,
    L: ValueLoader<K, V>,
    R: LoadRunner<K, V, S, L>,
{
    type Error = L::Error;

    fn load(&self, key: &K) -> impl Future<Output = Result<Loaded<V>, Self::Error>> {
        self.get_loaded(key)
    }
}

/// A [`LoadingCache`] over the least recently used cache in process memory,
/// as `LruLoadingCache` in the .NET and Java pipelines.
pub type LruLoadingCache<K, V, L, R = Inline> = LoadingCache<K, V, LruStore<K, V>, L, R>;

impl<K, V, L> LoadingCache<K, V, LruStore<K, V>, L>
where
    K: Hash + Eq + Clone + Send + Sync,
    V: Clone + Send + Sync,
    L: ValueLoader<K, V>,
{
    /// A cache of up to `size` values in process memory, the least recently
    /// used evicted first, loading missing values with `loader`. The values
    /// have no lifetime, and the clock is the system clock. Use
    /// [`LoadingCache::builder`] with an [`LruStore`] for more settings.
    ///
    /// # Panics
    ///
    /// On `wasm32-unknown-unknown`, which has no system clock.
    pub fn new(size: usize, loader: L) -> Self {
        LoadingCache::builder(LruStore::builder().size(size).build(), loader).build()
    }
}

/// Builds a [`LoadingCache`].
pub struct LoadingCacheBuilder<K, V, S, L, R = Inline> {
    store: S,
    loader: L,
    time_to_live: Option<Duration>,
    time_to_idle: Option<Duration>,
    renewal_window: Option<Duration>,
    clock: Option<Arc<dyn Clock>>,
    concurrency: usize,
    runner: R,
    types: PhantomData<fn(K) -> V>,
}

impl<K, V, S, L, R> LoadingCacheBuilder<K, V, S, L, R>
where
    K: Hash + Eq + Clone,
    V: Clone,
    S: Store<K, V>,
    L: ValueLoader<K, V>,
{
    /// How long a copy is used, from when this cache writes it. Without one
    /// a copy lasts as long as the value it was loaded with allows, which
    /// for a source that sets no expiry is until it is evicted.
    pub fn time_to_live(mut self, time_to_live: Duration) -> Self {
        self.time_to_live = Some(time_to_live);
        self
    }

    /// How long a copy lasts unused. Using a copy renews it, at most once
    /// per [renewal window](Self::renewal_window), and the store is told to
    /// keep it no longer than this, so a copy left unused leaves the store.
    pub fn time_to_idle(mut self, time_to_idle: Duration) -> Self {
        self.time_to_idle = Some(time_to_idle);
        self
    }

    /// The least time between two renewals of one copy, so a busy key does
    /// not write to the store on every use. A copy can therefore leave up to
    /// this much sooner than [`time_to_idle`](Self::time_to_idle) after its
    /// last use. Defaults to a quarter of the idle time, and may be at most
    /// half of it.
    pub fn renewal_window(mut self, renewal_window: Duration) -> Self {
        self.renewal_window = Some(renewal_window);
        self
    }

    /// The clock the cache reads. Defaults to the system clock, which
    /// `wasm32-unknown-unknown` does not have.
    pub fn clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = Some(clock);
        self
    }

    /// How many shards the record of loads in progress is split into, so
    /// callers loading different keys rarely wait for the same lock.
    /// Defaults to the number of processors.
    pub fn concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency;
        self
    }

    /// Runs each load as a task of its own, started by `spawner` on any
    /// thread, so a dropped caller neither stops a load nor starts another,
    /// as a .NET `Lazy<Task>` does. Every caller, the first included, waits
    /// for the load's result.
    ///
    /// A load can move to another thread, so the cache's key, value, error,
    /// store and loader must be `Send`, `Sync` and `'static`. The load's own
    /// future need not be `Send`, because the spawner makes it on the thread
    /// that runs it. Without a spawner, nothing is asked of these types.
    pub fn spawner<P>(self, spawner: P) -> LoadingCacheBuilder<K, V, S, L, Spawned<P>>
    where
        Spawned<P>: LoadRunner<K, V, S, L>,
    {
        self.runner(Spawned(spawner))
    }

    /// Runs each load as a task of its own on the current thread, started by
    /// `spawner`, so a dropped caller neither stops a load nor starts
    /// another. Every caller, the first included, waits for the load's
    /// result. The cache's key, value, error, store and loader must be
    /// `'static`, since the task owns them.
    pub fn local_spawner<P>(self, spawner: P) -> LoadingCacheBuilder<K, V, S, L, SpawnedLocal<P>>
    where
        SpawnedLocal<P>: LoadRunner<K, V, S, L>,
    {
        self.runner(SpawnedLocal(spawner))
    }

    fn runner<R2>(self, runner: R2) -> LoadingCacheBuilder<K, V, S, L, R2> {
        LoadingCacheBuilder {
            store: self.store,
            loader: self.loader,
            time_to_live: self.time_to_live,
            time_to_idle: self.time_to_idle,
            renewal_window: self.renewal_window,
            clock: self.clock,
            concurrency: self.concurrency,
            runner,
            types: PhantomData,
        }
    }

    /// Builds the cache.
    ///
    /// # Panics
    ///
    /// If the renewal window is more than half the idle time, or on
    /// `wasm32-unknown-unknown` when no [clock](Self::clock) was given.
    pub fn build(self) -> LoadingCache<K, V, S, L, R> {
        let renewal_window = match (self.time_to_idle, self.renewal_window) {
            (Some(idle), Some(window)) => {
                assert!(
                    window <= idle / 2,
                    "the renewal window must be at most half the idle time"
                );
                window
            }
            (Some(idle), None) => idle / 4,
            (None, window) => window.unwrap_or_default(),
        };
        LoadingCache {
            inner: Arc::new(Inner {
                store: self.store,
                loader: self.loader,
                lifetimes: Lifetimes {
                    time_to_live: self.time_to_live,
                    time_to_idle: self.time_to_idle,
                    renewal_window,
                },
                clock: clock::given_or_system(self.clock),
                flights: Arc::new(Flights::new(self.concurrency)),
            }),
            runner: self.runner,
        }
    }
}

/// How long one cache's copies may be used.
#[derive(Debug, Clone, Copy)]
struct Lifetimes {
    time_to_live: Option<Duration>,
    time_to_idle: Option<Duration>,
    renewal_window: Duration,
}

impl Lifetimes {
    /// Whether `entry` may be used at `now`.
    fn is_fresh<V>(&self, entry: &Entry<V>, now: SystemTime) -> bool {
        let before = |end: SystemTime| now < end;
        entry.expires.is_none_or(before) && self.idle_end(entry).is_none_or(before)
    }

    /// When `entry` lapses unless it is used and renewed.
    fn idle_end<V>(&self, entry: &Entry<V>) -> Option<SystemTime> {
        self.time_to_idle
            .and_then(|idle| entry.renewed.checked_add(idle))
    }

    /// This cache's copy of a loaded value, written at `now`. It ends at
    /// this cache's time to live or the loaded value's own end, whichever is
    /// sooner.
    fn copy<V>(&self, loaded: Loaded<V>, now: SystemTime) -> Entry<V> {
        let own_end = self.time_to_live.and_then(|ttl| now.checked_add(ttl));
        Entry {
            value: loaded.value,
            written: loaded.written.unwrap_or(now),
            expires: earliest(own_end, loaded.expires),
            renewed: now,
        }
    }

    /// Whether the use of `entry` at `now` is due to renew it, and renewing
    /// it lets it last longer.
    fn renewal_due<V>(&self, entry: &Entry<V>, now: SystemTime) -> bool {
        let Some(idle_end) = self.idle_end(entry) else {
            return false;
        };
        let due = entry
            .renewed
            .checked_add(self.renewal_window)
            .is_some_and(|due| now >= due);
        due && entry.expires.is_none_or(|end| end > idle_end)
    }

    /// The copy of `entry` renewed at `now`, when the use at `now` is due to
    /// renew it.
    fn renewal<V: Clone>(&self, entry: &Entry<V>, now: SystemTime) -> Option<Entry<V>> {
        self.renewal_due(entry, now).then(|| Entry {
            renewed: now,
            ..entry.clone()
        })
    }

    /// How `entry` is given to callers, including a cache above. Its end is
    /// brought forward to one renewal window before its idle end, so a cache
    /// above comes back for the value while this cache can still renew it.
    fn serve<V>(&self, entry: Entry<V>) -> Loaded<V> {
        let renew_by = self.time_to_idle.and_then(|idle| {
            entry
                .renewed
                .checked_add(idle.saturating_sub(self.renewal_window))
        });
        Loaded {
            value: entry.value,
            written: Some(entry.written),
            expires: earliest(entry.expires, renew_by),
        }
    }

    /// As [`serve`](Self::serve), for an entry the caller still needs.
    fn served<V: Clone>(&self, entry: &Entry<V>) -> Loaded<V> {
        self.serve(entry.clone())
    }

    /// How long the store should keep `entry`, written at `now`. `None`
    /// means the entry is already past its end and is not written. The
    /// store keeps it until its end, or for the idle time, whichever is
    /// sooner, since a renewal writes it again.
    fn keep_for<V>(&self, entry: &Entry<V>, now: SystemTime) -> Option<Option<Duration>> {
        let until_end = match entry.expires {
            Some(end) => match end.duration_since(now) {
                Ok(left) if !left.is_zero() => Some(left),
                _ => return None,
            },
            None => None,
        };
        Some(match (until_end, self.time_to_idle) {
            (Some(left), Some(idle)) => Some(left.min(idle)),
            (left, idle) => left.or(idle),
        })
    }
}
