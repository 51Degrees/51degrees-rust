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

//! Stores over Workers KV and the Cache API, a spawner that keeps a load
//! running with the request's `wait_until`, and a clock, for
//! `wasm32-unknown-unknown` on Cloudflare Workers, turned on by the
//! `cloudflare` feature. Turn default features off, because the `pipeline`
//! feature does not build for that target.
//!
//! [`KvStore`] is global and durable, so it suits the cache of record.
//! [`CacheApi`] is the cache of the data center serving the request, so it
//! suits a layer between memory and the KV store. Each is a
//! [`ByteStore`](crate::ByteStore), made the store of a
//! [`LoadingCache`](crate::LoadingCache) by
//! [`EncodedStore`](crate::EncodedStore). The target has no system clock, so
//! give every builder a [`DateClock`].
//!
//! # Sharing a cache between requests
//!
//! A Worker isolate serves many requests at once, and a cache shared by them
//! makes each one that misses wait for the first one's load. The runtime has
//! three rules that bear on this. It may stop a load once the request that
//! started it has its response. It stops a request that has nothing of its
//! own pending as hung. And it refuses I/O a request makes in another
//! request's turn, such as building its response after being woken by that
//! request's load.
//!
//! Wrap the handling of each request in [`with_context`], and build the
//! cache with the [`WaitUntil`] spawner. The spawner keeps each load running
//! with the `wait_until` of the request that started it, and the wrapper
//! keeps a waiting request alive with a short timer and lets it go on only
//! in its own turns. Without the wrapper, a request that waits on another
//! request's load fails.
//!
//! ```no_run
//! use std::cell::OnceCell;
//! use std::rc::Rc;
//! use std::sync::Arc;
//! use std::time::Duration;
//!
//! use fiftyone_caching::cloudflare::{self, DateClock, KvStore, WaitUntil};
//! use fiftyone_caching::{EncodedStore, Loaded, LoadingCache, SpawnedLocal, Utf8, ValueLoader};
//! use worker::{Context, Env, Request, Response, Result};
//!
//! /// Where a value comes from when no store holds it.
//! struct Origin;
//!
//! impl ValueLoader<String, String> for Origin {
//!     type Error = String;
//!
//!     async fn load(&self, key: &String) -> std::result::Result<Loaded<String>, String> {
//!         Ok(Loaded::new(format!("value of {key}")))
//!     }
//! }
//!
//! type Cache =
//!     LoadingCache<String, String, EncodedStore<KvStore, Utf8>, Origin, SpawnedLocal<WaitUntil>>;
//!
//! thread_local! {
//!     // One cache for every request the isolate serves.
//!     static CACHE: OnceCell<Rc<Cache>> = const { OnceCell::new() };
//! }
//!
//! fn cache(env: &Env) -> Result<Rc<Cache>> {
//!     if let Some(cache) = CACHE.with(|cache| cache.get().cloned()) {
//!         return Ok(cache);
//!     }
//!     let store = EncodedStore::builder(KvStore::from(env.kv("CACHE")?), Utf8)
//!         .clock(Arc::new(DateClock))
//!         .build();
//!     let cache = LoadingCache::builder(store, Origin)
//!         .time_to_live(Duration::from_secs(3600))
//!         .clock(Arc::new(DateClock))
//!         .local_spawner(WaitUntil)
//!         .build();
//!     Ok(CACHE.with(|shared| Rc::clone(shared.get_or_init(|| Rc::new(cache)))))
//! }
//!
//! /// The Worker's fetch handler, which a Worker marks `#[event(fetch)]`.
//! async fn fetch(req: Request, env: Env, ctx: Context) -> Result<Response> {
//!     let cache = cache(&env)?;
//!     let value = cloudflare::with_context(&ctx, cache.get(&req.path())).await;
//!     Response::ok(value.unwrap_or_else(|error| error))
//! }
//! # drop(fetch);
//! ```

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

use crate::LoadTask;

/// The shortest time to live Workers KV accepts, in seconds.
pub(crate) const KV_SHORTEST: u64 = 60;

/// The lifetime given to an entry written with no lifetime, where the
/// platform needs one.
pub(crate) const LONGEST: Duration = Duration::from_secs(365 * 24 * 60 * 60);

/// The whole seconds `keep_for` lasts, rounded up.
fn whole_seconds(keep_for: Duration) -> u64 {
    keep_for
        .as_secs()
        .saturating_add(u64::from(keep_for.subsec_nanos() > 0))
}

/// The Workers KV `expiration_ttl` for an entry kept for `keep_for`. Workers
/// KV refuses a time to live under a minute, so a shorter one is raised to a
/// minute and the entry's drop time, checked on every read, holds the exact
/// lifetime.
pub(crate) fn kv_expiration_ttl(keep_for: Option<Duration>) -> Option<u64> {
    keep_for.map(|keep_for| whole_seconds(keep_for).max(KV_SHORTEST))
}

/// The `Cache-Control` header the Cache API keeps an entry by.
pub(crate) fn cache_control(keep_for: Option<Duration>) -> String {
    format!("max-age={}", whole_seconds(keep_for.unwrap_or(LONGEST)))
}

/// The URL the Cache API keeps `key` under, below `base`. Keys come from
/// [`EncodedStore`](crate::EncodedStore), so they are already safe in a URL
/// path.
pub(crate) fn cache_url(base: &str, key: &str) -> String {
    format!("{}/{key}", base.trim_end_matches('/'))
}

/// A short wait. A request keeps one pending while it waits, see
/// [`Scoped`].
pub(crate) type Tick = Pin<Box<dyn Future<Output = ()>>>;

/// One request's handling, with how it keeps a started load running and
/// how it waits a tick.
pub(crate) struct Request {
    id: u64,
    keep: Box<dyn Fn(LoadTask)>,
    tick: Box<dyn Fn() -> Tick>,
}

impl Request {
    pub(crate) fn new(
        keep: impl Fn(LoadTask) + 'static,
        tick: impl Fn() -> Tick + 'static,
    ) -> Rc<Self> {
        let id = NEXT_ID.with(|next| {
            let id = next.get();
            next.set(id.wrapping_add(1));
            id
        });
        Rc::new(Request {
            id,
            keep: Box::new(keep),
            tick: Box::new(tick),
        })
    }
}

thread_local! {
    static NEXT_ID: Cell<u64> = const { Cell::new(0) };
    /// The request being polled, if any.
    static CURRENT: RefCell<Option<Rc<Request>>> = const { RefCell::new(None) };
}

/// A future polled as part of one request.
///
/// The runtime stops a request that has nothing of its own pending, and
/// refuses I/O a request makes outside its own turns. A request waiting on
/// another request's load has nothing of its own pending, and when that
/// load wakes it, the wake comes in the other request's turn. So the
/// future is polled with a waker of its own, which ignores a wake made
/// while another request is being polled, and keeps a tick pending while
/// the future waits. The tick tells the runtime the request is still
/// working, and when it ends the future is polled in this request's own
/// turn. Every other wake goes straight through.
pub(crate) struct Scoped<F> {
    request: Rc<Request>,
    waker: Arc<ScopedWaker>,
    tick: Option<Tick>,
    future: Pin<Box<F>>,
}

impl<F: Future> Scoped<F> {
    pub(crate) fn new(request: Rc<Request>, future: F) -> Self {
        let waker = Arc::new(ScopedWaker {
            request: request.id,
            real: Mutex::new(Waker::noop().clone()),
        });
        Scoped {
            request,
            waker,
            tick: None,
            future: Box::pin(future),
        }
    }
}

impl<F: Future> Future for Scoped<F> {
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        let this = &mut *self;
        if let Some(tick) = &mut this.tick {
            if tick.as_mut().poll(cx).is_ready() {
                this.tick = None;
            }
        }
        this.waker.set(cx.waker());
        let waker = Waker::from(Arc::clone(&this.waker));
        let polled = {
            let _current = Current::enter(Rc::clone(&this.request));
            this.future.as_mut().poll(&mut Context::from_waker(&waker))
        };
        if polled.is_pending() && this.tick.is_none() {
            let mut tick = (this.request.tick)();
            if tick.as_mut().poll(cx).is_ready() {
                cx.waker().wake_by_ref();
            } else {
                this.tick = Some(tick);
            }
        }
        polled
    }
}

/// The waker a [`Scoped`] future is polled with.
struct ScopedWaker {
    request: u64,
    real: Mutex<Waker>,
}

impl ScopedWaker {
    fn set(&self, waker: &Waker) {
        let mut real = self.real.lock().unwrap_or_else(PoisonError::into_inner);
        if !real.will_wake(waker) {
            *real = waker.clone();
        }
    }
}

impl Wake for ScopedWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        let in_other_turn = CURRENT.with(|current| {
            current
                .borrow()
                .as_ref()
                .is_some_and(|request| request.id != self.request)
        });
        if !in_other_turn {
            self.real
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .wake_by_ref();
        }
    }
}

/// Makes a request the one being polled, and puts back the one before when
/// dropped, so polls can nest and a panic leaves nothing behind.
struct Current(Option<Rc<Request>>);

impl Current {
    fn enter(request: Rc<Request>) -> Self {
        Current(CURRENT.with(|current| current.replace(Some(request))))
    }
}

impl Drop for Current {
    fn drop(&mut self) {
        let previous = self.0.take();
        CURRENT.with(|current| *current.borrow_mut() = previous);
    }
}

/// Hands `task` to the request being polled, to keep running as part of
/// that request, or to `fallback` when no request is being polled.
pub(crate) fn keep_running(task: LoadTask, fallback: impl FnOnce(LoadTask)) {
    // The request is cloned out first, so keeping the task may poll it.
    let request = CURRENT.with(|current| current.borrow().clone());
    match request {
        Some(request) => {
            let task: LoadTask = Box::pin(Scoped::new(Rc::clone(&request), task));
            (request.keep)(task);
        }
        None => fallback(task),
    }
}

#[cfg(all(feature = "cloudflare", target_arch = "wasm32", target_os = "unknown"))]
pub use binding::{with_context, CacheApi, DateClock, KvStore, WaitUntil};

#[cfg(all(feature = "cloudflare", target_arch = "wasm32", target_os = "unknown"))]
mod binding {
    use std::convert::Infallible;
    use std::future::Future;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use ::worker::wasm_bindgen::{JsCast, JsValue};

    use super::{cache_control, cache_url, keep_running, kv_expiration_ttl, Request, Scoped};
    use crate::{ByteLookup, ByteStore, Clock, LoadTask, SpawnLocal};

    /// A store over a Workers KV namespace, global and durable.
    ///
    /// Each entry is written with an `expiration_ttl` of its lifetime in
    /// whole seconds, rounded up, and at least the minute Workers KV allows.
    /// [`EncodedStore`](crate::EncodedStore) checks each entry's own drop
    /// time when it reads it, so a shorter lifetime still holds. A write
    /// can take a minute or more to be seen in other locations, a key takes
    /// at most one write a second, and a write over that limit is dropped.
    /// Keys are up to 512 bytes.
    pub struct KvStore {
        kv: ::worker::KvStore,
    }

    impl From<::worker::KvStore> for KvStore {
        fn from(kv: ::worker::KvStore) -> Self {
            KvStore { kv }
        }
    }

    impl ByteStore for KvStore {
        type Reservation = Infallible;

        async fn get(&self, key: &str) -> ByteLookup<Infallible> {
            match self.kv.get(key).bytes().await {
                Ok(Some(bytes)) => ByteLookup::Hit(bytes),
                _ => ByteLookup::Miss,
            }
        }

        async fn put(&self, key: &str, bytes: Vec<u8>, keep_for: Option<Duration>) {
            let Ok(mut put) = self.kv.put_bytes(key, &bytes) else {
                return;
            };
            if let Some(ttl) = kv_expiration_ttl(keep_for) {
                put = put.expiration_ttl(ttl);
            }
            let _ = put.execute().await;
        }

        async fn remove(&self, key: &str) {
            let _ = self.kv.delete(key).await;
        }
    }

    /// A store over the Cache API, the cache of the data center serving the
    /// request.
    ///
    /// Each entry is kept under a URL below a base URL the Worker chooses,
    /// such as one on its own zone, with a `Cache-Control` max age of its
    /// lifetime, rounded up to whole seconds, or a year when it has none.
    /// [`remove`](ByteStore::remove) deletes it in this data center only.
    /// The Cache API does nothing in dashboard previews and is not available
    /// to a Worker behind Cloudflare Access.
    pub struct CacheApi {
        cache: ::worker::Cache,
        base: String,
    }

    impl CacheApi {
        /// A store over the Worker's default cache, keeping entries below
        /// `base`, an absolute `https` URL.
        pub fn new(base: impl Into<String>) -> Self {
            CacheApi::with_cache(::worker::Cache::default(), base)
        }

        /// A store over `cache`, such as one opened by name, keeping
        /// entries below `base`, an absolute `https` URL.
        pub fn with_cache(cache: ::worker::Cache, base: impl Into<String>) -> Self {
            CacheApi {
                cache,
                base: base.into(),
            }
        }
    }

    impl ByteStore for CacheApi {
        type Reservation = Infallible;

        async fn get(&self, key: &str) -> ByteLookup<Infallible> {
            match self.cache.get(cache_url(&self.base, key), false).await {
                Ok(Some(mut response)) => match response.bytes().await {
                    Ok(bytes) => ByteLookup::Hit(bytes),
                    Err(_) => ByteLookup::Miss,
                },
                _ => ByteLookup::Miss,
            }
        }

        async fn put(&self, key: &str, bytes: Vec<u8>, keep_for: Option<Duration>) {
            let Ok(mut response) = ::worker::Response::from_bytes(bytes) else {
                return;
            };
            if response
                .headers_mut()
                .set("cache-control", &cache_control(keep_for))
                .is_err()
            {
                return;
            }
            let _ = self.cache.put(cache_url(&self.base, key), response).await;
        }

        async fn remove(&self, key: &str) {
            let _ = self.cache.delete(cache_url(&self.base, key), false).await;
        }
    }

    /// The Worker's clock, JavaScript's `Date.now()`, to the millisecond.
    /// The runtime moves it on only between I/O, so it is enough for
    /// lifetimes of seconds and more.
    #[derive(Debug, Clone, Copy, Default)]
    pub struct DateClock;

    impl Clock for DateClock {
        fn now(&self) -> SystemTime {
            UNIX_EPOCH + Duration::from_millis(::worker::Date::now().as_millis())
        }
    }

    /// How often a request waiting on another request's load is polled.
    const TICK: Duration = Duration::from_millis(10);

    /// Runs each load as a task of its own, kept running with the
    /// `wait_until` of the request whose call started it, so it finishes
    /// even after that request has its response. Give it to
    /// [`LoadingCacheBuilder::local_spawner`](crate::LoadingCacheBuilder::local_spawner),
    /// and wrap each request's handling in [`with_context`].
    ///
    /// A load started outside [`with_context`] still runs as a task of its
    /// own, but nothing keeps it running once its request has its response.
    #[derive(Debug, Clone, Copy, Default)]
    pub struct WaitUntil;

    impl SpawnLocal for WaitUntil {
        fn spawn_local(&self, task: LoadTask) {
            keep_running(task, ::worker::wasm_bindgen_futures::spawn_local);
        }
    }

    /// Runs `future`, the handling of one request, so it can share a cache
    /// with the other requests the isolate serves.
    ///
    /// A load that a cache with the [`WaitUntil`] spawner starts while the
    /// future runs is kept running with `ctx.wait_until`. While the future
    /// waits, it keeps a 10 millisecond timer pending, so the runtime does
    /// not stop it as hung when it waits on another request's load, and it
    /// goes on in its own turns rather than in that request's.
    pub fn with_context<F: Future>(
        ctx: &::worker::Context,
        future: F,
    ) -> impl Future<Output = F::Output> {
        let inner: &::worker::worker_sys::Context = ctx.as_ref();
        let value: &JsValue = inner.as_ref();
        // A second handle on the same JavaScript context object.
        let ctx = ::worker::Context::new(value.clone().unchecked_into());
        let request = Request::new(
            move |task: LoadTask| ctx.wait_until(task),
            || Box::pin(::worker::Delay::from(TICK)),
        );
        Scoped::new(request, future)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::future::{pending, poll_fn, Future};
    use std::rc::Rc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};

    use super::*;

    #[test]
    fn raises_a_kv_time_to_live_to_the_minute_workers_kv_allows() {
        let ms = Duration::from_millis;
        assert_eq!(kv_expiration_ttl(None), None);
        assert_eq!(kv_expiration_ttl(Some(ms(1))), Some(60));
        assert_eq!(kv_expiration_ttl(Some(Duration::from_secs(59))), Some(60));
        assert_eq!(kv_expiration_ttl(Some(Duration::from_secs(60))), Some(60));
        assert_eq!(kv_expiration_ttl(Some(ms(60_001))), Some(61));
        assert_eq!(
            kv_expiration_ttl(Some(Duration::from_secs(3600))),
            Some(3600)
        );
    }

    #[test]
    fn keeps_cache_api_entries_for_their_lifetime_in_whole_seconds() {
        assert_eq!(cache_control(Some(Duration::from_millis(1))), "max-age=1");
        assert_eq!(cache_control(Some(Duration::from_secs(90))), "max-age=90");
        assert_eq!(cache_control(None), "max-age=31536000");
    }

    #[test]
    fn puts_cache_api_keys_below_the_base_url() {
        assert_eq!(
            cache_url("https://example.com/c", "ns/1"),
            "https://example.com/c/ns/1"
        );
        assert_eq!(
            cache_url("https://example.com/c/", "ns/1"),
            "https://example.com/c/ns/1"
        );
    }

    /// A request whose kept loads and ticks a test can see. Its ticks never
    /// end by themselves.
    struct Seen {
        request: Rc<Request>,
        kept: Rc<RefCell<Vec<LoadTask>>>,
        ticks: Rc<Cell<usize>>,
    }

    fn request() -> Seen {
        let kept = Rc::new(RefCell::new(Vec::new()));
        let ticks = Rc::new(Cell::new(0));
        let (keep, tick) = (Rc::clone(&kept), Rc::clone(&ticks));
        let request = Request::new(
            move |task| keep.borrow_mut().push(task),
            move || {
                tick.set(tick.get() + 1);
                Box::pin(pending())
            },
        );
        Seen {
            request,
            kept,
            ticks,
        }
    }

    /// Counts the times it is woken.
    #[derive(Default)]
    struct Count(AtomicUsize);

    impl Wake for Count {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl Count {
        fn woken(&self) -> usize {
            self.0.load(Ordering::SeqCst)
        }
    }

    fn poll_with<F: Future + ?Sized>(future: &mut Pin<Box<F>>, waker: &Waker) -> Poll<F::Output> {
        future.as_mut().poll(&mut Context::from_waker(waker))
    }

    fn poll_once<F: Future + ?Sized>(future: &mut Pin<Box<F>>) -> Poll<F::Output> {
        poll_with(future, Waker::noop())
    }

    fn task() -> LoadTask {
        Box::pin(async {})
    }

    #[test]
    fn falls_back_outside_any_request() {
        let fell_back = Rc::new(Cell::new(0));
        let count = Rc::clone(&fell_back);
        keep_running(task(), move |_| count.set(count.get() + 1));
        assert_eq!(fell_back.get(), 1);
    }

    #[test]
    fn gives_a_load_to_the_request_being_polled() {
        let seen = request();
        let mut handling = Box::pin(Scoped::new(Rc::clone(&seen.request), async {
            keep_running(task(), |_| panic!("no fallback inside a request"));
        }));
        assert!(poll_once(&mut handling).is_ready());
        assert_eq!(seen.kept.borrow().len(), 1);
    }

    #[test]
    fn leaves_no_request_current_between_polls() {
        let seen = request();
        let mut handling = Box::pin(Scoped::new(Rc::clone(&seen.request), pending::<()>()));
        assert!(poll_once(&mut handling).is_pending());
        let fell_back = Rc::new(Cell::new(false));
        let flag = Rc::clone(&fell_back);
        keep_running(task(), move |_| flag.set(true));
        assert!(fell_back.get());
        assert!(seen.kept.borrow().is_empty());
    }

    #[test]
    fn gives_each_load_to_the_request_whose_poll_started_it() {
        let (first, second) = (request(), request());
        let turn = Rc::new(Cell::new(0));
        // Each request starts a load on its second poll, so their polls
        // interleave as requests in one isolate do.
        let handling = |seen: &Seen| {
            let turn = Rc::clone(&turn);
            Box::pin(Scoped::new(Rc::clone(&seen.request), async move {
                poll_fn(|_| {
                    turn.set(turn.get() + 1);
                    if turn.get() <= 2 {
                        Poll::Pending
                    } else {
                        Poll::Ready(())
                    }
                })
                .await;
                keep_running(task(), |_| panic!("no fallback inside a request"));
            }))
        };
        let mut a = handling(&first);
        let mut b = handling(&second);
        assert!(poll_once(&mut a).is_pending());
        assert!(poll_once(&mut b).is_pending());
        assert!(poll_once(&mut b).is_ready());
        assert!(poll_once(&mut a).is_ready());
        assert_eq!(
            (first.kept.borrow().len(), second.kept.borrow().len()),
            (1, 1)
        );
    }

    #[test]
    fn restores_the_outer_request_after_a_nested_one() {
        let (outer, inner) = (request(), request());
        let inner_request = Rc::clone(&inner.request);
        let mut handling = Box::pin(Scoped::new(Rc::clone(&outer.request), async move {
            Scoped::new(inner_request, async {
                keep_running(task(), |_| panic!("inside the inner request"));
            })
            .await;
            keep_running(task(), |_| panic!("inside the outer request"));
        }));
        assert!(poll_once(&mut handling).is_ready());
        assert_eq!(
            (outer.kept.borrow().len(), inner.kept.borrow().len()),
            (1, 1)
        );
    }

    #[test]
    fn keeps_one_tick_pending_while_the_request_waits() {
        let seen = request();
        let mut waiting = Box::pin(Scoped::new(Rc::clone(&seen.request), pending::<()>()));
        assert!(poll_once(&mut waiting).is_pending());
        assert!(poll_once(&mut waiting).is_pending());
        assert_eq!(seen.ticks.get(), 1);
        let mut finished = Box::pin(Scoped::new(Rc::clone(&seen.request), async {}));
        assert!(poll_once(&mut finished).is_ready());
        assert_eq!(seen.ticks.get(), 1, "a finished request keeps no tick");
    }

    #[test]
    fn ignores_a_wake_made_in_another_requests_turn() {
        let (a, b) = (request(), request());
        let waker_of_b = Rc::new(RefCell::new(None::<Waker>));
        let saved = Rc::clone(&waker_of_b);
        let mut waiting = Box::pin(Scoped::new(
            Rc::clone(&b.request),
            poll_fn(move |cx| {
                *saved.borrow_mut() = Some(cx.waker().clone());
                Poll::<()>::Pending
            }),
        ));
        let count = Arc::new(Count::default());
        assert!(poll_with(&mut waiting, &Waker::from(Arc::clone(&count))).is_pending());

        let wake = Rc::clone(&waker_of_b);
        let mut other = Box::pin(Scoped::new(Rc::clone(&a.request), async move {
            wake.borrow().as_ref().unwrap().wake_by_ref();
        }));
        assert!(poll_once(&mut other).is_ready());
        assert_eq!(count.woken(), 0, "left for the waiting request's tick");

        waker_of_b.borrow().as_ref().unwrap().wake_by_ref();
        assert_eq!(count.woken(), 1, "a wake outside any request goes through");
    }

    #[test]
    fn a_cache_shared_by_two_requests_loads_once_and_wakes_by_tick() {
        use std::time::UNIX_EPOCH;

        use crate::{from_fn, Clock, LoadingCache, LruStore};

        let clock: Arc<dyn Clock> = Arc::new(|| UNIX_EPOCH + Duration::from_secs(1_000_000));
        let store: LruStore<u32, String> = LruStore::builder().clock(Arc::clone(&clock)).build();
        let (loads, open) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(false)));
        let (load_count, gate) = (Rc::clone(&loads), Rc::clone(&open));
        let loader = from_fn(move |key: u32| {
            load_count.set(load_count.get() + 1);
            let gate = Rc::clone(&gate);
            async move {
                poll_fn(|_| {
                    if gate.get() {
                        Poll::Ready(())
                    } else {
                        Poll::Pending
                    }
                })
                .await;
                Ok::<_, String>(format!("{key} loaded"))
            }
        });
        let cache = LoadingCache::builder(store, loader)
            .clock(clock)
            .local_spawner(|task: LoadTask| {
                keep_running(task, |_| panic!("loads start inside a request"))
            })
            .build();

        let (leading, following) = (request(), request());
        let mut lead = Box::pin(Scoped::new(Rc::clone(&leading.request), cache.get(&1)));
        let mut follow = Box::pin(Scoped::new(Rc::clone(&following.request), cache.get(&1)));
        let follower_woken = Arc::new(Count::default());
        let follower_waker = Waker::from(Arc::clone(&follower_woken));
        assert!(poll_once(&mut lead).is_pending());
        assert!(poll_with(&mut follow, &follower_waker).is_pending());
        assert_eq!(
            leading.kept.borrow().len(),
            1,
            "the leading request keeps the load"
        );
        assert!(following.kept.borrow().is_empty());
        assert_eq!(
            following.ticks.get(),
            1,
            "the following request keeps a tick"
        );

        // The leading request's wait_until runs the load to its end, in the
        // leading request's turn, so the following request is not woken.
        open.set(true);
        let mut load = leading.kept.borrow_mut().pop().unwrap();
        assert!(poll_once(&mut load).is_ready());
        assert_eq!(follower_woken.woken(), 0);

        // Its tick ends, and it is polled in its own turn.
        assert_eq!(
            poll_with(&mut follow, &follower_waker),
            Poll::Ready(Ok("1 loaded".to_string()))
        );
        assert_eq!(
            poll_once(&mut lead),
            Poll::Ready(Ok("1 loaded".to_string()))
        );
        assert_eq!(loads.get(), 1);
    }
}
