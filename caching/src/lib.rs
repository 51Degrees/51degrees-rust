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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-caching-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-caching-lib.rs&utm_term=logo)
//!
//! # 51Degrees caching
//!
//! The caches of the 51Degrees pipeline and of services built with it, as in
//! the caching packages of the other languages.
//!
//! - A sharded least-recently-used cache. It implements the custom cache
//!   described in the
//!   [caching specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/caching.md)
//!   and exists chiefly to speed up the cloud request engine when many
//!   requests share the same evidence.
//! - A [loading cache](#loading-cache) that loads a missing value once,
//!   however many callers ask for it at the same time, over a store in
//!   memory or a platform's key-value store.
//!
//! ## Why sharded
//!
//! A general-purpose cache was found too slow on the pipeline's hot path. This
//! cache is kept simple and predictable instead. The entries are split across a
//! fixed number of shards, each a [`std::sync::Mutex`] around an
//! insertion-ordered map, so concurrent requests contend on a lock only when
//! they hash to the same shard. Memory use is bounded because each shard evicts
//! its least-recently-used entry once full.
//!
//! ## The pieces
//!
//! - [`Cache`] and [`PutCache`] are the small read and write trait surfaces, so
//!   an engine can depend on the abstraction rather than the concrete cache.
//! - [`LruCache`] is the default sharded LRU implementation. It is generic over
//!   a key `K` and a value `V`, both `Send + Sync`, with `V: Clone`. It is
//!   never generic over `dyn ElementData` because element data is not `Sync`.
//!   Engines cache their own concrete `Send + Sync + Clone` data, typically an
//!   [`std::sync::Arc`] around an aspect-data struct, or the cloud JSON.
//! - [`CacheBuilder`] applies the two tunables from the specification: the
//!   total `size` (default 1000) and the `concurrency`, the number of shards
//!   (default the CPU count).
#![cfg_attr(
    feature = "pipeline",
    doc = "- [`DataKeyedCache`] wraps an [`LruCache`] keyed by
  [`fiftyone_pipeline_core::DataKey`]. An engine hands it a flow data and an
  [`fiftyone_pipeline_core::EvidenceKeyFilter`]; it derives a deterministic,
  case-insensitive key from the relevant evidence, so equivalent requests
  share an entry. It comes with the `pipeline` feature, on by default."
)]
#![cfg_attr(
    not(feature = "pipeline"),
    doc = "- `DataKeyedCache` keys an [`LruCache`] by a flow data's evidence. It
  comes with the `pipeline` feature, which this build leaves out."
)]
//!
//! ## Loading cache
//!
//! [`LoadingCache`] loads a missing value once, however many callers ask for
//! it at the same time. The first caller for a key runs the load, every caller
//! that arrives while it runs waits for the same result, and all of them
//! continue when it completes. A failed load reaches every waiting caller and
//! is not stored, so the next caller loads again. It needs no async runtime
//! and starts no threads of its own.
//!
//! - [`LoadingCache`] does all the work. It is generic over a [`Store`] and a
//!   [`ValueLoader`]. [`LruLoadingCache`] is the form over [`LruStore`], the
//!   least recently used cache in process memory, as `LruLoadingCache` in the
//!   .NET and Java pipelines.
//! - A [`Store`] keeps entries. [`LruStore`] keeps them in process memory.
//!   [`EncodedStore`] keeps them in a platform's key-value store or cache,
//!   through a [`ByteStore`].
//! - A [`ValueLoader`] produces a value the store does not hold. The source is
//!   a loader, [`from_fn`] makes one from a function, and every
//!   [`LoadingCache`] is one.
//! - A [`Clock`] gives the time. The system clock is the default, except on
//!   `wasm32-unknown-unknown`, which has none, so a host there supplies one.
//!
//! ### Layers
//!
//! Caches stack by using one as the loader of another. A brief copy in
//! process memory, over a shared key-value store, over the source:
//!
//! ```text
//! LoadingCache(LruStore, time to live 5 s)
//!   loads from LoadingCache(key-value store, time to live 1 day, idle 1 hour)
//!     loads from the source
//! ```
//!
//! A read looks in memory first. On a miss it asks the cache below, which
//! looks in the key-value store, and only a miss there reaches the source.
//! Each cache collapses its own concurrent misses, so many callers missing
//! in memory make one call to the cache below, and many processes missing
//! in a store that can make callers wait make one call to the source. A
//! value changed or removed in the shared store is seen once the memory
//! copy's short life ends.
//!
//! A value carries the time it was written and the time it stops being
//! usable, and a cache never keeps a copy longer than the copy it loaded
//! from, so no copy in a stack outlives the copy below it.
//!
//! ### Loads that outlive their caller
//!
//! By default the first caller for a key does the load, and if it is dropped
//! a waiting caller starts the load again. A host that can run work on its
//! own can give the cache a spawner instead, a [`Spawn`] for any thread or a
//! [`SpawnLocal`] for the current one. The cache then runs each load as a
//! task of its own and every caller, the first included, waits for its
//! result, so a dropped caller neither stops a load nor starts another, as
//! a .NET `Lazy<Task>` does. A hit is still served by the caller, with no
//! task. With a spawner the cache's types must be `'static`, and `Send` and
//! `Sync` too for a spawner on any thread. A load lost with its task, by a
//! panic in the load or by a spawner that drops the task, is asked for once
//! more by each caller waiting on it, and a caller that loses a second load
//! panics rather than ask without end.
//!
//! ### What a store does and what the cache does
//!
//! A store keeps each entry for the time the cache tells it when writing,
//! and may drop entries sooner to make room. A store may also make callers
//! in other processes wait for one load, by answering
//! [`Lookup::Reserved`]. A store whose lookup costs little, as one in
//! process memory does, also answers [`Store::try_get`], and a hit there is
//! served by that lookup alone, with no part in the key's load.
//!
//! The cache does the rest.
//!
//! - It allows one load per key at a time in its process.
//! - It decides each copy's lifetime from its time to live, its time to
//!   idle and the copy it was loaded from.
//! - It renews a used copy at most once per renewal window, so a copy left
//!   unused for the idle time leaves the store.
//! - It checks every entry it reads is still fresh, so a store that drops
//!   entries late, or never, still gives correct results.
//!
//! ### Platform stores
//!
//! A platform's key-value store or cache keeps bytes under string keys. It
//! is a [`ByteStore`], and [`EncodedStore`] makes it a [`Store`] by writing
//! each entry in one versioned format, described at [`encode_entry`], with
//! the value turned into bytes by a [`Codec`]. The format holds the time the
//! store must drop the entry, and [`EncodedStore`] checks it on every read,
//! so a platform that deletes late, rounds lifetimes up or keeps none never
//! returns an entry past its time.
//!
//! Each of these features adds a platform's stores, off by default, and
//! pulls in that platform's SDK only on the target the platform runs.
//!
//! | Feature | Target | Adds |
//! |---|---|---|
//! | `fastly` | `wasm32-wasip1` | The `fastly` module, with stores over the KV store and the core cache |
//! | `cloudflare` | `wasm32-unknown-unknown` | The `cloudflare` module, with stores over Workers KV and the Cache API, a spawner that keeps loads running with `wait_until`, and a clock |
//! | `spin` | `wasm32-wasip2` | The `spin` module, with a store over Spin's key-value store |
//! | `tokio` | Native | The `tokio` module, with a spawner over a tokio-util local pool |
//!
//! ### Example
//!
//! ```
//! use std::time::Duration;
//! use fiftyone_caching::{Loaded, LoadingCache, LruLoadingCache, LruStore, ValueLoader};
//!
//! /// Fetches a page from the origin.
//! struct Origin;
//!
//! impl ValueLoader<String, String> for Origin {
//!     type Error = String;
//!
//!     async fn load(&self, url: &String) -> Result<Loaded<String>, String> {
//!         Ok(Loaded::new(format!("page at {url}")))
//!     }
//! }
//!
//! /// The shared copies. A store in memory stands in for a platform store.
//! type Shared = LruLoadingCache<String, String, Origin>;
//!
//! /// A brief copy in memory over the shared copies.
//! type Pages = LruLoadingCache<String, String, Shared>;
//!
//! fn pages() -> Pages {
//!     let shared = LoadingCache::builder(LruStore::builder().build(), Origin)
//!         .time_to_live(Duration::from_secs(24 * 60 * 60))
//!         .time_to_idle(Duration::from_secs(60 * 60))
//!         .build();
//!     LoadingCache::builder(LruStore::builder().size(100).build(), shared)
//!         .time_to_live(Duration::from_secs(5))
//!         .build()
//! }
//!
//! async fn page(pages: &Pages, url: &String) -> Result<String, String> {
//!     pages.get(url).await
//! }
//! # let _ = (pages(), page);
//! ```
//!
//! ## WebAssembly
//!
//! The crate builds for `wasm32-wasip1` and `wasm32-wasip2`, and for
//! `wasm32-unknown-unknown` with default features off. The `pipeline`
//! feature is the only part that needs `fiftyone-pipeline-core`. On
//! WebAssembly `ahash` is seeded when the crate is compiled rather than at
//! run time, because `wasm32-unknown-unknown` has no source of randomness.
//!
//! ## A minimal cache
//!
//! ```
//! use fiftyone_caching::{Cache, LruCache, PutCache};
//!
//! let cache: LruCache<String, u32> = LruCache::with_defaults();
//! cache.put("query.user-agent=abc".to_owned(), 42);
//! assert_eq!(cache.get(&"query.user-agent=abc".to_owned()), Some(42));
//! assert_eq!(cache.get(&"missing".to_owned()), None);
//! ```

#![warn(missing_docs)]

mod cache;
mod config;
#[cfg(feature = "pipeline")]
mod data_keyed;
mod loading;
mod lru;

#[cfg(all(feature = "cloudflare", target_arch = "wasm32", target_os = "unknown"))]
pub mod cloudflare;
// The Cloudflare store's own logic is tested on every target, without the SDK.
#[cfg(all(
    test,
    not(all(feature = "cloudflare", target_arch = "wasm32", target_os = "unknown"))
))]
mod cloudflare;
#[cfg(all(feature = "fastly", target_os = "wasi", target_env = "p1"))]
pub mod fastly;
#[cfg(all(feature = "spin", target_os = "wasi", target_env = "p2"))]
pub mod spin;
#[cfg(all(feature = "tokio", not(target_family = "wasm")))]
pub mod tokio;
// The Fastly store's own logic is tested on every target, without the SDK.
#[cfg(all(
    test,
    not(all(feature = "fastly", target_os = "wasi", target_env = "p1"))
))]
mod fastly;

pub use cache::{Cache, PutCache};
pub use config::{default_concurrency, CacheBuilder, DEFAULT_SIZE};
#[cfg(feature = "pipeline")]
pub use data_keyed::DataKeyedCache;
#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub use loading::SystemClock;
pub use loading::{
    decode_entry, encode_entry, from_fn, ByteLookup, ByteStore, Clock, Codec, DecodeError,
    EncodedStore, EncodedStoreBuilder, Entry, FnLoader, Inline, ListKeys, LoadRunner, LoadTask,
    Loaded, LoadingCache, LoadingCacheBuilder, Lookup, LruLoadingCache, LruStore, LruStoreBuilder,
    Raw, Spawn, SpawnLocal, Spawned, SpawnedLocal, StartLoad, Store, Stored, Utf8, ValueLoader,
    ENTRY_FORMAT,
};
pub use lru::LruCache;
