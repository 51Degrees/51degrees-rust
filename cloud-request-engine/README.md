[![51Degrees](https://raw.githubusercontent.com/51Degrees/51degrees-rust/main/.github/images/logo-160.png "Data rewards the curious")](https://51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fiftyone-cloud-request-engine-readme.md&utm_term=logo)

# 51Degrees Cloud Request Engine

Cloud HTTP engine that resolves the accepted evidence keys and accessible properties at build time, retrying on first use after a transient failure, with recovery mode, for the 51Degrees pipeline.

This crate is part of the [51Degrees](https://51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fiftyone-cloud-request-engine-readme.md&utm_term=introduction) Rust solution for high-performance
device detection and IP intelligence, available both on-premise from a local
data file and from the 51Degrees cloud.

## Human Confidence from a sealed result

Where a page runs the 51Degrees browser tests, `HumanConfidence` reaches the
server sealed, and the page sends it as the parameter `humanconfidence`.
`CloudRequestEngine::redeem_human_confidence` redeems it through the `json`
endpoint with the engine's License Key, and gives `HumanConfidence` and
`HumanConfidenceSource`. A sealed result that is not redeemed gives `Medium`
and `Active`, never an error.

```rust
if let Some(properties) = engine.redeem_human_confidence(&sealed, None).await {
    println!("{}", properties.human_confidence().as_str());
}
```

## WebAssembly

The engine builds for `wasm32-wasip1` and for `wasm32-unknown-unknown`, the
target Cloudflare Workers run. On both, leave the `reqwest-client` feature off,
because reqwest's blocking client does not build there. On `wasm32-wasip1`,
give the builder a `CloudHttpClient` of your own.

A Worker can only await an HTTP request, so it gives the builder an
`AsyncCloudHttpClient` with `async_http_client`, builds the engine with
`build_async`, and awaits `CloudRequestEngine::fetch` with each flow data
before processing it. Processing then uses the answer `fetch` received. The
`cloudflare` feature adds `cloudflare::FetchClient`, an `AsyncCloudHttpClient`
over the Workers `fetch`, and compiles only for `wasm32-unknown-unknown`.

```rust
let engine = Arc::new(
    CloudRequestEngine::builder()
        .resource_key(resource_key)
        .async_http_client(Arc::new(FetchClient::new(Duration::from_secs(2))))
        .build_async()
        .await?,
);
let pipeline = Pipeline::builder().add_element(engine.clone()).build()?;
let mut data = pipeline.create_flow_data_with(evidence);
engine.fetch(&mut data).await;
data.process()?;
```

`fetch` sends the evidence the flow data holds when it is called, so evidence
that an element placed before the engine adds while processing is not sent.

`wasm32-unknown-unknown` has no clock in its standard library, so the engine
reads the time for its recovery mode through `web_time::Instant` from the
[web-time](https://crates.io/crates/web-time) crate. On that target
`web_time::Instant` reads the host's `performance.now()`, which Workers
provide, and the engine panics the first time it reads the clock on a host
whose JavaScript global object has no `performance`. On every other target
`web_time::Instant` is `std::time::Instant`. The `RecoveryGate` methods that
take an instant therefore take a `web_time::Instant` on
`wasm32-unknown-unknown`.

`fiftyone-pipeline-core` hashes with `ahash`, which seeds itself at run time
through `getrandom`, and `getrandom` builds for `wasm32-unknown-unknown` only
with its JavaScript backend turned on. The crate that builds the Worker turns
that backend on with this line in its `[dependencies]`. Version 0.3.4 is the
first in which the `wasm_js` feature alone is enough.

```toml
getrandom = { version = "0.3.4", features = ["wasm_js"] }
```

## License

Licensed under the European Union Public Licence v1.2 (EUPL-1.2). See the
[repository](https://github.com/51Degrees/51degrees-rust) for the full text.

## Find out more

- Source and issues: [github.com/51Degrees/51degrees-rust](https://github.com/51Degrees/51degrees-rust)
- API documentation: [docs.rs/fiftyone-cloud-request-engine](https://docs.rs/fiftyone-cloud-request-engine)
- Build custom data services with the Pipeline API: [51degrees.com/build-your-own](https://51degrees.com/build-your-own?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-cloud-request-engine-readme.md&utm_term=build-your-own)
- Device detection: [51degrees.com/device-detection](https://51degrees.com/device-detection?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-cloud-request-engine-readme.md&utm_term=device-detection)
- IP intelligence: [51degrees.com/ip-intelligence](https://51degrees.com/ip-intelligence?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-cloud-request-engine-readme.md&utm_term=ip-intelligence)
- Pipeline API documentation: [51degrees.com/documentation](https://51degrees.com/documentation/_pipeline_api__concepts__pipeline_basics.html?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-cloud-request-engine-readme.md&utm_term=documentation)
- Get a resource key: [51degrees.com/configure](https://51degrees.com/configure/?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-cloud-request-engine-readme.md&utm_term=configure)
- About 51Degrees: [51degrees.com](https://51degrees.com/?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-cloud-request-engine-readme.md&utm_term=about)
- Data files and pricing: [51degrees.com/pricing](https://51degrees.com/pricing?utm_source=docs.rs&utm_medium=package&utm_campaign=51degrees-rust&utm_content=fiftyone-cloud-request-engine-readme.md&utm_term=pricing)
