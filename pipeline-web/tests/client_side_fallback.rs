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

//! The client-side endpoints when a builder left no data for a request: the
//! records logged at warning level and above, the body and the caching
//! headers.
//!
//! Each record is kept with the thread that logged it, because the tests in
//! this binary run in parallel and each counts only its own records.

use std::any::Any;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, ThreadId};

use fiftyone_javascript_builder::{
    JavaScriptBuilderElement, JAVASCRIPT_BUILDER_DATA_KEY, JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY,
};
use fiftyone_json_builder::{
    JsonBuilderElement, JSON_BUILDER_DATA_KEY, JSON_BUILDER_ELEMENT_DATA_KEY,
};
use fiftyone_pipeline_core::{
    ElementData, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    MapElementData, NoValueError, Pipeline, PropertyMetaData, PropertyValue, PropertyValueType,
    Result, TypedKey,
};
use fiftyone_pipeline_web::{
    serve_javascript, serve_json, EndpointOptions, RequestData, WebIntegrationOptions, WebPipeline,
    WebResponse, CACHE_CONTROL_CACHEABLE, CACHE_CONTROL_NO_CACHE,
};

// ----------------------------------------------------------------------------
// A logger keeping each record with the thread that logged it.
// ----------------------------------------------------------------------------

struct CapturingLogger {
    entries: Mutex<Vec<(ThreadId, log::Level, String)>>,
}

impl log::Log for CapturingLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            self.entries.lock().unwrap().push((
                thread::current().id(),
                record.level(),
                record.args().to_string(),
            ));
        }
    }

    fn flush(&self) {}
}

fn logger() -> &'static CapturingLogger {
    static LOGGER: OnceLock<&'static CapturingLogger> = OnceLock::new();
    LOGGER.get_or_init(|| {
        let logger: &'static CapturingLogger = Box::leak(Box::new(CapturingLogger {
            entries: Mutex::new(Vec::new()),
        }));
        log::set_logger(logger).expect("no other logger is installed in this binary");
        log::set_max_level(log::LevelFilter::Warn);
        logger
    })
}

/// The records the current thread has logged, from the `from`th on.
fn records_since(from: usize) -> Vec<(log::Level, String)> {
    let me = thread::current().id();
    logger()
        .entries
        .lock()
        .unwrap()
        .iter()
        .filter(|(id, _, _)| *id == me)
        .skip(from)
        .map(|(_, level, message)| (*level, message.clone()))
        .collect()
}

/// How many records the current thread has logged so far.
fn mark() -> usize {
    records_since(0).len()
}

/// The record logged when the builder with this data key left no data.
fn missing(data_key: &str) -> (log::Level, String) {
    (
        log::Level::Error,
        format!("Failed to get data from element '{data_key}', so the response is not cached."),
    )
}

// ----------------------------------------------------------------------------
// Elements.
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

/// Adds one property, `ismobile`, under the data key `device`.
struct DeviceElement {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

impl DeviceElement {
    fn new() -> Self {
        DeviceElement {
            filter: EvidenceKeyFilterWhitelist::new(["header.user-agent"]),
            properties: vec![PropertyMetaData::new(
                "ismobile",
                "device",
                PropertyValueType::Bool,
            )],
        }
    }
}

impl FlowElement for DeviceElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        data.get_or_add(TypedKey::<TestData>::new("device"), || {
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

/// Puts data of another type under a builder's data key, so the builder fails
/// to add its own and leaves no data.
struct ClashElement {
    builder_key: &'static str,
    filter: EvidenceKeyFilterWhitelist,
}

impl ClashElement {
    fn new(builder_key: &'static str) -> Self {
        ClashElement {
            builder_key,
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        }
    }
}

impl FlowElement for ClashElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        data.get_or_add(TypedKey::<TestData>::new(self.builder_key), || {
            TestData(MapElementData::new())
        })?;
        Ok(())
    }
    fn data_key(&self) -> &str {
        "clash"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

// ----------------------------------------------------------------------------
// Requests and responses.
// ----------------------------------------------------------------------------

#[derive(Default)]
struct TestRequest;

impl RequestData for TestRequest {
    fn headers(&self) -> Vec<(String, String)> {
        Vec::new()
    }
    fn cookies(&self) -> Vec<(String, String)> {
        Vec::new()
    }
    fn query_params(&self) -> Vec<(String, String)> {
        Vec::new()
    }
    fn client_ip(&self) -> Option<String> {
        None
    }
    fn is_https(&self) -> bool {
        true
    }
}

fn header<'a>(response: &'a WebResponse, name: &str) -> Option<&'a str> {
    response
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn body(response: &WebResponse) -> String {
    String::from_utf8(response.body.clone()).expect("the body is UTF-8")
}

/// A web pipeline holding the device element, then the given elements.
fn web_pipeline(clashes: &[&'static str], options: WebIntegrationOptions) -> WebPipeline {
    let mut elements: Vec<Arc<dyn FlowElement>> = vec![Arc::new(DeviceElement::new())];
    for key in clashes {
        elements.push(Arc::new(ClashElement::new(key)));
    }
    WebPipeline::build(elements, options).expect("the web pipeline builds")
}

fn processed(pipeline: &Arc<Pipeline>) -> FlowData {
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", "test-agent")
            .build(),
    );
    data.process().expect("errors are suppressed");
    data
}

fn options(web: &WebPipeline) -> EndpointOptions {
    EndpointOptions::new(web.vary_whitelist().clone())
}

/// The response is served with `no-cache` and no `ETag`.
fn assert_not_cached(response: &WebResponse) {
    assert_eq!(response.status, WebResponse::STATUS_OK);
    assert_eq!(
        header(response, "Cache-Control"),
        Some(CACHE_CONTROL_NO_CACHE)
    );
    assert_eq!(header(response, "ETag"), None);
}

// ----------------------------------------------------------------------------
// Tests.
// ----------------------------------------------------------------------------

#[test]
fn the_web_pipeline_holds_its_builders_as_resources() {
    let web = web_pipeline(&[], WebIntegrationOptions::default());
    assert!(web.pipeline().resource::<JsonBuilderElement>().is_some());
    assert!(web
        .pipeline()
        .resource::<JavaScriptBuilderElement>()
        .is_some());

    let without = web_pipeline(
        &[],
        WebIntegrationOptions {
            client_side_evidence_enabled: false,
            ..WebIntegrationOptions::default()
        },
    );
    assert!(without
        .pipeline()
        .resource::<JsonBuilderElement>()
        .is_none());
    assert!(without
        .pipeline()
        .resource::<JavaScriptBuilderElement>()
        .is_none());
}

#[test]
fn builders_that_left_data_log_nothing_and_are_cached() {
    let web = web_pipeline(&[], WebIntegrationOptions::default());
    let data = processed(web.pipeline());
    let from = mark();

    let json = serve_json(&data, &TestRequest, &options(&web));
    let javascript = serve_javascript(&data, &TestRequest, &options(&web));

    assert_eq!(records_since(from), Vec::new());
    for response in [&json, &javascript] {
        assert_eq!(
            header(response, "Cache-Control"),
            Some(CACHE_CONTROL_CACHEABLE)
        );
        assert!(header(response, "ETag").is_some());
    }
    let expected = data.get(JSON_BUILDER_DATA_KEY).expect("JSON").json();
    assert_eq!(body(&json), expected);
    let expected = data
        .get(JAVASCRIPT_BUILDER_DATA_KEY)
        .expect("JavaScript")
        .javascript();
    assert_eq!(body(&javascript), expected);
}

#[test]
fn a_json_builder_that_left_no_data_is_logged_and_its_json_built_again() {
    let web = web_pipeline(
        &[JSON_BUILDER_ELEMENT_DATA_KEY],
        WebIntegrationOptions::default(),
    );
    let data = processed(web.pipeline());
    assert!(data.get(JSON_BUILDER_DATA_KEY).is_none());
    let from = mark();

    let response = serve_json(&data, &TestRequest, &options(&web));

    assert_eq!(
        records_since(from),
        vec![missing(JSON_BUILDER_ELEMENT_DATA_KEY)]
    );
    assert_not_cached(&response);
    let builder = web
        .pipeline()
        .resource::<JsonBuilderElement>()
        .expect("the JSON builder");
    assert_eq!(body(&response), builder.json_for(&data));
    assert!(body(&response).contains("\"ismobile\": true"));
    assert_eq!(
        header(&response, "Content-Length"),
        Some(response.body.len().to_string().as_str())
    );
}

#[test]
fn a_javascript_builder_that_left_no_data_is_logged_and_built_again_around_the_json() {
    let web = web_pipeline(
        &[JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY],
        WebIntegrationOptions::default(),
    );
    let data = processed(web.pipeline());
    assert!(data.get(JAVASCRIPT_BUILDER_DATA_KEY).is_none());
    let json = data
        .get(JSON_BUILDER_DATA_KEY)
        .expect("JSON")
        .json()
        .to_owned();
    let from = mark();

    let response = serve_javascript(&data, &TestRequest, &options(&web));

    assert_eq!(
        records_since(from),
        vec![missing(JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY)]
    );
    assert_not_cached(&response);
    let builder = web
        .pipeline()
        .resource::<JavaScriptBuilderElement>()
        .expect("the JavaScript builder");
    assert_eq!(body(&response), builder.javascript_for(&data, &json));
    assert!(body(&response).contains("ismobile"));
}

#[test]
fn both_builders_that_left_no_data_are_logged_javascript_first() {
    let web = web_pipeline(
        &[
            JSON_BUILDER_ELEMENT_DATA_KEY,
            JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY,
        ],
        WebIntegrationOptions::default(),
    );
    let data = processed(web.pipeline());
    let from = mark();

    let response = serve_javascript(&data, &TestRequest, &options(&web));

    assert_eq!(
        records_since(from),
        vec![
            missing(JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY),
            missing(JSON_BUILDER_ELEMENT_DATA_KEY),
        ]
    );
    assert_not_cached(&response);
    assert!(body(&response).contains("ismobile"));
}

/// Unprocessed flow data records no errors, so only the missing data stops
/// the response being cached.
#[test]
fn missing_data_alone_stops_the_response_being_cached() {
    let web = web_pipeline(&[], WebIntegrationOptions::default());
    let data = web.pipeline().create_flow_data();
    assert!(data.errors().is_empty());
    let from = mark();

    let json = serve_json(&data, &TestRequest, &options(&web));
    let javascript = serve_javascript(&data, &TestRequest, &options(&web));

    assert_eq!(
        records_since(from),
        vec![
            missing(JSON_BUILDER_ELEMENT_DATA_KEY),
            missing(JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY),
            missing(JSON_BUILDER_ELEMENT_DATA_KEY),
        ]
    );
    assert_not_cached(&json);
    assert_not_cached(&javascript);
    assert!(body(&json).starts_with('{'));
    assert!(!body(&javascript).is_empty());
}

/// The builder the pipeline holds is the one used, so its configuration
/// applies to the body built again.
#[test]
fn the_builder_held_by_the_pipeline_builds_the_body() {
    let pipeline = Pipeline::builder()
        .suppress_process_exceptions(true)
        .add_element(Arc::new(DeviceElement::new()))
        .add_element(Arc::new(ClashElement::new(JSON_BUILDER_ELEMENT_DATA_KEY)))
        .add_element(Arc::new(JsonBuilderElement::new()))
        .add_resource(Arc::new(
            JsonBuilderElement::builder()
                .exclude_element("device")
                .build(),
        ))
        .build()
        .expect("the pipeline builds");
    let data = processed(&pipeline);
    assert!(JsonBuilderElement::new()
        .json_for(&data)
        .contains("\"device\""));
    let from = mark();

    let response = serve_json(&data, &TestRequest, &EndpointOptions::default());

    assert_eq!(
        records_since(from),
        vec![missing(JSON_BUILDER_ELEMENT_DATA_KEY)]
    );
    assert!(
        !body(&response).contains("\"device\""),
        "{}",
        body(&response)
    );
    assert!(body(&response).starts_with('{'));
}

#[test]
fn a_builder_the_pipeline_does_not_hold_gives_an_empty_body() {
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(DeviceElement::new()))
        .add_element(Arc::new(JsonBuilderElement::new()))
        .add_element(Arc::new(JavaScriptBuilderElement::new()))
        .build()
        .expect("the pipeline builds");
    let data = pipeline.create_flow_data();
    let options = EndpointOptions::new(EvidenceKeyFilterWhitelist::new(Vec::<String>::new()));
    let from = mark();

    let json = serve_json(&data, &TestRequest, &options);
    let javascript = serve_javascript(&data, &TestRequest, &options);

    assert_eq!(
        records_since(from),
        vec![
            missing(JSON_BUILDER_ELEMENT_DATA_KEY),
            missing(JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY),
        ]
    );
    for response in [&json, &javascript] {
        assert_not_cached(response);
        assert!(response.body.is_empty());
    }
}

#[test]
fn a_pipeline_without_the_builders_logs_nothing() {
    let web = web_pipeline(
        &[],
        WebIntegrationOptions {
            client_side_evidence_enabled: false,
            ..WebIntegrationOptions::default()
        },
    );
    let data = processed(web.pipeline());
    let from = mark();

    let json = serve_json(&data, &TestRequest, &options(&web));
    let javascript = serve_javascript(&data, &TestRequest, &options(&web));

    assert_eq!(records_since(from), Vec::new());
    for response in [&json, &javascript] {
        assert_eq!(
            header(response, "Cache-Control"),
            Some(CACHE_CONTROL_CACHEABLE)
        );
        assert!(header(response, "ETag").is_some());
        assert!(response.body.is_empty());
    }
}
