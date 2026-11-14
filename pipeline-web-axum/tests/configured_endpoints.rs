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

//! The script the JavaScript endpoint serves when the web integration options
//! give the JSON endpoint another path. The web integration specification
//! says the two intercepted URLs "SHOULD be configurable", and the script has
//! to call back to the JSON endpoint the integration serves.

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::Router;
use fiftyone_pipeline_core::FlowElement;
use fiftyone_pipeline_engines_fiftyone::SequenceElement;
use fiftyone_pipeline_web::{WebIntegrationOptions, WebPipeline};
use fiftyone_pipeline_web_axum::{register, FiftyOneState};
use tower::ServiceExt;

/// The JavaScript endpoint's path in this test.
const JAVASCRIPT_PATH: &str = "/assets/fod.js";

/// The JSON endpoint's path in this test.
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

/// The script the JavaScript endpoint serves calls back to the configured
/// JSON path, as that is the one the integration serves.
#[tokio::test]
async fn the_script_calls_back_to_the_configured_json_path() {
    let request = Request::builder()
        .method("GET")
        .uri(JAVASCRIPT_PATH)
        .header("host", "example.com")
        .body(Body::empty())
        .unwrap();
    let javascript = app().oneshot(request).await.unwrap();
    assert_eq!(javascript.status(), StatusCode::OK);
    let bytes = to_bytes(javascript.into_body(), usize::MAX).await.unwrap();
    let script = String::from_utf8(bytes.to_vec()).unwrap();
    // The callback URL as the script holds it, for the failure message.
    let callback = script
        .find("//example.com")
        .and_then(|at| script.get(at..(at + CALLBACK_SHOWN).min(script.len())))
        .unwrap_or("no callback URL");
    assert!(
        script.contains(&format!("//example.com{JSON_PATH}")),
        "the script does not call back to {JSON_PATH}: {callback}"
    );
    assert!(
        !script.contains("/51dpipeline/json"),
        "the script calls back to the default path: {callback}"
    );
}

/// How many bytes of the callback URL a failure message shows.
const CALLBACK_SHOWN: usize = 60;
