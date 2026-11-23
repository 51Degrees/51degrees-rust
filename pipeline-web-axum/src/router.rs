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

//! Wiring the 51Degrees routes and middleware onto an [`axum::Router`].
//!
//! [`register`] is the one-call helper: given a [`Router`] and the shared
//! [`FiftyOneState`], it mounts the JavaScript and JSON endpoint routes and
//! installs the [`crate::middleware::fiftyone_middleware`]. After calling it the
//! application adds its own routes; the middleware processes every request and
//! the two client-side endpoints serve themselves.
//!
//! # Endpoint paths
//!
//! The routes are mounted at the paths in the state's
//! [`fiftyone_pipeline_web::WebIntegrationOptions`] (defaulting to
//! `/51Degrees.core.js` and `/51dpipeline/json`). The middleware also matches
//! those paths by suffix, so even an application mounted under a prefix has the
//! endpoints served correctly through the short-circuit.
//!
//! # Client IP
//!
//! To get the connection peer address as the client IP, serve the router with
//! `into_make_service_with_connect_info::<SocketAddr>()` so axum records a
//! [`axum::extract::ConnectInfo`]. Without it the adapter still works and uses
//! the `X-Forwarded-For` header when present.

use axum::middleware::from_fn_with_state;
use axum::routing::{get, post};
use axum::Router;

use crate::handlers::{javascript_handler, json_handler};
use crate::middleware::fiftyone_middleware;
use crate::state::FiftyOneState;

/// Mount the 51Degrees endpoint routes and per-request middleware onto a router.
///
/// The router's state type `S` is the application's own state; the 51Degrees
/// pieces carry their state ([`FiftyOneState`]) themselves, so they do not
/// constrain it. The returned router is the input router with:
///
/// - `GET <javascript_endpoint>` routed to the JavaScript handler,
/// - `POST <json_endpoint>` routed to the JSON handler,
/// - the [`fiftyone_middleware`] layered over everything.
///
/// Call it before or after adding the application's own routes; the middleware
/// applies to all of them because [`Router::layer`] wraps the whole router.
///
/// # Example
///
/// ```
/// use axum::{routing::get, Router};
/// use fiftyone_pipeline_web::{WebIntegrationOptions, WebPipeline};
/// use fiftyone_pipeline_web_axum::{register, FiftyOneState};
///
/// let web = WebPipeline::build(Vec::new(), WebIntegrationOptions::default()).unwrap();
/// let state = FiftyOneState::from_web_pipeline(&web);
///
/// let app: Router = register(
///     Router::new().route("/", get(|| async { "home" })),
///     state,
/// );
/// let _ = app;
/// ```
pub fn register<S>(router: Router<S>, state: FiftyOneState) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let options = state.options();
    let javascript_endpoint = options.javascript_endpoint.clone();
    let json_endpoint = options.json_endpoint.clone();

    // The endpoint handlers carry FiftyOneState, so they are added as a nested
    // router with that state and merged in. with_state erases the state type so
    // the result composes with the application's own state S.
    let endpoints = Router::new()
        .route(&javascript_endpoint, get(javascript_handler))
        .route(&json_endpoint, post(json_handler))
        .with_state(state.clone());

    router
        .merge(endpoints)
        .layer(from_fn_with_state(state, fiftyone_middleware))
}
