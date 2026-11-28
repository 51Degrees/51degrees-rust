[![51Degrees](https://raw.githubusercontent.com/51Degrees/51degrees-rust/main/.github/images/logo-160.png "Data rewards the curious")](https://51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fiftyone-cloud-request-engine-readme.md&utm_term=logo)

# 51Degrees Cloud Request Engine

Cloud HTTP engine that resolves the accepted evidence keys and accessible properties at build time, retrying on first use after a transient failure, with recovery mode, for the 51Degrees pipeline.

This crate is part of the [51Degrees](https://51degrees.com/?utm_source=github&utm_medium=readme&utm_campaign=51degrees-rust&utm_content=fiftyone-cloud-request-engine-readme.md&utm_term=introduction) Rust solution for high-performance
device detection and IP intelligence, available both on-premise from a local
data file and from the 51Degrees cloud.

## Human Confidence from a sealed result

Where a page runs the 51Degrees browser tests, `HumanConfidence` reaches the
server sealed. The engine redeems it in the json call it makes for the request
that carries it, with its License Key, and gives `HumanConfidence` and
`HumanConfidenceSource`. A sealed result that is not redeemed gives `Medium`
and `Active`, never an error.

Each page that asks for Human Confidence gets a page from
`engine.human_confidence_page()`: an id, and a transaction the engine keeps
on the server. The page loads the client script from `page.script_url(..)`,
which carries the transaction, and sends its sealed result back with the id,
as `humanconfidence` and `humanconfidencepage`. Those become the evidence
`query.humanconfidence` and `query.humanconfidencepage`, and the engine
redeems the sealed result inside that flow data's own json call, with the
transaction it kept for the page, used once. Any `transaction` or `challenge`
evidence is left out of the call.

`fiftyone_cloud_request_engine::human_confidence(&data)` reads the two
Properties, as often as needed, with no further request:

```rust
// After the flow data with the page's sealed result has been processed.
if let Some(properties) = human_confidence(&data) {
    println!("{}", properties.human_confidence().as_str());
}
```

With no License Key, or no transaction kept for the page, the sealed result is
not sent, and they are `Medium` and `Active`. Such a request is never cached.

### On a page

Naming `HumanConfidence` makes the client script run the 51Degrees browser
tests and send their answers with one more request, so its complete handlers
run later, by the time the tests take and that request. Read the first
response's Properties sooner through the change handlers. Where the page sets
a Content Security Policy, the browser tests run only if it allows:

| Directive     | Must allow                                    |
|---------------|-----------------------------------------------|
| `script-src`  | The remote server's host, and `'unsafe-eval'` |
| `connect-src` | The remote server's host                      |
| `worker-src`  | `blob:`                                       |

A page whose policy blocks any of these runs no browser tests, and gets a
`Passive` `HumanConfidenceSource`.

## The derived Properties as Element Data

A `DerivedCloudEngine` after the engine gives the `derived` member of the
response as `DerivedData`, under `DERIVED_DATA_KEY`. On a request that carries
a sealed result, its `human_confidence()` and `human_confidence_source()` are
the two Properties above. On any other request each is the Value the response
holds, or no value with the message that says why: the remote server's null
reason where the response holds it as null, and the missing property reason
where the response leaves it out.

```rust
let derived = DerivedCloudEngine::builder()
    .cloud_request_engine(request_engine.clone())
    .build()?;
// Add request_engine, then derived, to the Pipeline, and process.
match data.get(DERIVED_DATA_KEY).map(|d| d.human_confidence()) {
    Some(value) if value.has_value() => println!("{}", value.value()?),
    Some(value) => println!("no value: {}", value.no_value_message().unwrap_or("")),
    None => {}
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
