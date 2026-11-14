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

//! @page ipi-web-getting-started-cloud-example Getting Started Cloud (IP Intelligence, Web)
//!
//! IP Intelligence web example: cloud Getting Started (server side).
//!
//! It builds a cloud IP-intelligence pipeline (with usage sharing, as a web
//! deployment should), wraps it in a [`WebPipeline`] so the two client-side
//! endpoints (`/51Degrees.core.js` and `/51dpipeline/json`) are mounted, and
//! serves an HTML page that shows the weighted IP Intelligence (country,
//! location, network) for the connecting client's IP address, with a form to
//! look up any other address.
//!
//! @snippet ipi-web-getting-started-cloud.rs example

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use axum::extract::Query;
use axum::response::Html;
use axum::routing::get;
use axum::Router;
use fiftyone_ip_intelligence::{
    CloudEngineState, CloudRequestEngine, IpIntelligenceCloudEngine, IP_DATA_KEY,
};
use fiftyone_pipeline_core::FlowElement;
use fiftyone_pipeline_engines_fiftyone::ShareUsageElement;
use fiftyone_pipeline_web::{WebIntegrationOptions, WebPipeline};
use fiftyone_pipeline_web_axum::{register, FiftyOneResult, FiftyOneState};

// The shared example HTML helpers (design-system markup, the vendored assets and
// the static-asset handlers) live in a sibling module so every IP Intelligence
// web bin renders to the same `.c-eg-*` contract without duplicating the markup.
#[path = "../web_support/mod.rs"]
mod web_support;

use web_support::{
    coordinates, display_values, evidence_table, map_init_script, results_table, serve_css,
    serve_js, PageOptions, ASSETS_CSS_ROUTE, ASSETS_JS_ROUTE,
};

/// The query-string parameter the IP form submits under. The cloud service
/// accepts the 51Degrees client-IP query key, so the form lands directly on an
/// evidence key the cloud pipeline uses.
const FORM_FIELD: &str = "client-ip-51d";

/// Options that drive [`run`]. The resource key is required for the cloud
/// deployment; the bind address lets the test pick an ephemeral port while the
/// binary uses a fixed one.
pub struct Options {
    /// The 51Degrees cloud resource key (from <https://51degrees.com/configure?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-web-getting-started-cloud.rs&utm_term=resource_key>).
    pub resource_key: String,
    /// An optional override for the cloud endpoint. `None` uses the public one.
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
/// The request engine resolves its discovery (the accepted evidence keys and the
/// accessible properties) when it builds. Passing `Some(state)` supplies that
/// discovery up front so the build does no cloud call, which the integration test
/// uses to assemble the app offline. Passing `None` lets the builder fetch the
/// discovery from the cloud, which is the live path the binary takes.
///
/// `share_usage` is true when the binary serves the page. The tests pass false,
/// so a test run sends nothing to 51Degrees.
pub fn build_app(
    resource_key: &str,
    endpoint: Option<&str>,
    state: Option<CloudEngineState>,
    share_usage: bool,
) -> anyhow::Result<Router> {
    // Assemble the cloud IP-intelligence elements by hand rather than through the
    // facade's `build()`, because a web deployment must add the ShareUsageElement
    // (REQUIRED for web examples, forbidden for console examples) and the facade
    // returns a finished pipeline. The order matches the facade: share usage,
    // then the cloud request engine, then the IP-intelligence cloud engine.
    let mut request_builder = CloudRequestEngine::builder().resource_key(resource_key);
    if let Some(endpoint) = endpoint {
        request_builder = request_builder.endpoint(endpoint);
    }
    let request_engine = Arc::new(
        request_builder
            .set_state_opt(state)
            .build()
            .context("building the cloud request engine")?,
    );
    let ipi_engine = IpIntelligenceCloudEngine::builder()
        .cloud_request_engine(Arc::clone(&request_engine))
        .build()
        .context("building the IP-intelligence cloud engine")?;

    let mut elements: Vec<Arc<dyn FlowElement>> = vec![request_engine, Arc::new(ipi_engine)];
    if share_usage {
        elements.insert(0, Arc::new(ShareUsageElement::with_defaults()));
    }

    // WebPipeline wraps these application elements with the sequence, set-headers,
    // JSON and JavaScript elements the web integration needs.
    let web = WebPipeline::build(elements, WebIntegrationOptions::default())
        .context("assembling the web pipeline")?;
    let state = FiftyOneState::from_web_pipeline(&web);

    // register mounts GET /51Degrees.core.js and POST /51dpipeline/json plus the
    // per-request middleware. The home page and the two static asset routes are
    // added on top; the middleware runs the lookup for every request (using the
    // client IP, or the form's IP when supplied) and stores the result for the
    // FiftyOneResult extractor the page handler reads.
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
        println!("IP Intelligence cloud web example listening on http://{bound}");
        println!(
            "Open it in a browser to see IP Intelligence for your address, or look up any IP."
        );
        // into_make_service_with_connect_info records the peer address so the
        // pipeline can use it as client-IP evidence when no form IP or forwarding
        // header is present.
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
/// request (looking up the client IP, or the form-supplied IP), so the result is
/// read from the [`FiftyOneResult`] extractor and rendered into the shared page.
///
/// `Query` exposes the submitted IP so it can be pre-filled back into the form
/// and shown as the address the result describes.
async fn home(
    Query(params): Query<std::collections::HashMap<String, String>>,
    result: FiftyOneResult,
) -> Html<String> {
    // Read every displayed property, the coordinates and the evidence while the
    // flow data is briefly locked, then build the HTML outside the lock.
    let (results_html, coordinates_pair, evidence_html, evidence_ip) = result.with(|flow_data| {
        let (results, coords) = flow_data
            .get(IP_DATA_KEY)
            .map(|ip| {
                let values = display_values(ip);
                (results_table(&values), coordinates(ip))
            })
            .unwrap_or_else(|| {
                (
                    "<p class=\"c-eg-page__lead\">No IP Intelligence data was produced.</p>"
                        .to_owned(),
                    None,
                )
            });

        let evidence_pairs: Vec<(String, String)> = flow_data
            .evidence()
            .iter()
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect();

        // The IP the result describes is the client-IP evidence the pipeline saw.
        let ip = evidence_pairs
            .iter()
            .find(|(key, _)| key.contains("client-ip"))
            .map(|(_, value)| value.clone())
            .unwrap_or_default();

        (results, coords, evidence_table(&evidence_pairs), ip)
    });

    // The address shown is the one the visitor typed (if any), else the evidence
    // IP the pipeline used.
    let form_value = params.get(FORM_FIELD).cloned().unwrap_or_default();
    let client_ip = if form_value.is_empty() {
        evidence_ip
    } else {
        form_value.clone()
    };

    let map_script = map_init_script(&coordinates_pair);

    Html(web_support::render_page(PageOptions {
        title: "IP Intelligence - Cloud Web Example",
        lead: "This example performs IP Intelligence in the cloud within a small axum web server. \
               It shows the network and location properties for the connecting client's \
               IP address. Enter any IP address below to look it up.",
        top_alert: "",
        client_ip: &client_ip,
        results_html: &results_html,
        evidence_html: &evidence_html,
        form_field: FORM_FIELD,
        form_value: &form_value,
        // For the cloud deployment the contact-us banner invites on-premise.
        message_html: web_support::CLOUD_CONTACT_BANNER,
        map_script: &map_script,
        repositories: examples_shared::CLOUD_REPOSITORIES,
    }))
}

fn main() -> anyhow::Result<()> {
    // The resource key is read from `_51DEGREES_RESOURCE_KEY` or the CI names.
    let resource_key = examples_shared::resource_key_from_env().context(
        "no 51Degrees cloud resource key found. Set _51DEGREES_RESOURCE_KEY to a key from \
         https://51degrees.com/configure?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-web-getting-started-cloud.rs&utm_term=resource-key-required and run again.",
    )?;

    // A fixed local port for the runnable binary; override with PORT if taken.
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8082);
    let address = SocketAddr::from(([127, 0, 0, 1], port));

    run(Options {
        resource_key,
        // The cloud endpoint from _51DEGREES_CLOUD_ENDPOINT, or the public cloud
        // when unset. A host other than cloud.51degrees.com is an on premise web
        // server or a privately hosted 51Degrees cloud (see
        // examples_shared::CLOUD_ENDPOINT_ENV_VAR).
        endpoint: examples_shared::cloud_endpoint_from_env(),
        address,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::{header, HeaderValue, Request, StatusCode};
    use axum::response::Response;
    use tower::ServiceExt;

    /// Drive one GET request through a clone of the app and return the response.
    ///
    /// A clone is used (cloning a `Router` is cheap, sharing the same `Arc`
    /// elements) so the owning `app` the caller holds outlives the request. That
    /// matters because the cloud engine, and the usage-sharing element when it is
    /// on, wrap a blocking `reqwest` client whose backing runtime must not be
    /// dropped from inside an async context. Keeping the owning `app` in the synchronous test
    /// body ensures the final drop happens outside the runtime.
    async fn get(app: Router, uri: &str) -> Response {
        app.oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .header("host", "localhost")
                .header("x-forwarded-for", "185.28.167.77")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
    }

    /// Read a response body to an owned UTF-8 string.
    async fn body_string(response: Response) -> String {
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    /// True when a CSS response carries the expected `text/css` content type.
    fn is_css(response: &Response) -> bool {
        response.headers().get(header::CONTENT_TYPE) == Some(&HeaderValue::from_static("text/css"))
    }

    #[test]
    fn server_starts_and_serves_page_and_client_script() {
        // This test only checks that the server serves its page, the client
        // script and the CSS asset, none of which need real cloud data. So the
        // app is built offline: a placeholder key plus an injected default state
        // means the request engine resolves its discovery from that state and
        // makes no cloud call as it builds. The per-request data POST then fails
        // against the placeholder key, but the web integration suppresses that by
        // default, so the page still renders with its static markup.
        let app = build_app(
            "resource-key-placeholder",
            None,
            Some(CloudEngineState::default()),
            false,
        )
        .expect("cloud app builds offline from an injected state");

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("test runtime builds");

        runtime.block_on(async {
            // The page renders (200, HTML) with the shared design-system markup.
            let page = get(app.clone(), "/").await;
            assert_eq!(page.status(), StatusCode::OK);
            let page_body = body_string(page).await;
            assert!(
                page_body.contains("c-eg-page"),
                "the page uses the shared design system markup"
            );
            assert!(
                page_body.contains("c-eg-form"),
                "the page includes the IP look-up form"
            );
            assert!(
                page_body.contains(examples_shared::FIND_OUT_MORE_HEADING),
                "the page ends with the Find out more section"
            );

            // The server-mounted client script is served and names the fod manager.
            let script = get(app.clone(), "/51Degrees.core.js").await;
            assert_eq!(script.status(), StatusCode::OK);
            let script_body = body_string(script).await;
            assert!(
                script_body.contains("fod"),
                "the client script names the 'fod' manager object, got: {script_body}"
            );

            // The static CSS asset is embedded, so it serves regardless of the key.
            let css = get(app.clone(), ASSETS_CSS_ROUTE).await;
            assert_eq!(css.status(), StatusCode::OK);
            assert!(
                is_css(&css),
                "the CSS asset carries a text/css content type"
            );
        });

        // `app` (and its blocking clients) is dropped here, in the synchronous
        // test body, after the runtime has finished its work.
        drop(app);
    }
}

/*
 * @example ipi-web-getting-started-cloud.rs
 *
 * IP Intelligence cloud web example (server-side).
 *
 * This example demonstrates the 51Degrees Pipeline performing IP Intelligence in
 * the cloud inside a small axum web server. It shows:
 *
 *   1. Server-side IP Intelligence. The 51Degrees middleware (installed by
 *      `register`) runs the cloud pipeline for the incoming request, using the
 *      client's IP address (taken from `X-Forwarded-For` when behind a proxy,
 *      otherwise the connection peer) as evidence. The home-page handler reads
 *      the result through the `FiftyOneResult` extractor and renders the weighted
 *      network and location properties.
 *
 *   2. The probabilistic nature of IP Intelligence. Each property can return
 *      several candidate values, ordered high weighting first. The results table
 *      shows the most probable value with its weighting (a 0.0 to 1.0
 *      confidence).
 *
 *   3. An IP look-up form. A visitor can enter any IPv4 or IPv6 address; the form
 *      submits it as the `client-ip-51d` query parameter, which the cloud service
 *      accepts as the client-IP evidence, so the page re-renders for that address.
 *
 *   4. An approximate-location map. When the lookup resolves a latitude and
 *      longitude, the shared `examples.min.js` helper (`initLocationMap`) draws
 *      the point on a Leaflet map.
 *
 * Usage sharing is enabled (`ShareUsageElement`), which web deployments are
 * expected to do so anonymous evidence improves the data for everyone. The
 * console examples in this crate deliberately leave it off. The cloud elements
 * are assembled by hand (rather than through the facade `build()`) precisely so
 * the share-usage element can be added.
 *
 * The page is written to the shared 51Degrees example design system: it
 * references the vendored `examples-main.min.css` and `examples.min.js` (embedded
 * in the binary and served from `/static/...`) and uses the `.c-eg-*` class
 * contract for the results table, the evidence table, the IP form, the location
 * map and the `c-eg-message` contact-us banner. The page ends with a "Find out
 * more" section.
 *
 * Build a resource key at https://51degrees.com/configure?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-web-getting-started-cloud.rs&utm_term=ipi-web-getting-started-cloud and export it as
 * `_51DEGREES_RESOURCE_KEY`, then run the binary and open http://127.0.0.1:8082.
 */
