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

//! The route handlers for the two client-side endpoints.
//!
//! - [`javascript_handler`] serves `GET /51Degrees.core.js`: it builds evidence
//!   from the request, processes the pipeline and returns the JavaScript include
//!   the JavaScript builder produced.
//! - [`json_handler`] serves `POST /51dpipeline/json`: it does the same and
//!   returns the JSON the JSON builder produced. The POST body is the
//!   client-side callback's `application/x-www-form-urlencoded` evidence, which
//!   is folded into the `query.` evidence prefix.
//!
//! Both handlers process the pipeline and call the framework-neutral
//! [`serve_javascript`] / [`serve_json`] functions in one task on the blocking
//! thread pool, through [`crate::process::process_then`], because when a
//! builder left no data, serving builds the body again, which for the script
//! means rendering and minifying it. The resulting
//! [`fiftyone_pipeline_web::WebResponse`] is mapped onto an axum response,
//! including the `If-None-Match` short circuit those functions implement,
//! being `304 Not Modified` for the JavaScript endpoint's `GET` (or `HEAD`)
//! and `412 Precondition Failed` for the JSON endpoint's `POST`.

use std::net::SocketAddr;

use axum::body::{to_bytes, Body};
use axum::extract::{ConnectInfo, State};
use axum::http::{Request, StatusCode};
use axum::response::{IntoResponse, Response};
use fiftyone_pipeline_web::{serve_javascript, serve_json};

use crate::process::{process_then, sent_uri, CapturedRequest};
use crate::response::into_axum_response;
use crate::state::FiftyOneState;

/// The maximum request body the JSON endpoint will buffer, in bytes.
///
/// The client-side callback body is a short list of form fields, so a generous
/// but bounded cap protects against an oversized or hostile body without
/// rejecting any legitimate request.
const MAX_BODY_BYTES: usize = 64 * 1024;

/// Which endpoint a handler is serving, selecting the body the response carries.
#[derive(Clone, Copy)]
enum Endpoint {
    /// The JavaScript include endpoint.
    JavaScript,
    /// The JSON callback endpoint.
    Json,
}

/// Handle `GET /51Degrees.core.js`.
///
/// Builds evidence from the request, processes the pipeline off the runtime and
/// returns the generated JavaScript with the caching, `Vary`, weak `ETag` and
/// CORS headers the web crate computes. An `If-None-Match` that matches the
/// `ETag` under the weak comparison, with or without `W/`, yields `304 Not
/// Modified`.
pub async fn javascript_handler(
    State(state): State<FiftyOneState>,
    request: Request<Body>,
) -> Response {
    // The GET has no meaningful body, so capture the parts with an empty body.
    serve(state, request, Vec::new(), Endpoint::JavaScript).await
}

/// Handle `POST /51dpipeline/json`.
///
/// Buffers the form body (bounded by `MAX_BODY_BYTES`), builds evidence
/// including the form fields, processes the pipeline off the runtime and returns
/// the generated JSON with the caching, `Vary`, weak `ETag` and CORS headers
/// the web crate computes. An `If-None-Match` that matches the `ETag` yields
/// `412 Precondition Failed`, because RFC 9110 section 13.1.2 gives a `304`
/// only to `GET` and `HEAD`.
pub async fn json_handler(State(state): State<FiftyOneState>, request: Request<Body>) -> Response {
    // Split the body off so it can be buffered before the rest is captured.
    let (parts, body) = request.into_parts();
    let body_bytes = match to_bytes(body, MAX_BODY_BYTES).await {
        Ok(bytes) => bytes.to_vec(),
        Err(_) => {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                "request body exceeds the 51Degrees JSON endpoint limit",
            )
                .into_response()
        }
    };
    let request = Request::from_parts(parts, Body::empty());
    serve(state, request, body_bytes, Endpoint::Json).await
}

/// Shared body of both handlers: capture the request, process it and serve the
/// chosen endpoint on the blocking thread pool, then map to an axum response.
async fn serve(
    state: FiftyOneState,
    request: Request<Body>,
    body: Vec<u8>,
    endpoint: Endpoint,
) -> Response {
    let (parts, _) = request.into_parts();
    let peer_addr = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|connect_info| connect_info.0);
    let captured = CapturedRequest {
        headers: parts.headers,
        uri: sent_uri(&parts.extensions, parts.uri),
        body,
        peer_addr,
    };

    let options = state.endpoint_options();
    let served = process_then(
        &state,
        captured,
        Some(parts.method),
        move |flow_data, request| match endpoint {
            Endpoint::JavaScript => serve_javascript(&flow_data, request, &options),
            Endpoint::Json => serve_json(&flow_data, request, &options),
        },
    )
    .await;

    match served {
        Ok(web_response) => into_axum_response(web_response),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "51Degrees pipeline processing failed",
        )
            .into_response(),
    }
}
