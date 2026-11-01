[![51Degrees](https://51degrees.com/img/logo.png?utm_source=github&utm_medium=readme&utm_campaign=rust&utm_content=fiftyone-caching-readme.md&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=rust&utm_content=fiftyone-caching-readme.md&utm_term=logo)

# 51Degrees Caching

Sharded-Least Recently Used (LRU) cache trait and default implementation for the 51Degrees pipeline.

It also holds the loading cache, which loads a missing value once however
many callers ask for it at the same time. The first caller for a key runs the
load, every caller that arrives meanwhile waits for the same result, and a
failed load reaches every waiting caller without being stored. The loading
cache keeps its entries in a pluggable store, from the least recently used
cache in process memory to a platform's key-value store, and caches stack by
using one as the loader of another. A host that can run work on its own can
give the cache a spawner, so each load runs as a task of its own and a
dropped caller neither stops a load nor starts another. The names follow the
caching packages of the other languages, being `LoadingCache`,
`LruLoadingCache`, `LoadingCacheBuilder` and `ValueLoader`.

`EncodedStore` keeps the entries in a platform's key-value store or cache,
writing each one in a single versioned format with the time the store must
drop it, which it checks on every read. These features, all off by default,
add a platform's stores and pull in its SDK only on the target the platform
runs.

- `fastly`, for `wasm32-wasip1` on Fastly Compute, adds stores over the KV
  store and the core cache.
- `cloudflare`, for `wasm32-unknown-unknown` on Cloudflare Workers, adds
  stores over Workers KV and the Cache API, a spawner that keeps loads
  running with `wait_until`, and a clock.
- `spin`, for `wasm32-wasip2` on Spin 4, adds a store over Spin's key-value
  store.
- `tokio`, for native hosts, adds a spawner over a tokio-util local pool.

The crate builds for native targets, `wasm32-wasip1` and `wasm32-wasip2`,
and for `wasm32-unknown-unknown` with default features off. The `pipeline`
feature, on by default, brings `DataKeyedCache` and the dependency on
`fiftyone-pipeline-core` it needs.

This crate is part of the [51Degrees](https://51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=rust&utm_content=fiftyone-caching-readme.md&utm_term=introduction) Rust solution for high-performance
device detection and IP intelligence, available both on-premise from a local
data file and from the 51Degrees cloud.

## Links

- Source and issues: [github.com/51Degrees/51degrees-rust](https://github.com/51Degrees/51degrees-rust)
- API documentation: [docs.rs/fiftyone-caching](https://docs.rs/fiftyone-caching)
- About 51Degrees: [51degrees.com](https://51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=rust&utm_content=fiftyone-caching-readme.md&utm_term=about)
- Data files and pricing: [51degrees.com/pricing](https://51degrees.com/pricing?utm_source=github&utm_medium=readme&utm_campaign=rust&utm_content=fiftyone-caching-readme.md&utm_term=pricing)

## License

Licensed under the European Union Public Licence v1.2 (EUPL-1.2). See the
[repository](https://github.com/51Degrees/51degrees-rust) for the full text.
