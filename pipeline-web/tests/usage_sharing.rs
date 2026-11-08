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
//! ETag or the `Vary` header, and it still shares the headers of the web
//! request that no other element accepts. Its ignore filter reads the web
//! request only for the keys it shares.

use std::any::Any;
use std::sync::{Arc, Mutex};

use fiftyone_pipeline_core::{
    ElementData, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
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
    /// The method and target of the request line, if given.
    line: Option<(&'static str, &'static str)>,
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
    fn method(&self) -> Option<String> {
        self.line.map(|(method, _)| method.to_owned())
    }
    fn request_target(&self) -> Option<String> {
        self.line.map(|(_, target)| target.to_owned())
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
    // Usage sharing adds no header to Vary, so only the engine's and the
    // JavaScript builder's headers are named.
    assert_eq!(first.header("Vary"), Some("host,protocol,user-agent"));
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
        line: None,
    };
    let data = processed(&web, &request);
    // No element accepts X-Test, so it is not in the evidence, and the
    // element can only find it in the web request.
    assert_eq!(data.evidence().get("header.x-test"), None);
    drop(data);

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

/// The ignore filter reads a key of the web request only when usage sharing
/// shares that key, so a pair on a key that is not shared and that no element
/// accepts does not stop the request being shared. A pair on a shared key
/// still does.
#[test]
fn the_ignore_filter_reads_only_the_shared_keys_of_the_web_request() {
    let request = TestRequest {
        headers: pairs(&[
            ("User-Agent", "Mozilla/5.0"),
            ("X-Test", "skip"),
            ("X-Blocked", "1"),
            ("Cookie", "internal=1"),
        ]),
        cookies: pairs(&[("internal", "1")]),
        query: pairs(&[("other", "1")]),
        line: Some(("GET", "/page?other=1")),
    };
    let documents = |key: &str, value: &str| {
        let sink = Arc::new(Keep::default());
        let share = Arc::new(ShareUsageElement::with_sink(
            ShareUsageConfig::builder()
                .blocked_http_headers(["x-blocked"])
                .ignore_flow_data_evidence_filter([(key, value)])
                .build(),
            Arc::clone(&sink),
        ));
        let web = web_pipeline(&share);
        drop(processed(&web, &request));
        // The element delivers what it holds when it is dropped.
        drop(web);
        drop(share);
        let delivered = sink.0.lock().unwrap();
        delivered.len()
    };

    let ignored: Vec<&str> = [
        ("cookie.internal", "1"),
        ("header.cookie", "internal=1"),
        ("header.x-blocked", "1"),
        ("query.other", "1"),
        ("server.request-path", "/page"),
    ]
    .into_iter()
    .filter(|&(key, value)| documents(key, value) != 1)
    .map(|(key, _)| key)
    .collect();
    assert!(
        ignored.is_empty(),
        "ignored on keys not shared: {ignored:?}"
    );

    assert_eq!(documents("header.x-test", "skip"), 0);
}

/// The document with its `DateSent` value removed, being the time it was
/// written and the one part that differs between two runs.
fn without_date_sent(document: &str) -> String {
    let start = document.find("<DateSent>").expect("a DateSent") + "<DateSent>".len();
    let end = start + document[start..].find("</DateSent>").expect("its end");
    format!("{}{}", &document[..start], &document[end..])
}

/// A request through the web pipeline gives the record that the same request
/// gives as Evidence holding each key the pipeline or the rules for what is
/// shared accept, with no web request, which is how a web integration built
/// the Evidence while usage sharing accepted the keys it shares. The ignore
/// pair names a cookie that is not shared, so neither route ignores it.
#[test]
fn a_web_request_gives_the_record_its_shared_keys_as_evidence_give() {
    let request = TestRequest {
        headers: pairs(&[
            ("User-Agent", "Mozilla/5.0"),
            ("Accept-Language", "en-GB"),
            ("X-Test", "test"),
            ("X-Blocked", "blocked"),
            ("Cookie", "51D_Test=kept; internal=1"),
        ]),
        cookies: pairs(&[("51D_Test", "kept"), ("internal", "1")]),
        query: pairs(&[
            ("session-id", SESSION_ID),
            ("51D_Query", "q"),
            ("other", "1"),
        ]),
        line: Some(("GET", "/page?51D_Query=q&other=1")),
    };
    let record = |through_the_web_request: bool| {
        let sink = Arc::new(Keep::default());
        let share = Arc::new(ShareUsageElement::with_sink(
            ShareUsageConfig::builder()
                .blocked_http_headers(["x-blocked"])
                .ignore_flow_data_evidence_filter([("cookie.internal", "1")])
                .build(),
            Arc::clone(&sink),
        ));
        let web = web_pipeline(&share);
        let filter = web.pipeline().evidence_key_filter();
        let mut evidence = build_evidence(&request, filter);
        if !through_the_web_request {
            let shared = share.shared_evidence_filter();
            let mut accepted = Evidence::builder();
            for entry in evidence.http_request().expect("the request").entries() {
                if filter.include(entry.key()) || shared.include(entry.key()) {
                    accepted = accepted.add(entry.key_as_given(), entry.value());
                }
            }
            evidence = accepted.build();
        }
        let mut data = web.pipeline().create_flow_data_with(evidence);
        data.process()
            .expect("processing does not error (suppressed)");
        drop(data);
        // The element delivers what it holds when it is dropped.
        drop(web);
        drop(share);
        let documents = sink.0.lock().unwrap().clone();
        assert_eq!(documents.len(), 1, "{documents:?}");
        without_date_sent(&documents[0])
    };

    assert_eq!(record(true), record(false));
}

/// The Sequence element the web pipeline adds accepts the session id and the
/// sequence itself, so with usage sharing accepting nothing both are still
/// taken from the request, and the sequence is still counted.
#[test]
fn the_session_id_and_sequence_are_still_taken_from_the_request() {
    let share = Arc::new(ShareUsageElement::with_sink(
        ShareUsageConfig::builder().build(),
        Keep::default(),
    ));
    let web = web_pipeline(&share);
    let request = TestRequest {
        query: pairs(&[("session-id", SESSION_ID), ("sequence", "4")]),
        ..Default::default()
    };
    let evidence = build_evidence(&request, web.pipeline().evidence_key_filter());
    assert_eq!(evidence.get("query.session-id"), Some(SESSION_ID));
    assert_eq!(evidence.get("query.sequence"), Some("4"));

    let data = processed(&web, &request);
    assert_eq!(data.evidence().get("query.session-id"), Some(SESSION_ID));
    assert_eq!(data.evidence().get("query.sequence"), Some("5"));
}
