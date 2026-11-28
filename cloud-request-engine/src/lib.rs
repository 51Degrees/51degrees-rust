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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-cloud-request-engine-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-cloud-request-engine-lib.rs&utm_term=logo)
//!
//! # 51Degrees cloud request engine
//!
//! The engine that offloads pipeline processing to the 51Degrees cloud service.
//! It implements the
//! [cloud-request-engine specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/cloud-request-engine.md).
//!
//! ## What it does
//!
//! A pipeline usually has a single [`CloudRequestEngine`] followed by one or
//! more cloud aspect engines (device detection, IP intelligence and so on). The
//! request engine makes one HTTP call per flow data and stores the raw JSON
//! response, so the cloud aspect engines after it just deserialize the parts they
//! own without each making their own request. This keeps the number of HTTP
//! round-trips to one regardless of how many aspects are involved, which matters
//! because the HTTP time dominates.
//!
//! On [`fiftyone_pipeline_core::FlowElement::process`] the engine:
//!
//! 1. Takes every evidence value, whether or not the server lists its key as
//!    accepted, apart from the reserved names `resource`, `license`, `licence`
//!    and `values` and the web request key `server.http-request`, and strips
//!    each key's prefix following the evidence precedence rules, so
//!    `query.user-agent` becomes `user-agent` and a query value beats a header
//!    value of the same name in any letter case. Each name is sent as the
//!    winning key spells it. The accepted evidence, resolved at build time
//!    (see [discovery](#discovery)), is what the engine advertises to the
//!    pipeline and what a stored response is found by.
//! 2. POSTs the result as url-encoded form data (with the credential, and the
//!    asked-for property list when there is one) to the `json` endpoint.
//! 3. Stores the raw JSON response body in its [`CloudRequestData`] under the
//!    `cloud-response` data key.
//!
//! ## Credentials and the properties asked for
//!
//! An engine authenticates with a resource key, a license key, or both, and
//! which of them is present decides whether the caller names the properties it
//! wants. [`CloudRequestEngineBuilder::build`] settles the combination and
//! refuses the ones the service cannot answer as the caller intends, with a
//! message naming the setting to change.
//!
//! ```no_run
//! # use fiftyone_cloud_request_engine::CloudRequestEngine;
//! // A resource key states which properties it carries, so it is used on its
//! // own and the service answers with all of them.
//! let _engine = CloudRequestEngine::builder()
//!     .resource_key("my-resource-key")
//!     .build()
//!     .unwrap();
//!
//! // A property list beside a resource key adds the listed properties to
//! // those it carries, and a license key alongside grants the ones it covers.
//! let _engine = CloudRequestEngine::builder()
//!     .resource_key("my-resource-key")
//!     .license_key("my-license-key")
//!     .values(["device.hardwarevendor"])
//!     .build()
//!     .unwrap();
//!
//! // A license key on its own names no properties, so the caller names the ones
//! // it wants and the service answers with those alone.
//! let _engine = CloudRequestEngine::builder()
//!     .license_key("my-license-key")
//!     .values(["device.ismobile", "device.iscrawler"])
//!     .build()
//!     .unwrap();
//! ```
//!
//! A resource key is public by design, because it travels to the browser inside a
//! script URL, so it is scoped to what a page is meant to read and it answers
//! with everything it carries, plus any properties the call lists. A license
//! key stays on the server and names what it wants per request, which is
//! usually what a server-side caller needs.
//!
//! Because the combination is settled when the engine is built, a request cannot
//! fail for this reason afterwards, so a downstream element always receives an
//! answer rather than meeting a configuration mistake as a failed request on the
//! critical path.
//!
//! A property the credential does not cover is left out of the answer without
//! comment, since the service answers `200` and names nothing it dropped as long
//! as one asked-for property is covered. The engine therefore compares what it
//! asked for against what arrived and reports the difference once, as a warning
//! on its element data and one line on stderr. It is an entitlement matter rather
//! than a fault, so the request stands and the remaining properties are used.
//!
//! ## Discovery
//!
//! The accepted evidence keys (`evidencekeys`) and accessible properties
//! (`accessibleproperties`) are fetched from the cloud. The builder fetches them
//! as it builds the engine, so a built engine is normally ready to process. A
//! 4xx answer, such as an unknown resource key, makes
//! [`CloudRequestEngineBuilder::build`] return an error. Any other failure is
//! logged, the build completes, and each process call retries
//! the missing discovery and fails until it succeeds, so a cloud outage at
//! start-up cannot stop the application starting.
//!
//! The accessible-properties request carries the resource key and, when one is
//! set, the license key, because the cloud adds the products the license grants
//! to those of the resource key. An engine holding a license key and no resource
//! key asks with the license key, sent as `license` in the body of a POST so
//! that the key stays out of request logs, and the cloud answers with every
//! property the license key entitles.
//!
//! Because both results depend only on the keys, a consumer can persist
//! them and skip the build-time fetch on the next start. The builder retains the
//! state it resolves, so [`CloudRequestEngineBuilder::export_state`] returns a
//! serializable [`CloudEngineState`] snapshot after a build, and
//! [`CloudRequestEngineBuilder::set_state`] injects one back in. The engine holds
//! only the working values it needs and knows nothing about the snapshot. This is
//! aimed at short-lived hosts, such as edge runtimes on `wasm32-wasip1` and
//! Cloudflare Workers on `wasm32-unknown-unknown`, where repeating the two
//! round-trips on every cold start would be wasteful.
//!
//! ## Recovery mode
//!
//! Repeated request failures within a window trip a recovery gate, which
//! short-circuits requests for a recovery period so a slow or failing cloud
//! cannot stall consumer requests. Only a failure that says the service is in
//! trouble counts, being a request that did not complete, a status of 500 or
//! more, a 429, or a success status whose body could not be used. A status
//! from 400 to 499 answers the request's own content, which any visitor can
//! choose, so it does not count.
//!
//! ## Element data shape
//!
//! [`CloudRequestData`] (data key `cloud-response`) carries:
//!
//! | Field            | Type         | Description                            |
//! |------------------|--------------|----------------------------------------|
//! | `cloud-response` | string       | The raw JSON response body.            |
//! | `json-response`  | string       | The same raw JSON, under an alias field name. |
//! | `process-started`| bool         | True once the engine began processing. |
//! | `warnings`       | string list  | Advisory messages, being the service's own warnings and, once per engine, any asked-for property that did not come back. |
//!
//! ## Human Confidence from a sealed result
//!
//! Where a page runs the 51Degrees browser tests, `HumanConfidence` reaches
//! the publisher's server sealed, and the page sends it to the server as the
//! parameter `humanconfidence`. The engine redeems it in the json call it
//! makes for that request, with its License Key, and gives the two
//! Properties, `HumanConfidence` and `HumanConfidenceSource`, as a
//! [`SealedHumanConfidence`], so a consumer reads them and never redeems a
//! sealed result itself. A sealed result that is not redeemed gives
//! `Medium` and `Active`, never an error.
//!
//! Each page that asks for Human Confidence is given a
//! [`HumanConfidencePage`] by [`CloudRequestEngine::human_confidence_page`]:
//! an id, and a transaction the engine keeps on the server. The page loads
//! the client script from [`HumanConfidencePage::script_url`], which carries
//! the transaction, so the remote server binds the page's sealed result to
//! it. The page sends its sealed result back with the id, as the parameters
//! `humanconfidence` and `humanconfidencepage`, which the web integration
//! adds as the evidence [`SEALED_RESULT_EVIDENCE`] and [`PAGE_EVIDENCE`].
//!
//! The engine redeems that sealed result inside the flow data's own json
//! call, with the transaction it kept for the page, and
//! [`human_confidence()`] reads the two Properties from it. The kept
//! transaction is the only one in the call, so `transaction` and `challenge`
//! evidence under any prefix is left out, and it is used once. Where the
//! engine has no License Key, or keeps no transaction for the page named,
//! the sealed result is not sent and the Properties are `Medium` and
//! `Active`. Every read gives the same Properties and makes no request, such
//! a request is neither stored in nor served from the response cache, and
//! the sealed result, the transaction and the License Key are never logged
//! and are masked in every error message and warning.
//!
//! On a page, naming `HumanConfidence` makes the client script run the
//! 51Degrees browser tests and send their answers with one more request, so
//! its complete handlers run later than they would otherwise, by the time
//! the tests take and that request. The first response's Properties can be
//! read sooner through the client script's change handlers. Where the page
//! sets a Content Security Policy, the browser tests run only if it allows:
//!
//! | Directive     | Must allow                                    |
//! |---------------|-----------------------------------------------|
//! | `script-src`  | The remote server's host, and `'unsafe-eval'` |
//! | `connect-src` | The remote server's host                      |
//! | `worker-src`  | `blob:`                                       |
//!
//! A page whose policy blocks any of these runs no browser tests, and its
//! `HumanConfidenceSource` is `Passive`.
//!
//! ## The derived Properties as Element Data
//!
//! A [`DerivedCloudEngine`] after the engine gives the `derived` member of
//! the response as [`DerivedData`], under [`DERIVED_DATA_KEY`], among it
//! [`DerivedData::human_confidence`] and
//! [`DerivedData::human_confidence_source`]. On a request that carries a
//! sealed result they are the two Properties [`human_confidence()`] gives.
//! On any other request each is the Value the response holds, or no value
//! with the message that says why: the remote server's null reason where
//! the response holds it as null, and the reason a missing Property gives,
//! naming the Resource Key, where the response leaves it out. In a Pipeline
//! with no Cloud Request Engine, built with no
//! [`DerivedCloudEngineBuilder::cloud_request_engine`], nothing is redeemed:
//! a request that carries a sealed result gives `Medium` and `Active`, and
//! any other request gives both Properties no value with the reason.
//!
//! ## Testing
//!
//! The HTTP transport is abstracted behind [`CloudHttpClient`], so the engine
//! can be driven against an in-process fake in unit tests. A built-in blocking
//! transport backed by `reqwest` is available behind the `reqwest-client`
//! feature. Without it a consumer supplies its own [`CloudHttpClient`] or
//! [`AsyncCloudHttpClient`], as on the targets in
//! [WebAssembly](#webassembly).
//!
//! ## WebAssembly
//!
//! The engine builds for `wasm32-wasip1` and for `wasm32-unknown-unknown`,
//! the target Cloudflare Workers run. On both, leave the `reqwest-client`
//! feature off, because reqwest's blocking client does not build there. On
//! `wasm32-wasip1`, give the builder a [`CloudHttpClient`] of your own.
//!
//! A Worker can only await an HTTP request, so it gives the builder an
//! [`AsyncCloudHttpClient`] with
//! [`CloudRequestEngineBuilder::async_http_client`], builds the engine with
//! [`CloudRequestEngineBuilder::build_async`], and awaits
//! [`CloudRequestEngine::fetch`] with each flow data before processing it.
//! The `cloudflare` feature adds `cloudflare::FetchClient`, an
//! [`AsyncCloudHttpClient`] over the Workers `fetch`, and compiles only for
//! `wasm32-unknown-unknown`.
//!
//! `wasm32-unknown-unknown` has no clock in its standard library, so the
//! engine reads the time for its recovery mode through `web_time::Instant`
//! from the [web-time](https://docs.rs/web-time/1.1.0) crate. On that target
//! `web_time::Instant` reads the host's `performance.now()`, which Workers
//! provide, and the engine panics the first time it reads the clock on a
//! host whose JavaScript global object has no `performance`. On every other
//! target `web_time::Instant` is `std::time::Instant`. The [`RecoveryGate`]
//! methods that take an instant therefore take a `web_time::Instant` on
//! `wasm32-unknown-unknown`.
//!
//! `fiftyone-pipeline-core` hashes with `ahash`, which seeds itself at run
//! time through `getrandom`, and `getrandom` builds for
//! `wasm32-unknown-unknown` only with its JavaScript backend turned on. The
//! crate that builds the Worker turns that backend on with this line in its
//! `[dependencies]`. Version 0.3.4 is the first in which the `wasm_js`
//! feature alone is enough.
//!
//! ```toml
//! getrandom = { version = "0.3.4", features = ["wasm_js"] }
//! ```
//!
//! ## Configuration
//!
//! [`configuration::add_builders`] adds the engine's builder to a
//! [`fiftyone_pipeline_core::configuration::BuilderRegistry`] under the name
//! .NET gives it, with .NET's settings, so a configuration file written for
//! .NET makes the same engine here.
//!
//! [`aspect`] holds the processing every Cloud Aspect Engine shares, reading
//! the portion of this engine's response for its Data Key and each property's
//! value or the reason it has none, which the Device Detection and IP
//! Intelligence cloud engines use.

// A warning in a documentation example is an error, as everywhere else.
#![doc(test(attr(deny(warnings))))]
#![warn(missing_docs)]

pub mod aspect;
#[cfg(all(feature = "cloudflare", target_arch = "wasm32", target_os = "unknown"))]
pub mod cloudflare;
pub mod configuration;
mod constants;
mod data;
mod derived;
mod engine;
mod http;
mod human_confidence;
mod properties;
mod recovery;
mod response;
mod state;
mod transaction;

pub use constants::{
    CLOUD_ENDPOINT_VARIABLE, CLOUD_URI_DEFAULT, ELEMENT_DATA_KEY, EVIDENCE_KEYS_FILENAME,
    FAILURES_TO_ENTER_RECOVERY_DEFAULT, FAILURES_WINDOW_SECONDS_DEFAULT, JSON_RESPONSE_KEY,
    ORIGIN_HEADER_NAME, PROCESS_STARTED_KEY, PROPERTIES_FILENAME, RECOVERY_SECONDS_DEFAULT,
    TIMEOUT_DEFAULT_SECONDS,
};
pub use data::CloudRequestData;
pub use derived::{
    DerivedCloudEngine, DerivedCloudEngineBuilder, DerivedData, DERIVED_DATA_KEY,
    DERIVED_ELEMENT_DATA_KEY,
};
pub use engine::{CloudRequestEngine, CloudRequestEngineBuilder};
// Re-exported so a consumer can supply a response cache to the builder (and use
// the in-process default) without taking a direct dependency on the caching crate.
pub use fiftyone_caching::{Cache, LruCache, PutCache};
#[cfg(feature = "reqwest-client")]
pub use http::ReqwestClient;
pub use http::{
    AsyncCloudHttpClient, CloudHttpClient, CloudHttpRequest, CloudHttpResponse, HttpMethod,
    LocalBoxFuture,
};
pub use human_confidence::{human_confidence, SEALED_RESULT_EVIDENCE};
pub use human_confidence::{HumanConfidence, HumanConfidenceSource, SealedHumanConfidence};
pub use properties::{CloudPropertyMetaData, LicensedProducts, ProductMetaData};
pub use recovery::{RecoveryConfig, RecoveryGate};
pub use response::{cloud_error, parse_retry_after, validate_response, ParsedResponse};
pub use state::{CloudEngineState, EvidenceKeyEntry};
pub use transaction::{HumanConfidencePage, HumanConfidenceTransactions, PAGE_EVIDENCE};
