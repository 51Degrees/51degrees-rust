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

use std::sync::{Arc, OnceLock};

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::routing::get;
use axum::Router;
use fiftyone_javascript_builder::JavaScriptBuilderElement;
use fiftyone_json_builder::JsonBuilderElement;
use fiftyone_pipeline_core::{
    EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData, Result,
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
    // First request: capture the ETag.
    let first = app()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/51Degrees.core.js")
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
                .uri("/51Degrees.core.js")
                .header("host", "localhost")
                .header("if-none-match", etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(second.status(), StatusCode::NOT_MODIFIED);
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
