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

//! Human Confidence web demo. The server lives in this library so the tests
//! can drive it, and `src/bin/human-confidence-web.rs` runs it.
//!
//! Human Confidence says how likely it is that a person is looking at the
//! page. The page shows it in three steps.
//!
//! 1. Passive. This server asks the cloud itself, sending the visitor's
//!    request headers and IP address, and reads `HumanConfidenceSource` and
//!    `HumanConfidence` from the answer. The server asks rather than reading
//!    a level from the page, because a level a browser sends can be forged.
//! 2. Active. The page loads the 51Degrees client script. When it completes,
//!    `HumanConfidenceSource` is `Active` and `HumanConfidence` holds a
//!    sealed value the browser can't read or change.
//! 3. Read. Each page is given an id, and a transaction kept on this server
//!    that the client script's address carries to the cloud. The page sends
//!    the sealed value and its id back as `humanconfidence` and
//!    `humanconfidencepage`. The cloud request engine redeems the sealed
//!    value with the account's License Key and the page's transaction, and
//!    this server reads the two Properties it gives, `HumanConfidenceSource`
//!    and `HumanConfidence`. This server never redeems anything itself, and
//!    the License Key and the transaction stay on it.

#![warn(missing_docs)]
// A warning in a documentation example is an error, as everywhere else.
#![doc(test(attr(deny(warnings))))]

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{ConnectInfo, Form, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use examples_web_shared::serve_css;
use fiftyone_cloud_request_engine::{
    human_confidence, AsyncCloudHttpClient, CloudHttpRequest, CloudHttpResponse,
    CloudRequestEngine, HttpMethod, HumanConfidenceTransactions, LocalBoxFuture, PAGE_EVIDENCE,
    SEALED_RESULT_EVIDENCE,
};
use fiftyone_pipeline_core::{Evidence, Pipeline};
use serde_json::{json, Value};
use tokio::sync::OnceCell;

/// The port the demo listens on when `_51DEGREES_EXAMPLE_HTTP_PORTS` is not
/// set.
pub const DEFAULT_PORT: u16 = 5151;

/// The cloud API base used when `_51DEGREES_CLOUD_ENDPOINT` is not set.
pub const DEFAULT_API: &str = "https://cloud.51degrees.com/api/v4/";

/// Where the page and the console send someone whose Resource Key lacks the
/// HumanConfidence property.
pub const CONTACT_URL: &str = "https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-human-confidence-examples-src-lib.rs&utm_term=human-confidence";

/// The links the page and the console output end with.
pub const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("Documentation", "https://51degrees.com/documentation/index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-human-confidence-examples-src-lib.rs&utm_term=find-out-more"),
    ("Pricing", "https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-human-confidence-examples-src-lib.rs&utm_term=find-out-more"),
    ("Contact us", "https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-human-confidence-examples-src-lib.rs&utm_term=find-out-more"),
];

/// The property the Resource Key needs.
pub const PROPERTY: &str = "HumanConfidence";

/// The demo page, embedded so the binary is self-contained. Its relative
/// stylesheet URL is served from the shared Rust example asset.
const PAGE: &str = include_str!("../assets/page.html");

/// The longest the demo waits for the cloud, so a slow cloud can't hang it.
const TIMEOUT: Duration = Duration::from_secs(10);

/// What the server needs to run.
pub struct Settings {
    /// The Resource Key, put into the page and used for the passive call.
    pub resource_key: String,
    /// A License Key of the same account, which the cloud request engine
    /// redeems with. Without one every sealed value reads as `Medium`.
    pub license_key: Option<String>,
    /// The cloud API base, ending in `/api/v4/`.
    pub api: String,
    /// Whether the Resource Key includes HumanConfidence. When it doesn't,
    /// the page says so and links to [`CONTACT_URL`].
    pub human_confidence: bool,
}

/// The router state.
struct Demo {
    settings: Settings,
    http: reqwest::Client,
    /// The request header names the cloud reads, fetched once.
    header_names: OnceCell<HashSet<String>>,
    /// The transaction the package keeps for each page, shared with the
    /// cloud request engine, which redeems with it.
    transactions: Arc<HumanConfidenceTransactions>,
    /// The Pipeline holding the cloud request engine, built once.
    pipeline: OnceCell<(Arc<CloudRequestEngine>, Arc<Pipeline>)>,
}

/// The cloud request engine's transport, over this demo's reqwest client.
struct Transport(reqwest::Client);

impl AsyncCloudHttpClient for Transport {
    fn send<'a>(
        &'a self,
        request: &'a CloudHttpRequest,
    ) -> LocalBoxFuture<'a, Result<CloudHttpResponse, String>> {
        Box::pin(async move {
            let mut builder = match request.method {
                HttpMethod::Get => self.0.get(&request.url),
                HttpMethod::Post => self.0.post(&request.url).form(&request.form),
            };
            if let Some(origin) = &request.origin {
                builder = builder.header(header::ORIGIN, origin);
            }
            // Without the URL, which can carry the Resource Key.
            let failed = |e: reqwest::Error| e.without_url().to_string();
            let response = builder.send().await.map_err(failed)?;
            let status = response.status().as_u16();
            let retry_after = response
                .headers()
                .get(header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            let body = response.text().await.map_err(failed)?;
            Ok(CloudHttpResponse {
                status,
                body,
                retry_after,
            })
        })
    }
}

/// The API base from the value of `_51DEGREES_CLOUD_ENDPOINT`, with exactly
/// one trailing slash, or [`DEFAULT_API`] when it is unset.
pub fn api_from(value: Option<&str>) -> String {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => format!("{}/", value.trim_end_matches('/')),
        None => DEFAULT_API.to_owned(),
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .expect("the HTTP client builds")
}

/// Whether the Resource Key includes HumanConfidence, from the cloud's
/// `accessibleproperties` list.
pub async fn has_human_confidence(api: &str, resource_key: &str) -> anyhow::Result<bool> {
    let answer: Value = client()
        .get(reqwest::Url::parse_with_params(
            &format!("{api}accessibleproperties"),
            [("resource", resource_key)],
        )?)
        .send()
        .await
        .map_err(reqwest::Error::without_url)?
        .error_for_status()
        .map_err(reqwest::Error::without_url)?
        .json()
        .await
        .map_err(reqwest::Error::without_url)?;
    Ok(names_property(&answer))
}

/// True when any object in the answer has a `Name` of [`PROPERTY`].
fn names_property(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            map.get("Name")
                .and_then(Value::as_str)
                .is_some_and(|name| name.eq_ignore_ascii_case(PROPERTY))
                || map.values().any(names_property)
        }
        Value::Array(items) => items.iter().any(names_property),
        _ => false,
    }
}

/// The demo's routes.
pub fn app(settings: Settings) -> Router {
    let demo = Arc::new(Demo {
        settings,
        http: client(),
        header_names: OnceCell::new(),
        transactions: Arc::default(),
        pipeline: OnceCell::new(),
    });
    Router::new()
        .route("/", get(home))
        .route("/examples-main.min.css", get(serve_css))
        .route("/passive", get(passive))
        .route("/active", post(active))
        .with_state(demo)
}

/// The page, filled in. It must never come from a cache, as each load runs
/// the browser tests.
async fn home(State(demo): State<Arc<Demo>>) -> Response {
    let settings = &demo.settings;
    // Each page gets its own id, and a transaction kept here that the client
    // script's address carries to the cloud. The page sends the id back with
    // its sealed value.
    let (script, page_id) = match demo.transactions.issue() {
        Some(page) => (
            page.script_url(&settings.api, &settings.resource_key),
            page.id().to_owned(),
        ),
        None => (
            format!("{}{}.js", settings.api, settings.resource_key),
            String::new(),
        ),
    };
    let mut page = page_with_find_out_more()
        .replace("__SCRIPT__", &script)
        .replace("__PAGE__", &page_id);
    if !settings.human_confidence {
        let notice = format!(
            "<p class=\"c-eg-page__lead\"><strong>This Resource Key doesn't \
             include the {PROPERTY} property.</strong> \
             <a href=\"{}\">Contact us</a> to add it.</p>\n  \
             <table class=\"c-eg-table\">",
            CONTACT_URL.replace('&', "&amp;")
        );
        page = page.replacen("<table class=\"c-eg-table\">", &notice, 1);
    }
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        page,
    )
        .into_response()
}

/// [`PAGE`] with a "Find out more" section, set apart by a rule, before its
/// closing body tag.
fn page_with_find_out_more() -> String {
    let mut html = format!(
        "<div class=\"c-eg-page\">\n<hr>\n\
         <h3 class=\"c-eg-page__heading\">{}</h3>\n<ul>\n",
        examples_shared::FIND_OUT_MORE_HEADING
    );
    for (label, url) in FIND_OUT_MORE {
        let href = url.replace('&', "&amp;");
        html.push_str(&format!("<li><a href=\"{href}\">{label}</a></li>\n"));
    }
    for url in examples_shared::CLOUD_REPOSITORIES {
        html.push_str(&format!("<li><a href=\"{url}\">{url}</a></li>\n"));
    }
    html.push_str("</ul>\n</div>\n</body>");
    PAGE.replacen("</body>", &html, 1)
}

/// The request header names the cloud reads, from its `evidencekeys` list.
/// Only these are passed on, so headers such as Authorization stay here.
async fn header_names(demo: &Demo) -> Result<&HashSet<String>, reqwest::Error> {
    demo.header_names
        .get_or_try_init(|| async {
            let keys: Vec<String> = demo
                .http
                .get(format!("{}evidencekeys", demo.settings.api))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            Ok(keys
                .iter()
                .filter_map(|key| {
                    let lower = key.to_ascii_lowercase();
                    lower.strip_prefix("header.").map(str::to_owned)
                })
                .collect())
        })
        .await
}

/// Passive. This server asks the cloud for the visitor, with the visitor's
/// headers and IP address, and reads the result from the answer.
async fn passive(
    State(demo): State<Arc<Demo>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    match ask_passive(&demo, Some(peer), &headers).await {
        Ok(answer) => json_response(StatusCode::OK, answer),
        Err(error) => json_response(StatusCode::OK, json!({ "error": error })),
    }
}

async fn ask_passive(
    demo: &Demo,
    peer: Option<SocketAddr>,
    headers: &HeaderMap,
) -> Result<Value, String> {
    let names = header_names(demo).await.map_err(|e| e.to_string())?;
    let mut evidence: HashMap<String, String> = HashMap::new();
    for name in headers.keys() {
        let key = name.as_str().to_ascii_lowercase();
        if names.contains(&key) {
            let value = headers
                .get_all(name)
                .iter()
                .filter_map(|value| value.to_str().ok())
                .collect::<Vec<_>>()
                .join(",");
            evidence.insert(key, value);
        }
    }
    // The visitor's address is the address of the connection. Set last, so
    // no header can replace it.
    if let Some(peer) = peer {
        evidence.insert("client-ip".to_owned(), peer.ip().to_canonical().to_string());
    }
    let answer = demo
        .http
        .post(format!(
            "{}{}.json",
            demo.settings.api, demo.settings.resource_key
        ))
        .form(&evidence)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = answer.status();
    let body = answer.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let start: String = body.chars().take(200).collect();
        return Err(format!("HTTP {}: {start}", status.as_u16()));
    }
    let parsed: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let derived = &parsed["derived"];
    Ok(json!({
        "source": derived["humanconfidencesource"].as_str(),
        "humanConfidence": derived["humanconfidence"].as_str(),
    }))
}

/// Active. The page sends its sealed value as `humanconfidence` and its page
/// id as `humanconfidencepage`, which become the evidence the cloud request
/// engine redeems with, in the flow data's one call to the cloud. This
/// server only reads the two Properties it gives, which are `Active` and the
/// level, or `Active` and `Medium` where the sealed value wasn't redeemed.
async fn active(
    State(demo): State<Arc<Demo>>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let mut evidence = Evidence::builder();
    for (parameter, key) in [
        ("humanconfidence", SEALED_RESULT_EVIDENCE),
        ("humanconfidencepage", PAGE_EVIDENCE),
    ] {
        if let Some(value) = form.get(parameter).filter(|value| !value.is_empty()) {
            evidence = evidence.add(key, value.as_str());
        }
    }
    let evidence = evidence.build();
    let demo_license_key = demo.settings.license_key.clone();
    // The engine's client returns futures that need not be Send, which an
    // axum handler can't hold, so the engine runs on a thread of its own.
    let runtime = tokio::runtime::Handle::current();
    let read = tokio::task::spawn_blocking(move || {
        runtime.block_on(async move {
            let (engine, pipeline) = pipeline(&demo).await?;
            let mut data = pipeline.create_flow_data_with(evidence);
            engine.fetch(&mut data).await;
            // A failed request leaves the Properties at Medium and Active,
            // so it is not an error here.
            let _ = data.process();
            Ok::<_, String>(human_confidence(&data))
        })
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|read| read);
    match read {
        Ok(Some(properties)) => json_response(
            StatusCode::OK,
            json!({
                "source": properties.human_confidence_source().as_str(),
                "humanConfidence": properties.human_confidence().as_str(),
            }),
        ),
        Ok(None) => json_response(
            StatusCode::OK,
            json!({ "error": "The page sent no sealed value." }),
        ),
        Err(error) => {
            // The details go to the server's log, never to the page, which
            // must receive no part of the License Key. The engine already
            // leaves the key out of its messages, and the log entry has it
            // taken out again in case another part of the text named it.
            let secrets: Vec<&str> = demo_license_key.iter().map(String::as_str).collect();
            eprintln!(
                "Reading the Human Confidence Properties failed: {}",
                fiftyone_pipeline_core::redact::hide_with(&error, &secrets)
            );
            json_response(StatusCode::OK, json!({ "error": READ_FAILED }))
        }
    }
}

/// What the page is told when the Pipeline can't be built or read. The
/// details go to the server's log instead.
pub const READ_FAILED: &str =
    "The Human Confidence Properties could not be read. See the server's log.";

/// The Pipeline with the cloud request engine, built the first time a
/// sealed value arrives. With a License Key the engine authenticates with
/// that alone and names the one Property it reads, so the key travels only
/// in a request body. Without one it has the Resource Key, and every sealed
/// value reads as `Medium`.
async fn pipeline(demo: &Demo) -> Result<&(Arc<CloudRequestEngine>, Arc<Pipeline>), String> {
    demo.pipeline
        .get_or_try_init(|| async {
            let settings = &demo.settings;
            let builder = CloudRequestEngine::builder()
                .endpoint(settings.api.clone())
                .timeout(TIMEOUT)
                .async_http_client(Arc::new(Transport(demo.http.clone())))
                .human_confidence_transactions(demo.transactions.clone());
            let mut builder = match settings
                .license_key
                .as_deref()
                .filter(|key| !key.trim().is_empty())
            {
                Some(license) => builder
                    .license_key(license)
                    .values(["derived.HumanConfidence"]),
                None => builder.resource_key(settings.resource_key.clone()),
            };
            let engine = Arc::new(builder.build_async().await.map_err(|e| e.to_string())?);
            let pipeline = Pipeline::builder()
                .add_element(engine.clone())
                .suppress_process_exceptions(true)
                .build()
                .map_err(|e| e.to_string())?;
            Ok((engine, pipeline))
        })
        .await
}

fn json_response(status: StatusCode, body: Value) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}
