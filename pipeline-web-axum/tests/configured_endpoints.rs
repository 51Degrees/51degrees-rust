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

//! The client-side endpoints served from the paths the web integration
//! options give. The web integration specification says the two intercepted
//! URLs "SHOULD be configurable", with `/51Degrees.core.js` and
//! `/51dpipeline/json` as the defaults.

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::Router;
use fiftyone_pipeline_core::FlowElement;
use fiftyone_pipeline_engines_fiftyone::SequenceElement;
use fiftyone_pipeline_web::{WebIntegrationOptions, WebPipeline};
use fiftyone_pipeline_web_axum::{register, FiftyOneState};
use tower::ServiceExt;

/// The JavaScript endpoint's path in these tests.
const JAVASCRIPT_PATH: &str = "/assets/fod.js";

/// The JSON endpoint's path in these tests.
const JSON_PATH: &str = "/api/fod/json";

/// A router over a web pipeline whose endpoints are at the paths above, to
/// which `WebPipeline` adds the JSON and JavaScript builders.
fn app() -> Router {
    let elements: Vec<Arc<dyn FlowElement>> = vec![Arc::new(SequenceElement::new())];
    let options = WebIntegrationOptions {
        javascript_endpoint: JAVASCRIPT_PATH.to_owned(),
        json_endpoint: JSON_PATH.to_owned(),
        ..WebIntegrationOptions::default()
    };
    let web = WebPipeline::build(elements, options).expect("web pipeline builds");
    register(Router::new(), FiftyOneState::from_web_pipeline(&web))
}

fn get(path: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(path)
        .header("host", "example.com")
        .body(Body::empty())
        .unwrap()
}

fn post(path: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(path)
        .header("host", "example.com")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from("session-id=abc&sequence=1"))
        .unwrap()
}

fn content_type(response: &axum::response::Response) -> Option<String> {
    response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

async fn body(response: axum::response::Response) -> String {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

/// Both endpoints are served from the configured paths, and not from the
/// default ones.
#[tokio::test]
async fn both_endpoints_are_served_from_the_configured_paths() {
    let javascript = app().oneshot(get(JAVASCRIPT_PATH)).await.unwrap();
    assert_eq!(javascript.status(), StatusCode::OK);
    assert_eq!(
        content_type(&javascript).as_deref(),
        Some("application/x-javascript")
    );

    let json = app().oneshot(post(JSON_PATH)).await.unwrap();
    assert_eq!(json.status(), StatusCode::OK);
    assert_eq!(content_type(&json).as_deref(), Some("application/json"));
    let json = body(json).await;
    let value: serde_json::Value = serde_json::from_str(&json)
        .unwrap_or_else(|error| panic!("body is JSON: {error}; body was: {json}"));
    assert!(value.is_object(), "{json}");

    for request in [get("/51Degrees.core.js"), post("/51dpipeline/json")] {
        let response = app().oneshot(request).await.unwrap();
        assert_ne!(
            response.status(),
            StatusCode::OK,
            "a default path is served"
        );
    }
}

/// The script the JavaScript endpoint serves calls back to the configured
/// JSON path, as that is the one the integration serves.
#[tokio::test]
async fn the_script_calls_back_to_the_configured_json_path() {
    let javascript = app().oneshot(get(JAVASCRIPT_PATH)).await.unwrap();
    assert_eq!(javascript.status(), StatusCode::OK);
    let script = body(javascript).await;
    assert!(
        script.contains(&format!("//example.com{JSON_PATH}'")),
        "the script does not call back to {JSON_PATH}"
    );
    assert!(
        !script.contains("/51dpipeline/json"),
        "the script calls back to the default path"
    );
}
