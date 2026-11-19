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

//! End-to-end unit tests for the framework-neutral web integration. No web
//! server is started; a small test element and the real JSON / JavaScript
//! builders are assembled through [`WebPipeline`], and the endpoint functions
//! are exercised against hand-built requests.

use std::any::Any;
use std::sync::{Arc, OnceLock};

use fiftyone_javascript_builder::JavaScriptBuilderElement;
use fiftyone_pipeline_core::{
    ElementData, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    MapElementData, NoValueError, Pipeline, PropertyMetaData, PropertyValue, PropertyValueType,
    Result, TypedKey,
};
use fiftyone_pipeline_engines_fiftyone::SetHeadersElement;
use fiftyone_pipeline_web::{
    apply_set_headers, build_evidence, compute_etag, response_headers, serve_if_none_match,
    serve_javascript, serve_json, vary_header, EndpointOptions, RequestData, WebEndpoint,
    WebIntegrationOptions, WebPipeline, WebResponse,
};

// ----------------------------------------------------------------------------
// A tiny test element: publishes one boolean property and one SetHeader
// property, and may optionally fail so error handling can be tested.
// ----------------------------------------------------------------------------

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

struct TestElement {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
    fail: bool,
}

impl TestElement {
    const KEY: TypedKey<TestData> = TypedKey::new("device");

    fn new(fail: bool) -> Self {
        TestElement {
            // Accepts the User-Agent header, so it shows up in Vary.
            filter: EvidenceKeyFilterWhitelist::new(["header.user-agent"]),
            properties: vec![
                PropertyMetaData::new("ismobile", "device", PropertyValueType::Bool),
                // A SetHeader property so the set-headers element has work to do.
                PropertyMetaData::new(
                    "SetHeaderBrowserAccept-CH",
                    "device",
                    PropertyValueType::String,
                ),
            ],
            fail,
        }
    }
}

impl FlowElement for TestElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        if self.fail {
            return Err(fiftyone_pipeline_core::Error::configuration(
                "forced failure",
            ));
        }
        data.get_or_add(Self::KEY, || {
            TestData(
                MapElementData::new()
                    .set("ismobile", true)
                    .set("SetHeaderBrowserAccept-CH", "Sec-CH-UA"),
            )
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

// ----------------------------------------------------------------------------
// A configurable test request implementing RequestData.
// ----------------------------------------------------------------------------

#[derive(Default)]
struct TestRequest {
    headers: Vec<(String, String)>,
    cookies: Vec<(String, String)>,
    query: Vec<(String, String)>,
    form: Vec<(String, String)>,
    client_ip: Option<String>,
    https: bool,
    method: Option<String>,
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
    fn form_params(&self) -> Vec<(String, String)> {
        self.form.clone()
    }
    fn client_ip(&self) -> Option<String> {
        self.client_ip.clone()
    }
    fn is_https(&self) -> bool {
        self.https
    }
    fn method(&self) -> Option<String> {
        self.method.clone()
    }
}

// ----------------------------------------------------------------------------
// Helpers.
// ----------------------------------------------------------------------------

/// Build a web pipeline with one (optionally failing) test element.
fn web_pipeline(fail: bool) -> WebPipeline {
    WebPipeline::build(
        vec![Arc::new(TestElement::new(fail))],
        WebIntegrationOptions::default(),
    )
    .expect("web pipeline builds")
}

/// The session id the requests in these tests carry.
const SESSION_ID: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";

/// Evidence for a request from `user_agent` in the session [`SESSION_ID`].
fn page_evidence(user_agent: &str) -> Evidence {
    Evidence::builder()
        .add("header.user-agent", user_agent)
        .add("query.session-id", SESSION_ID)
        .build()
}

/// Process a flow data through the pipeline with the given evidence.
fn processed(pipeline: &Arc<Pipeline>, evidence: Evidence) -> FlowData {
    let mut data = pipeline.create_flow_data_with(evidence);
    data.process()
        .expect("processing does not error (suppressed)");
    data
}

// ----------------------------------------------------------------------------
// Element ordering.
// ----------------------------------------------------------------------------

#[test]
fn element_ordering_matches_spec() {
    let web = web_pipeline(false);
    let keys: Vec<&str> = web
        .pipeline()
        .flow_elements()
        .iter()
        .map(|e| e.data_key())
        .collect();
    assert_eq!(
        keys,
        vec![
            "sequence",
            "device",
            "set-headers",
            "json-builder",
            "javascriptbuilderelement",
        ]
    );
}

#[test]
fn a_sequence_element_among_the_elements_is_not_added_again() {
    let web = WebPipeline::build(
        vec![
            Arc::new(fiftyone_pipeline_engines_fiftyone::SequenceElement::new()),
            Arc::new(TestElement::new(false)),
        ],
        WebIntegrationOptions::default(),
    )
    .expect("web pipeline builds");
    let keys: Vec<&str> = web
        .pipeline()
        .flow_elements()
        .iter()
        .map(|e| e.data_key())
        .collect();
    assert_eq!(
        keys,
        vec![
            "sequence",
            "device",
            "set-headers",
            "json-builder",
            "javascriptbuilderelement",
        ]
    );
}

/// Runs a Sequence element under the data key it is given, as an element
/// wrapping another does.
struct WrappedSequence {
    data_key: &'static str,
    inner: fiftyone_pipeline_engines_fiftyone::SequenceElement,
}

impl FlowElement for WrappedSequence {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        self.inner.process(data)
    }
    fn data_key(&self) -> &str {
        self.data_key
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        self.inner.evidence_key_filter()
    }
    fn properties(&self) -> &[PropertyMetaData] {
        self.inner.properties()
    }
}

/// A second Sequence element would add one to the sequence the first set, so
/// the first request of a session would carry 2.
#[test]
fn a_sequence_element_is_found_by_its_data_key_ignoring_case() {
    let given = Arc::new(WrappedSequence {
        data_key: "Sequence",
        inner: fiftyone_pipeline_engines_fiftyone::SequenceElement::new(),
    });
    let web = WebPipeline::build(
        vec![
            Arc::clone(&given) as Arc<dyn FlowElement>,
            Arc::new(TestElement::new(false)),
        ],
        WebIntegrationOptions::default(),
    )
    .expect("web pipeline builds");
    assert_eq!(
        data_keys(&web),
        vec![
            "Sequence",
            "device",
            "set-headers",
            "json-builder",
            "javascriptbuilderelement",
        ]
    );
    assert!(runs(&web, "Sequence", &given));
    let data = processed(web.pipeline(), page_evidence("agent"));
    assert_eq!(data.evidence().get("query.sequence"), Some("1"));
}

/// Runs a set-headers element under the data key it is given, as an element
/// wrapping another does.
struct WrappedSetHeaders {
    data_key: &'static str,
    inner: SetHeadersElement,
}

impl FlowElement for WrappedSetHeaders {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        self.inner.process(data)
    }
    fn data_key(&self) -> &str {
        self.data_key
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        self.inner.evidence_key_filter()
    }
    fn properties(&self) -> &[PropertyMetaData] {
        self.inner.properties()
    }
}

/// The data keys of the pipeline's elements, in the order they run.
fn data_keys(web: &WebPipeline) -> Vec<&str> {
    web.pipeline()
        .flow_elements()
        .iter()
        .map(|element| element.data_key())
        .collect()
}

/// True if the pipeline's element with this data key is `element` itself.
fn runs<T>(web: &WebPipeline, data_key: &str, element: &Arc<T>) -> bool {
    web.pipeline()
        .flow_elements()
        .iter()
        .find(|e| e.data_key() == data_key)
        .is_some_and(|e| std::ptr::addr_eq(Arc::as_ptr(e), Arc::as_ptr(element)))
}

/// True if processing a request gives the `Accept-CH` header the test
/// element asks for, which only a set-headers element after it writes.
fn sets_accept_ch(web: &WebPipeline) -> bool {
    let data = processed(web.pipeline(), page_evidence("agent"));
    response_headers(&data)
        .iter()
        .any(|(name, value)| name == "Accept-CH" && value == "Sec-CH-UA")
}

#[test]
fn a_set_headers_element_among_the_elements_is_not_added_again() {
    let given = Arc::new(SetHeadersElement::new());
    let web = WebPipeline::build(
        vec![
            Arc::new(TestElement::new(false)),
            Arc::clone(&given) as Arc<dyn FlowElement>,
        ],
        WebIntegrationOptions::default(),
    )
    .expect("web pipeline builds");
    assert_eq!(
        data_keys(&web),
        vec![
            "sequence",
            "device",
            "set-headers",
            "json-builder",
            "javascriptbuilderelement",
        ]
    );
    assert!(runs(&web, "set-headers", &given));
    assert!(sets_accept_ch(&web));
}

#[test]
fn a_set_headers_element_among_the_elements_runs_where_it_was_put() {
    // Given before a JavaScript builder the application also gives. Added in
    // its usual place, a set-headers element would follow that builder.
    let given = Arc::new(SetHeadersElement::new());
    let web = WebPipeline::build(
        vec![
            Arc::new(TestElement::new(false)),
            Arc::clone(&given) as Arc<dyn FlowElement>,
            Arc::new(JavaScriptBuilderElement::new()),
        ],
        WebIntegrationOptions::default(),
    )
    .expect("web pipeline builds");
    assert_eq!(
        data_keys(&web),
        vec![
            "sequence",
            "device",
            "set-headers",
            "json-builder",
            "javascriptbuilderelement",
        ]
    );
    assert!(runs(&web, "set-headers", &given));
    assert!(sets_accept_ch(&web));
}

#[test]
fn a_set_headers_element_among_the_elements_runs_without_the_option() {
    let given = Arc::new(SetHeadersElement::new());
    let options = WebIntegrationOptions {
        use_set_header_properties: false,
        ..WebIntegrationOptions::default()
    };
    let web = WebPipeline::build(
        vec![
            Arc::new(TestElement::new(false)),
            Arc::clone(&given) as Arc<dyn FlowElement>,
        ],
        options,
    )
    .expect("builds");
    assert_eq!(
        data_keys(&web),
        vec![
            "sequence",
            "device",
            "set-headers",
            "json-builder",
            "javascriptbuilderelement",
        ]
    );
    assert!(runs(&web, "set-headers", &given));
    assert!(sets_accept_ch(&web));
}

#[test]
fn a_set_headers_element_is_found_by_its_data_key_ignoring_case() {
    let given = Arc::new(WrappedSetHeaders {
        data_key: "Set-Headers",
        inner: SetHeadersElement::new(),
    });
    let web = WebPipeline::build(
        vec![
            Arc::new(TestElement::new(false)),
            Arc::clone(&given) as Arc<dyn FlowElement>,
        ],
        WebIntegrationOptions::default(),
    )
    .expect("web pipeline builds");
    assert_eq!(
        data_keys(&web),
        vec![
            "sequence",
            "device",
            "Set-Headers",
            "json-builder",
            "javascriptbuilderelement",
        ]
    );
    assert!(runs(&web, "Set-Headers", &given));
    assert!(sets_accept_ch(&web));
}

#[test]
fn element_ordering_without_client_side_drops_builders() {
    let options = WebIntegrationOptions {
        client_side_evidence_enabled: false,
        ..WebIntegrationOptions::default()
    };
    let web = WebPipeline::build(vec![Arc::new(TestElement::new(false))], options).expect("builds");
    let keys: Vec<&str> = web
        .pipeline()
        .flow_elements()
        .iter()
        .map(|e| e.data_key())
        .collect();
    assert_eq!(keys, vec!["sequence", "device", "set-headers"]);
}

#[test]
fn element_ordering_without_set_headers() {
    let options = WebIntegrationOptions {
        use_set_header_properties: false,
        ..WebIntegrationOptions::default()
    };
    let web = WebPipeline::build(vec![Arc::new(TestElement::new(false))], options).expect("builds");
    let keys: Vec<&str> = web
        .pipeline()
        .flow_elements()
        .iter()
        .map(|e| e.data_key())
        .collect();
    assert_eq!(
        keys,
        vec![
            "sequence",
            "device",
            "json-builder",
            "javascriptbuilderelement"
        ]
    );
}

// ----------------------------------------------------------------------------
// Endpoint suffix matching.
// ----------------------------------------------------------------------------

#[test]
fn endpoint_matching_is_case_insensitive_suffix() {
    let web = web_pipeline(false);
    assert_eq!(
        web.endpoint_for("/51Degrees.core.js"),
        Some(WebEndpoint::JavaScript)
    );
    // Case-insensitive.
    assert_eq!(
        web.endpoint_for("/51degrees.CORE.JS"),
        Some(WebEndpoint::JavaScript)
    );
    // Mounted under a sub-path: suffix still matches.
    assert_eq!(
        web.endpoint_for("/app/v1/51Degrees.core.js"),
        Some(WebEndpoint::JavaScript)
    );
    assert_eq!(
        web.endpoint_for("/51dpipeline/json"),
        Some(WebEndpoint::Json)
    );
    assert_eq!(
        web.endpoint_for("/some/prefix/51dpipeline/json"),
        Some(WebEndpoint::Json)
    );
    // Unrelated path.
    assert_eq!(web.endpoint_for("/index.html"), None);
    assert_eq!(web.endpoint_for("/json"), None);
}

// ----------------------------------------------------------------------------
// Content-Type and Content-Length.
// ----------------------------------------------------------------------------

#[test]
fn json_content_type_and_length() {
    let web = web_pipeline(false);
    let evidence = build_evidence(
        &TestRequest {
            headers: vec![("User-Agent".into(), "test".into())],
            https: true,
            ..Default::default()
        },
        web.pipeline().evidence_key_filter(),
    );
    let data = processed(web.pipeline(), evidence);
    let options = EndpointOptions::new(web.vary_whitelist().clone());
    let response = serve_json(&data, &TestRequest::default(), &options);

    assert_eq!(response.status, WebResponse::STATUS_OK);
    assert_eq!(response.header("Content-Type"), Some("application/json"));
    let len: usize = response.header("Content-Length").unwrap().parse().unwrap();
    assert_eq!(len, response.body.len());
    // The serialized JSON includes our property.
    assert!(response.body_str().unwrap().contains("ismobile"));
}

#[test]
fn javascript_content_type() {
    let web = web_pipeline(false);
    let evidence = build_evidence(
        &TestRequest {
            headers: vec![("Host".into(), "localhost".into())],
            ..Default::default()
        },
        web.pipeline().evidence_key_filter(),
    );
    let data = processed(web.pipeline(), evidence);
    let options = EndpointOptions::new(web.vary_whitelist().clone());
    let response = serve_javascript(&data, &TestRequest::default(), &options);

    assert_eq!(
        response.header("Content-Type"),
        Some("application/x-javascript")
    );
    let len: usize = response.header("Content-Length").unwrap().parse().unwrap();
    assert_eq!(len, response.body.len());
}

// ----------------------------------------------------------------------------
// Cache-Control: private on every response, no-cache on failure.
// ----------------------------------------------------------------------------

#[test]
fn cache_control_private_when_no_errors() {
    let web = web_pipeline(false);
    let data = processed(web.pipeline(), Evidence::default());
    let options = EndpointOptions::new(web.vary_whitelist().clone());
    let response = serve_json(&data, &TestRequest::default(), &options);
    assert_eq!(
        response.header("Cache-Control"),
        Some("private, max-age=1800")
    );
}

#[test]
fn cache_control_no_cache_on_processing_failure() {
    let web = web_pipeline(true); // element forced to fail
    let data = processed(web.pipeline(), Evidence::default());
    assert!(!data.errors().is_empty(), "processing recorded an error");
    let options = EndpointOptions::new(web.vary_whitelist().clone());
    let response = serve_json(&data, &TestRequest::default(), &options);
    assert_eq!(response.header("Cache-Control"), Some("private, no-cache"));
}

// ----------------------------------------------------------------------------
// Vary derivation from a whitelist.
// ----------------------------------------------------------------------------

#[test]
fn vary_derivation_from_whitelist() {
    let whitelist = EvidenceKeyFilterWhitelist::new([
        "header.User-Agent",
        "header.Sec-CH-UA",
        "query.sequence",
        "cookie.x",
    ]);
    assert_eq!(vary_header(&whitelist), "sec-ch-ua,user-agent");
}

#[test]
fn vary_header_present_on_response() {
    let web = web_pipeline(false);
    let data = processed(web.pipeline(), Evidence::default());
    let options = EndpointOptions::new(web.vary_whitelist().clone());
    let response = serve_json(&data, &TestRequest::default(), &options);
    // The test element accepts header.user-agent, and the JS builder accepts
    // header.host / header.protocol, so all three appear (sorted).
    let vary = response.header("Vary").unwrap();
    assert!(vary.split(',').any(|h| h == "user-agent"));
    assert!(vary.split(',').any(|h| h == "host"));
    // Sorted and comma-joined with no spaces.
    assert!(!vary.contains(' '));
}

#[test]
fn no_vary_header_when_whitelist_empty() {
    let web = web_pipeline(false);
    let data = processed(web.pipeline(), Evidence::default());
    // Supply an empty whitelist explicitly.
    let options = EndpointOptions::new(EvidenceKeyFilterWhitelist::new(Vec::<String>::new()));
    let response = serve_json(&data, &TestRequest::default(), &options);
    assert_eq!(response.header("Vary"), None);
}

// ----------------------------------------------------------------------------
// ETag + If-None-Match -> 304.
// ----------------------------------------------------------------------------

#[test]
fn etag_is_stable_and_present() {
    let web = web_pipeline(false);
    let evidence_a = page_evidence("abc");
    let evidence_b = page_evidence("abc");
    let data_a = processed(web.pipeline(), evidence_a);
    let data_b = processed(web.pipeline(), evidence_b);
    let options = EndpointOptions::new(web.vary_whitelist().clone());

    let resp_a = serve_json(&data_a, &TestRequest::default(), &options);
    let resp_b = serve_json(&data_b, &TestRequest::default(), &options);

    let etag_a = resp_a.header("ETag").unwrap();
    let etag_b = resp_b.header("ETag").unwrap();
    // Same evidence => same ETag, and it is a weak entity tag holding 16 hex
    // digits, because it stands for the evidence rather than the content
    // (RFC 9110 section 8.8.3).
    assert_eq!(etag_a, etag_b);
    let digits = etag_a
        .strip_prefix("W/\"")
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or_else(|| panic!("{etag_a} is not W/\"...\""));
    assert_eq!(digits.len(), 16, "{etag_a}");
    assert!(
        digits
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "{etag_a}"
    );
}

/// RFC 9110 sections 8.8.3.2 and 13.1.2: `If-None-Match` takes the weak
/// comparison, so the tag a `200` sent matches with or without `W/`, alone or
/// in a list, as does `*`.
#[test]
fn the_weak_etag_matches_in_every_form() {
    let web = web_pipeline(false);
    let options = EndpointOptions::new(web.vary_whitelist().clone());
    let serve = |if_none_match: Option<&str>| {
        let request = TestRequest {
            headers: if_none_match
                .map(|value| ("If-None-Match".to_owned(), value.to_owned()))
                .into_iter()
                .collect(),
            ..Default::default()
        };
        serve_javascript(
            &processed(web.pipeline(), page_evidence("abc")),
            &request,
            &options,
        )
    };

    let first = serve(None);
    assert_eq!(first.status, WebResponse::STATUS_OK);
    let etag = first
        .header("ETag")
        .expect("the 200 has an ETag")
        .to_owned();
    let strong = etag.strip_prefix("W/").expect("a weak ETag").to_owned();

    for matching in [
        etag.clone(),
        strong.clone(),
        format!("\"other\", {etag}"),
        format!("W/\"other\", {strong}"),
        "*".to_owned(),
    ] {
        let response = serve(Some(&matching));
        assert_eq!(
            response.status,
            WebResponse::STATUS_NOT_MODIFIED,
            "{matching}"
        );
        assert_eq!(response.header("ETag"), Some(etag.as_str()), "{matching}");
    }
    for other in ["W/\"other\"".to_owned(), format!("w/{strong}")] {
        assert_eq!(
            serve(Some(&other)).status,
            WebResponse::STATUS_OK,
            "{other}"
        );
    }
}

/// The Sequence element adds a new session id to the evidence of each request
/// that has none, but the ETag is worked out from the evidence the request
/// supplied. So two requests with the same evidence and no session id get the
/// same tag, and the second, sending it back, gets a `304` from both
/// endpoints.
#[test]
fn requests_without_a_session_id_get_the_same_etag_and_a_304() {
    type Serve = fn(&FlowData, &dyn RequestData, &EndpointOptions) -> WebResponse;
    let web = web_pipeline(false);
    let options = EndpointOptions::new(web.vary_whitelist().clone());
    let request = |if_none_match: Option<&str>| TestRequest {
        headers: if_none_match
            .map(|value| ("If-None-Match".to_owned(), value.to_owned()))
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let evidence = || Evidence::builder().add("header.user-agent", "abc").build();
    let endpoints: [(&str, Serve); 2] = [("javascript", serve_javascript), ("json", serve_json)];

    for (name, serve) in endpoints {
        let first_data = processed(web.pipeline(), evidence());
        let second_data = processed(web.pipeline(), evidence());
        let session_id = |data: &FlowData| {
            data.evidence()
                .get("query.session-id")
                .map(str::to_owned)
                .expect("the Sequence element added a session id")
        };
        assert_ne!(session_id(&first_data), session_id(&second_data), "{name}");

        let first = serve(&first_data, &request(None), &options);
        assert_eq!(first.status, WebResponse::STATUS_OK, "{name}");
        let etag = first
            .header("ETag")
            .expect("the 200 has an ETag")
            .to_owned();
        let second = serve(&second_data, &request(Some(&etag)), &options);

        assert_eq!(
            second.status,
            WebResponse::STATUS_NOT_MODIFIED,
            "{name} {etag}"
        );
        assert_eq!(second.header("ETag"), Some(etag.as_str()), "{name}");
    }
}

#[test]
fn different_evidence_gives_different_etag() {
    let web = web_pipeline(false);
    let options = EndpointOptions::new(web.vary_whitelist().clone());

    let data_a = processed(web.pipeline(), page_evidence("abc"));
    let data_b = processed(web.pipeline(), page_evidence("xyz"));
    let etag_a = serve_json(&data_a, &TestRequest::default(), &options)
        .header("ETag")
        .unwrap()
        .to_owned();
    let etag_b = serve_json(&data_b, &TestRequest::default(), &options)
        .header("ETag")
        .unwrap()
        .to_owned();
    assert_ne!(etag_a, etag_b);
}

/// RFC 9110 section 15.4.5: a 304 carries the `Cache-Control`, `Vary` and
/// `ETag` a 200 to the same request would, and the CORS header comes with
/// them so a script that reads the 304 passes the CORS check.
#[test]
fn a_304_carries_the_fields_the_200_would() {
    let web = web_pipeline(false);
    let options = EndpointOptions::new(web.vary_whitelist().clone());
    let request = |etag: Option<&str>| TestRequest {
        headers: std::iter::once(("Origin".to_owned(), "https://example.com".to_owned()))
            .chain(etag.map(|etag| ("If-None-Match".to_owned(), etag.to_owned())))
            .collect(),
        ..Default::default()
    };
    let evidence = || page_evidence("abc");

    let first = serve_json(
        &processed(web.pipeline(), evidence()),
        &request(None),
        &options,
    );
    let etag = first
        .header("ETag")
        .expect("the 200 has an ETag")
        .to_owned();
    let second = serve_json(
        &processed(web.pipeline(), evidence()),
        &request(Some(&etag)),
        &options,
    );

    assert_eq!(second.status, WebResponse::STATUS_NOT_MODIFIED);
    assert!(second.is_not_modified());
    assert!(second.body.is_empty());
    for name in [
        "Cache-Control",
        "Vary",
        "ETag",
        "Access-Control-Allow-Origin",
    ] {
        assert!(first.header(name).is_some(), "the 200 has {name}");
        assert_eq!(second.header(name), first.header(name), "{name}");
    }
    assert_eq!(
        second.header("Cache-Control"),
        Some("private, max-age=1800")
    );
    for name in ["Content-Type", "Content-Length"] {
        assert_eq!(second.header(name), None, "the 304 has no {name}");
    }
    assert_eq!(second.headers.len(), 4, "{:?}", second.headers);
}

#[test]
fn a_304_without_the_extra_headers_carries_none() {
    let web = web_pipeline(false);
    let options = EndpointOptions::new(web.vary_whitelist().clone()).without_extra_headers();
    let evidence = || page_evidence("abc");
    let data = processed(web.pipeline(), evidence());
    let request = TestRequest {
        headers: vec![(
            "If-None-Match".to_owned(),
            compute_etag(&data, data.evidence_key_filter()),
        )],
        ..Default::default()
    };

    let response = serve_json(&data, &request, &options);

    assert_eq!(response.status, WebResponse::STATUS_NOT_MODIFIED);
    assert!(response.headers.is_empty(), "{:?}", response.headers);
}

#[test]
fn non_matching_if_none_match_returns_200() {
    let web = web_pipeline(false);
    let options = EndpointOptions::new(web.vary_whitelist().clone());
    let data = processed(web.pipeline(), Evidence::default());
    for method in [None, Some("POST")] {
        let conditional = TestRequest {
            headers: vec![("If-None-Match".into(), "\"not-the-tag\"".into())],
            method: method.map(str::to_owned),
            ..Default::default()
        };
        let response = serve_json(&data, &conditional, &options);
        assert_eq!(response.status, WebResponse::STATUS_OK, "{method:?}");
    }
}

/// RFC 9110 section 13.1.2: a field that is neither `*` nor a list of entity
/// tags makes the condition true, so both endpoints give `200` with the body,
/// even when the field holds the tag. A field over two lines is read whole
/// (section 5.3), so the tag on the second line matches, and a second line
/// that is not a tag stops the first matching.
#[test]
fn an_if_none_match_that_is_not_a_list_of_tags_gets_the_body() {
    let web = web_pipeline(false);
    let options = EndpointOptions::new(web.vary_whitelist().clone());
    let data = processed(web.pipeline(), page_evidence("abc"));
    let etag = compute_etag(&data, data.evidence_key_filter());
    let cases = [
        (vec![format!("{etag}, other")], false),
        (vec![format!("\"other\" {etag}")], false),
        (vec![format!("other, {etag}")], false),
        (vec![format!("\"oth er\", {etag}")], false),
        (vec![format!("{etag}, *")], false),
        (vec![format!("\"other\",\t{etag}")], true),
        (vec!["\"other\"".to_owned(), etag.clone()], true),
        (vec![etag.clone(), "other".to_owned()], false),
    ];
    for (lines, matches) in &cases {
        for method in [None, Some("POST")] {
            let request = TestRequest {
                headers: lines
                    .iter()
                    .map(|line| ("If-None-Match".to_owned(), line.clone()))
                    .collect(),
                method: method.map(str::to_owned),
                ..Default::default()
            };
            let status = match (matches, method) {
                (false, _) => WebResponse::STATUS_OK,
                (true, None) => WebResponse::STATUS_NOT_MODIFIED,
                (true, Some(_)) => WebResponse::STATUS_PRECONDITION_FAILED,
            };
            for response in [
                serve_json(&data, &request, &options),
                serve_javascript(&data, &request, &options),
            ] {
                assert_eq!(response.status, status, "{lines:?} {method:?}");
                assert_eq!(
                    response.body.is_empty(),
                    status != WebResponse::STATUS_OK,
                    "{lines:?} {method:?}"
                );
            }
        }
    }
}

/// RFC 9110 section 13.1.2: a matching `If-None-Match` gets a `304` on `GET`
/// or `HEAD` and a `412` on any other method. Method names are case-sensitive
/// (section 9.1), so `get` is not `GET`. A request that gives no method is
/// treated as `GET`.
#[test]
fn a_matching_if_none_match_gets_a_304_only_on_get_or_head() {
    let web = web_pipeline(false);
    let options = EndpointOptions::new(web.vary_whitelist().clone());
    let data = processed(web.pipeline(), page_evidence("abc"));
    let etag = compute_etag(&data, data.evidence_key_filter());
    let cases = [
        (None, WebResponse::STATUS_NOT_MODIFIED),
        (Some("GET"), WebResponse::STATUS_NOT_MODIFIED),
        (Some("HEAD"), WebResponse::STATUS_NOT_MODIFIED),
        (Some("POST"), WebResponse::STATUS_PRECONDITION_FAILED),
        (Some("PUT"), WebResponse::STATUS_PRECONDITION_FAILED),
        (Some("get"), WebResponse::STATUS_PRECONDITION_FAILED),
    ];
    for (method, status) in cases {
        let request = TestRequest {
            headers: vec![("If-None-Match".to_owned(), etag.clone())],
            method: method.map(str::to_owned),
            ..Default::default()
        };
        for response in [
            serve_json(&data, &request, &options),
            serve_javascript(&data, &request, &options),
        ] {
            assert_eq!(response.status, status, "{method:?}");
            assert!(response.body.is_empty(), "{method:?}");
        }
    }
}

/// The `412` carries only the CORS header, so a script on another origin can
/// read the status, and none of the caching fields of a `200` or `304`.
#[test]
fn a_412_carries_only_the_cors_header() {
    let web = web_pipeline(false);
    let options = EndpointOptions::new(web.vary_whitelist().clone());
    let request = |etag: Option<&str>| TestRequest {
        headers: std::iter::once(("Origin".to_owned(), "https://example.com".to_owned()))
            .chain(etag.map(|etag| ("If-None-Match".to_owned(), etag.to_owned())))
            .collect(),
        method: Some("POST".to_owned()),
        ..Default::default()
    };
    let evidence = || page_evidence("abc");

    let first = serve_json(
        &processed(web.pipeline(), evidence()),
        &request(None),
        &options,
    );
    assert_eq!(first.status, WebResponse::STATUS_OK);
    let etag = first
        .header("ETag")
        .expect("the 200 has an ETag")
        .to_owned();
    let second = serve_json(
        &processed(web.pipeline(), evidence()),
        &request(Some(&etag)),
        &options,
    );

    assert_eq!(second.status, WebResponse::STATUS_PRECONDITION_FAILED);
    assert!(second.body.is_empty());
    assert_eq!(
        second.headers,
        vec![(
            "Access-Control-Allow-Origin".to_owned(),
            "https://example.com".to_owned()
        )]
    );
}

#[test]
fn a_412_without_the_extra_headers_carries_none() {
    let web = web_pipeline(false);
    let options = EndpointOptions::new(web.vary_whitelist().clone()).without_extra_headers();
    let data = processed(web.pipeline(), page_evidence("abc"));
    let request = TestRequest {
        headers: vec![
            ("Origin".to_owned(), "https://example.com".to_owned()),
            (
                "If-None-Match".to_owned(),
                compute_etag(&data, data.evidence_key_filter()),
            ),
        ],
        method: Some("POST".to_owned()),
        ..Default::default()
    };

    let response = serve_json(&data, &request, &options);

    assert_eq!(response.status, WebResponse::STATUS_PRECONDITION_FAILED);
    assert!(response.headers.is_empty(), "{:?}", response.headers);
}

/// Set in the environment of the second process the ETag test starts, which
/// then prints its ETag rather than comparing.
const ETAG_CHILD_VARIABLE: &str = "_51DEGREES_WEB_ETAG_CHILD";

#[test]
fn the_same_evidence_gives_the_same_etag_in_another_process() {
    // A browser revalidates against whichever process answers, which is a
    // different one after a restart or behind a load balancer, so the ETag for
    // the same evidence must not depend on the process that computed it.
    let web = web_pipeline(false);
    let options = EndpointOptions::new(web.vary_whitelist().clone());
    let data = processed(web.pipeline(), page_evidence("abc"));
    let response = serve_json(&data, &TestRequest::default(), &options);
    let etag = response.header("ETag").expect("an ETag").to_owned();

    if std::env::var_os(ETAG_CHILD_VARIABLE).is_some() {
        println!("etag={etag}");
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().expect("the test binary"))
        .args([
            "the_same_evidence_gives_the_same_etag_in_another_process",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(ETAG_CHILD_VARIABLE, "1")
        .output()
        .expect("the test binary runs a second time");
    // The test harness prints the test's name on the same line as its output,
    // so the ETag is found anywhere in the second process's output.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let other = stdout
        .split_once("etag=")
        .and_then(|(_, rest)| rest.split_whitespace().next())
        .unwrap_or_else(|| panic!("the second process printed no ETag:\n{stdout}"));
    assert_eq!(
        other, etag,
        "the same evidence gave a different ETag in another process"
    );
}

// ----------------------------------------------------------------------------
// Access-Control-Allow-Origin echo (present / absent / null).
// ----------------------------------------------------------------------------

#[test]
fn acao_echoes_present_origin() {
    let web = web_pipeline(false);
    let options = EndpointOptions::new(web.vary_whitelist().clone());
    let request = TestRequest {
        headers: vec![("Origin".into(), "https://example.com".into())],
        ..Default::default()
    };
    let data = processed(web.pipeline(), Evidence::default());
    let response = serve_json(&data, &request, &options);
    assert_eq!(
        response.header("Access-Control-Allow-Origin"),
        Some("https://example.com")
    );
}

#[test]
fn acao_absent_when_no_origin() {
    let web = web_pipeline(false);
    let options = EndpointOptions::new(web.vary_whitelist().clone());
    let data = processed(web.pipeline(), Evidence::default());
    let response = serve_json(&data, &TestRequest::default(), &options);
    assert_eq!(response.header("Access-Control-Allow-Origin"), None);
}

#[test]
fn acao_absent_when_origin_is_null() {
    let web = web_pipeline(false);
    let options = EndpointOptions::new(web.vary_whitelist().clone());
    let request = TestRequest {
        headers: vec![("Origin".into(), "null".into())],
        ..Default::default()
    };
    let data = processed(web.pipeline(), Evidence::default());
    let response = serve_json(&data, &request, &options);
    assert_eq!(response.header("Access-Control-Allow-Origin"), None);
}

// ----------------------------------------------------------------------------
// Evidence population honouring the filter.
// ----------------------------------------------------------------------------

#[test]
fn evidence_population_honours_filter() {
    // A filter that accepts a header, a cookie, a query param, the client IP and
    // the protocol, but not an unrelated header.
    let filter = EvidenceKeyFilterWhitelist::new([
        "header.user-agent",
        "cookie.session",
        "query.fod-js-object-name",
        "server.client-ip",
        "header.protocol",
    ]);

    let request = TestRequest {
        headers: vec![
            ("User-Agent".into(), "agent".into()),
            ("X-Ignored".into(), "nope".into()),
        ],
        cookies: vec![("session".into(), "s1".into())],
        query: vec![("fod-js-object-name".into(), "fod".into())],
        // A form field folded into the query prefix.
        form: vec![("fod-js-object-name".into(), "fromform".into())],
        client_ip: Some("1.2.3.4".into()),
        https: true,
        method: Some("POST".into()),
    };

    let evidence = build_evidence(&request, &filter);

    assert_eq!(evidence.get("header.user-agent"), Some("agent"));
    assert_eq!(evidence.get("cookie.session"), Some("s1"));
    // The form value overwrites the query value (last writer wins).
    assert_eq!(evidence.get("query.fod-js-object-name"), Some("fromform"));
    assert_eq!(evidence.get("server.client-ip"), Some("1.2.3.4"));
    assert_eq!(evidence.get("header.protocol"), Some("https"));
    // The unrelated header was filtered out.
    assert_eq!(evidence.get("header.x-ignored"), None);
}

#[test]
fn evidence_population_skips_client_ip_when_not_in_filter() {
    let filter = EvidenceKeyFilterWhitelist::new(["header.user-agent"]);
    let request = TestRequest {
        headers: vec![("User-Agent".into(), "agent".into())],
        client_ip: Some("9.9.9.9".into()),
        ..Default::default()
    };
    let evidence = build_evidence(&request, &filter);
    assert_eq!(evidence.get("header.user-agent"), Some("agent"));
    assert_eq!(evidence.get("server.client-ip"), None);
}

// ----------------------------------------------------------------------------
// SetHeaders application helper.
// ----------------------------------------------------------------------------

#[test]
fn set_headers_read_from_flow_data() {
    let web = web_pipeline(false);
    // The element only emits its SetHeader property when it actually ran, which
    // it does here. Provide the User-Agent so device data is present.
    let evidence = Evidence::builder()
        .add("header.user-agent", "agent")
        .build();
    let data = processed(web.pipeline(), evidence);

    let headers = response_headers(&data);
    assert!(
        headers
            .iter()
            .any(|(name, value)| name == "Accept-CH" && value == "Sec-CH-UA"),
        "expected Accept-CH: Sec-CH-UA, got {headers:?}"
    );
}

#[test]
fn apply_set_headers_appends_to_existing() {
    let mut existing = vec![("Accept-CH".to_owned(), "Sec-CH-UA".to_owned())];
    let to_set = vec![("Accept-CH".to_owned(), "Sec-CH-UA-Platform".to_owned())];
    apply_set_headers(&mut existing, &to_set);
    assert_eq!(existing.len(), 1);
    assert_eq!(existing[0].1, "Sec-CH-UA, Sec-CH-UA-Platform");
}

#[test]
fn apply_set_headers_adds_new_header() {
    let mut existing: Vec<(String, String)> = Vec::new();
    let to_set = vec![("Accept-CH".to_owned(), "Sec-CH-UA".to_owned())];
    apply_set_headers(&mut existing, &to_set);
    assert_eq!(
        existing,
        vec![("Accept-CH".to_owned(), "Sec-CH-UA".to_owned())]
    );
}

#[test]
fn apply_set_headers_case_insensitive_match() {
    let mut existing = vec![("accept-ch".to_owned(), "Sec-CH-UA".to_owned())];
    let to_set = vec![("Accept-CH".to_owned(), "Sec-CH-UA-Mobile".to_owned())];
    apply_set_headers(&mut existing, &to_set);
    assert_eq!(
        existing.len(),
        1,
        "should match existing case-insensitively"
    );
    assert_eq!(existing[0].1, "Sec-CH-UA, Sec-CH-UA-Mobile");
}

// ----------------------------------------------------------------------------
// Vary lists every header an element names, not only a fixed set.
// ----------------------------------------------------------------------------

#[test]
fn vary_lists_a_header_only_the_element_knows() {
    let element = TestElement {
        filter: EvidenceKeyFilterWhitelist::new([
            "header.user-agent",
            "header.x-forwarded-for",
            "query.client-ip",
        ]),
        properties: Vec::new(),
        fail: false,
    };
    let web = WebPipeline::build(vec![Arc::new(element)], WebIntegrationOptions::default())
        .expect("web pipeline builds");
    assert!(web.vary_whitelist().include("header.x-forwarded-for"));
    assert!(!web.vary_whitelist().include("query.client-ip"));

    let data = processed(web.pipeline(), Evidence::default());
    let options = EndpointOptions::new(web.vary_whitelist().clone());
    let response = serve_json(&data, &TestRequest::default(), &options);
    let vary = response.header("Vary").unwrap();
    assert!(vary.split(',').any(|h| h == "x-forwarded-for"), "{vary}");
    assert!(vary.split(',').any(|h| h == "user-agent"), "{vary}");
}

// ----------------------------------------------------------------------------
// Vary after an element learns its accepted evidence.
// ----------------------------------------------------------------------------

/// An element that learns the evidence it accepts after it is built, as the
/// cloud request engine does after a transient discovery failure. Its filter
/// is complete once `learned` is set. `later` stands for a filter that changes
/// after it said it was complete, so a test can see whether the list was kept.
#[derive(Default)]
struct LearningElement {
    learned: OnceLock<EvidenceKeyFilterWhitelist>,
    later: OnceLock<EvidenceKeyFilterWhitelist>,
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
        self.later
            .get()
            .or(self.learned.get())
            .unwrap_or(&self.nothing)
    }
    fn is_evidence_key_filter_complete(&self) -> bool {
        self.learned.get().is_some()
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

/// The `Vary` header names of a JSON response served with `options`.
fn vary_names(web: &WebPipeline, options: &EndpointOptions) -> Vec<String> {
    let data = processed(web.pipeline(), Evidence::default());
    serve_json(&data, &TestRequest::default(), options)
        .header("Vary")
        .map(|vary| vary.split(',').map(str::to_owned).collect())
        .unwrap_or_default()
}

#[test]
fn vary_gains_the_headers_an_element_learns_after_the_build() {
    let element = Arc::new(LearningElement::default());
    let web = WebPipeline::build(vec![element.clone()], WebIntegrationOptions::default())
        .expect("web pipeline builds");
    let options = web.endpoint_options();
    let copy = EndpointOptions::new(web.vary_whitelist().clone());

    let before = vary_names(&web, &options);
    assert!(before.iter().any(|h| h == "host"), "{before:?}");
    assert!(!before.iter().any(|h| h == "x-learned"), "{before:?}");
    assert!(!web.vary_whitelist().include("header.x-learned"));

    element
        .learned
        .set(EvidenceKeyFilterWhitelist::new(["header.x-learned"]))
        .unwrap();

    let after = vary_names(&web, &options);
    assert!(after.iter().any(|h| h == "x-learned"), "{after:?}");
    assert!(after.iter().any(|h| h == "host"), "{after:?}");
    assert!(web.vary_whitelist().include("header.x-learned"));
    assert!(options
        .current_vary_whitelist()
        .is_some_and(|list| list.include("header.x-learned")));

    // A copy of the list taken before the element learned stays as it was.
    let copied = vary_names(&web, &copy);
    assert!(!copied.iter().any(|h| h == "x-learned"), "{copied:?}");
}

#[test]
fn vary_is_kept_once_every_element_is_complete() {
    let element = Arc::new(LearningElement::default());
    let web = WebPipeline::build(vec![element.clone()], WebIntegrationOptions::default())
        .expect("web pipeline builds");
    let options = web.endpoint_options();
    element
        .learned
        .set(EvidenceKeyFilterWhitelist::new(["header.x-learned"]))
        .unwrap();
    let first = vary_names(&web, &options);
    assert!(first.iter().any(|h| h == "x-learned"), "{first:?}");

    // The list is collected once after every filter is complete, so a later
    // change to a filter does not reach it.
    element
        .later
        .set(EvidenceKeyFilterWhitelist::new(["header.x-later"]))
        .unwrap();
    assert_eq!(vary_names(&web, &options), first);
    assert!(!web.vary_whitelist().include("header.x-later"));
}

#[test]
fn a_supplied_vary_list_is_used_in_place_of_the_pipelines() {
    let web = web_pipeline(false);
    let mut options = web.endpoint_options();
    options.vary_whitelist = Some(EvidenceKeyFilterWhitelist::new(["header.x-mine"]));
    assert_eq!(vary_names(&web, &options), vec!["x-mine".to_owned()]);
}

#[test]
fn endpoint_options_give_the_same_vary_as_the_list() {
    let web = web_pipeline(false);
    let from_list = EndpointOptions::new(web.vary_whitelist().clone());
    let names = vary_names(&web, &web.endpoint_options());
    assert!(names.iter().any(|h| h == "user-agent"), "{names:?}");
    assert_eq!(names, vary_names(&web, &from_list));
}

// ----------------------------------------------------------------------------
// The response to a failed request.
// ----------------------------------------------------------------------------

/// A device element that fails while `failing` is set, standing in for an
/// engine whose cloud call failed on one request and worked on the next.
struct FlakyElement {
    inner: TestElement,
    failing: Arc<std::sync::atomic::AtomicBool>,
}

impl FlowElement for FlakyElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        if self.failing.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(fiftyone_pipeline_core::Error::CloudRequest {
                status_code: 0,
                retry_after_seconds: None,
                message: "the request timed out".to_owned(),
            });
        }
        self.inner.process(data)
    }
    fn data_key(&self) -> &str {
        self.inner.data_key()
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        self.inner.evidence_key_filter()
    }
    fn properties(&self) -> &[PropertyMetaData] {
        self.inner.properties()
    }
}

#[test]
fn the_response_to_a_failed_request_is_not_kept_once_processing_recovers() {
    // A browser keeps a response it may store and revalidates it with the
    // response's ETag on the next page view. The response to a failed request
    // must not be confirmed by a 304 once processing works again, or the
    // browser goes on using the failed result for as long as the evidence
    // stays the same.
    let failing = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let web = WebPipeline::build(
        vec![Arc::new(FlakyElement {
            inner: TestElement::new(false),
            failing: Arc::clone(&failing),
        })],
        WebIntegrationOptions::default(),
    )
    .expect("web pipeline builds");
    let options = EndpointOptions::new(web.vary_whitelist().clone());
    let evidence = || page_evidence("abc");

    let failed = serve_javascript(
        &processed(web.pipeline(), evidence()),
        &TestRequest::default(),
        &options,
    );
    assert_eq!(failed.status, WebResponse::STATUS_OK);
    assert_eq!(failed.header("Cache-Control"), Some("private, no-cache"));
    assert!(
        failed.header("ETag").is_none(),
        "the response to a failed request carries no validator"
    );

    failing.store(false, std::sync::atomic::Ordering::SeqCst);
    let revalidation = TestRequest {
        headers: failed
            .header("ETag")
            .map(|etag| vec![("If-None-Match".to_owned(), etag.to_owned())])
            .unwrap_or_default(),
        ..Default::default()
    };
    let recovered = serve_javascript(
        &processed(web.pipeline(), evidence()),
        &revalidation,
        &options,
    );
    assert_eq!(
        recovered.status,
        WebResponse::STATUS_OK,
        "the browser was told to keep the response to the failed request"
    );
    assert!(
        recovered
            .body_str()
            .unwrap_or_default()
            .contains("ismobile"),
        "the response once processing recovers carries the device data"
    );
}

#[test]
fn a_304_while_processing_is_failing_carries_the_fields_of_a_good_200() {
    // A browser that holds a good response revalidates it with its ETag.
    // The ETag depends on the evidence alone, so the 304 carries the fields
    // of a 200 whose processing recorded no errors, while processing is
    // failing as at any other time, and the browser keeps the good response
    // and its max-age (web integration specification, the 304 response).
    let failing = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let web = WebPipeline::build(
        vec![Arc::new(FlakyElement {
            inner: TestElement::new(false),
            failing: Arc::clone(&failing),
        })],
        WebIntegrationOptions::default(),
    )
    .expect("web pipeline builds");
    let options = EndpointOptions::new(web.vary_whitelist().clone());
    let evidence = || page_evidence("abc");

    let good = serve_javascript(
        &processed(web.pipeline(), evidence()),
        &TestRequest::default(),
        &options,
    );
    let etag = good.header("ETag").expect("a good response has an ETag");
    let revalidation = TestRequest {
        headers: vec![("If-None-Match".to_owned(), etag.to_owned())],
        ..Default::default()
    };

    failing.store(true, std::sync::atomic::Ordering::SeqCst);
    let during = serve_javascript(
        &processed(web.pipeline(), evidence()),
        &revalidation,
        &options,
    );
    assert_eq!(during.status, WebResponse::STATUS_NOT_MODIFIED);
    assert_eq!(
        during.header("Cache-Control"),
        Some("private, max-age=1800")
    );
    assert_eq!(during.header("ETag"), Some(etag));
    assert!(during.header("Vary").is_some());
    assert_eq!(during.header("Vary"), good.header("Vary"));

    failing.store(false, std::sync::atomic::Ordering::SeqCst);
    let after = serve_javascript(
        &processed(web.pipeline(), evidence()),
        &revalidation,
        &options,
    );
    assert_eq!(after.status, WebResponse::STATUS_NOT_MODIFIED);
    assert_eq!(after.header("Cache-Control"), Some("private, max-age=1800"));
    assert_eq!(after.header("ETag"), Some(etag));
}

/// `EndpointOptions` is `Send`, `Sync`, `UnwindSafe` and `RefUnwindSafe`, as
/// removing an auto trait from a public type breaks callers. This test fails
/// to compile if it loses one.
#[test]
fn endpoint_options_keep_their_auto_traits() {
    fn has_auto_traits<T: Send + Sync + std::panic::UnwindSafe + std::panic::RefUnwindSafe>() {}
    has_auto_traits::<EndpointOptions>();
}

/// `serve_if_none_match` answers a request whose `If-None-Match` matches
/// from a flow data that was never processed, with the headers of a good
/// `200`, and leaves the flow data unprocessed. A request with no
/// `If-None-Match`, or one that does not match, gets `None`.
#[test]
fn a_matching_request_is_answered_before_processing() {
    let web = web_pipeline(false);
    let options = web.endpoint_options();
    let request = |if_none_match: Option<&str>, method: Option<&str>| TestRequest {
        headers: if_none_match
            .map(|tag| vec![("If-None-Match".to_owned(), tag.to_owned())])
            .unwrap_or_default(),
        method: method.map(str::to_owned),
        ..Default::default()
    };
    let good = serve_javascript(
        &processed(web.pipeline(), page_evidence("abc")),
        &request(None, None),
        &options,
    );
    let etag = good.header("ETag").expect("a good response has an ETag");
    assert!(good.header("Vary").is_some());

    let mut unprocessed = web.pipeline().create_flow_data_with(page_evidence("abc"));
    assert!(serve_if_none_match(&unprocessed, &request(None, None), &options).is_none());
    assert!(serve_if_none_match(&unprocessed, &request(Some("W/\"0\""), None), &options).is_none());

    for method in [None, Some("GET"), Some("HEAD")] {
        let answer = serve_if_none_match(&unprocessed, &request(Some(etag), method), &options)
            .expect("a matching request is answered");
        assert_eq!(
            answer.status,
            WebResponse::STATUS_NOT_MODIFIED,
            "{method:?}"
        );
        assert!(answer.body.is_empty(), "{method:?}");
        for name in ["Cache-Control", "Vary", "ETag"] {
            assert_eq!(answer.header(name), good.header(name), "{name} {method:?}");
        }
    }
    let post = serve_if_none_match(&unprocessed, &request(Some(etag), Some("POST")), &options)
        .expect("a matching POST is answered");
    assert_eq!(post.status, WebResponse::STATUS_PRECONDITION_FAILED);
    assert!(post.headers.is_empty(), "no Origin, so no header");

    unprocessed
        .process()
        .expect("the flow data was not processed before");
}
