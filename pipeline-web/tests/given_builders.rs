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

//! A JSON builder or JavaScript builder the application gives among its
//! elements is the one the web pipeline runs and holds, and the pipeline adds
//! a builder only of a kind the application does not give.
//!
//! A pipeline given neither builder keeps its order, which
//! `element_ordering_matches_spec` in `web_integration.rs` and
//! `the_web_pipeline_holds_its_builders_as_resources` in
//! `client_side_fallback.rs` check.

use std::any::Any;
use std::sync::Arc;

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
    serve_javascript, serve_json, RequestData, WebIntegrationOptions, WebPipeline, WebResponse,
};

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

/// Adds two properties, `ismobile` and `browsername`, under the data key
/// `device`.
struct DeviceElement {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

impl DeviceElement {
    fn new() -> Self {
        DeviceElement {
            filter: EvidenceKeyFilterWhitelist::new(["header.user-agent"]),
            properties: vec![
                PropertyMetaData::new("ismobile", "device", PropertyValueType::Bool),
                PropertyMetaData::new("browsername", "device", PropertyValueType::String),
            ],
        }
    }
}

impl FlowElement for DeviceElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        data.get_or_add(TypedKey::<TestData>::new("device"), || {
            TestData(
                MapElementData::new()
                    .set("ismobile", true)
                    .set("browsername", "Test Browser"),
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

/// Adds nothing, under the data key it is given.
struct QuietElement {
    data_key: &'static str,
    filter: EvidenceKeyFilterWhitelist,
}

impl QuietElement {
    fn new(data_key: &'static str) -> Self {
        QuietElement {
            data_key,
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        }
    }
}

impl FlowElement for QuietElement {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
        Ok(())
    }
    fn data_key(&self) -> &str {
        self.data_key
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
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

/// Runs a JSON builder under the data key it is given, and does not give back
/// the builder's type, as an element wrapping another does not.
struct WrappedJsonBuilder {
    data_key: &'static str,
    inner: JsonBuilderElement,
}

impl FlowElement for WrappedJsonBuilder {
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

// ----------------------------------------------------------------------------
// Helpers.
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

/// A JSON builder that leaves out the `browsername` property.
fn json_builder_without_browser_name() -> Arc<JsonBuilderElement> {
    Arc::new(
        JsonBuilderElement::builder()
            .exclude_property("browsername")
            .build(),
    )
}

/// A JavaScript builder that names its object `myObj`.
fn javascript_builder_named_my_obj() -> Arc<JavaScriptBuilderElement> {
    Arc::new(
        JavaScriptBuilderElement::builder()
            .set_object_name("myObj")
            .expect("myObj is a valid object name")
            .build(),
    )
}

fn web_pipeline(elements: Vec<Arc<dyn FlowElement>>) -> WebPipeline {
    WebPipeline::build(elements, WebIntegrationOptions::default()).expect("the web pipeline builds")
}

/// The data keys of the pipeline's elements, in the order they run.
fn data_keys(web: &WebPipeline) -> Vec<&str> {
    web.pipeline()
        .flow_elements()
        .iter()
        .map(|element| element.data_key())
        .collect()
}

/// True if the pipeline's element with this data key is `builder` itself.
fn runs<T>(web: &WebPipeline, data_key: &str, builder: &Arc<T>) -> bool {
    web.pipeline()
        .flow_elements()
        .iter()
        .find(|element| element.data_key() == data_key)
        .is_some_and(|element| std::ptr::addr_eq(Arc::as_ptr(element), Arc::as_ptr(builder)))
}

/// True if the pipeline holds `builder` itself as a resource.
fn holds<T: Any + Send + Sync>(web: &WebPipeline, builder: &Arc<T>) -> bool {
    web.pipeline()
        .resource::<T>()
        .is_some_and(|held| Arc::ptr_eq(&held, builder))
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

fn json(data: &FlowData) -> String {
    data.get(JSON_BUILDER_DATA_KEY)
        .expect("the JSON builder's data")
        .json()
        .to_owned()
}

fn body(response: &WebResponse) -> String {
    String::from_utf8(response.body.clone()).expect("the body is UTF-8")
}

// ----------------------------------------------------------------------------
// Tests.
// ----------------------------------------------------------------------------

/// A JSON builder the application gives writes the JSON with its own
/// exclusions, and no second JSON builder is added.
#[test]
fn a_given_json_builder_writes_its_exclusions_and_runs_alone() {
    let given = json_builder_without_browser_name();
    let web = web_pipeline(vec![
        Arc::new(DeviceElement::new()),
        Arc::clone(&given) as Arc<dyn FlowElement>,
    ]);

    assert_eq!(
        data_keys(&web),
        [
            "sequence",
            "device",
            "json-builder",
            "set-headers",
            "javascriptbuilderelement",
        ]
    );
    assert!(runs(&web, JSON_BUILDER_ELEMENT_DATA_KEY, &given));
    assert!(holds(&web, &given));

    let data = processed(web.pipeline());
    let json = json(&data);
    assert!(json.contains("\"ismobile\": true"), "{json}");
    assert!(!json.contains("browsername"), "{json}");
    let added = JsonBuilderElement::new().json_for(&data);
    assert!(
        added.contains("\"browsername\": \"Test Browser\""),
        "{added}"
    );
}

/// A JavaScript builder the application gives renders the script with its
/// own object name, and no second JavaScript builder is added. The JSON
/// builder added for it goes immediately before it.
#[test]
fn a_given_javascript_builder_renders_its_settings_and_runs_alone() {
    let given = javascript_builder_named_my_obj();
    let web = web_pipeline(vec![
        Arc::new(DeviceElement::new()),
        Arc::clone(&given) as Arc<dyn FlowElement>,
    ]);

    assert_eq!(
        data_keys(&web),
        [
            "sequence",
            "device",
            "json-builder",
            "javascriptbuilderelement",
            "set-headers",
        ]
    );
    assert!(runs(&web, JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY, &given));
    assert!(holds(&web, &given));
    assert!(web.pipeline().resource::<JsonBuilderElement>().is_some());

    let data = processed(web.pipeline());
    let script = data
        .get(JAVASCRIPT_BUILDER_DATA_KEY)
        .expect("the JavaScript builder's data")
        .javascript()
        .to_owned();
    assert_eq!(script, given.javascript_for(&data, &json(&data)));
    assert!(script.contains("myObj"));
    let added = JavaScriptBuilderElement::new().javascript_for(&data, &json(&data));
    assert!(!added.contains("myObj"));
}

/// Builders the application gives together run where it put them, both are
/// held, and neither kind is added.
#[test]
fn builders_given_together_run_where_the_application_put_them() {
    let json_builder = json_builder_without_browser_name();
    let javascript_builder = javascript_builder_named_my_obj();
    let web = web_pipeline(vec![
        Arc::new(DeviceElement::new()),
        Arc::clone(&json_builder) as Arc<dyn FlowElement>,
        Arc::clone(&javascript_builder) as Arc<dyn FlowElement>,
        Arc::new(QuietElement::new("after")),
    ]);

    assert_eq!(
        data_keys(&web),
        [
            "sequence",
            "device",
            "json-builder",
            "javascriptbuilderelement",
            "after",
            "set-headers",
        ]
    );
    assert!(runs(&web, JSON_BUILDER_ELEMENT_DATA_KEY, &json_builder));
    assert!(runs(
        &web,
        JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY,
        &javascript_builder
    ));
    assert!(holds(&web, &json_builder));
    assert!(holds(&web, &javascript_builder));
}

/// When a JSON builder the application gives left no data, the endpoint
/// builds the JSON again with that builder, so its exclusions still apply.
#[test]
fn a_given_json_builder_that_left_no_data_builds_the_json_again_with_its_exclusions() {
    let given = json_builder_without_browser_name();
    let web = web_pipeline(vec![
        Arc::new(DeviceElement::new()),
        Arc::new(ClashElement::new(JSON_BUILDER_ELEMENT_DATA_KEY)),
        Arc::clone(&given) as Arc<dyn FlowElement>,
    ]);
    let data = processed(web.pipeline());
    assert!(data.get(JSON_BUILDER_DATA_KEY).is_none());

    let response = serve_json(&data, &TestRequest, &web.endpoint_options());

    let body = body(&response);
    assert_eq!(body, given.json_for(&data));
    assert!(body.contains("\"ismobile\": true"), "{body}");
    assert!(!body.contains("browsername"), "{body}");
}

/// When a JavaScript builder the application gives left no data, the
/// endpoint builds the script again with that builder, so its object name
/// still applies.
#[test]
fn a_given_javascript_builder_that_left_no_data_builds_the_script_again_with_its_settings() {
    let given = javascript_builder_named_my_obj();
    let web = web_pipeline(vec![
        Arc::new(DeviceElement::new()),
        Arc::new(ClashElement::new(JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY)),
        Arc::clone(&given) as Arc<dyn FlowElement>,
    ]);
    let data = processed(web.pipeline());
    assert!(data.get(JAVASCRIPT_BUILDER_DATA_KEY).is_none());

    let response = serve_javascript(&data, &TestRequest, &web.endpoint_options());

    let body = body(&response);
    assert_eq!(body, given.javascript_for(&data, &json(&data)));
    assert!(body.contains("myObj"));
}

/// A builder the application gives runs, and is held, when the client-side
/// endpoints are not in use, and no builder is added then.
#[test]
fn a_given_builder_runs_and_is_held_without_client_side_evidence() {
    let given = json_builder_without_browser_name();
    let web = WebPipeline::build(
        vec![
            Arc::new(DeviceElement::new()),
            Arc::clone(&given) as Arc<dyn FlowElement>,
        ],
        WebIntegrationOptions {
            client_side_evidence_enabled: false,
            ..WebIntegrationOptions::default()
        },
    )
    .expect("the web pipeline builds");

    assert_eq!(
        data_keys(&web),
        ["sequence", "device", "json-builder", "set-headers"]
    );
    assert!(holds(&web, &given));
    assert!(web
        .pipeline()
        .resource::<JavaScriptBuilderElement>()
        .is_none());
}

/// A builder is found by its data key ignoring case, as the flow data keeps
/// element data under the key in lower case, so no builder of that kind is
/// added beside it.
#[test]
fn a_builder_is_found_by_its_data_key_ignoring_case() {
    let web = web_pipeline(vec![
        Arc::new(DeviceElement::new()),
        Arc::new(WrappedJsonBuilder {
            data_key: "JSON-Builder",
            inner: JsonBuilderElement::new(),
        }),
    ]);

    assert_eq!(
        data_keys(&web),
        [
            "sequence",
            "device",
            "JSON-Builder",
            "set-headers",
            "javascriptbuilderelement",
        ]
    );
}

/// A builder that does not give back its own type, such as one another
/// element wraps, runs in place of the one that would be added, but the
/// pipeline cannot hold it.
#[test]
fn a_builder_that_cannot_give_its_type_runs_but_is_not_held() {
    let web = web_pipeline(vec![
        Arc::new(DeviceElement::new()),
        Arc::new(WrappedJsonBuilder {
            data_key: JSON_BUILDER_ELEMENT_DATA_KEY,
            inner: JsonBuilderElement::builder()
                .exclude_property("browsername")
                .build(),
        }),
    ]);

    assert_eq!(
        data_keys(&web),
        [
            "sequence",
            "device",
            "json-builder",
            "set-headers",
            "javascriptbuilderelement",
        ]
    );
    assert!(web.pipeline().resource::<JsonBuilderElement>().is_none());
    assert!(web
        .pipeline()
        .resource::<JavaScriptBuilderElement>()
        .is_some());

    let data = processed(web.pipeline());
    let json = json(&data);
    assert!(!json.contains("browsername"), "{json}");
}
