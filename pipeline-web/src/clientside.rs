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

//! The client-side endpoint logic.
//!
//! This is the framework-neutral client-side property service. It turns a
//! processed [`fiftyone_pipeline_core::FlowData`] into a
//! [`crate::WebResponse`] for the two client-side endpoints:
//!
//! - [`serve_javascript`] renders the JavaScript include produced by the
//!   JavaScript builder element, served by default at
//!   [`fiftyone_pipeline_core::constants::DEFAULT_CORE_JS_ENDPOINT`].
//! - [`serve_json`] renders the JSON produced by the JSON builder element,
//!   served by default at
//!   [`fiftyone_pipeline_core::constants::DEFAULT_JSON_ENDPOINT`].
//!
//! Both functions apply the same caching and CORS rules, described on each
//! function and realised by the shared helpers below.
//!
//! # The exact response contract
//!
//! For a non-conditional request both endpoints return `200 OK` with:
//!
//! - `Content-Type`: `application/x-javascript` (JS) or `application/json`
//!   (JSON).
//! - `Content-Length`: the UTF-8 byte length of the body.
//! - `Cache-Control`: `private, max-age=1800` when the flow data recorded no
//!   processing errors and the builder's data was there, otherwise
//!   `private, no-cache`. That response carries no `ETag`, so a browser that
//!   kept it fetches it again in full rather than having it confirmed by a
//!   `304`.
//! - `Vary`: the comma-joined list of HTTP-header evidence keys in the
//!   evidence-key whitelist [`EndpointOptions::current_vary_whitelist`] gives,
//!   with the `header.` prefix stripped and the names de-duplicated
//!   case-insensitively. Omitted when the list is empty or there is none.
//! - `ETag`: a digest of the cache key the flow data's evidence produces
//!   under the pipeline filter, the same in every process running the same
//!   build. The digest is opaque and non-portable (see [`compute_etag`]).
//! - `Access-Control-Allow-Origin`: the request `Origin` header echoed back,
//!   but only when an origin is present and is not the literal string `null`.
//!
//! When the request's `If-None-Match` equals the computed `ETag`, both
//! endpoints instead return `304 Not Modified` with an empty body and the
//! `Cache-Control`, `Vary`, `ETag` and `Access-Control-Allow-Origin` fields
//! the `200` would have carried, as
//! [RFC 9110 section 15.4.5](https://www.rfc-editor.org/rfc/rfc9110#section-15.4.5)
//! requires for the first three. So while processing is failing the `304`
//! carries `private, no-cache` and no `ETag`. Whether a builder left no data
//! is read without building the body, so nothing below is logged or built for
//! a `304`.
//!
//! A `304` answers `GET` or `HEAD` only. On any other method, such as the
//! `POST` the axum adapter serves the JSON endpoint on, a matching
//! `If-None-Match` gets `412 Precondition Failed` with an empty body, as
//! [RFC 9110 section 13.1.2](https://www.rfc-editor.org/rfc/rfc9110#section-13.1.2)
//! requires, because a response to `POST` is never a `304`
//! ([section 9.3.3](https://www.rfc-editor.org/rfc/rfc9110#section-9.3.3)).
//! It carries only the `Access-Control-Allow-Origin` field, so a script on
//! another origin can read the status. The method is
//! [`crate::RequestData::method`], and a request that does not give one is
//! treated as `GET`. Nothing is logged or built for a `412` either.
//!
//! # When a builder left no data
//!
//! A builder in the pipeline can leave no data for a request, for example when
//! it failed and the pipeline suppresses errors. The endpoint then logs that
//! at error level on each such request, serves the response with
//! `private, no-cache`, and builds the body again from the flow data with the
//! builder the pipeline holds as a resource (see
//! [`fiftyone_pipeline_core::Pipeline::resource`]), used only when it is the
//! same `Arc` as the builder element, as [`crate::WebPipeline`] holds the
//! builders it runs. The JavaScript is built around the JSON builder's output, itself
//! built again if that builder left no data too. When only the JSON builder
//! left no data, the JavaScript builder's script holds placeholder JSON, so it
//! is treated as left out and built again too. With no such resource the body
//! is empty, and a JSON builder that left no data is still logged after the
//! JavaScript builder. A pipeline without the builder serves an empty body and
//! logs nothing.
//!
//! # Where the `Vary` whitelist comes from
//!
//! The [`crate::WebPipeline`] builder collects the header keys every element
//! names through its evidence key filter, and the adapter serves the endpoints
//! with [`crate::WebPipeline::endpoint_options`], which takes that list at each
//! response. An element can still be learning its accepted evidence after the
//! pipeline is built, as the cloud request engine is after a transient
//! discovery failure, so the list is collected again once every element's
//! filter is complete (see [`crate::WebPipeline::vary_whitelist`]). An adapter
//! with its own view of the headers that change the result can pass a
//! different list to [`EndpointOptions::new`].

use std::any::Any;
use std::hash::{BuildHasher, Hash, Hasher};
use std::ops::Deref;
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::sync::Arc;

use ahash::RandomState;
use fiftyone_javascript_builder::{
    JavaScriptBuilderElement, JAVASCRIPT_BUILDER_DATA_KEY, JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY,
};
use fiftyone_json_builder::{
    JsonBuilderElement, JSON_BUILDER_DATA_KEY, JSON_BUILDER_ELEMENT_DATA_KEY,
};
use fiftyone_pipeline_core::constants::EVIDENCE_HTTP_HEADER_PREFIX;
use fiftyone_pipeline_core::{EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData};

use crate::pipeline::VaryList;
use crate::request::RequestData;
use crate::response::WebResponse;

/// The `Content-Type` used for the JavaScript endpoint, `application/x-javascript`.
pub const CONTENT_TYPE_JAVASCRIPT: &str = "application/x-javascript";

/// The `Content-Type` used for the JSON endpoint.
pub const CONTENT_TYPE_JSON: &str = "application/json";

/// The `Cache-Control` value used when processing recorded no errors and the
/// builder whose output the endpoint serves left its data.
///
/// `private` so shared caches (proxies) do not store the per-device result, and
/// `max-age=1800` (30 minutes) so the client may reuse it for half an hour.
pub const CACHE_CONTROL_CACHEABLE: &str = "private, max-age=1800";

/// The `Cache-Control` value used when processing recorded one or more errors,
/// or the builder whose output the endpoint serves left no data.
///
/// `private` keeps shared caches (proxies) from storing a response made for
/// one device, as in [`CACHE_CONTROL_CACHEABLE`]. A browser may keep the
/// response but must ask the server before using it again, and the response
/// carries no `ETag`, so asking again always gets a full response and never a
/// `304` confirming the faulty one. The web integration specification gives
/// this value for a failed response
/// ([specifications#53](https://github.com/51Degrees/specifications/pull/53)).
pub const CACHE_CONTROL_NO_CACHE: &str = "private, no-cache";

/// The first fixed seed for the ETag hasher (see [`compute_etag`]).
const ETAG_SEED_0: u64 = 0x_51de_9e55_0000_0001;
/// The second fixed seed for the ETag hasher.
const ETAG_SEED_1: u64 = 0x_0000_0002_51de_9e55;
/// The third fixed seed for the ETag hasher.
const ETAG_SEED_2: u64 = 0x_51de_9e55_51de_9e55;
/// The fourth fixed seed for the ETag hasher.
const ETAG_SEED_3: u64 = 0x_dead_beef_51de_9e55;

/// Options shared by both client-side endpoints.
///
/// Take them from [`crate::WebPipeline::endpoint_options`] so the response
/// advertises the HTTP headers that influence the result, and the caching and
/// CORS headers are emitted. [`EndpointOptions::new`] takes a fixed list
/// instead.
#[derive(Debug, Clone, Default)]
pub struct EndpointOptions {
    /// An evidence-key whitelist whose `header.*` keys derive the `Vary`
    /// header, used in place of the pipeline's list. `None` with no pipeline
    /// list, or an empty whitelist, means no `Vary` header is emitted.
    pub vary_whitelist: Option<EvidenceKeyFilterWhitelist>,

    /// The pipeline's list, for options from
    /// [`crate::WebPipeline::endpoint_options`].
    pipeline_vary: Option<SharedVaryList>,

    /// When `true`, set by [`EndpointOptions::without_extra_headers`], the
    /// caching, `Vary`, `ETag` and CORS headers are not added (only
    /// `Content-Type` and `Content-Length` are), so an adapter can apply its
    /// own. The `If-None-Match` short circuit still applies, and its `304` or
    /// `412` carries no headers. `false` by default.
    suppress_extra_headers: bool,
}

/// A pipeline's shared `Vary` list that keeps [`EndpointOptions`]
/// `UnwindSafe` and `RefUnwindSafe`.
///
/// The options only read the list's whitelists, which are set once, so a
/// panic elsewhere cannot leave a value half changed for them to read.
#[derive(Debug, Clone)]
struct SharedVaryList(Arc<VaryList>);

impl UnwindSafe for SharedVaryList {}
impl RefUnwindSafe for SharedVaryList {}

impl Deref for SharedVaryList {
    type Target = VaryList;

    fn deref(&self) -> &VaryList {
        &self.0
    }
}

impl EndpointOptions {
    /// Create options with the given `Vary` whitelist and the caching/CORS
    /// headers enabled.
    pub fn new(vary_whitelist: EvidenceKeyFilterWhitelist) -> Self {
        EndpointOptions {
            vary_whitelist: Some(vary_whitelist),
            pipeline_vary: None,
            suppress_extra_headers: false,
        }
    }

    /// Options taking the `Vary` header from a pipeline's list, with the
    /// caching/CORS headers enabled.
    pub(crate) fn from_vary_list(list: Arc<VaryList>) -> Self {
        EndpointOptions {
            vary_whitelist: None,
            pipeline_vary: Some(SharedVaryList(list)),
            suppress_extra_headers: false,
        }
    }

    /// The whitelist the next response's `Vary` header is derived from, being
    /// [`EndpointOptions::vary_whitelist`] when set, otherwise the pipeline's
    /// list for options from [`crate::WebPipeline::endpoint_options`].
    pub fn current_vary_whitelist(&self) -> Option<&EvidenceKeyFilterWhitelist> {
        self.vary_whitelist
            .as_ref()
            .or_else(|| self.pipeline_vary.as_deref().map(VaryList::whitelist))
    }

    /// Disable the caching, `Vary`, `ETag` and CORS headers. Returns `self` for
    /// chaining.
    pub fn without_extra_headers(mut self) -> Self {
        self.suppress_extra_headers = true;
        self
    }
}

/// Serve the generated client-side JavaScript for a processed flow data.
///
/// The body is the JavaScript produced by the JavaScript builder element (its
/// element data under
/// [`fiftyone_javascript_builder::JAVASCRIPT_BUILDER_DATA_KEY`]). If that element
/// left no data the body is built again as the module documentation describes,
/// and if the pipeline has no such element the body is empty. The response uses
/// the [`CONTENT_TYPE_JAVASCRIPT`] content type and, unless
/// [`EndpointOptions::without_extra_headers`] was used, the shared caching and
/// CORS headers described in the module documentation.
///
/// `request` supplies the conditional-request (`If-None-Match`) and `Origin`
/// headers and the method. When `If-None-Match` matches the computed `ETag` a
/// `304 Not Modified` is returned with no body and the caching and CORS
/// headers the `200` would have carried, or on a method other than `GET` or
/// `HEAD` a `412 Precondition Failed` with only the CORS header.
pub fn serve_javascript(
    flow_data: &FlowData,
    request: &dyn RequestData,
    options: &EndpointOptions,
) -> WebResponse {
    let left_no_data = javascript_left_no_data(flow_data);
    build_response(
        flow_data,
        request,
        left_no_data,
        || javascript_body(flow_data, left_no_data),
        CONTENT_TYPE_JAVASCRIPT,
        options,
    )
}

/// Serve the generated JSON for a processed flow data.
///
/// The body is the JSON produced by the JSON builder element (its element data
/// under [`fiftyone_json_builder::JSON_BUILDER_DATA_KEY`], the `json` property).
/// If that element left no data the body is built again as the module
/// documentation describes, and if the pipeline has no such element the body
/// is empty. The response uses the
/// [`CONTENT_TYPE_JSON`] content type and the shared caching and CORS headers
/// described in the module documentation. As with the JavaScript endpoint a
/// matching `If-None-Match` yields a `304 Not Modified` with no body and the
/// caching and CORS headers the `200` would have carried, or on a method other
/// than `GET` or `HEAD`, such as the `POST` the client-side script sends, a
/// `412 Precondition Failed` with only the CORS header.
pub fn serve_json(
    flow_data: &FlowData,
    request: &dyn RequestData,
    options: &EndpointOptions,
) -> WebResponse {
    let left_no_data = json_left_no_data(flow_data);
    build_response(
        flow_data,
        request,
        left_no_data,
        || json_body(flow_data, left_no_data),
        CONTENT_TYPE_JSON,
        options,
    )
}

/// True when the JavaScript builder left no data, or the JSON builder left
/// none, because the script then holds placeholder JSON in place of the
/// properties. A pipeline without the JavaScript builder leaves nothing out.
fn javascript_left_no_data(flow_data: &FlowData) -> bool {
    match flow_data.get(JAVASCRIPT_BUILDER_DATA_KEY) {
        Some(_) => json_left_no_data(flow_data),
        None => has_element(flow_data, JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY),
    }
}

/// True when the pipeline has the JSON builder and it left no data.
fn json_left_no_data(flow_data: &FlowData) -> bool {
    flow_data.get(JSON_BUILDER_DATA_KEY).is_none()
        && has_element(flow_data, JSON_BUILDER_ELEMENT_DATA_KEY)
}

/// The JavaScript builder's output for the request, built again when
/// [`javascript_left_no_data`] is true. A JSON builder that left no data is
/// logged after the JavaScript builder even when the script cannot be built.
fn javascript_body(flow_data: &FlowData, left_no_data: bool) -> String {
    if !left_no_data {
        return flow_data
            .get(JAVASCRIPT_BUILDER_DATA_KEY)
            .map(|data| data.javascript().to_owned())
            .unwrap_or_default();
    }
    log_missing_data(JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY);
    let json_left_no_data = json_left_no_data(flow_data);
    match held_builder::<JavaScriptBuilderElement>(flow_data, JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY) {
        Some(builder) => {
            let json = json_body(flow_data, json_left_no_data);
            builder.javascript_for(flow_data, &json)
        }
        None => {
            if json_left_no_data {
                log_missing_data(JSON_BUILDER_ELEMENT_DATA_KEY);
            }
            String::new()
        }
    }
}

/// The JSON builder's output for the request, built again when
/// [`json_left_no_data`] is true.
fn json_body(flow_data: &FlowData, left_no_data: bool) -> String {
    if !left_no_data {
        return flow_data
            .get(JSON_BUILDER_DATA_KEY)
            .map(|data| data.json().to_owned())
            .unwrap_or_default();
    }
    log_missing_data(JSON_BUILDER_ELEMENT_DATA_KEY);
    held_builder::<JsonBuilderElement>(flow_data, JSON_BUILDER_ELEMENT_DATA_KEY)
        .map(|builder| builder.json_for(flow_data))
        .unwrap_or_default()
}

/// The builder the pipeline holds as a resource, when it is the same `Arc` as
/// the element with this data key, so the body is built again by the element
/// that ran, with its configuration.
fn held_builder<T: Any + Send + Sync>(flow_data: &FlowData, data_key: &str) -> Option<Arc<T>> {
    let pipeline = flow_data.pipeline();
    let element = pipeline
        .flow_elements()
        .iter()
        .find(|element| element.data_key() == data_key)?;
    let builder = pipeline.resource::<T>()?;
    std::ptr::addr_eq(Arc::as_ptr(element), Arc::as_ptr(&builder)).then_some(builder)
}

/// True if the flow data's pipeline has an element with this data key.
fn has_element(flow_data: &FlowData, data_key: &str) -> bool {
    flow_data
        .pipeline()
        .flow_elements()
        .iter()
        .any(|element| element.data_key() == data_key)
}

/// Log that the builder with this data key left no data for the request.
fn log_missing_data(data_key: &str) {
    log::error!("Failed to get data from element '{data_key}', so the response is not cached.");
}

/// Build the response for either endpoint, applying the shared header rules.
/// `left_no_data` says whether a builder left no data for the request, and
/// `body` is called only when the response is a `200`.
fn build_response(
    flow_data: &FlowData,
    request: &dyn RequestData,
    left_no_data: bool,
    body: impl FnOnce() -> String,
    content_type: &str,
    options: &EndpointOptions,
) -> WebResponse {
    let filter = flow_data.evidence_key_filter();
    let etag = compute_etag(flow_data, filter);
    let matched = request
        .if_none_match()
        .is_some_and(|if_none_match| etag_matches(&if_none_match, &etag));

    // A matching validator on a method other than GET or HEAD fails the
    // precondition (RFC 9110 section 13.1.2).
    if matched && !is_get_or_head(request) {
        let headers = if options.suppress_extra_headers {
            Vec::new()
        } else {
            access_control_allow_origin(request)
                .map(|origin| ("Access-Control-Allow-Origin".to_owned(), origin))
                .into_iter()
                .collect()
        };
        return WebResponse {
            headers,
            ..WebResponse::precondition_failed()
        };
    }

    let extra = if options.suppress_extra_headers {
        Vec::new()
    } else {
        extra_headers(flow_data, request, left_no_data, &etag, options)
    };

    // A conditional request whose validator matches the current ETag gets a
    // 304 with the headers the 200 would carry, ETag included only when the
    // 200 would send one.
    if matched {
        return WebResponse {
            headers: extra,
            ..WebResponse::not_modified()
        };
    }

    let body = body();
    let mut headers: Vec<(String, String)> = Vec::with_capacity(extra.len() + 2);
    headers.push(("Content-Type".to_owned(), content_type.to_owned()));
    // String::len is the UTF-8 byte length, which is exactly the Content-Length
    // the body needs.
    headers.push(("Content-Length".to_owned(), body.len().to_string()));
    headers.extend(extra);

    WebResponse::text(body, headers)
}

/// True when the request method is `GET` or `HEAD`, or the request does not
/// give one. Method names are case-sensitive
/// ([RFC 9110 section 9.1](https://www.rfc-editor.org/rfc/rfc9110#section-9.1)).
fn is_get_or_head(request: &dyn RequestData) -> bool {
    request
        .method()
        .is_none_or(|method| method == "GET" || method == "HEAD")
}

/// The caching, `Vary`, `ETag` and CORS headers, which the `200` and the `304`
/// to the same request both carry.
fn extra_headers(
    flow_data: &FlowData,
    request: &dyn RequestData,
    left_no_data: bool,
    etag: &str,
    options: &EndpointOptions,
) -> Vec<(String, String)> {
    let cacheable = !left_no_data && flow_data.errors().is_empty();
    let mut headers = vec![("Cache-Control".to_owned(), cache_control(cacheable))];

    if let Some(whitelist) = options.current_vary_whitelist() {
        let vary = vary_header(whitelist);
        if !vary.is_empty() {
            headers.push(("Vary".to_owned(), vary));
        }
    }

    // The validator stands for the evidence, not for whether processing
    // worked, so a response that is not cacheable goes without one rather
    // than being confirmed by a 304 once processing recovers.
    if cacheable {
        headers.push(("ETag".to_owned(), etag.to_owned()));
    }

    if let Some(origin) = access_control_allow_origin(request) {
        headers.push(("Access-Control-Allow-Origin".to_owned(), origin));
    }

    headers
}

/// The `Cache-Control` value for a response.
fn cache_control(cacheable: bool) -> String {
    if cacheable {
        CACHE_CONTROL_CACHEABLE.to_owned()
    } else {
        CACHE_CONTROL_NO_CACHE.to_owned()
    }
}

/// Derive the `Vary` header value from an evidence-key whitelist.
///
/// Every HTTP-header evidence key the whitelist accepts (a key under the
/// `header.` prefix) contributes the header name (the part after the prefix).
/// Names are de-duplicated case-insensitively keeping the first spelling seen,
/// sorted for determinism, then comma-joined. An empty string means no
/// HTTP-header evidence is used and the caller should omit the `Vary` header
/// entirely.
pub fn vary_header(whitelist: &EvidenceKeyFilterWhitelist) -> String {
    let header_prefix = format!("{EVIDENCE_HTTP_HEADER_PREFIX}.");

    let mut names: Vec<String> = Vec::new();

    for (key, _order) in whitelist.whitelist() {
        // Whitelist keys are already stored lowercased.
        if let Some(stripped) = key.strip_prefix(&header_prefix) {
            if stripped.is_empty() {
                continue;
            }
            if !names.iter().any(|s| s == stripped) {
                names.push(stripped.to_owned());
            }
        }
    }

    names.sort();
    names.join(",")
}

/// Compute the `ETag` for a flow data.
///
/// The tag is a hex digest of the deterministic [`fiftyone_pipeline_core::DataKey`]
/// the flow data's evidence produces under the pipeline filter, rendered as a
/// quoted string per the HTTP `ETag` grammar.
///
/// The digest is produced by an [`ahash::RandomState`] hasher built from the
/// four fixed seeds in this module, so the same evidence gives the same tag in
/// every process running the same build. A browser revalidates against
/// whichever process answers, which after a restart or behind a load balancer
/// is not the one that made the tag. The tag is deliberately **opaque and
/// non-portable**: callers must treat it as a meaningless validator and never
/// parse it, compare it across language ports, or persist it expecting another
/// implementation to reproduce it. Only equality against a value this same code
/// produced is meaningful, which is exactly what conditional requests need.
pub fn compute_etag(flow_data: &FlowData, filter: &dyn EvidenceKeyFilter) -> String {
    let key = flow_data.generate_key(filter);

    // The seeds are given explicitly because ahash draws the seeds of its
    // default hasher at run time on native targets, once per process.
    let mut hasher =
        RandomState::with_seeds(ETAG_SEED_0, ETAG_SEED_1, ETAG_SEED_2, ETAG_SEED_3).build_hasher();

    // Hash the ordered entries. The key's entry order is already deterministic.
    for (name, value) in key.entries() {
        name.hash(&mut hasher);
        // Separate the name from the value so ("ab","c") and ("a","bc") differ.
        0u8.hash(&mut hasher);
        value.hash(&mut hasher);
        0u8.hash(&mut hasher);
    }

    let digest = hasher.finish();
    format!("\"{digest:016x}\"")
}

/// Compare an incoming `If-None-Match` value against a computed `ETag`.
///
/// The comparison tolerates the optional weak-validator prefix (`W/`) and a
/// comma-separated list of candidate tags, returning true if any candidate
/// equals the computed tag. The wildcard `*` always matches. This is the subset
/// of [RFC 9110 section 13.1.2](https://www.rfc-editor.org/rfc/rfc9110#section-13.1.2)
/// the endpoints need.
pub fn etag_matches(if_none_match: &str, etag: &str) -> bool {
    for candidate in if_none_match.split(',') {
        let candidate = candidate.trim();
        if candidate == "*" {
            return true;
        }
        let candidate = candidate.strip_prefix("W/").unwrap_or(candidate);
        if candidate == etag {
            return true;
        }
    }
    false
}

/// Determine the `Access-Control-Allow-Origin` value to echo, if any.
///
/// Returns the request `Origin` header verbatim when it is present and is not
/// the literal string `null`. A `null` origin (which browsers send for, for
/// example, sandboxed iframes and `file://` pages) is never echoed, so the
/// header is omitted in that case.
fn access_control_allow_origin(request: &dyn RequestData) -> Option<String> {
    match request.origin() {
        Some(origin) if !origin.eq_ignore_ascii_case("null") => Some(origin),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etag_matches_handles_weak_and_lists() {
        assert!(etag_matches("\"abc\"", "\"abc\""));
        assert!(etag_matches("W/\"abc\"", "\"abc\""));
        assert!(etag_matches("\"x\", \"abc\"", "\"abc\""));
        assert!(etag_matches("*", "\"abc\""));
        assert!(!etag_matches("\"def\"", "\"abc\""));
    }

    #[test]
    fn vary_strips_prefix_and_dedupes() {
        let filter = EvidenceKeyFilterWhitelist::new([
            "header.User-Agent",
            "header.user-agent",
            "header.Sec-CH-UA",
            "query.sequence",
            "cookie.session",
        ]);
        // header.* keys only, prefix stripped, de-duplicated, sorted.
        assert_eq!(vary_header(&filter), "sec-ch-ua,user-agent");
    }

    #[test]
    fn vary_empty_when_no_header_keys() {
        let filter = EvidenceKeyFilterWhitelist::new(["query.sequence", "cookie.x"]);
        assert_eq!(vary_header(&filter), "");
    }
}
