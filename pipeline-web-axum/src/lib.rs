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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-pipeline-web-axum-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-pipeline-web-axum-lib.rs&utm_term=logo)
//!
//! # 51Degrees axum web integration
//!
//! This crate is the [axum](https://docs.rs/axum) adapter over the
//! framework-neutral [`fiftyone_pipeline_web`] crate. Where that crate defines
//! *what* the web integration does (build evidence from a request, serve the
//! client-side endpoints, apply set-headers), this crate supplies the small
//! amount of axum-specific glue: reading a real request, processing the pipeline
//! off the async runtime, and mapping the result onto an axum response.
//!
//! It provides the two client-side endpoints, the evidence mapping and the
//! caching, `Vary`, `ETag` and CORS behavior, expressed through idiomatic axum
//! pieces: a
//! [`FromRequestParts`](axum::extract::FromRequestParts) extractor, a
//! [`from_fn_with_state`](axum::middleware::from_fn_with_state) middleware and a
//! pair of route handlers.
//!
//! ## The pieces
//!
//! - [`AxumRequestData`] implements [`fiftyone_pipeline_web::RequestData`] over an
//!   axum request: headers, cookies, query string, form body, client IP (from
//!   `X-Forwarded-For` or the connection peer) and protocol (from
//!   `X-Forwarded-Proto`, `Forwarded` or the URI scheme).
//! - [`FiftyOneState`] is the shared, cheaply-cloned state (the built pipeline,
//!   the web options and the `Vary` whitelist) the middleware and handlers carry.
//!   Build it from a [`fiftyone_pipeline_web::WebPipeline`] with
//!   [`FiftyOneState::from_web_pipeline`].
//! - [`fiftyone_middleware`] processes the pipeline per request, stores the
//!   result for the extractor, applies the set-headers output to the response,
//!   and short-circuits the two client-side endpoints.
//! - [`javascript_handler`] and [`json_handler`] serve `GET /51Degrees.core.js`
//!   and `POST /51dpipeline/json`.
//! - [`FiftyOneResult`] is the extractor a handler uses to read the processed
//!   flow data.
//! - [`register`] mounts the routes and middleware onto a [`Router`](axum::Router)
//!   in one call.
//!
//! ## Why processing runs on a blocking thread
//!
//! [`fiftyone_pipeline_core::FlowData::process`] is synchronous and CPU-bound (an
//! on-premise engine does real work; the cloud engines block on a synchronous
//! HTTP call). Running it inline would stall an async worker thread, so every
//! request's processing is dispatched through
//! [`tokio::task::spawn_blocking`]. This is why the crate depends on tokio.
//!
//! ## Quick start
//!
//! ```no_run
//! use std::net::SocketAddr;
//! use axum::{routing::get, Router};
//! use fiftyone_pipeline_web::{WebIntegrationOptions, WebPipeline};
//! use fiftyone_pipeline_web_axum::{register, FiftyOneResult, FiftyOneState};
//!
//! # async fn run() {
//! // Assemble a web pipeline from the application's own elements (here none).
//! let web = WebPipeline::build(Vec::new(), WebIntegrationOptions::default()).unwrap();
//! let state = FiftyOneState::from_web_pipeline(&web);
//!
//! // A handler reads the processed results through the extractor.
//! async fn home(result: FiftyOneResult) -> String {
//!     format!("had errors: {}", result.has_errors())
//! }
//!
//! let app = register(Router::new().route("/", get(home)), state);
//!
//! // Serve with connect-info so the client IP is available.
//! let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
//! axum::serve(
//!     listener,
//!     app.into_make_service_with_connect_info::<SocketAddr>(),
//! )
//! .await
//! .unwrap();
//! # }
//! ```

#![warn(missing_docs)]

mod form;
mod handlers;
mod middleware;
mod process;
mod request;
mod response;
mod result;
mod router;
mod state;

pub use handlers::{javascript_handler, json_handler};
pub use middleware::fiftyone_middleware;
pub use process::{process_request, CapturedRequest};
pub use request::AxumRequestData;
pub use response::into_axum_response;
pub use result::FiftyOneResult;
pub use router::register;
pub use state::FiftyOneState;

// Re-export the framework-neutral types an adapter user commonly needs, so a
// downstream crate can build a WebPipeline and the state without also naming
// fiftyone-pipeline-web directly.
pub use fiftyone_pipeline_web::{WebEndpoint, WebIntegrationOptions, WebPipeline};
