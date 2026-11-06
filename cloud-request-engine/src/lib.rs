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
//! 1. Filters the flow data's evidence down to the keys the server accepts (the
//!    accepted-evidence filter was resolved at build time, see
//!    [discovery](#discovery)) and strips each key's prefix following the evidence
//!    precedence rules, so `query.user-agent` becomes `user-agent` and a query
//!    value beats a header value of the same name.
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
//! // A license key alongside a resource key adds the products it grants to
//! // those the resource key carries, so the answer widens.
//! let _engine = CloudRequestEngine::builder()
//!     .resource_key("my-resource-key")
//!     .license_key("my-license-key")
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
//! with everything it carries whatever the call asked for. A license key stays on
//! the server and names what it wants per request, which is usually what a
//! server-side caller needs.
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
//! key is the exception, because the endpoint takes a resource key and refuses a
//! request without one. The builder does not call it for such an engine, which
//! therefore starts with no accessible properties, and a downstream cloud aspect
//! engine reads the response JSON and infers each property's type from the value,
//! as it already does for a resource key that grants it no product.
//!
//! Because both results depend only on the keys, a consumer can persist
//! them and skip the build-time fetch on the next start. The builder retains the
//! state it resolves, so [`CloudRequestEngineBuilder::export_state`] returns a
//! serializable [`CloudEngineState`] snapshot after a build, and
//! [`CloudRequestEngineBuilder::set_state`] injects one back in. The engine holds
//! only the working values it needs and knows nothing about the snapshot. This is
//! aimed at short-lived hosts such as `wasm32-wasip1` edge runtimes, where
//! repeating the two round-trips on every cold start would be wasteful.
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
//! ## Testing
//!
//! The HTTP transport is abstracted behind [`CloudHttpClient`], so the engine
//! can be driven against an in-process fake in unit tests. A built-in blocking
//! transport backed by `reqwest` is available behind the `reqwest-client`
//! feature; without it a consumer supplies its own [`CloudHttpClient`] (for
//! example on `wasm32-wasip1`).
//!
//! ## Configuration
//!
//! [`configuration::add_builders`] adds the engine's builder to a
//! [`fiftyone_pipeline_core::configuration::BuilderRegistry`] under the name
//! .NET gives it, with .NET's settings, so a configuration file written for
//! .NET makes the same engine here.

#![warn(missing_docs)]

pub mod configuration;
mod constants;
mod data;
mod engine;
mod http;
mod properties;
mod recovery;
mod response;
mod state;

pub use constants::{
    CLOUD_ENDPOINT_ENV_VAR, CLOUD_URI_DEFAULT, ELEMENT_DATA_KEY, EVIDENCE_KEYS_FILENAME,
    FAILURES_TO_ENTER_RECOVERY_DEFAULT, FAILURES_WINDOW_SECONDS_DEFAULT, JSON_RESPONSE_KEY,
    ORIGIN_HEADER_NAME, PROCESS_STARTED_KEY, PROPERTIES_FILENAME, RECOVERY_SECONDS_DEFAULT,
    TIMEOUT_DEFAULT_SECONDS,
};
pub use data::CloudRequestData;
pub use engine::{CloudRequestEngine, CloudRequestEngineBuilder};
// Re-exported so a consumer can supply a response cache to the builder (and use
// the in-process default) without taking a direct dependency on the caching crate.
pub use fiftyone_caching::{Cache, LruCache, PutCache};
#[cfg(feature = "reqwest-client")]
pub use http::ReqwestClient;
pub use http::{CloudHttpClient, CloudHttpRequest, CloudHttpResponse, HttpMethod};
pub use properties::{CloudPropertyMetaData, LicensedProducts, ProductMetaData};
pub use recovery::{RecoveryConfig, RecoveryGate};
pub use response::{cloud_error, parse_retry_after, validate_response, ParsedResponse};
pub use state::{CloudEngineState, EvidenceKeyEntry};
