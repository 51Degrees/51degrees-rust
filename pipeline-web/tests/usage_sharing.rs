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

//! Usage sharing in a web pipeline. The usage sharing element accepts no
//! evidence, so a revalidation's `If-None-Match` header does not change the
//! ETag, and it still shares the headers of the web request that no other
//! element accepts.

use std::any::Any;
use std::sync::{Arc, Mutex};

use fiftyone_pipeline_core::{
    ElementData, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    MapElementData, NoValueError, PropertyMetaData, PropertyValue, PropertyValueType, Result,
    TypedKey,
};
use fiftyone_pipeline_engines_fiftyone::{
    ShareUsageConfig, ShareUsageElement, UsageBatch, UsageSink,
};
use fiftyone_pipeline_web::{
    build_evidence, serve_json, RequestData, WebIntegrationOptions, WebPipeline, WebResponse,
};

/// The session id the requests carry, so the Sequence element does not
/// create a new one, and with it a new ETag, for each request.
const SESSION_ID: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";

/// Keeps each document the usage sharing element delivers.
#[derive(Default)]
struct Keep(Mutex<Vec<String>>);

impl UsageSink for Keep {
    fn receive(
        &self,
        batch: UsageBatch,
    ) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.0.lock().unwrap().push(batch.document().to_owned());
        Ok(())
    }
}

struct TestData(MapElementData);

impl ElementData for TestData {
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

/// An engine that accepts the User-Agent header and nothing else, and gives
/// one property.
struct Engine {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

impl Engine {
    const KEY: TypedKey<TestData> = TypedKey::new("device");

    fn new() -> Self {
        Engine {
            filter: EvidenceKeyFilterWhitelist::new(["header.user-agent"]),
            properties: vec![PropertyMetaData::new(
                "ismobile",
                "device",
                PropertyValueType::Bool,
            )],
        }
    }
}

impl FlowElement for Engine {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        data.get_or_add(Self::KEY, || {
            TestData(MapElementData::new().set("ismobile", true))
        })?;
        Ok(())
    }
    fn data_key(&self) -> &str {
        "device"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

#[derive(Default)]
struct TestRequest {
    headers: Vec<(String, String)>,
    cookies: Vec<(String, String)>,
    query: Vec<(String, String)>,
}

impl RequestData for TestRequest {
    fn headers(&self) -> Vec<(String, String)> {
        self.headers.clone()
    }
    fn cookies(&self) -> Vec<(String, String)> {
        self.cookies.clone()
    }
    fn query_params(&self) -> Vec<(String, String)> {
        self.query.clone()
    }
    fn client_ip(&self) -> Option<String> {
        None
    }
    fn is_https(&self) -> bool {
        true
    }
}

fn pairs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

/// A web pipeline holding the engine and `share`.
fn web_pipeline(share: &Arc<ShareUsageElement>) -> WebPipeline {
    WebPipeline::build(
        vec![
            Arc::new(Engine::new()) as Arc<dyn FlowElement>,
            Arc::clone(share) as Arc<dyn FlowElement>,
        ],
        WebIntegrationOptions::default(),
    )
    .expect("web pipeline builds")
}

/// Process `request` through `web` as a web integration does, with the
/// evidence built from the request.
fn processed(web: &WebPipeline, request: &TestRequest) -> FlowData {
    let evidence = build_evidence(request, web.pipeline().evidence_key_filter());
    let mut data = web.pipeline().create_flow_data_with(evidence);
    data.process()
        .expect("processing does not error (suppressed)");
    data
}

#[test]
fn a_revalidation_gets_a_304_with_usage_sharing_in_the_pipeline() {
    let share = Arc::new(ShareUsageElement::with_sink(
        ShareUsageConfig::builder().build(),
        Keep::default(),
    ));
    let web = web_pipeline(&share);
    let options = web.endpoint_options();
    let serve = |if_none_match: Option<&str>| {
        let mut headers = pairs(&[("User-Agent", "Mozilla/5.0")]);
        headers.extend(if_none_match.map(|etag| ("If-None-Match".to_owned(), etag.to_owned())));
        let request = TestRequest {
            headers,
            query: pairs(&[("session-id", SESSION_ID)]),
            ..Default::default()
        };
        serve_json(&processed(&web, &request), &request, &options)
    };

    let first = serve(None);
    assert_eq!(first.status, WebResponse::STATUS_OK);
    let etag = first
        .header("ETag")
        .expect("the 200 has an ETag")
        .to_owned();

    let revalidation = serve(Some(&etag));
    assert_eq!(
        revalidation.status,
        WebResponse::STATUS_NOT_MODIFIED,
        "If-None-Match: {etag} got ETag {:?}",
        revalidation.header("ETag")
    );
    assert_eq!(revalidation.header("ETag"), Some(etag.as_str()));
}

#[test]
fn usage_sharing_shares_the_web_requests_headers_no_element_accepts() {
    let sink = Arc::new(Keep::default());
    let share = Arc::new(ShareUsageElement::with_sink(
        ShareUsageConfig::builder()
            .blocked_http_headers(["x-blocked"])
            .build(),
        Arc::clone(&sink),
    ));
    let web = web_pipeline(&share);
    let request = TestRequest {
        headers: pairs(&[
            ("User-Agent", "Mozilla/5.0"),
            ("X-Test", "test"),
            ("X-Blocked", "blocked"),
            ("Cookie", "51D_Test=kept; other=dropped"),
        ]),
        cookies: pairs(&[("51D_Test", "kept"), ("other", "dropped")]),
        query: pairs(&[("session-id", SESSION_ID)]),
    };
    drop(processed(&web, &request));

    // The element delivers what it holds when it is dropped.
    drop(web);
    drop(share);
    let documents = sink.0.lock().unwrap();
    assert_eq!(documents.len(), 1, "{documents:?}");
    let document = &documents[0];
    let session = format!("<SessionId>{SESSION_ID}</SessionId>");
    for shared in [
        "<header Name=\"User-Agent\"><![CDATA[Mozilla/5.0]]></header>",
        "<header Name=\"X-Test\"><![CDATA[test]]></header>",
        "<cookie Name=\"51D_Test\"><![CDATA[kept]]></cookie>",
        session.as_str(),
    ] {
        assert!(document.contains(shared), "{shared} in {document}");
    }
    for left_out in ["Name=\"X-Blocked\"", "Name=\"Cookie\"", "Name=\"other\""] {
        assert!(!document.contains(left_out), "{left_out} in {document}");
    }
}
