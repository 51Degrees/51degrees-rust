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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-pipeline-web-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-pipeline-web-lib.rs&utm_term=logo)
//!
//! # 51Degrees framework-neutral web integration
//!
//! This crate is the framework-neutral half of the 51Degrees web integration,
//! implementing the
//! [web-integration specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/web-integration.md).
//! It carries **no dependency on any web framework**. A separate adapter crate
//! (for example `fiftyone-pipeline-web-axum`) wires it into a specific
//! framework by implementing one trait and mapping one response type.
//!
//! ## What it provides
//!
//! - [`RequestData`] and [`build_evidence`]: a trait an adapter implements over
//!   its request type, and the function that turns a request into pipeline
//!   [`fiftyone_pipeline_core::Evidence`], honouring the pipeline's evidence-key
//!   filter. This is the web request evidence service.
//! - [`serve_javascript`] and [`serve_json`]: the client-side endpoint logic
//!   that turns a processed flow data into a [`WebResponse`], applying the
//!   caching, `Vary`, `ETag` and CORS rules. This is the client-side property
//!   service.
//! - [`WebPipeline`] and [`WebIntegrationOptions`]: a helper that assembles the
//!   application's elements into the order the specification requires (sequence,
//!   elements, set-headers, JSON builder, JavaScript builder), and resolves the
//!   endpoint paths and `Vary` whitelist.
//! - [`response_headers`] and [`apply_set_headers`]: read the set-headers
//!   element's output and merge it into an outgoing response. This is the
//!   set-headers service.
//!
//! ## The response contract
//!
//! [`WebResponse`] is a plain `{ status, headers, body }` value. The endpoints
//! return:
//!
//! - **`Content-Type`**: `application/x-javascript` for JavaScript,
//!   `application/json` for JSON.
//! - **`Content-Length`**: the UTF-8 byte length of the body.
//! - **`Cache-Control`**: `private, max-age=1800` when processing recorded no
//!   errors and the builder's data was there, else `private, no-cache` and no
//!   `ETag`, so a browser that kept the response to a failed request fetches
//!   it again in full rather than having it confirmed by a `304`.
//! - **`Vary`**: the comma-joined HTTP-header evidence keys (prefix stripped,
//!   de-duplicated case-insensitively, sorted) the pipeline's elements accept,
//!   or those of a whitelist supplied in their place; omitted when empty.
//! - **`ETag`**: a weak entity tag, `W/"<16 hex digits>"`, holding an opaque,
//!   non-portable digest of the evidence cache key, the same in every process
//!   running the same build (see [`compute_etag`]). It is weak because the
//!   content for the same evidence changes when a data file is refreshed.
//! - **`Access-Control-Allow-Origin`**: the request `Origin` echoed back, unless
//!   it is absent or the literal string `null`.
//! - A matching `If-None-Match` yields a `304 Not Modified` with an empty body
//!   and the `Cache-Control`, `Vary`, `ETag` and `Access-Control-Allow-Origin`
//!   fields the `200` would have carried, as RFC 9110 section 15.4.5 requires
//!   for the first three. On a method other than `GET` or `HEAD` (see
//!   [`RequestData::method`]) it yields a `412 Precondition Failed` with an
//!   empty body and only `Access-Control-Allow-Origin`, as RFC 9110 section
//!   13.1.2 requires.
//!
//! ## A worked, framework-free example
//!
//! ```
//! use std::sync::Arc;
//! use fiftyone_pipeline_core::{
//!     ElementData, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData,
//!     FlowElement, MapElementData, PropertyMetaData, PropertyValueType, Result,
//!     TypedKey,
//! };
//! use fiftyone_pipeline_web::{
//!     build_evidence, serve_json, RequestData, WebEndpoint,
//!     WebIntegrationOptions, WebPipeline,
//! };
//!
//! // A trivial element so the pipeline has something to serialize.
//! struct DeviceData(MapElementData);
//! impl ElementData for DeviceData {
//!     fn get(&self, n: &str) -> std::result::Result<
//!         fiftyone_pipeline_core::PropertyValue,
//!         fiftyone_pipeline_core::NoValueError,
//!     > { self.0.get(n) }
//!     fn keys(&self) -> Vec<String> { self.0.keys() }
//!     fn as_any(&self) -> &dyn std::any::Any { self }
//!     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
//! }
//! struct DeviceElement { filter: EvidenceKeyFilterWhitelist, props: Vec<PropertyMetaData> }
//! impl DeviceElement { const KEY: TypedKey<DeviceData> = TypedKey::new("device"); }
//! impl FlowElement for DeviceElement {
//!     fn process(&self, data: &mut FlowData) -> Result<()> {
//!         data.get_or_add(Self::KEY, || DeviceData(MapElementData::new().set("ismobile", true)))?;
//!         Ok(())
//!     }
//!     fn data_key(&self) -> &str { "device" }
//!     fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter { &self.filter }
//!     fn properties(&self) -> &[PropertyMetaData] { &self.props }
//! }
//!
//! // A request the adapter would normally build from a real HTTP request.
//! struct Req;
//! impl RequestData for Req {
//!     fn headers(&self) -> Vec<(String, String)> {
//!         vec![("User-Agent".into(), "test".into())]
//!     }
//!     fn cookies(&self) -> Vec<(String, String)> { Vec::new() }
//!     fn query_params(&self) -> Vec<(String, String)> { Vec::new() }
//!     fn client_ip(&self) -> Option<String> { None }
//!     fn is_https(&self) -> bool { true }
//! }
//!
//! let element = Arc::new(DeviceElement {
//!     filter: EvidenceKeyFilterWhitelist::new(["header.user-agent"]),
//!     props: vec![PropertyMetaData::new("ismobile", "device", PropertyValueType::Bool)],
//! });
//!
//! let web = WebPipeline::build(vec![element], WebIntegrationOptions::default()).unwrap();
//!
//! // Routing: a request to the JSON endpoint resolves to the JSON endpoint.
//! assert_eq!(web.endpoint_for("/51dpipeline/json"), Some(WebEndpoint::Json));
//!
//! // Per request: build evidence, process, then serve.
//! let evidence = build_evidence(&Req, web.pipeline().evidence_key_filter());
//! let mut data = web.pipeline().create_flow_data_with(evidence);
//! data.process().unwrap();
//!
//! let options = web.endpoint_options();
//! let response = serve_json(&data, &Req, &options);
//! assert_eq!(response.status, 200);
//! assert_eq!(response.header("Content-Type"), Some("application/json"));
//! ```

#![warn(missing_docs)]

mod clientside;
mod pipeline;
mod request;
mod response;
mod set_headers;

pub use clientside::{
    compute_etag, etag_matches, serve_javascript, serve_json, vary_header, EndpointOptions,
    CACHE_CONTROL_CACHEABLE, CACHE_CONTROL_NO_CACHE, CONTENT_TYPE_JAVASCRIPT, CONTENT_TYPE_JSON,
};
pub use pipeline::{endpoint_matches, WebEndpoint, WebIntegrationOptions, WebPipeline};
pub use request::{build_evidence, RequestData};
pub use response::WebResponse;
pub use set_headers::{apply_set_headers, response_headers};
