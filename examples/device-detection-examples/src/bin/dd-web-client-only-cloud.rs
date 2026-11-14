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

//! @page dd-web-client-only-cloud-example Client Only Cloud (Device Detection, Web)
//!
//! Device Detection web example: client-side-only cloud detection.
//!
//! There is no server-side detection and no 51Degrees pipeline on the server at
//! all. The page loads the 51Degrees cloud resource script directly in the
//! browser (`<cloud endpoint><resource-key>.js`, the endpoint including
//! `/api/v4/`), which performs detection client-side and raises the `complete`
//! event. The shared `examples.min.js` helper subscribes to it and renders the
//! results into the page.
//!
//! @snippet dd-web-client-only-cloud.rs example

use std::net::SocketAddr;

use anyhow::Context;
use axum::extract::State;
use axum::response::Html;
use axum::routing::get;
use axum::Router;

#[path = "../web_support/mod.rs"]
mod web_support;

use web_support::{serve_css, serve_js, ASSETS_CSS_ROUTE, ASSETS_JS_ROUTE};

/// The public 51Degrees cloud API base the resource script is loaded from when
/// `_51DEGREES_CLOUD_ENDPOINT` is unset. The resource script lives at
/// `<base><resource-key>.js`. Mirrors the cloud request engine default.
const DEFAULT_CLOUD_BASE: &str = "https://cloud.51degrees.com/api/v4/";

/// The cloud API base the resource script is loaded from, normalised to end in
/// one `/` as the cloud request engine does. This example has no server-side
/// pipeline, so the endpoint variable is honoured here rather than by the
/// engine. A host other than cloud.51degrees.com is an on premise web server
/// or a privately hosted 51Degrees cloud (see
/// `examples_shared::CLOUD_ENDPOINT_ENV_VAR`).
fn cloud_base(endpoint: Option<&str>) -> String {
    format!(
        "{}/",
        endpoint.unwrap_or(DEFAULT_CLOUD_BASE).trim_end_matches('/')
    )
}

/// Options that drive [`run`].
pub struct Options {
    /// The 51Degrees cloud resource key.
    pub resource_key: String,
    /// An optional override for the cloud endpoint, read from
    /// `_51DEGREES_CLOUD_ENDPOINT`. `None` selects the public cloud.
    pub endpoint: Option<String>,
    /// The socket address the server binds to.
    pub address: SocketAddr,
}

/// What the page handler needs, carried as the router state.
#[derive(Clone)]
struct PageState {
    resource_key: String,
    cloud_base: String,
}

/// Build the client-only application router.
///
/// The server only serves a static page and the two vendored assets; all
/// detection happens in the browser. The resource key is carried as router state
/// so the page handler can build the resource-script URL, together with the
/// cloud base it is loaded from.
pub fn build_app(resource_key: &str, endpoint: Option<&str>) -> Router {
    Router::new()
        .route("/", get(home))
        .route(ASSETS_CSS_ROUTE, get(serve_css))
        .route(ASSETS_JS_ROUTE, get(serve_js))
        .with_state(PageState {
            resource_key: resource_key.to_owned(),
            cloud_base: cloud_base(endpoint),
        })
}

// [example]
/// Serve the page over TCP until interrupted.
pub fn run(options: Options) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building the tokio runtime")?;
    runtime.block_on(async move {
        let app = build_app(&options.resource_key, options.endpoint.as_deref());
        let listener = tokio::net::TcpListener::bind(options.address)
            .await
            .with_context(|| format!("binding {}", options.address))?;
        let bound = listener.local_addr().context("reading the bound address")?;
        println!("Device Detection client-only cloud example listening on http://{bound}");
        println!("Open it in a browser to see detection run entirely client-side.");
        axum::serve(listener, app.into_make_service())
            .await
            .context("serving the application")
    })
}
// [example]

/// The home-page handler: a client-only page whose `#content` region is filled
/// by the cloud resource script and the shared callback.
async fn home(State(state): State<PageState>) -> Html<String> {
    // The cloud resource script URL: <base><resource-key>.js. The script raises
    // the `complete` event the shared examples.min.js helper binds to.
    let resource_url = format!(
        "{}{}.js",
        state.cloud_base,
        web_support::html_escape(&state.resource_key)
    );
    let client_script = format!(
        "<script async src=\"{resource_url}\" type=\"text/javascript\"></script>\
         <script src=\"{ASSETS_JS_ROUTE}\"></script>\
         <script>window.onload = function () {{ fodExamples.bindDeviceCallback({{ targetId: \"content\" }}); }};</script>"
    );

    Html(web_support::render_client_only_page(
        "Device Detection - Client-Side Only Example",
        "Detection runs entirely client-side. The 51Degrees JavaScript collects evidence in the \
         browser and fills the results below once it completes, with no server-side rendering.",
        web_support::CLOUD_CONTACT_BANNER,
        &client_script,
        examples_shared::CLOUD_REPOSITORIES,
    ))
}

fn main() -> anyhow::Result<()> {
    let resource_key = examples_shared::resource_key_from_env().context(
        "no 51Degrees cloud resource key found. Set _51DEGREES_RESOURCE_KEY to a key from \
         https://51degrees.com/configure?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-web-client-only-cloud.rs&utm_term=resource-key-required and run again.",
    )?;

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8082);
    let address = SocketAddr::from(([127, 0, 0, 1], port));

    run(Options {
        resource_key,
        // The cloud endpoint from _51DEGREES_CLOUD_ENDPOINT, or the public cloud
        // when unset (see cloud_base).
        endpoint: examples_shared::cloud_endpoint_from_env(),
        address,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use axum::response::Response;
    use tower::ServiceExt;

    async fn get(app: Router, uri: &str) -> Response {
        app.oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .header("host", "localhost")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
    }

    async fn body_string(response: Response) -> String {
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn page_renders_with_resource_script_and_assets() {
        // This example needs no live cloud call to render its page (the browser
        // makes the call), so a placeholder key exercises the page fully and the
        // test runs offline.
        let app = build_app("test-resource-key", None);
        let page = get(app, "/").await;
        assert_eq!(page.status(), StatusCode::OK);
        let body = body_string(page).await;
        assert!(
            body.contains("c-eg-page"),
            "the page uses the shared design system markup"
        );
        // The cloud resource script URL is embedded with the resource key.
        assert!(
            body.contains("cloud.51degrees.com/api/v4/test-resource-key.js"),
            "the page loads the cloud resource script for the key, got: {body}"
        );
        // The shared JavaScript helper is referenced for the client callback.
        assert!(
            body.contains(ASSETS_JS_ROUTE),
            "the page references the shared examples JavaScript"
        );
        assert!(
            body.contains(examples_shared::FIND_OUT_MORE_HEADING),
            "the page ends with the Find out more section"
        );

        // The static assets serve too.
        let css = get(build_app("test-resource-key", None), ASSETS_CSS_ROUTE).await;
        assert_eq!(css.status(), StatusCode::OK);
    }
}

/*
 * @example dd-web-client-only-cloud.rs
 *
 * Device Detection client-side-only cloud web example.
 *
 * This example demonstrates client-side-only device detection against the
 * 51Degrees cloud. Unlike the Getting Started web examples there is no
 * server-side pipeline and no server-side detection. The server only serves a
 * static HTML page and the two vendored assets.
 *
 * The page loads the 51Degrees cloud resource script directly in the browser,
 * from `https://cloud.51degrees.com/api/v4/<resource-key>.js`, or from the host
 * named by `_51DEGREES_CLOUD_ENDPOINT` (an on premise web server or a privately
 * hosted 51Degrees cloud, see `examples_shared::CLOUD_ENDPOINT_ENV_VAR`). That
 * script gathers evidence client-side, calls the cloud, and raises the
 * `complete` event. The shared `examples.min.js` helper subscribes to the event and renders
 * the detection results into the page's `#content` region. The resource key is
 * therefore visible to the client, which is expected for this deployment style.
 *
 * The page is written to the shared 51Degrees example design system (the
 * `.c-eg-*` classes) and shows the cloud `c-eg-message` contact-us banner.
 *
 * Set `_51DEGREES_RESOURCE_KEY` to a key from https://51degrees.com/configure?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-web-client-only-cloud.rs&utm_term=dd-web-client-only-cloud,
 * run the binary and open http://127.0.0.1:8082. The page renders without a key;
 * the live detection in the browser needs a real one.
 */
