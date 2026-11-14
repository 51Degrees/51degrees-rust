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

//! Client-side evidence: gather the birth date from client-side JavaScript.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use axum::response::Html;
use axum::routing::get;
use axum::Router;
use fiftyone_pipeline_core::{
    EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData,
    PropertyValueType, TypedKey,
};
use fiftyone_pipeline_engines_fiftyone::{ShareUsageConfig, ShareUsageElement};
use fiftyone_pipeline_web::{WebIntegrationOptions, WebPipeline};
use fiftyone_pipeline_web_axum::{register, FiftyOneResult, FiftyOneState};
use pipeline_examples::star_sign::{
    parse_day_month, star_sign_for, StarSignData, DOB_JAVASCRIPT_PROPERTY, STAR_SIGNS,
    STAR_SIGN_DATA_KEY, STAR_SIGN_PROPERTY, UNKNOWN_STAR_SIGN,
};

/// The links the example's output ends with.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("Client-side evidence", "https://51degrees.com/documentation/_pipeline_api__features__client_side_evidence.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-pipeline-examples-src-bin-ss-clientside.rs&utm_term=find-out-more"),
    ("Pipeline API", "https://51degrees.com/documentation/_pipeline_api__index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-pipeline-examples-src-bin-ss-clientside.rs&utm_term=find-out-more"),
    ("Device Detection", "https://51degrees.com/device-detection?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-pipeline-examples-src-bin-ss-clientside.rs&utm_term=find-out-more"),
    ("IP Intelligence", "https://51degrees.com/ip-intelligence?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-pipeline-examples-src-bin-ss-clientside.rs&utm_term=find-out-more"),
];

/// The cookie the client-side JavaScript writes the birth date to. The element
/// reads it back as `cookie.date-of-birth` evidence on the next request.
const DATE_OF_BIRTH_COOKIE: &str = "cookie.date-of-birth";

/// The client-side star-sign element.
///
/// Unlike the simple element, this one does not expect the birth date to be
/// supplied directly. On the first request there is no `cookie.date-of-birth`, so
/// it cannot compute a sign; instead it emits a JavaScript snippet (the
/// `dobjavascript` property) that prompts the visitor, writes their answer to the
/// cookie and reloads the page. The 51Degrees client JavaScript bundle runs that
/// snippet on the client. On the reload the cookie is present, the element
/// computes the sign, and the JavaScript becomes empty.
struct SimpleClientSideElement {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

impl SimpleClientSideElement {
    const KEY: TypedKey<StarSignData> = TypedKey::new(STAR_SIGN_DATA_KEY);

    fn new() -> Self {
        SimpleClientSideElement {
            filter: EvidenceKeyFilterWhitelist::new([DATE_OF_BIRTH_COOKIE]),
            properties: vec![
                PropertyMetaData::new(
                    STAR_SIGN_PROPERTY,
                    STAR_SIGN_DATA_KEY,
                    PropertyValueType::String,
                ),
                // The JavaScript property: the snippet that gathers the date on
                // the client. Declaring it as a JavaScript-typed property is what
                // makes the JavaScript builder bundle it into 51Degrees.core.js.
                PropertyMetaData::new(
                    DOB_JAVASCRIPT_PROPERTY,
                    STAR_SIGN_DATA_KEY,
                    PropertyValueType::JavaScript,
                ),
            ],
        }
    }
}

impl FlowElement for SimpleClientSideElement {
    fn process(&self, data: &mut FlowData) -> Result<(), fiftyone_pipeline_core::Error> {
        // The date arrives as a `dd/mm/...` string in the cookie. If it is
        // present and parses, compute the sign and clear the JavaScript; if not,
        // emit the prompt JavaScript and leave the sign Unknown.
        let parsed = data
            .evidence()
            .get(DATE_OF_BIRTH_COOKIE)
            .and_then(parse_day_month);

        let star_sign_data = data.get_or_add(Self::KEY, StarSignData::new)?;
        match parsed.and_then(|(month, day)| star_sign_for(&STAR_SIGNS, month, day)) {
            Some(sign) => {
                star_sign_data.set_star_sign(sign);
                // No more client work to do, so the JavaScript is empty.
                star_sign_data.set_dob_javascript("");
            }
            None => {
                star_sign_data.set_star_sign(UNKNOWN_STAR_SIGN);
                // Prompt for the date, store it in the cookie and reload so the
                // next request carries the cookie evidence.
                star_sign_data.set_dob_javascript(
                    "var dob = window.prompt('Enter your date of birth.', 'dd/mm/yyyy');\
                     if (dob != null) { document.cookie = 'date-of-birth=' + dob; \
                     location.reload(); }",
                );
            }
        }
        Ok(())
    }
    fn data_key(&self) -> &str {
        STAR_SIGN_DATA_KEY
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

/// Build the web pipeline: the client-side element plus usage sharing, wrapped by
/// the web integration so the client-side endpoints are mounted.
///
/// Web examples MUST enable usage sharing, so the binary passes true for
/// `share_usage`. The element batches generously, so a short run of the binary
/// sends nothing, because stopping the server ends the process first. The tests
/// pass false, because they drop the pipeline, and a dropped element sends what
/// it holds.
fn build_web_pipeline(share_usage: bool) -> Result<WebPipeline> {
    let mut elements: Vec<Arc<dyn FlowElement>> = vec![Arc::new(SimpleClientSideElement::new())];
    if share_usage {
        elements.push(Arc::new(ShareUsageElement::new(
            ShareUsageConfig::builder()
                .minimum_entries_per_message(1000)
                .maximum_queue_size(10_000)
                .build(),
        )));
    }

    // The default web options enable the client-side endpoints (the JSON and
    // JavaScript builders), which is exactly what this example needs.
    Ok(WebPipeline::build(
        elements,
        WebIntegrationOptions::default(),
    )?)
}

/// The home page handler. Reads the processed star sign from the request's flow
/// data and renders a page that pulls in the 51Degrees client JavaScript.
async fn home(result: FiftyOneResult) -> Html<String> {
    // Read the sign (and whether client JavaScript is still pending) out of the
    // processed flow data. The closure runs while the flow data is borrowed.
    let (sign, awaiting_input) = result
        .get(SimpleClientSideElement::KEY, |data| {
            let sign = data.star_sign().unwrap_or(UNKNOWN_STAR_SIGN).to_owned();
            let awaiting = data
                .dob_javascript()
                .map(|js| !js.is_empty())
                .unwrap_or(false);
            (sign, awaiting)
        })
        .unwrap_or_else(|| (UNKNOWN_STAR_SIGN.to_owned(), false));

    let message = if awaiting_input {
        "Waiting for your date of birth. Allow the prompt to see your star sign.".to_owned()
    } else {
        format!("Your star sign is {sign}.")
    };

    Html(home_page_html(&message))
}

/// Render the home page HTML.
///
/// The single important line is the script tag pulling in `/51Degrees.core.js`.
/// That endpoint, mounted by `register`, serves the bundled client JavaScript for
/// every element in the pipeline, which includes the star-sign prompt snippet
/// when the cookie is absent.
fn home_page_html(message: &str) -> String {
    let _ = find_out_more_html();
    let find_out_more = String::new();
    format!(
        "<!doctype html>\n\
         <html lang=\"en\">\n\
         <head>\n\
           <meta charset=\"utf-8\">\n\
           <title>51Degrees star sign (client-side evidence)</title>\n\
           <!-- The 51Degrees core JavaScript bundles every element's client-side \
                JavaScript, so it runs the star-sign prompt when no date of birth \
                cookie is set yet. -->\n\
           <script type=\"application/javascript\" src=\"/51Degrees.core.js\"></script>\n\
         </head>\n\
         <body>\n\
           <h1>51Degrees star sign</h1>\n\
           <p>{message}</p>\n\
           {find_out_more}\
         </body>\n\
         </html>\n"
    )
}

/// The page's "Find out more" section, set apart by a rule, listing
/// [`FIND_OUT_MORE`] and the repositories as links.
fn find_out_more_html() -> String {
    let mut html = format!(
        "<hr>\n<h2>{}</h2>\n<ul>\n",
        examples_shared::FIND_OUT_MORE_HEADING
    );
    for (label, url) in FIND_OUT_MORE {
        let href = url.replace('&', "&amp;");
        html.push_str(&format!("<li><a href=\"{href}\">{label}</a></li>\n"));
    }
    for url in pipeline_examples::REPOSITORIES {
        html.push_str(&format!("<li><a href=\"{url}\">{url}</a></li>\n"));
    }
    html.push_str("</ul>\n");
    html
}

/// Assemble the axum application: the home route plus the 51Degrees endpoints and
/// middleware registered onto it.
fn build_app(web: &WebPipeline) -> Router {
    let state = FiftyOneState::from_web_pipeline(web);
    register(Router::new().route("/", get(home)), state)
}

/// Options controlling one run of the example.
pub struct ExampleOptions {
    /// The address to bind the server to.
    pub address: SocketAddr,
}

impl Default for ExampleOptions {
    fn default() -> Self {
        ExampleOptions {
            address: SocketAddr::from(([127, 0, 0, 1], 3000)),
        }
    }
}

/// Run the example: build the web pipeline and serve it until the process is
/// stopped. Visit `http://<address>/` in a browser to see the prompt appear and,
/// after answering, the computed star sign.
pub fn run(options: &ExampleOptions) -> Result<()> {
    // Build the pipeline (and so the usage-sharing element's `reqwest::blocking`
    // client, which owns a short-lived inner runtime) on this plain thread,
    // before the async runtime is entered. tokio forbids dropping a runtime from
    // inside another runtime's async context, so building here, rather than
    // inside `block_on`, avoids that panic. This mirrors the test harness below.
    let web = build_web_pipeline(true)?;
    let app = build_app(&web);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(options.address).await?;
        println!(
            "Serving the client-side star-sign example on http://{} (press Ctrl+C to stop).",
            listener.local_addr()?
        );
        // Connect-info is supplied so the client IP is available to the pipeline.
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await?;
        Ok::<(), anyhow::Error>(())
    });

    // `web` and `runtime` drop here on the plain thread, so the blocking
    // usage-sharing client shuts down outside any async context.
    drop(web);
    result
}

/// Read an optional bind address from the command line, then serve the example.
fn main() -> Result<()> {
    let mut options = ExampleOptions::default();
    if let Some(addr) = std::env::args().nth(1) {
        options.address = addr.parse()?;
    }
    run(&options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    /// Build a multi-thread runtime, send one request through the app and return
    /// the status and body.
    ///
    /// The web pipeline is built without usage sharing, so the test sends nothing
    /// to 51Degrees. The runtime and the pipeline are still created and dropped
    /// inside this plain, non-async test function, as the binary needs them to be
    /// with usage sharing on, because the usage-sharing element's
    /// `reqwest::blocking` client must not have its inner runtime torn down inside
    /// an outer runtime. The web middleware processes the pipeline on a
    /// `spawn_blocking` thread, so a multi-thread runtime with a blocking pool is
    /// used.
    fn request(uri: &str) -> (StatusCode, String) {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("the test runtime builds");

        let web = build_web_pipeline(false).expect("the web pipeline builds");
        let app = build_app(&web);

        let result = runtime.block_on(async {
            let response = app
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .body(Body::empty())
                        .expect("the request builds"),
                )
                .await
                .expect("the route responds");
            let status = response.status();
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("the body reads");
            (
                status,
                String::from_utf8(bytes.to_vec()).expect("the body is utf-8"),
            )
        });

        // `web` and `runtime` drop here, on the plain test thread, so the blocking
        // usage-sharing client shuts down outside any async context.
        drop(web);
        result
    }

    /// The home page renders, returns 200 and pulls in the client JavaScript. The
    /// app is exercised in-process with `oneshot`, so the test neither binds a
    /// socket nor reaches the network.
    #[test]
    fn home_page_serves_and_includes_client_javascript() {
        let (status, body) = request("/");
        assert_eq!(status, StatusCode::OK);
        assert!(
            body.contains("/51Degrees.core.js"),
            "the page must include the 51Degrees client JavaScript"
        );
    }

    /// The client JavaScript endpoint is mounted by `register` and serves a
    /// JavaScript body.
    #[test]
    fn client_javascript_endpoint_is_mounted() {
        let (status, _body) = request("/51Degrees.core.js");
        assert_eq!(status, StatusCode::OK);
    }
}

/* ---------------------------------------------------------------------------
 * Example: Client-Side Evidence (star sign from a browser-supplied date)
 *
 * This example modifies the star-sign element to get its input from client-side
 * JavaScript rather than from evidence supplied by the application. It is a small
 * web application built on the 51Degrees axum web integration.
 *
 * The round trip
 * --------------
 *   1. A visitor loads the page. There is no date-of-birth cookie yet, so the
 *      element cannot compute a sign. It sets its `dobjavascript` property to a
 *      snippet that prompts for the date, writes it to a cookie and reloads.
 *
 *   2. The page includes <script src="/51Degrees.core.js">. That endpoint, added
 *      by `register`, serves the bundled client JavaScript for every element in
 *      the pipeline, so the prompt snippet runs in the browser.
 *
 *   3. After the visitor answers, the cookie is set and the page reloads. This
 *      time the request carries `cookie.date-of-birth`, the element computes the
 *      sign, and clears the JavaScript (there is nothing left to gather).
 *
 * How the JavaScript gets to the client
 * -------------------------------------
 * The element declares `dobjavascript` as a JavaScript-typed property and stores
 * its value as a `PropertyValue::JavaScript`. The JSON builder serializes it and
 * the JavaScript builder wraps it into the bundle served at 51Degrees.core.js.
 * The element advertises `cookie.date-of-birth` as its evidence so the web layer
 * knows the response varies on that cookie.
 *
 * Usage sharing
 * -------------
 * Web examples MUST enable usage sharing, so the binary's pipeline includes the
 * usage-sharing element. It batches generously, so a short demo run sends
 * nothing. The tests leave the element out, because they drop the pipeline and
 * a dropped element sends what it holds.
 *
 * Running it
 * ----------
 *   cargo run -p pipeline-examples --bin ss-clientside [address:port]
 *
 * Then open http://127.0.0.1:3000/ and answer the prompt. The default bind
 * address is 127.0.0.1:3000.
 * ------------------------------------------------------------------------- */
