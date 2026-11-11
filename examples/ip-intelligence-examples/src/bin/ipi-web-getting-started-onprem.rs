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

//! @page ipi-web-getting-started-onprem-example Getting Started On-premise (IP Intelligence, Web)
//!
//! IP Intelligence web example: on-premise Getting Started (server side).
//!
//! It builds an on-premise IP-intelligence pipeline from a local `.ipi` data file
//! (with usage sharing, as a web deployment should), wraps it in a
//! [`WebPipeline`] so the two client-side endpoints are mounted, and serves an
//! HTML page showing the weighted IP Intelligence for the connecting client's IP
//! address, with a form to look up any other address. Because the default data
//! file is a free tier, the page shows the standard data-file warnings as a
//! `c-eg-alert`, says which properties need the Enterprise data file with a
//! link to ask about a copy, and shows a contact-us banner describing a paid
//! Enterprise file and a "Find out more" section.
//!
//! @snippet ipi-web-getting-started-onprem.rs example

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use axum::extract::Query;
use axum::response::Html;
use axum::routing::get;
use axum::Router;
use fiftyone_ip_intelligence::{IpIntelligenceOnPremiseEngine, PerformanceProfile, IP_DATA_KEY};
use fiftyone_pipeline_core::FlowElement;
use fiftyone_pipeline_engines_fiftyone::ShareUsageElement;
use fiftyone_pipeline_web::{WebIntegrationOptions, WebPipeline};
use fiftyone_pipeline_web_axum::{register, FiftyOneResult, FiftyOneState};

#[path = "../web_support/mod.rs"]
mod web_support;

use ip_intelligence_examples::ENTERPRISE_PROPERTIES;
use web_support::{
    coordinates, display_values, enterprise_message_html, evidence_table, map_init_script,
    mark_enterprise_only, results_table, serve_css, serve_js, warnings_alert, PageOptions,
    ASSETS_CSS_ROUTE, ASSETS_JS_ROUTE,
};

/// Where to ask about a copy of the Enterprise data file, as the console
/// output links it when the server starts.
const CONTACT_US_URL: &str = "https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-web-getting-started-onprem.rs&utm_term=enterprise-data-file";

/// The query-string parameter the IP form submits under. The on-premise engine
/// accepts the `query.client-ip` evidence key, so the form lands directly on an
/// evidence key the on-premise pipeline uses.
const FORM_FIELD: &str = "client-ip";

/// What the page shows about the data file, read once when the engine is
/// built.
#[derive(Clone, Default)]
pub struct DataFileNotes {
    /// The standard free-tier and stale-file warnings.
    pub warnings: Vec<String>,
    /// The properties the data file carries.
    pub available: Vec<String>,
}

/// Options that drive [`run`].
pub struct Options {
    /// The path to the on-premise `.ipi` data file.
    pub data_file: PathBuf,
    /// The socket address the server binds to.
    pub address: SocketAddr,
}

/// Build the on-premise web application router plus the data-file notes.
///
/// The engine is built once so its data-file warnings (free tier, and over 30
/// days old) can be read with [`examples_shared::check_data_file`] and shown on
/// the page, along with the properties the data file carries, which say which
/// rows need the Enterprise data file. The very same engine instance is then
/// used as an application element of the web pipeline. Usage sharing is added
/// first (REQUIRED for a web example).
///
/// `share_usage` is true when the binary serves the page. The tests pass false,
/// so a test run sends nothing to 51Degrees.
pub fn build_app(
    data_file: &std::path::Path,
    share_usage: bool,
) -> anyhow::Result<(Router, DataFileNotes)> {
    // Build the on-premise engine. Every property the data file supports is
    // loaded by default. LowMemory reads the data file from disk, so the server
    // needs little memory even with the Enterprise file, which is over 6 GB.
    // Where there is memory for the whole file, HighPerformance holds it in
    // memory for faster lookups, and the answers are the same.
    let engine = Arc::new(
        IpIntelligenceOnPremiseEngine::builder(data_file.to_path_buf())
            .performance_profile(PerformanceProfile::LowMemory)
            // Off so no licence key is needed; see the update-data-file example.
            .auto_update(false)
            .build()
            .with_context(|| format!("loading the .ipi data file at {}", data_file.display()))?,
    );

    // Read the standard free-tier and stale-file warnings and the data file's
    // own list of properties for display, before the engine is moved into the
    // pipeline element list.
    let notes = DataFileNotes {
        warnings: examples_shared::check_data_file(engine.as_ref()),
        available: engine.available_property_names(),
    };

    // Assemble the application's own elements: usage sharing first (REQUIRED for a
    // web example), then the on-premise engine. WebPipeline wraps these with the
    // sequence, set-headers, JSON and JavaScript elements.
    let mut elements: Vec<Arc<dyn FlowElement>> = vec![engine];
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
    Ok((app, notes))
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
    let (app, notes) = build_app(&options.data_file, true)?;
    store_notes(notes);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building the tokio runtime")?;
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(options.address)
            .await
            .with_context(|| format!("binding {}", options.address))?;
        let bound = listener.local_addr().context("reading the bound address")?;
        println!("IP Intelligence on-premise web example listening on http://{bound}");
        let notes = current_notes();
        for warning in &notes.warnings {
            println!("WARNING: {warning}");
        }
        let missing = examples_shared::missing_properties(ENTERPRISE_PROPERTIES, &notes.available);
        if let Some(message) =
            examples_shared::enterprise_data_file_message(&missing, CONTACT_US_URL)
        {
            println!("{message}");
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

// The data-file notes are computed once at build time and read on every page
// render. A web handler cannot easily carry extra state alongside FiftyOneState
// without threading a custom router state, so for this example they are kept in a
// process-wide slot. A real application would store them in its own state.
static NOTES: std::sync::OnceLock<std::sync::Mutex<DataFileNotes>> = std::sync::OnceLock::new();

/// Record the notes for the running server to show on each page.
fn store_notes(notes: DataFileNotes) {
    let slot = NOTES.get_or_init(|| std::sync::Mutex::new(DataFileNotes::default()));
    *slot.lock().expect("notes lock") = notes;
}

/// The notes recorded by [`store_notes`], or empty notes if none.
fn current_notes() -> DataFileNotes {
    NOTES
        .get()
        .map(|slot| slot.lock().expect("notes lock").clone())
        .unwrap_or_default()
}

/// The home-page handler: render the weighted server-side IP-intelligence
/// results, the evidence table, the IP look-up form, the location map, the
/// free-tier data-file warnings and which properties need the Enterprise data
/// file.
async fn home(
    Query(params): Query<std::collections::HashMap<String, String>>,
    result: FiftyOneResult,
) -> Html<String> {
    let notes = current_notes();
    let (results_html, coordinates_pair, evidence_html, evidence_ip) = result.with(|flow_data| {
        let (results, coords) = flow_data
            .get(IP_DATA_KEY)
            .map(|ip| {
                let mut values = display_values(ip);
                mark_enterprise_only(&mut values, &notes.available);
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

        let ip = evidence_pairs
            .iter()
            .find(|(key, _)| key.contains("client-ip"))
            .map(|(_, value)| value.clone())
            .unwrap_or_default();

        (results, coords, evidence_table(&evidence_pairs), ip)
    });

    let form_value = params.get(FORM_FIELD).cloned().unwrap_or_default();
    let client_ip = if form_value.is_empty() {
        evidence_ip
    } else {
        form_value.clone()
    };

    // The free-tier and stale-file warnings are shown as a single top-of-page
    // c-eg-alert. The Enterprise properties the data file lacks are named
    // under the results, with a link to ask about a copy.
    let top_alert = warnings_alert(&notes.warnings);
    let missing = examples_shared::missing_properties(ENTERPRISE_PROPERTIES, &notes.available);
    let results_html = format!("{results_html}{}", enterprise_message_html(&missing));
    let map_script = map_init_script(&coordinates_pair);

    Html(web_support::render_page(PageOptions {
        title: "IP Intelligence - On-Premise Web Example",
        lead: "This example performs IP Intelligence on premise (from a local .ipi data file) \
               within a small axum web server. It shows the network and location \
               properties for the connecting client's IP address. Enter any IP address below to \
               look it up.",
        top_alert: &top_alert,
        client_ip: &client_ip,
        results_html: &results_html,
        evidence_html: &evidence_html,
        form_field: FORM_FIELD,
        form_value: &form_value,
        // The on-premise free-tier page describes what a paid data file adds.
        message_html: web_support::ONPREM_CONTACT_BANNER,
        map_script: &map_script,
        repositories: examples_shared::ON_PREMISE_REPOSITORIES,
    }))
}

fn main() -> anyhow::Result<()> {
    // Resolve the on-premise data file through the shared scheme. It defaults to
    // the Lite file from the data repository, downloaded when it is missing, or
    // Enterprise when the production share is reachable.
    let data_file = examples_shared::ipi_data_path(examples_shared::IpiTier::default()).context(
        "no on-premise IP Intelligence data file found. Set _51DEGREES_IPI_PATH to an .ipi \
         data file.",
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
    use axum::response::Response;
    use tower::ServiceExt;

    /// Build the app for the test, skipping when no data file is available.
    ///
    /// The test pins the ASN tier so it always exercises the small, current,
    /// always-loadable 4.5 ASN file, independent of whether the production
    /// Enterprise share happens to be mounted on the build machine. The binary
    /// itself uses `IpiTier::default()`, the Enterprise file or the Lite file.
    fn test_app() -> Option<Router> {
        let data_file = examples_shared::ipi_data_path(examples_shared::IpiTier::Asn)?;
        let (app, notes) = build_app(&data_file, false).expect("on-premise app builds");
        store_notes(notes);
        Some(app)
    }

    /// Drive one GET request through a clone of the app and return the response.
    ///
    /// A clone is used (cloning a `Router` is cheap, sharing the same `Arc`
    /// elements) so the owning `app` the caller holds outlives the request. With
    /// usage sharing on, the app wraps a blocking `reqwest` client whose backing
    /// runtime must not be dropped from inside an async context, so keeping the
    /// owning `app` in the synchronous test body lets its final drop happen
    /// outside the runtime.
    async fn get(app: Router, uri: &str) -> Response {
        app.oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .header("host", "localhost")
                .header("x-forwarded-for", "1.1.1.1")
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

    #[test]
    fn server_starts_and_serves_page_and_client_script() {
        // The on-premise pipeline needs a local data file. Skip (not fail) when it
        // is absent so a plain `cargo test` is green without the submodule.
        let Some(app) = test_app() else {
            eprintln!(
                "skipping server_starts_and_serves_page_and_client_script: no .ipi data file \
                 (set _51DEGREES_IPI_PATH to enable this test)"
            );
            return;
        };

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("test runtime builds");

        runtime.block_on(async {
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
            assert!(page_body.contains("ASN"), "the page shows the ASN rows");
            // The ASN file carries none of the Enterprise properties, so the
            // page says so. _51DEGREES_IPI_PATH can name another file.
            let asn_file = examples_shared::IPI_ASN_RELATIVE_PATH.rsplit('/').next();
            let data_file = examples_shared::ipi_data_path(examples_shared::IpiTier::Asn);
            if data_file
                .as_deref()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                == asn_file
            {
                assert!(page_body.contains("needs the Enterprise data file"));
                assert!(page_body.contains("utm_term=enterprise-data-file"));
            }

            let script = get(app.clone(), "/51Degrees.core.js").await;
            assert_eq!(script.status(), StatusCode::OK);
            let script_body = body_string(script).await;
            assert!(
                script_body.contains("fod"),
                "the client script names the 'fod' manager object"
            );
        });

        // `app` (and the usage-sharing element's blocking client) is dropped here,
        // in the synchronous test body, after the runtime has finished.
        drop(app);
    }
}

/*
 * @example ipi-web-getting-started-onprem.rs
 *
 * IP Intelligence on-premise web example (server-side).
 *
 * This example demonstrates the 51Degrees Pipeline performing IP Intelligence
 * from a local `.ipi` data file inside a small axum web server. It is the
 * on-premise twin of `ipi-web-getting-started-cloud.rs` and works the same way:
 *
 *   1. Server-side IP Intelligence by the 51Degrees middleware, using the
 *      client's IP address (from `X-Forwarded-For` when behind a proxy, otherwise
 *      the connection peer) as evidence, read back through the `FiftyOneResult`
 *      extractor and rendered as a weighted properties table.
 *
 *   2. The probabilistic results, each property's most probable value shown with
 *      its 0.0 to 1.0 weighting.
 *
 *   3. An IP look-up form, submitting under the `client-ip` query parameter which
 *      the on-premise engine accepts as client-IP evidence, so any IPv4 or IPv6
 *      address can be looked up.
 *
 *   4. An approximate-location map drawn by the shared `examples.min.js`
 *      `initLocationMap` helper when the lookup resolves coordinates.
 *
 * Because the data file is a free tier (the Lite file from the data repository
 * by default), the page shows the standard data-file warnings from
 * `examples_shared::check_data_file`: a free-tier notice and a stale-file notice
 * when the file is more than 30 days old. These
 * are rendered as a `c-eg-alert` at the top of the page. The results table
 * shows `RegisteredName`, `RegisteredOwner` and `RegisteredCountry`, which the
 * free Lite file carries, `Asn` and `AsnName`, which the free ASN file
 * carries, and properties only the Enterprise data file carries, such as
 * `Country`, `Town`,
 * `Latitude`, `Longitude`, `IsHosted` and `IsVPN`. The data file's own list of
 * properties says which it has, so with a free file each of those rows says
 * "needs the Enterprise data file", and a message under the table names them
 * with a link to contact 51Degrees about a copy. The `c-eg-message`
 * contact-us banner describes what a paid Enterprise data file adds (more
 * detailed and accurate location and registered-network ownership, broader
 * coverage), and the page ends with a "Find out more" section.
 *
 * Usage sharing is enabled, as a web deployment should. The engine is built once:
 * its data-file warnings are read before it is handed to the web pipeline as an
 * application element, so no data file is opened twice.
 *
 * The engine uses the `LowMemory` profile, which reads the data file from
 * disk, so the server runs in little memory even with the Enterprise file.
 * Where there is memory for the whole file, changing the profile in
 * `build_app` to `HighPerformance` holds the file in memory for faster
 * lookups. The profile changes memory use and speed, not the answers.
 *
 * Resolve the data file with `_51DEGREES_IPI_PATH` (or rely on the Lite file in
 * the data repository inside the `cxx` submodule, which is downloaded when it
 * is missing), then run the binary and open http://127.0.0.1:8083.
 */
