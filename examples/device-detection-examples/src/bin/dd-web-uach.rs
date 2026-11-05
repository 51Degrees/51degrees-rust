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

//! @page dd-web-uach-example UA Client Hints (Device Detection, Web)
//!
//! Device Detection web example: User-Agent Client Hints (UACH) flow.
//!
//! A focused, on-premise web example that drives the UA-CH high-entropy decoder
//! ([`UachJsConversionElement`]). The browser's `getHighEntropyValues()` blob,
//! delivered as a query or cookie parameter on the client callback, is decoded
//! into the `sec-ch-ua*` HTTP headers the Hash engine understands. The page
//! shows the evidence the pipeline received (including any decoded client hints)
//! and refreshes client-side after the high-entropy round trip.
//!
//! @snippet dd-web-uach.rs example

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

/// Build the UACH web application router.
///
/// The pipeline is on-premise so the UA-CH high-entropy decoder
/// ([`UachJsConversionElement`]) is wired explicitly ahead of the Hash engine.
/// The decoder turns a `getHighEntropyValues` blob into the `sec-ch-ua*` headers
/// the engine consumes, which is the heart of this example.
///
/// `share_usage` is true when the binary serves the page. The tests pass false,
/// so a test run sends nothing to 51Degrees.
pub fn build_app(data_file: &std::path::Path, share_usage: bool) -> anyhow::Result<Router> {
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(data_file.to_path_buf())
        .performance_profile(PerformanceProfile::HighPerformance)
        .build()
        .with_context(|| format!("loading the Hash data file at {}", data_file.display()))?;

    // The element order: usage sharing (required for a web example), the UA-CH
    // high-entropy decoder, then the Hash engine. The decoder is the element
    // this example exists to show: it sits before the engine so the decoded
    // client hints are present when detection runs.
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
    let app = build_app(&options.data_file, true)?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building the tokio runtime")?;
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(options.address)
            .await
            .with_context(|| format!("binding {}", options.address))?;
        let bound = listener.local_addr().context("reading the bound address")?;
        println!("Device Detection UACH web example listening on http://{bound}");
        println!(
            "Open it in a Chromium-based browser to see the high-entropy client-hints round trip."
        );
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .context("serving the application")
    })
}
// [example]

/// The home-page handler: show the evidence the pipeline received (the decoded
/// `sec-ch-ua*` client hints become visible here after the client round trip),
/// the detection results and the `Accept-CH` request headers.
async fn home(result: FiftyOneResult) -> Html<String> {
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

    let accept_ch_alert = if has_accept_ch {
        ""
    } else {
        web_support::MISSING_ACCEPT_CH_ALERT
    };

    Html(web_support::render_page(PageOptions {
        title: "Device Detection - User-Agent Client Hints (UACH)",
        lead: "This example drives the User-Agent Client Hints flow. The server asks the browser \
               for high-entropy client hints through the Accept-CH response header. The browser's \
               getHighEntropyValues() blob is posted back on the client callback and decoded into \
               sec-ch-ua* headers, which the on-premise Hash engine then uses. Watch the evidence \
               table below grow with the decoded client hints after the callback.",
        top_alert: "",
        results_html: &results_html,
        evidence_html: &evidence_html,
        headers_html: &headers_html,
        accept_ch_alert,
        message_html: web_support::ONPREM_CONTACT_BANNER,
        client_script: web_support::SERVER_CLIENT_SCRIPT,
    }))
}

fn main() -> anyhow::Result<()> {
    let data_file = examples_shared::dd_data_path().context(
        "no on-premise Hash data file found. Set 51DEGREES_DD_PATH to a .hash data file.",
    )?;

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8083);
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

    fn test_app() -> Option<Router> {
        let data_file = examples_shared::dd_data_path()?;
        Some(build_app(&data_file, false).expect("UACH app builds"))
    }

    /// Dispatch a request carrying a full UACH header set, so the example
    /// exercises the client-hints evidence path on the server side too.
    ///
    /// Driven from a `#[test]` (not `#[tokio::test]`): with usage sharing on, the
    /// app holds a blocking reqwest client whose drop must stay off the tokio
    /// worker threads, so the app is owned and dropped on the plain test thread.
    async fn dispatch(app: &Router, uri: &str) -> Captured {
        let mut builder = Request::builder()
            .method("GET")
            .uri(uri)
            .header("host", "localhost")
            .header(
                "user-agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                 (KHTML, like Gecko) Chrome/98.0.4758.102 Safari/537.36",
            );
        // Add the sample UACH headers so the decoded client hints appear as
        // evidence the pipeline accepts. The evidence keys are prefixed
        // `header.`; strip that for the real HTTP header name.
        for (key, value) in examples_shared::evidence::uach_header_evidence() {
            let header_name = key.strip_prefix("header.").unwrap_or(key);
            builder = builder.header(header_name, value);
        }
        let response = app
            .clone()
            .oneshot(builder.body(Body::empty()).unwrap())
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
        let Some(app) = test_app() else {
            eprintln!(
                "skipping server_starts_and_serves_page_and_client_script: no Hash data file \
                 (set 51DEGREES_DD_PATH to enable this test)"
            );
            return;
        };

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
        // A UACH header sent on the request appears in the evidence table.
        assert!(
            page.body.contains("sec-ch-ua"),
            "the supplied client-hint evidence is shown, got: {}",
            page.body
        );

        assert_eq!(script.status, StatusCode::OK);
        assert!(
            script.body.contains("fod"),
            "the client script names the 'fod' manager object"
        );
        // app and runtime drop here, on the plain test thread.
    }
}

/*
 * @example dd-web-uach.rs
 *
 * Device Detection User-Agent Client Hints (UACH) web example.
 *
 * This example focuses on the User-Agent Client Hints flow, the modern
 * replacement for parsing the User-Agent string. It is an on-premise web server
 * (so the UA-CH high-entropy decoder, `UachJsConversionElement`, is wired
 * explicitly ahead of the Hash engine) and shows the full handshake:
 *
 *   1. The server asks the browser for high-entropy client hints by emitting an
 *      `Accept-CH` response header (the set-headers element). A `c-eg-alert`
 *      warns when no `Accept-CH` was produced, which usually means the browser
 *      does not support client hints.
 *
 *   2. On the client callback the browser's `getHighEntropyValues()` result is
 *      posted back (as a query or cookie parameter). The high-entropy decoder
 *      turns that blob into the `sec-ch-ua*` HTTP headers the engine consumes,
 *      so detection can use precise platform, browser and (where available)
 *      model information.
 *
 *   3. The decoded client hints appear in the evidence table, and the refreshed
 *      results table is appended client-side by the shared `examples.min.js`.
 *
 * The page is written to the shared 51Degrees example design system. Usage
 * sharing is enabled, as a web deployment should. Because the default data file
 * is the free Lite tier, the on-premise paid-data contact-us banner is shown.
 *
 * Resolve the data file with `51DEGREES_DD_PATH` (or the Lite file in the data
 * repository inside the `cxx` submodule, which is downloaded when it is
 * missing), run the binary and open the page in a
 * Chromium-based browser (which supports client hints) at http://127.0.0.1:8083.
 */
