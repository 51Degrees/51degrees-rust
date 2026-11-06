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

//! In-process integration tests for the axum web adapter.
//!
//! No TCP socket is bound. A minimal web pipeline (sequence, JSON builder,
//! JavaScript builder, no engine or data file) is assembled, mounted on a
//! router, and driven with `tower::ServiceExt::oneshot`, which dispatches one
//! request through the full middleware-plus-handler stack and returns the
//! response.

use std::any::Any;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, ThreadId};

use axum::body::{to_bytes, Body};
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use axum::Router;
use fiftyone_javascript_builder::{JavaScriptBuilderElement, JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY};
use fiftyone_json_builder::{JsonBuilderElement, JSON_BUILDER_ELEMENT_DATA_KEY};
use fiftyone_pipeline_core::{
    ElementData, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    MapElementData, NoValueError, PropertyMetaData, PropertyValue, Result, TypedKey,
};
use fiftyone_pipeline_engines_fiftyone::SequenceElement;
use fiftyone_pipeline_web::{WebIntegrationOptions, WebPipeline};
use fiftyone_pipeline_web_axum::{register, FiftyOneState};
use tower::ServiceExt;

/// Build a router with the minimal web pipeline mounted: sequence, JSON builder
/// and JavaScript builder. `WebPipeline` inserts the set-headers element too;
/// the builders give the two endpoints content to serve without needing a
/// detection engine or data file.
fn app() -> Router {
    // Pass the JSON and JavaScript builders as application elements. WebPipeline
    // prepends the sequence element and (because the default options enable it)
    // a set-headers element, so the ordered pipeline is
    // sequence -> json-builder -> javascript-builder -> set-headers -> json ...
    // The explicit builders here mean the test exercises the real client-side
    // assets rather than relying on WebPipeline re-adding them.
    let elements: Vec<Arc<dyn FlowElement>> = vec![
        Arc::new(SequenceElement::new()),
        Arc::new(JsonBuilderElement::new()),
        Arc::new(JavaScriptBuilderElement::new()),
    ];

    // Disable WebPipeline's own client-side builders so they are not added a
    // second time; the application supplies them explicitly above.
    let options = WebIntegrationOptions {
        client_side_evidence_enabled: false,
        ..WebIntegrationOptions::default()
    };

    let web = WebPipeline::build(elements, options).expect("web pipeline builds");
    let state = FiftyOneState::from_web_pipeline(&web);

    register(Router::new().route("/", get(|| async { "home" })), state)
}

/// Read the value of a response header as an owned string, if present.
fn header(response: &axum::response::Response, name: &str) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

#[tokio::test]
async fn javascript_endpoint_serves_200_with_object_name_and_etag() {
    let app = app();

    let request = Request::builder()
        .method("GET")
        .uri("/51Degrees.core.js")
        .header("host", "localhost")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        header(&response, "content-type").as_deref(),
        Some("application/x-javascript")
    );

    // An ETag is present (the caching validator).
    let etag = header(&response, "etag").expect("ETag header present");
    assert!(
        etag.starts_with('"') && etag.ends_with('"'),
        "ETag is quoted"
    );

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    // The global `fod` object must be ASSIGNED, not merely mentioned: the
    // template ends with `var fod = new fiftyoneDegreesManager();` and a
    // mis-configured minifier once dropped that binding as unused while the
    // `fod_` prefix string kept a bare contains("fod") check green. Accept the
    // minified and unminified spacings.
    assert!(
        body.contains("fod=new fiftyoneDegreesManager")
            || body.contains("fod = new fiftyoneDegreesManager"),
        "body assigns the global fod manager object, got: {body}"
    );
}

#[tokio::test]
async fn javascript_endpoint_returns_304_for_matching_if_none_match() {
    // Both requests carry the same session id. Without one the Sequence
    // element creates a new id for each request, and the ETag is worked out
    // from the evidence that id is added to.
    //
    // First request: capture the ETag.
    let first = app()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/51Degrees.core.js?session-id=abc")
                .header("host", "localhost")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let etag = header(&first, "etag").expect("ETag present on first response");

    // Second request: send the captured ETag as If-None-Match. The same
    // evidence yields the same ETag, so the endpoint returns 304.
    let second = app()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/51Degrees.core.js?session-id=abc")
                .header("host", "localhost")
                .header("if-none-match", &etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(second.status(), StatusCode::NOT_MODIFIED);
    // A 304 carries the caching fields the 200 would.
    assert_eq!(header(&second, "etag"), Some(etag));
    assert_eq!(
        header(&second, "cache-control"),
        header(&first, "cache-control")
    );
    assert!(header(&second, "cache-control").is_some());
    // A 304 carries no body.
    let body = to_bytes(second.into_body(), usize::MAX).await.unwrap();
    assert!(body.is_empty(), "304 has an empty body");
}

#[tokio::test]
async fn json_endpoint_serves_200_with_json_body() {
    let app = app();

    let request = Request::builder()
        .method("POST")
        .uri("/51dpipeline/json")
        .header("content-type", "application/x-www-form-urlencoded")
        .header("host", "localhost")
        .body(Body::from("session-id=abc&sequence=1"))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        header(&response, "content-type").as_deref(),
        Some("application/json")
    );

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    // The JSON builder produces a JSON object.
    let value: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|error| panic!("body is JSON: {error}; body was: {body}"));
    assert!(value.is_object(), "JSON body is an object, got: {body}");
}

/// RFC 9110 section 13.1.2: a matching `If-None-Match` on the JSON endpoint's
/// `POST` gets a `412`, never a `304` (section 9.3.3), with only the CORS
/// header.
#[tokio::test]
async fn json_endpoint_returns_412_for_matching_if_none_match() {
    let post = |etag: Option<&str>| {
        let mut builder = Request::builder()
            .method("POST")
            .uri("/51dpipeline/json")
            .header("content-type", "application/x-www-form-urlencoded")
            .header("host", "localhost")
            .header("origin", "https://example.com");
        if let Some(etag) = etag {
            builder = builder.header("if-none-match", etag);
        }
        // The same session and sequence in both, so the evidence and the ETag
        // are the same.
        builder
            .body(Body::from("session-id=abc&sequence=1"))
            .unwrap()
    };

    let first = app().oneshot(post(None)).await.unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let etag = header(&first, "etag").expect("ETag present on first response");

    let second = app().oneshot(post(Some(&etag))).await.unwrap();

    assert_eq!(second.status(), StatusCode::PRECONDITION_FAILED);
    assert_eq!(
        header(&second, "access-control-allow-origin").as_deref(),
        Some("https://example.com")
    );
    for name in ["etag", "cache-control", "vary", "content-type"] {
        assert_eq!(header(&second, name), None, "the 412 has no {name}");
    }
    let body = to_bytes(second.into_body(), usize::MAX).await.unwrap();
    assert!(body.is_empty(), "412 has an empty body");
}

#[tokio::test]
async fn non_endpoint_request_passes_through_to_application() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(&body[..], b"home");
}

/// An element that learns the evidence it accepts after it is built, as the
/// cloud request engine does after a transient discovery failure. Its filter
/// is complete once `learned` is set.
#[derive(Default)]
struct LearningElement {
    learned: OnceLock<EvidenceKeyFilterWhitelist>,
    nothing: EvidenceKeyFilterWhitelist,
}

impl FlowElement for LearningElement {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
        Ok(())
    }
    fn data_key(&self) -> &str {
        "learning"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        self.learned.get().unwrap_or(&self.nothing)
    }
    fn is_evidence_key_filter_complete(&self) -> bool {
        self.learned.get().is_some()
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

/// The state built from a web pipeline follows the pipeline's `Vary` list, so
/// it gains the evidence an element learns after the build. A copy of the
/// list taken at build would not.
#[test]
fn the_state_follows_the_vary_list_once_an_element_completes() {
    let element = Arc::new(LearningElement::default());
    let web = WebPipeline::build(vec![element.clone()], WebIntegrationOptions::default())
        .expect("web pipeline builds");
    let state = FiftyOneState::from_web_pipeline(&web);
    assert!(!state.vary_whitelist().include("header.x-learned"));

    element
        .learned
        .set(EvidenceKeyFilterWhitelist::new(["header.x-learned"]))
        .unwrap();

    assert!(state.vary_whitelist().include("header.x-learned"));
    assert!(state
        .endpoint_options()
        .current_vary_whitelist()
        .is_some_and(|list| list.include("header.x-learned")));
}

// ----------------------------------------------------------------------------
// Where an endpoint is served when its builder left no data.
// ----------------------------------------------------------------------------

/// Keeps each record at warning level and above with the thread that logged
/// it.
struct ThreadLogger {
    entries: Mutex<Vec<(ThreadId, String)>>,
}

impl log::Log for ThreadLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            self.entries
                .lock()
                .unwrap()
                .push((thread::current().id(), record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

fn logger() -> &'static ThreadLogger {
    static LOGGER: OnceLock<&'static ThreadLogger> = OnceLock::new();
    LOGGER.get_or_init(|| {
        let logger: &'static ThreadLogger = Box::leak(Box::new(ThreadLogger {
            entries: Mutex::new(Vec::new()),
        }));
        log::set_logger(logger).expect("no other logger is installed in this binary");
        log::set_max_level(log::LevelFilter::Warn);
        logger
    })
}

/// The threads that logged this message.
fn threads_logging(message: &str) -> Vec<ThreadId> {
    logger()
        .entries
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, logged)| logged == message)
        .map(|(id, _)| *id)
        .collect()
}

/// Records the thread each request is processed on.
#[derive(Default)]
struct ThreadElement {
    threads: Mutex<Vec<ThreadId>>,
    nothing: EvidenceKeyFilterWhitelist,
}

impl FlowElement for ThreadElement {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
        self.threads.lock().unwrap().push(thread::current().id());
        Ok(())
    }
    fn data_key(&self) -> &str {
        "thread"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.nothing
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

struct ClashData(MapElementData);

impl ElementData for ClashData {
    fn get(&self, name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        self.0.get(name)
    }
    fn keys(&self) -> Vec<String> {
        self.0.keys()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Puts data of another type under a builder's data key, so the builder fails
/// to add its own and leaves no data.
struct ClashElement {
    builder_key: &'static str,
    nothing: EvidenceKeyFilterWhitelist,
}

impl FlowElement for ClashElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        data.get_or_add(TypedKey::<ClashData>::new(self.builder_key), || {
            ClashData(MapElementData::new())
        })?;
        Ok(())
    }
    fn data_key(&self) -> &str {
        "clash"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.nothing
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

/// When a builder left no data, serving builds its output again, so each
/// endpoint is served on the blocking thread that processed the request and
/// never on the runtime's own thread, which runs this test.
#[tokio::test]
async fn an_endpoint_is_served_on_the_thread_that_processed_it() {
    let cases = [
        (
            JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY,
            Request::builder()
                .method("GET")
                .uri("/51Degrees.core.js")
                .header("host", "localhost")
                .body(Body::empty())
                .unwrap(),
        ),
        (
            JSON_BUILDER_ELEMENT_DATA_KEY,
            Request::builder()
                .method("POST")
                .uri("/51dpipeline/json")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("host", "localhost")
                .body(Body::from("session-id=abc&sequence=1"))
                .unwrap(),
        ),
    ];
    for (builder_key, request) in cases {
        let processed = Arc::new(ThreadElement::default());
        let elements: Vec<Arc<dyn FlowElement>> = vec![
            processed.clone(),
            Arc::new(ClashElement {
                builder_key,
                nothing: EvidenceKeyFilterWhitelist::default(),
            }),
        ];
        let web = WebPipeline::build(elements, WebIntegrationOptions::default())
            .expect("web pipeline builds");
        let app = register(Router::new(), FiftyOneState::from_web_pipeline(&web));
        let message = format!(
            "Failed to get data from element '{builder_key}', so the response is not cached."
        );
        // Installs the logger before the request logs anything.
        logger();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK, "{builder_key}");
        let processed_on = processed.threads.lock().unwrap().clone();
        assert_eq!(processed_on.len(), 1, "{builder_key} is processed once");
        assert_ne!(processed_on[0], thread::current().id(), "{builder_key}");
        assert_eq!(threads_logging(&message), processed_on, "{builder_key}");
    }
}

// ----------------------------------------------------------------------------
// What an endpoint takes from the request and the pipeline.
// ----------------------------------------------------------------------------

/// Records the client IP evidence each request carries, and accepts the
/// `User-Agent` header so the pipeline's `Vary` list names it.
struct ClientIpElement {
    seen: Mutex<Vec<Option<String>>>,
    filter: EvidenceKeyFilterWhitelist,
}

impl ClientIpElement {
    fn new() -> Self {
        ClientIpElement {
            seen: Mutex::new(Vec::new()),
            filter: EvidenceKeyFilterWhitelist::new(["server.client-ip", "header.user-agent"]),
        }
    }
}

impl FlowElement for ClientIpElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        let ip = data.evidence().get("server.client-ip").map(str::to_owned);
        self.seen.lock().unwrap().push(ip);
        Ok(())
    }
    fn data_key(&self) -> &str {
        "clientip"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

/// Each endpoint gives the pipeline the connection's peer address as the
/// client IP, and sends the `Vary` header from the pipeline's list.
#[tokio::test]
async fn an_endpoint_takes_the_peer_address_and_the_pipeline_vary_list() {
    let peer: SocketAddr = "203.0.113.7:50000".parse().unwrap();
    let requests = [
        Request::builder().method("GET").uri("/51Degrees.core.js"),
        Request::builder()
            .method("POST")
            .uri("/51dpipeline/json")
            .header("content-type", "application/x-www-form-urlencoded"),
    ];
    for request in requests {
        let recorder = Arc::new(ClientIpElement::new());
        let elements: Vec<Arc<dyn FlowElement>> = vec![recorder.clone()];
        let web = WebPipeline::build(elements, WebIntegrationOptions::default())
            .expect("web pipeline builds");
        let app = register(Router::new(), FiftyOneState::from_web_pipeline(&web));
        let request = request
            .header("host", "localhost")
            .header("user-agent", "test-agent")
            .extension(ConnectInfo(peer))
            .body(Body::empty())
            .unwrap();
        let path = request.uri().path().to_owned();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(
            *recorder.seen.lock().unwrap(),
            vec![Some("203.0.113.7".to_owned())],
            "{path}"
        );
        let vary = header(&response, "vary").unwrap_or_default();
        assert!(
            vary.split(',')
                .any(|name| name.trim().eq_ignore_ascii_case("user-agent")),
            "{path} sends Vary: {vary}"
        );
    }
}
