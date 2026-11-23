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

//! The routes test.mjs calls. Each answers `ok` or says what went wrong.
//!
//! - `/kv` and `/cache-api` write, read and remove bytes in each store.
//! - `/get?key=` reads a key through a cache the requests share, whose
//!   loads take 300 milliseconds and are kept running with `wait_until`.
//! - `/start?key=` starts the same read and answers after 20 milliseconds,
//!   leaving the load to finish after the response.
//! - `/loads` answers how many loads the shared cache has run.

use std::cell::{Cell, OnceCell};
use std::future::{poll_fn, Future};
use std::pin::pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use fiftyone_caching::cloudflare::{self, CacheApi, DateClock, KvStore, WaitUntil};
use fiftyone_caching::{
    ByteLookup, ByteStore, EncodedStore, Loaded, LoadingCache, SpawnedLocal, Utf8, ValueLoader,
};
use worker::{event, Context, Delay, Env, Request, Response, Result};

/// A source that counts its loads and takes a while to answer.
struct Slow {
    loads: Rc<Cell<u32>>,
}

impl ValueLoader<String, String> for Slow {
    type Error = String;

    async fn load(&self, key: &String) -> std::result::Result<Loaded<String>, String> {
        let load = self.loads.get() + 1;
        self.loads.set(load);
        Delay::from(Duration::from_millis(300)).await;
        Ok(Loaded::new(format!("{key} from load {load}")))
    }
}

type Shared =
    LoadingCache<String, String, EncodedStore<KvStore, Utf8>, Slow, SpawnedLocal<WaitUntil>>;

thread_local! {
    static LOADS: Rc<Cell<u32>> = Rc::new(Cell::new(0));
    static SHARED: OnceCell<Rc<Shared>> = const { OnceCell::new() };
}

/// The cache every request the isolate serves shares.
fn shared(env: &Env) -> Result<Rc<Shared>> {
    if let Some(cache) = SHARED.with(|shared| shared.get().cloned()) {
        return Ok(cache);
    }
    let store = EncodedStore::builder(KvStore::from(env.kv("CACHE")?), Utf8)
        .namespace("shared")
        .clock(Arc::new(DateClock))
        .build();
    let loader = Slow {
        loads: LOADS.with(Rc::clone),
    };
    let cache = LoadingCache::builder(store, loader)
        .time_to_live(Duration::from_secs(3600))
        .clock(Arc::new(DateClock))
        .local_spawner(WaitUntil)
        .build();
    Ok(SHARED.with(|shared| Rc::clone(shared.get_or_init(|| Rc::new(cache)))))
}

/// Writes, reads and removes bytes, answering `ok` or what went wrong.
async fn round_trip(store: &impl ByteStore, key: &str) -> String {
    // Shorter than Workers KV allows, which the store raises to a minute.
    store
        .put(key, b"bytes".to_vec(), Some(Duration::from_millis(500)))
        .await;
    match store.get(key).await {
        ByteLookup::Hit(bytes) if bytes == b"bytes" => {}
        ByteLookup::Hit(bytes) => return format!("read back {bytes:?}"),
        _ => return "missed after the write".to_string(),
    }
    store.remove(key).await;
    match store.get(key).await {
        ByteLookup::Miss => "ok".to_string(),
        _ => "still there after the remove".to_string(),
    }
}

/// The output of whichever future finishes first.
async fn first<T>(a: impl Future<Output = T>, b: impl Future<Output = T>) -> T {
    let (mut a, mut b) = (pin!(a), pin!(b));
    poll_fn(|cx| match a.as_mut().poll(cx) {
        Poll::Ready(value) => Poll::Ready(value),
        Poll::Pending => b.as_mut().poll(cx),
    })
    .await
}

#[event(fetch)]
async fn fetch(req: Request, env: Env, ctx: Context) -> Result<Response> {
    let url = req.url()?;
    let key = url
        .query_pairs()
        .find(|(name, _)| name == "key")
        .map(|(_, value)| value.into_owned())
        .unwrap_or_default();
    match url.path() {
        "/kv" => Response::ok(round_trip(&KvStore::from(env.kv("CACHE")?), "kv").await),
        "/cache-api" => {
            let store = CacheApi::new("https://cache.test/host-test");
            Response::ok(round_trip(&store, "cache-api").await)
        }
        "/get" => {
            let cache = shared(&env)?;
            let value = cloudflare::with_context(&ctx, cache.get(&key)).await;
            Response::ok(value.unwrap_or_else(|error| error))
        }
        "/start" => {
            let cache = shared(&env)?;
            let answered = first(
                async { cache.get(&key).await.unwrap_or_else(|error| error) },
                async {
                    Delay::from(Duration::from_millis(20)).await;
                    "started".to_string()
                },
            );
            Response::ok(cloudflare::with_context(&ctx, answered).await)
        }
        "/loads" => Response::ok(LOADS.with(|loads| loads.get()).to_string()),
        _ => Response::error("not found", 404),
    }
}
