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

//! Building evidence from a request and running the pipeline off the async
//! runtime.
//!
//! Pipeline processing is synchronous and CPU-bound: an on-premise engine does
//! a non-trivial amount of work, and even the cloud engines block a thread on a
//! synchronous HTTP call. Running that directly inside an async task would stall
//! the runtime's worker thread, so [`process_request`] hands the work to
//! [`tokio::task::spawn_blocking`], which runs it on the blocking thread pool and
//! returns the processed [`FlowData`]. The endpoint handlers use
//! [`process_then`] instead, which also serves the endpoint in that task,
//! because when a builder left no data, serving builds the body again, which
//! for the script means rendering and minifying it.
//!
//! The captured request parts (header map, URI, buffered body, peer address) are
//! moved into the blocking closure so it owns everything it reads. [`FlowData`]
//! is `Send`, so it travels back out of the closure to the caller.

use std::net::SocketAddr;

use axum::extract::OriginalUri;
use axum::http::{Extensions, HeaderMap, Method, Uri};
use fiftyone_pipeline_core::FlowData;
use fiftyone_pipeline_web::build_evidence;

use crate::request::AxumRequestData;
use crate::state::FiftyOneState;

/// The request parts the pipeline needs, captured so they can move onto a
/// blocking thread.
///
/// Built by the middleware and the endpoint handlers after they have read the
/// request, then moved onto the blocking thread pool to be processed, as
/// [`process_request`] does. Owning the parts (rather than
/// borrowing the live request) is what lets the heavy work move off the async
/// runtime.
pub struct CapturedRequest {
    /// The request headers.
    pub headers: HeaderMap,
    /// The request URI, carrying the path, the query string and any scheme
    /// and authority. The middleware and the endpoint handlers capture it as
    /// the client sent it, before a nested router takes its prefix off.
    pub uri: Uri,
    /// The buffered request body, used for form fields on a POST. Empty for a
    /// GET or a body-less request.
    pub body: Vec<u8>,
    /// The connection's peer address, if axum recorded one through
    /// [`axum::extract::ConnectInfo`].
    pub peer_addr: Option<SocketAddr>,
}

impl CapturedRequest {
    /// Build the [`AxumRequestData`] view over these captured parts.
    ///
    /// The borrow lives as long as `self`, so the resulting view is used and
    /// dropped before `self` is.
    fn request_data(&self) -> AxumRequestData<'_> {
        let mut request =
            AxumRequestData::new(&self.headers, &self.uri).with_form_body(&self.body[..]);
        if let Some(addr) = self.peer_addr {
            request = request.with_peer_addr(addr);
        }
        request
    }
}

/// The request URI as the client sent it. A router nested under a prefix
/// takes the prefix off the URI its routes see, and axum keeps the URI as
/// sent in [`OriginalUri`], so that is the one read where axum recorded it.
pub(crate) fn sent_uri(extensions: &Extensions, uri: Uri) -> Uri {
    match extensions.get::<OriginalUri>() {
        Some(OriginalUri(original)) => original.clone(),
        None => uri,
    }
}

/// Build evidence from the captured request, then process the pipeline on the
/// blocking thread pool and return the processed [`FlowData`].
///
/// [`CapturedRequest`] holds no method, so the evidence built here has no
/// request line. [`process_request_with_method`] gives it, as the middleware
/// and the endpoint handlers do.
///
/// The pipeline is configured to suppress process exceptions in the web
/// integration, so a per-element failure is recorded on the flow data rather
/// than surfaced here. The only way this returns `Err` is if the blocking task
/// itself panics, which [`tokio::task::JoinError`] reports.
pub async fn process_request(
    state: &FiftyOneState,
    captured: CapturedRequest,
) -> Result<FlowData, tokio::task::JoinError> {
    process_then(state, captured, None, |flow_data, _| flow_data).await
}

/// As [`process_request`], with the request's method, so the evidence carries
/// the request line: `server.request-method`, `server.request-path` and
/// `server.request-query`, the last two read from the captured URI.
pub async fn process_request_with_method(
    state: &FiftyOneState,
    captured: CapturedRequest,
    method: Method,
) -> Result<FlowData, tokio::task::JoinError> {
    process_then(state, captured, Some(method), |flow_data, _| flow_data).await
}

/// As [`process_request`], then pass the processed [`FlowData`] and the
/// request view, carrying `method` when given, to `then` in the same blocking
/// task and return its result. This returns `Err` when processing or `then`
/// panics.
pub(crate) async fn process_then<T, F>(
    state: &FiftyOneState,
    captured: CapturedRequest,
    method: Option<Method>,
    then: F,
) -> Result<T, tokio::task::JoinError>
where
    F: FnOnce(FlowData, &AxumRequestData<'_>) -> T + Send + 'static,
    T: Send + 'static,
{
    let pipeline = std::sync::Arc::clone(state.pipeline());

    tokio::task::spawn_blocking(move || {
        // Evidence is built inside the closure so the borrowed request view
        // never crosses the spawn boundary; only owned data does.
        let mut request = captured.request_data();
        if let Some(method) = method {
            request = request.with_method(method);
        }
        let evidence = build_evidence(&request, pipeline.evidence_key_filter());
        let mut flow_data = pipeline.create_flow_data_with(evidence);
        // Errors are suppressed onto the flow data by the web pipeline, so this
        // returns Ok; the result is read from flow_data.errors() downstream.
        let _ = flow_data.process();
        then(flow_data, &request)
    })
    .await
}
