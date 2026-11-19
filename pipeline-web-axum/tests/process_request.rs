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

//! `process_request` and `process_request_with_method` for an application
//! that captures the request itself rather than mounting the middleware.

use std::sync::Arc;

use axum::http::{HeaderMap, Method, Uri};
use fiftyone_pipeline_core::{
    EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData, Result,
};
use fiftyone_pipeline_web::{WebIntegrationOptions, WebPipeline};
use fiftyone_pipeline_web_axum::{
    process_request, process_request_with_method, CapturedRequest, FiftyOneState,
};

/// An element that reads the request line, so the pipeline's filter keeps
/// its three keys in the Evidence.
struct RequestLine {
    filter: EvidenceKeyFilterWhitelist,
}

impl FlowElement for RequestLine {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
        Ok(())
    }
    fn data_key(&self) -> &str {
        "requestline"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

fn state() -> (WebPipeline, FiftyOneState) {
    let element = RequestLine {
        filter: EvidenceKeyFilterWhitelist::new([
            "server.request-method",
            "server.request-path",
            "server.request-query",
        ]),
    };
    let web = WebPipeline::build(
        vec![Arc::new(element) as Arc<dyn FlowElement>],
        WebIntegrationOptions::default(),
    )
    .expect("web pipeline builds");
    let state = FiftyOneState::from_web_pipeline(&web);
    (web, state)
}

fn captured() -> CapturedRequest {
    CapturedRequest {
        headers: HeaderMap::new(),
        uri: Uri::from_static("/page?a=1"),
        body: Vec::new(),
        peer_addr: None,
    }
}

#[tokio::test]
async fn a_request_processed_with_its_method_has_the_request_line() {
    let (_web, state) = state();
    let data = process_request_with_method(&state, captured(), Method::POST)
        .await
        .expect("the blocking task completes");
    let evidence = data.evidence();
    assert_eq!(evidence.get("server.request-method"), Some("POST"));
    assert_eq!(evidence.get("server.request-path"), Some("/page"));
    assert_eq!(evidence.get("server.request-query"), Some("a=1"));
}

#[tokio::test]
async fn a_request_processed_without_its_method_has_no_request_line() {
    let (_web, state) = state();
    let data = process_request(&state, captured())
        .await
        .expect("the blocking task completes");
    let evidence = data.evidence();
    for key in [
        "server.request-method",
        "server.request-path",
        "server.request-query",
    ] {
        assert_eq!(evidence.get(key), None, "{key}");
    }
}
