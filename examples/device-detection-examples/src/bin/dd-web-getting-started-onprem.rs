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

//! @page dd-web-getting-started-onprem-example Getting Started On-premise (Device Detection, Web)
//!
//! Device Detection web example: on-premise Getting Started (server plus client).
//!
//! It builds an on-premise Hash device-detection pipeline, wraps it in a
//! [`WebPipeline`] so the two client-side endpoints are mounted, and serves an
//! HTML page that shows
//! server-side detection then refreshes it client-side. Because the default data
//! file is the free Lite tier, the page shows the standard Lite/stale warnings
//! and a contact-us banner that lists the benefits of a paid data file. Each
//! Enterprise property the data file does not carry says it needs the
//! Enterprise data file, with a link to ask about a copy.
//!
//! @snippet dd-web-getting-started-onprem.rs example

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use axum::response::Html;
use axum::routing::get;
use axum::Router;
use fiftyone_device_detection::{
    DeviceDetectionOnPremiseEngineBuilder, PerformanceProfile, UachJsConversionElement,
    DEVICE_DATA_KEY,
};
use fiftyone_pipeline_core::FlowElement;
use fiftyone_pipeline_engines_fiftyone::ShareUsageElement;
use fiftyone_pipeline_web::{response_headers, WebIntegrationOptions, WebPipeline};
use fiftyone_pipeline_web_axum::{register, FiftyOneResult, FiftyOneState};

#[path = "../web_support/mod.rs"]
mod web_support;

use web_support::{
    detection_results_table, evidence_table, response_header_table, serve_css, serve_js,
    PageOptions, ASSETS_CSS_ROUTE, ASSETS_JS_ROUTE,
};

/// Options that drive [`run`].
pub struct Options {
    /// The path to the on-premise Hash data file.
    pub data_file: PathBuf,
    /// The socket address the server binds to.
    pub address: SocketAddr,
}

/// Build the on-premise web application router plus the data-file warnings.
///
/// The Hash engine is built once so its data-file warnings (Lite tier, and over
/// 28/30 days old) can be read with [`examples_shared::check_data_file`] and
/// shown on the page, and the very same engine instance is then used as the
/// application element of the web pipeline. The engine's properties, which are
/// those its data file carries, go to the page handler so it can say which
/// Enterprise properties the data file lacks.
///
/// `share_usage` is true when the binary serves the page. The tests pass false,
/// so a test run sends nothing to 51Degrees.
pub fn build_app(
    data_file: &std::path::Path,
    share_usage: bool,
) -> anyhow::Result<(Router, Vec<String>)> {
    // Build the Hash engine. The HighPerformance profile suits a long-running
    // web server. Every property the data file supports is loaded by default.
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(data_file.to_path_buf())
        .performance_profile(PerformanceProfile::HighPerformance)
        .build()
        .with_context(|| format!("loading the Hash data file at {}", data_file.display()))?;

    // Read the standard Lite-tier and stale-file warnings for display, before
    // the engine is moved into the pipeline element list. `engine` is an
    // `Arc<DeviceDetectionOnPremiseEngine>`, which derefs to the trait object.
    let warnings = examples_shared::check_data_file(engine.as_ref());
    let available = Arc::new(engine.available_properties());

    // Assemble the application's own elements in the order the on-premise facade
    // uses: usage sharing first (REQUIRED for a web example), then the UA-CH
    // high-entropy decoder, then the Hash engine. WebPipeline wraps these with
    // the sequence, set-headers, JSON and JavaScript elements.
    let mut elements: Vec<Arc<dyn FlowElement>> =
        vec![Arc::new(UachJsConversionElement::new()), engine];
    if share_usage {
        elements.insert(0, Arc::new(ShareUsageElement::with_defaults()));
    }
    let web = WebPipeline::build(elements, WebIntegrationOptions::default())
        .context("assembling the web pipeline")?;
    let state = FiftyOneState::from_web_pipeline(&web);

    let app = register(
        Router::new()
            .route(
                "/",
                get(move |result: FiftyOneResult| home(result, Arc::clone(&available))),
            )
            .route(ASSETS_CSS_ROUTE, get(serve_css))
            .route(ASSETS_JS_ROUTE, get(serve_js)),
        state,
    );
    Ok((app, warnings))
}

// [example]
/// Serve the example page over TCP until interrupted.
pub fn run(options: Options) -> anyhow::Result<()> {
    // Build the pipeline (and so the share-usage element's blocking reqwest
    // client) on this plain thread, before the async runtime is entered. The
    // blocking client builds and then drops a short-lived runtime as it is
    // constructed, and tokio forbids dropping a runtime from inside another
    // runtime's async context, so doing this work outside block_on avoids that
    // panic. The warnings are baked into each page render, so capture them here
    // and store them in a process-wide slot the handler reads per request.
    let (app, warnings) = build_app(&options.data_file, true)?;
    store_warnings(warnings);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building the tokio runtime")?;
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(options.address)
            .await
            .with_context(|| format!("binding {}", options.address))?;
        let bound = listener.local_addr().context("reading the bound address")?;
        println!("Device Detection on-premise web example listening on http://{bound}");
        for warning in current_warnings() {
            println!("WARNING: {warning}");
        }
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .context("serving the application")
    })
}
// [example]

// The data-file warnings are computed once at build time and read on every page
// render. A web handler cannot easily carry extra state alongside FiftyOneState
// without threading a custom router state, so for this example they are kept in
// a process-wide slot. A real application would store them in its own state.
static WARNINGS: std::sync::OnceLock<std::sync::Mutex<Vec<String>>> = std::sync::OnceLock::new();

/// Record the warnings for the running server to show on each page.
fn store_warnings(warnings: Vec<String>) {
    let slot = WARNINGS.get_or_init(|| std::sync::Mutex::new(Vec::new()));
    *slot.lock().expect("warnings lock") = warnings;
}

/// The warnings recorded by [`store_warnings`], or an empty list if none.
fn current_warnings() -> Vec<String> {
    WARNINGS
        .get()
        .map(|slot| slot.lock().expect("warnings lock").clone())
        .unwrap_or_default()
}

/// The home-page handler: render the server-side detection results, the
/// evidence and response-header tables, and the Lite/stale data-file warnings.
/// `available` holds the properties the data file carries.
async fn home(result: FiftyOneResult, available: Arc<Vec<String>>) -> Html<String> {
    let (results_html, evidence_html, headers_html, has_accept_ch) = result.with(|flow_data| {
        let results = flow_data
            .get(DEVICE_DATA_KEY)
            .map(|device| detection_results_table(device, Some(available.as_slice())))
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

    // Render the data-file warnings as a single top-of-page c-eg-alert.
    let warnings = current_warnings();
    let top_alert = if warnings.is_empty() {
        String::new()
    } else {
        let body = warnings
            .iter()
            .map(|w| web_support::html_escape(w))
            .collect::<Vec<_>>()
            .join("<br>");
        format!("<div class=\"c-eg-alert\">{body}</div>")
    };

    let accept_ch_alert = if has_accept_ch {
        ""
    } else {
        web_support::MISSING_ACCEPT_CH_ALERT
    };

    Html(web_support::render_page(PageOptions {
        title: "Device Detection - On-Premise Web Example",
        lead: "This example performs device detection on premise (from a local Hash data file) \
               within a small axum web server. The first response is rendered server-side. The \
               51Degrees JavaScript then posts back high-entropy client evidence and refreshes \
               the results below.",
        top_alert: &top_alert,
        results_html: &results_html,
        evidence_html: &evidence_html,
        headers_html: &headers_html,
        accept_ch_alert,
        // The on-premise Lite page lists the benefits of a paid data file.
        message_html: web_support::ONPREM_CONTACT_BANNER,
        client_script: web_support::SERVER_CLIENT_SCRIPT,
        repositories: examples_shared::ON_PREMISE_REPOSITORIES,
    }))
}

fn main() -> anyhow::Result<()> {
    // Resolve the on-premise data file: an explicit _51DEGREES_DD_PATH first, then
    // the Lite Hash file in the data repository inside the cxx submodule, which
    // is downloaded when it is missing.
    let data_file = examples_shared::dd_data_path().context(
        "no on-premise Hash data file found. Set _51DEGREES_DD_PATH to a .hash data file.",
    )?;

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8081);
    let address = SocketAddr::from(([127, 0, 0, 1], port));

    run(Options { data_file, address })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    /// The pieces of a response a test inspects.
    struct Captured {
        status: StatusCode,
        body: String,
    }

    /// Build the app for the test, skipping when no data file is available.
    fn test_app() -> Option<Router> {
        let data_file = examples_shared::dd_data_path()?;
        let (app, _warnings) = build_app(&data_file, false).expect("on-premise app builds");
        Some(app)
    }

    /// Dispatch one request and capture the response. Driven from a `#[test]`
    /// (not `#[tokio::test]`): with usage sharing on, the app holds a blocking
    /// reqwest client whose drop must not happen on a tokio worker thread, so the
    /// app is owned and dropped on the plain test thread around a
    /// `Runtime::block_on`.
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
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        Captured {
            status,
            body: String::from_utf8(bytes.to_vec()).unwrap(),
        }
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn server_starts_and_serves_page_and_client_script() {
        // The on-premise pipeline needs a local data file. Skip (not fail) when
        // it is absent so a plain `cargo test` is green without the submodule.
        let Some(app) = test_app() else {
            eprintln!(
                "skipping server_starts_and_serves_page_and_client_script: no Hash data file \
                 (set _51DEGREES_DD_PATH to enable this test)"
            );
            return;
        };
        let lite = examples_shared::dd_data_path()
            .is_some_and(|file| file.ends_with(examples_shared::DD_LITE_RELATIVE_PATH));

        let runtime = runtime();
        let (page, script) = runtime.block_on(async {
            let page = dispatch(&app, "/").await;
            let script = dispatch(&app, "/51Degrees.core.js").await;
            (page, script)
        });

        assert_eq!(page.status, StatusCode::OK);
        assert!(
            page.body.contains("c-eg-page"),
            "the page uses the shared design system markup"
        );
        // A Lite data file always emits the Lite-tier warning as a c-eg-alert.
        assert!(
            page.body.contains("c-eg-alert"),
            "the on-premise Lite page shows a data-file alert"
        );
        assert!(
            page.body.contains(examples_shared::FIND_OUT_MORE_HEADING),
            "the page ends with the Find out more section"
        );
        // The Lite file carries none of the Enterprise properties, so each says
        // so with the contact link. _51DEGREES_DD_PATH can name another file.
        if lite {
            assert!(
                page.body
                    .contains(examples_shared::NEEDS_ENTERPRISE_DATA_FILE)
                    && page.body.contains("utm_term=enterprise-data-file"),
                "the Lite page says which properties need the Enterprise data file"
            );
        }

        assert_eq!(script.status, StatusCode::OK);
        assert!(
            script.body.contains("fod"),
            "the client script names the 'fod' manager object"
        );
        // app and runtime drop here, on the plain test thread.
    }
}

/*
 * @example dd-web-getting-started-onprem.rs
 *
 * Device Detection on-premise web example (server-side plus client-side).
 *
 * This example demonstrates the 51Degrees Pipeline performing device detection
 * from a local Hash data file inside a small axum web server. It is the
 * on-premise twin of `dd-web-getting-started-cloud.rs` and works the same way:
 *
 *   1. Server-side detection by the 51Degrees middleware, read back through the
 *      `FiftyOneResult` extractor and rendered as a properties table.
 *
 *   2. The Accept-CH handshake driven by the set-headers element, with a
 *      `c-eg-alert` warning when the browser sends no client hints.
 *
 *   3. Client-side refinement via `/51Degrees.core.js` and `/51dpipeline/json`,
 *      with the shared `examples.min.js` appending a refreshed results table.
 *
 * Because the default data file is the free Lite tier, the page shows the
 * standard data-file warnings (from `examples_shared::check_data_file`): a
 * Lite-tier notice, and a stale-file notice when the file is more than 30 days
 * old. These are rendered as a `c-eg-alert` at the top of the page. The
 * `c-eg-message` contact-us banner lists what a paid data file adds (more device
 * models, hardware vendor and model, Apple-model resolution and so on).
 *
 * The results table also shows properties only the Enterprise data file
 * carries (HardwareVendor, HardwareName, HardwareModel, DeviceType, PriceBand
 * and ScreenInchesDiagonal). The engine's properties are those its data file
 * carries, so with the Lite file each of these says it needs the Enterprise
 * data file, with a link to contact 51Degrees about a copy. The page ends with
 * a "Find out more" section.
 *
 * Usage sharing is enabled, as a web deployment should. The Hash engine is built
 * once: its data-file warnings are read before it is handed to the web pipeline
 * as the application element, so no data file is opened twice.
 *
 * Resolve the data file with `_51DEGREES_DD_PATH` (or rely on the Lite file in
 * the data repository inside the `cxx` submodule, which is downloaded when it
 * is missing), then run the binary and open http://127.0.0.1:8081.
 */
