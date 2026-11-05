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

//! @page dd-web-getting-started-cloud-example Getting Started Cloud (Device Detection, Web)
//!
//! Device Detection web example: cloud Getting Started (server plus client).
//!
//! It builds a cloud device-detection pipeline, wraps it in a [`WebPipeline`] so
//! the two
//! client-side endpoints (`/51Degrees.core.js` and `/51dpipeline/json`) are
//! mounted, and serves an HTML page that shows the server-side detection result
//! and then refreshes it client-side via the 51Degrees JavaScript.
//!
//! @snippet dd-web-getting-started-cloud.rs example

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use axum::response::Html;
use axum::routing::get;
use axum::Router;
use fiftyone_device_detection::{
    CloudEngineState, DeviceDetectionPipelineBuilder, DEVICE_DATA_KEY,
};
use fiftyone_pipeline_core::FlowElement;
use fiftyone_pipeline_web::{response_headers, WebIntegrationOptions, WebPipeline};
use fiftyone_pipeline_web_axum::{register, FiftyOneResult, FiftyOneState};

// The shared example HTML helpers (design-system markup, the vendored assets and
// the static-asset handlers) live in a sibling module so every Device Detection
// web bin renders to the same `.c-eg-*` contract without duplicating the markup.
#[path = "../web_support/mod.rs"]
mod web_support;

use web_support::{
    detection_results_table, evidence_table, response_header_table, serve_css, serve_js,
    PageOptions, ASSETS_CSS_ROUTE, ASSETS_JS_ROUTE,
};

/// Options that drive [`run`]. The resource key is required for the cloud
/// deployment; the bind address lets the test pick an ephemeral port while the
/// binary uses a fixed one.
pub struct Options {
    /// The 51Degrees cloud resource key (from <https://configure.51degrees.com?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-web-getting-started-cloud.rs&utm_term=resource_key>).
    pub resource_key: String,
    /// An optional override for the cloud endpoint, read from
    /// `51DEGREES_CLOUD_ENDPOINT`. `None` leaves the choice to the cloud request
    /// engine, which reads the same variable and defaults to the public cloud.
    pub endpoint: Option<String>,
    /// The socket address the server binds to.
    pub address: SocketAddr,
}

/// Build the cloud web application router, ready to serve.
///
/// The router carries the [`FiftyOneState`] through its middleware, so the two
/// client-side endpoints and the home page all share one built pipeline. It is
/// returned (rather than served) so the integration test can drive it in process
/// with `tower::ServiceExt::oneshot` while [`run`] serves it over TCP.
///
/// The cloud request engine resolves its discovery (the accepted evidence keys
/// and the accessible properties) when it builds. Passing `Some(state)` supplies
/// that discovery up front so the build does no cloud call, which the integration
/// test uses to assemble the app offline. Passing `None` lets the builder fetch
/// the discovery from the cloud, which is the live path the binary takes.
///
/// `share_usage` is true when the binary serves the page. The tests pass false,
/// so a test run sends nothing to 51Degrees.
pub fn build_app(
    resource_key: &str,
    endpoint: Option<&str>,
    state: Option<CloudEngineState>,
    share_usage: bool,
) -> anyhow::Result<Router> {
    // Build the cloud device-detection pipeline. Usage sharing is required for
    // a web example, because it lets 51Degrees improve detection. It is on by
    // default, and is set here from the caller so the binary turns it on and
    // the tests turn it off. The console examples turn it off.
    let mut builder = DeviceDetectionPipelineBuilder::cloud(resource_key)
        .set_state_opt(state)
        .share_usage(share_usage);
    // The cloud endpoint. Unset, the cloud request engine reads
    // 51DEGREES_CLOUD_ENDPOINT itself, and it is passed here as well so the
    // choice is visible. A host other than cloud.51degrees.com is an on premise
    // web server or a privately hosted 51Degrees cloud (see
    // examples_shared::CLOUD_ENDPOINT_ENV_VAR).
    if let Some(endpoint) = endpoint {
        builder = builder.endpoint(endpoint);
    }
    // WebPipeline wants the application's own elements in order. The facade has
    // already wired them (share-usage, the cloud request engine and the cloud
    // device engine), so its flow_elements are handed straight across; the web
    // layer then adds the sequence, set-headers, JSON and JavaScript elements.
    // The facade's pipeline is dropped at the end of this statement, because a
    // usage sharing element can be in only one pipeline at a time.
    let elements: Vec<Arc<dyn FlowElement>> = builder
        .build()
        .context("building the cloud device-detection pipeline")?
        .flow_elements()
        .to_vec();
    let web = WebPipeline::build(elements, WebIntegrationOptions::default())
        .context("assembling the web pipeline")?;
    let state = FiftyOneState::from_web_pipeline(&web);

    // register mounts GET /51Degrees.core.js and POST /51dpipeline/json and the
    // per-request middleware. The home page and the two static asset routes are
    // added on top; the middleware runs device detection for every request and
    // stores the result for the FiftyOneResult extractor the page handler uses.
    let app = register(
        Router::new()
            .route("/", get(home))
            .route(ASSETS_CSS_ROUTE, get(serve_css))
            .route(ASSETS_JS_ROUTE, get(serve_js)),
        state,
    );
    Ok(app)
}

// [example]
/// Serve the example page over TCP until interrupted.
pub fn run(options: Options) -> anyhow::Result<()> {
    // Build the pipeline (and so the share-usage element's blocking reqwest
    // client) on this plain thread, before the async runtime is entered. The
    // blocking client builds and then drops a short-lived runtime as it is
    // constructed, and tokio forbids dropping a runtime from inside another
    // runtime's async context, so doing this work outside block_on avoids that
    // panic.
    // No injected state on the live path: the builder fetches discovery from the
    // cloud as it builds.
    let app = build_app(
        &options.resource_key,
        options.endpoint.as_deref(),
        None,
        true,
    )?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building the tokio runtime")?;
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(options.address)
            .await
            .with_context(|| format!("binding {}", options.address))?;
        let bound = listener.local_addr().context("reading the bound address")?;
        println!("Device Detection cloud web example listening on http://{bound}");
        println!("Open it in a browser to see server-side then client-side detection.");
        // into_make_service_with_connect_info records the peer address so the
        // pipeline can use it as client-IP evidence when no forwarding header is
        // present.
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .context("serving the application")
    })
}
// [example]

/// The home-page handler. The 51Degrees middleware has already processed the
/// request, so the detection result is read from the [`FiftyOneResult`]
/// extractor and rendered into the shared design-system page.
async fn home(result: FiftyOneResult) -> Html<String> {
    // Read every displayed property and the evidence/response-header tables while
    // the flow data is briefly locked, then build the HTML outside the lock.
    let (results_html, evidence_html, headers_html, has_accept_ch) = result.with(|flow_data| {
        let results = flow_data
            .get(DEVICE_DATA_KEY)
            .map(|device| detection_results_table(device))
            .unwrap_or_else(|| {
                "<p class=\"c-eg-page__lead\">No device data was produced.</p>".to_owned()
            });

        let evidence_pairs: Vec<(String, String)> = flow_data
            .evidence()
            .iter()
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect();

        let headers = response_headers(flow_data);
        let has_accept_ch = headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("Accept-CH"));

        (
            results,
            evidence_table(&evidence_pairs),
            response_header_table(&headers),
            has_accept_ch,
        )
    });

    // The missing-Accept-CH warning is shown only when no Accept-CH was emitted.
    let accept_ch_alert = if has_accept_ch {
        ""
    } else {
        web_support::MISSING_ACCEPT_CH_ALERT
    };

    Html(web_support::render_page(PageOptions {
        title: "Device Detection - Cloud Web Example",
        lead: "This example performs device detection in the cloud within a small axum web \
               server. The first response is rendered server-side. The 51Degrees JavaScript then \
               posts back high-entropy client evidence (User-Agent Client Hints) and refreshes \
               the results below.",
        top_alert: "",
        results_html: &results_html,
        evidence_html: &evidence_html,
        headers_html: &headers_html,
        accept_ch_alert,
        // For the cloud deployment the contact-us banner invites on-premise.
        message_html: web_support::CLOUD_CONTACT_BANNER,
        // The cloud page uses the server-mounted client script, so the client
        // bootstrap loads /51Degrees.core.js then binds the shared callback.
        client_script: web_support::SERVER_CLIENT_SCRIPT,
    }))
}

fn main() -> anyhow::Result<()> {
    // The resource key is read from the aligned/legacy/CI environment variables.
    let resource_key = examples_shared::resource_key_from_env().context(
        "no 51Degrees cloud resource key found. Set 51DEGREES_RESOURCE_KEY to a key from \
         https://configure.51degrees.com?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-web-getting-started-cloud.rs&utm_term=resource-key-required and run again.",
    )?;

    // A fixed local port for the runnable binary; override with PORT if taken.
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8080);
    let address = SocketAddr::from(([127, 0, 0, 1], port));

    run(Options {
        resource_key,
        endpoint: examples_shared::cloud_endpoint_from_env(),
        address,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::{HeaderMap, Request, StatusCode};
    use tower::ServiceExt;

    /// The pieces of a response a test inspects.
    struct Captured {
        status: StatusCode,
        headers: HeaderMap,
        body: String,
    }

    /// Build a cloud app, dispatch one request through the full middleware stack
    /// and capture the response.
    ///
    /// This is intentionally a plain `async fn` driven from a `#[test]` (not a
    /// `#[tokio::test]`). The cloud request engine holds a
    /// `reqwest::blocking::Client`, whose drop blocks on an inner runtime; that
    /// drop is forbidden on a tokio worker thread. Driving the work with
    /// `Runtime::block_on` and returning before dropping the app keeps the app's
    /// drop on the plain test thread, where blocking is allowed.
    async fn dispatch(app: &Router, uri: &str) -> Captured {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(uri)
                    .header("host", "localhost")
                    .header(
                        "user-agent",
                        "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
                         AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 \
                         Safari/604.1",
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(bytes.to_vec()).unwrap();

        Captured {
            status,
            headers,
            body,
        }
    }

    /// A multi-thread runtime to drive one test's async work. Built and dropped
    /// on the plain test thread, never nested inside another runtime.
    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn server_starts_and_serves_page_and_client_script() {
        // This test only checks that the server serves its page and the client
        // script, neither of which needs real cloud data. So the app is built
        // offline: a placeholder key plus an injected default state means the
        // cloud request engine resolves its discovery from that state and makes
        // no cloud call as it builds. The per-request data POST then fails against
        // the placeholder key, but the web integration suppresses that by default,
        // so the page still renders with its static markup and generated script.
        let runtime = runtime();
        // Build the app outside the request futures so it (and its blocking HTTP
        // client) is owned and dropped here on the plain test thread.
        let app = build_app(
            "resource-key-placeholder",
            None,
            Some(CloudEngineState::default()),
            false,
        )
        .unwrap();

        let (page, script) = runtime.block_on(async {
            // The page renders (200, HTML) even before the client round trip.
            let page = dispatch(&app, "/").await;
            // The server-mounted client script is served and names the manager.
            let script = dispatch(&app, "/51Degrees.core.js").await;
            (page, script)
        });

        assert_eq!(page.status, StatusCode::OK);
        assert!(
            page.body.contains("c-eg-page"),
            "the page uses the shared design system markup"
        );
        assert_eq!(script.status, StatusCode::OK);
        assert!(
            script.body.contains("fod"),
            "the client script names the 'fod' manager object, got: {}",
            script.body
        );
        // app and runtime drop here, on the plain test thread.
    }

    /// Usage sharing is on when the binary serves the page. Building sends
    /// nothing, because no request is processed.
    #[test]
    fn builds_with_usage_sharing_on() {
        let _app = build_app(
            "resource-key-placeholder",
            None,
            Some(CloudEngineState::default()),
            true,
        )
        .unwrap();
    }

    #[test]
    fn static_assets_are_served() {
        // The static assets are embedded in the binary, so they serve without any
        // detection happening. The app is built offline with a placeholder key and
        // an injected default state, so the build makes no cloud call.
        let runtime = runtime();
        let app = build_app(
            "resource-key-placeholder",
            None,
            Some(CloudEngineState::default()),
            false,
        )
        .unwrap();
        let css = runtime.block_on(async { dispatch(&app, ASSETS_CSS_ROUTE).await });
        assert_eq!(css.status, StatusCode::OK);
        assert_eq!(
            css.headers
                .get(axum::http::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("text/css")
        );
    }

    #[test]
    #[ignore = "requires a live cloud resource key and reachable cloud; the request engine now fetches discovery at build time"]
    fn accept_ch_is_emitted_for_a_client_hints_browser() {
        // End-to-end check of the cloud Accept-CH path. With a resource key, a
        // request from a Client-Hints-capable browser (Chrome) must come back
        // with an Accept-CH response header. The cloud engine learns its
        // SetHeader* properties from the discovery the request engine resolves as
        // it builds, the set-headers element turns them into Accept-CH and the
        // middleware applies it to the response. This needs a live key and a
        // reachable cloud, so it is ignored by default and run explicitly.
        let Some(resource_key) = examples_shared::resource_key_from_env() else {
            eprintln!("skipping accept_ch_is_emitted_for_a_client_hints_browser: no resource key");
            return;
        };

        let runtime = runtime();
        // None on the live path: the builder fetches discovery from the cloud.
        let app = build_app(
            &resource_key,
            examples_shared::cloud_endpoint_from_env().as_deref(),
            None,
            false,
        )
        .unwrap();
        let (status, has_accept_ch) = runtime.block_on(async {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri("/")
                        .header("host", "localhost")
                        .header(
                            "user-agent",
                            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                             (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
                        )
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let has_accept_ch = response
                .headers()
                .keys()
                .any(|name| name.as_str().eq_ignore_ascii_case("accept-ch"));
            (status, has_accept_ch)
        });

        assert_eq!(status, StatusCode::OK);
        assert!(
            has_accept_ch,
            "a Client-Hints browser request must receive an Accept-CH response header"
        );
        // app and runtime drop here, on the plain test thread.
    }
}

/*
 * @example dd-web-getting-started-cloud.rs
 *
 * Device Detection cloud web example (server-side plus client-side).
 *
 * This example demonstrates the 51Degrees Pipeline performing device detection
 * in the cloud inside a small axum web server. It shows three things working
 * together:
 *
 *   1. Server-side detection. The 51Degrees middleware (installed by
 *      `register`) runs the pipeline for the incoming request and stores the
 *      result. The home-page handler reads it through the `FiftyOneResult`
 *      extractor and renders a properties table.
 *
 *   2. The Accept-CH handshake. The set-headers element asks the browser for
 *      User-Agent Client Hints by adding an `Accept-CH` response header. The
 *      page shows the response headers and warns (a `c-eg-alert`) when no
 *      `Accept-CH` was emitted, which usually means the browser does not support
 *      client hints.
 *
 *   3. Client-side refinement. The page loads `/51Degrees.core.js` (mounted by
 *      `register`) which gathers high-entropy client evidence and posts it back
 *      to `/51dpipeline/json`. The shared `examples.min.js` helper subscribes to
 *      the resulting `complete` event and appends a refreshed results table into
 *      `#content`, so Apple models and screen dimensions become available.
 *
 * Usage sharing is on (`.share_usage(true)`), as web deployments are expected
 * to have it, so evidence from real requests improves detection. The console
 * examples in this crate turn it off.
 *
 * The page is written to the shared 51Degrees example design system: it
 * references the vendored `examples-main.min.css` and `examples.min.js`
 * (embedded in the binary and served from `/static/...`) and uses the `.c-eg-*`
 * class contract for the results, evidence and response-header tables and the
 * `c-eg-message` contact-us banner.
 *
 * Build a resource key at https://configure.51degrees.com?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-web-getting-started-cloud.rs&utm_term=dd-web-getting-started-cloud and export it as
 * `51DEGREES_RESOURCE_KEY`, then run the binary and open http://127.0.0.1:8080.
 */
