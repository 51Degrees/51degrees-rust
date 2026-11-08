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

//! Integration tests for the JSON builder element.
//!
//! Each test builds a small pipeline whose final element is the JSON builder,
//! processes a flow data and asserts on the serialized JSON document. The
//! document is parsed back into a `serde_json::Value` for content assertions and
//! inspected as a string for ordering assertions.

use std::any::Any;
use std::collections::BTreeMap;
use std::sync::Arc;

use fiftyone_cloud_request_engine::CloudRequestEngine;
use fiftyone_json_builder::{
    JsonBuilderElement, JsonBuilderElementBuilder, JSON_BUILDER_DATA_KEY,
    JSON_BUILDER_ELEMENT_DATA_KEY, NEW_LINE,
};
use fiftyone_pipeline_core::{
    ElementData, Error, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData,
    FlowElement, MapElementData, NoValueError, Pipeline, PropertyMetaData, PropertyValue,
    PropertyValueType, Result, TypedKey, WktString,
};
use fiftyone_pipeline_engines::AspectDataBase;
use fiftyone_pipeline_engines_fiftyone::SequenceElement;
use serde_json::Value;

// ---------------------------------------------------------------------------
// Test element: a generic element that publishes a fixed element data instance
// and a fixed property-metadata list, so the tests can shape any output.
// ---------------------------------------------------------------------------

/// Element data that wraps a `MapElementData` but reports a configurable set of
/// no-value properties (returning `Err(NoValueError)` for them) so the
/// nullreason path can be exercised.
struct TestData {
    values: MapElementData,
    no_value: BTreeMap<String, String>,
}

impl ElementData for TestData {
    fn get(&self, name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        let lower = name.to_lowercase();
        if let Some(message) = self.no_value.get(&lower) {
            return Err(NoValueError::new(message.clone()));
        }
        self.values.get(name)
    }

    fn keys(&self) -> Vec<String> {
        let mut keys = self.values.keys();
        keys.extend(self.no_value.keys().cloned());
        keys
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A configurable test element.
struct TestElement {
    key: &'static str,
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
    values: Vec<(String, PropertyValue)>,
    no_value: Vec<(String, String)>,
}

impl TestElement {
    fn new(key: &'static str) -> Self {
        TestElement {
            key,
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            properties: Vec::new(),
            values: Vec::new(),
            no_value: Vec::new(),
        }
    }

    fn with_property(mut self, meta: PropertyMetaData) -> Self {
        self.properties.push(meta);
        self
    }

    fn with_value(mut self, name: &str, value: impl Into<PropertyValue>) -> Self {
        self.values.push((name.to_owned(), value.into()));
        self
    }

    fn with_no_value(mut self, name: &str, message: &str) -> Self {
        self.no_value.push((name.to_owned(), message.to_owned()));
        self
    }

    fn typed_key(&self) -> TypedKey<TestData> {
        // The data key string is leaked into a 'static through the constant
        // element keys used in tests; TypedKey requires a 'static name.
        TypedKey::new(self.key)
    }
}

impl FlowElement for TestElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        let mut map = MapElementData::new();
        for (name, value) in &self.values {
            map.insert(name, value.clone());
        }
        let no_value: BTreeMap<String, String> = self
            .no_value
            .iter()
            .map(|(name, message)| (name.to_lowercase(), message.clone()))
            .collect();
        data.get_or_add(self.typed_key(), || TestData {
            values: map,
            no_value,
        })?;
        Ok(())
    }

    fn data_key(&self) -> &str {
        self.key
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

/// An element that always records an error against the flow data.
struct FailingElement {
    key: &'static str,
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

impl FailingElement {
    fn new(key: &'static str) -> Self {
        FailingElement {
            key,
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            properties: Vec::new(),
        }
    }
}

impl FlowElement for FailingElement {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
        Err(fiftyone_pipeline_core::Error::NotProcessed {
            message: "deliberate test failure".to_owned(),
        })
    }

    fn data_key(&self) -> &str {
        self.key
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a pipeline of the Sequence element, the given elements and a
/// trailing JSON builder, process a flow data and return the JSON.
///
/// The request is sent with `sent_sequence` as its `query.sequence`, or
/// with none, and the Sequence element adds one to it, so a request sent
/// with none is request 1. The Sequence element's own data is left out of
/// the JSON, so the JSON holds only what the test's elements give.
fn run_with_sequence(
    elements: Vec<Arc<dyn FlowElement>>,
    json_builder: JsonBuilderElementBuilder,
    sent_sequence: Option<&str>,
) -> String {
    let mut builder = Pipeline::builder()
        .suppress_process_exceptions(true)
        .add_element(Arc::new(SequenceElement::new()));
    for element in elements {
        builder = builder.add_element(element);
    }
    let json_builder = json_builder
        .exclude_element(SequenceElement::DEFAULT_ELEMENT_DATA_KEY)
        .build();
    let pipeline = builder
        .add_element(Arc::new(json_builder))
        .build()
        .expect("pipeline builds");

    let mut evidence = Evidence::builder();
    if let Some(sent_sequence) = sent_sequence {
        evidence = evidence.add("query.sequence", sent_sequence);
    }
    let mut data = pipeline.create_flow_data_with(evidence.build());
    data.process()
        .expect("processing succeeds (exceptions suppressed)");

    data.get(JSON_BUILDER_DATA_KEY)
        .expect("json builder data present")
        .json()
        .to_owned()
}

/// Run as request 1, the first request of a session.
fn run(elements: Vec<Arc<dyn FlowElement>>, json_builder: JsonBuilderElementBuilder) -> String {
    run_with_sequence(elements, json_builder, None)
}

fn parse(json: &str) -> Value {
    serde_json::from_str(json).expect("output is valid JSON")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn serialises_property_values() {
    let device = TestElement::new("device")
        .with_property(PropertyMetaData::new(
            "ismobile",
            "device",
            PropertyValueType::Bool,
        ))
        .with_property(PropertyMetaData::new(
            "platformname",
            "device",
            PropertyValueType::String,
        ))
        .with_value("ismobile", true)
        .with_value("platformname", "iOS");

    let json = run(vec![Arc::new(device)], JsonBuilderElement::builder());
    let value = parse(&json);

    let device = value.get("device").expect("device object present");
    assert_eq!(device.get("ismobile"), Some(&Value::Bool(true)));
    assert_eq!(
        device.get("platformname"),
        Some(&Value::String("iOS".to_owned()))
    );
}

#[test]
fn excludes_internal_elements_by_default() {
    // The cloud request engine, set-headers and usage-sharing elements are
    // excluded by default and must not appear in the output. Each test element
    // takes the real element's data key.
    use fiftyone_pipeline_engines_fiftyone::constants::{
        SET_HEADERS_DEFAULT_ELEMENT_DATA_KEY, SHARE_USAGE_DEFAULT_ELEMENT_DATA_KEY,
    };
    let cloud_key = fiftyone_cloud_request_engine::ELEMENT_DATA_KEY;
    let cloud = TestElement::new(cloud_key).with_value("json-response", "{}");
    let headers = TestElement::new(SET_HEADERS_DEFAULT_ELEMENT_DATA_KEY).with_value("any", "x");
    let usage = TestElement::new(SHARE_USAGE_DEFAULT_ELEMENT_DATA_KEY).with_value("any", "x");
    let device = TestElement::new("device").with_value("ismobile", true);

    let json = run(
        vec![
            Arc::new(cloud),
            Arc::new(headers),
            Arc::new(usage),
            Arc::new(device),
        ],
        JsonBuilderElement::builder(),
    );
    let value = parse(&json);

    assert!(value.get(cloud_key).is_none(), "cloud excluded");
    assert!(
        value.get(SET_HEADERS_DEFAULT_ELEMENT_DATA_KEY).is_none(),
        "set-headers excluded"
    );
    assert!(
        value.get(SHARE_USAGE_DEFAULT_ELEMENT_DATA_KEY).is_none(),
        "usage excluded"
    );
    assert!(value.get("device").is_some(), "device retained");
    // The builder must never serialize its own element data.
    assert!(
        value.get(JSON_BUILDER_ELEMENT_DATA_KEY).is_none(),
        "json builder excludes itself"
    );
}

#[test]
fn writes_every_property_and_hides_configured_ones() {
    let device = TestElement::new("device")
        .with_value("products", "written")
        .with_value("properties", "written")
        .with_value("ismobile", true)
        .with_value("secret", "hide-me");

    // No property name is excluded by default, as in .NET, so `products` and
    // `properties` are written. The builder hides `secret`.
    let element = JsonBuilderElement::builder().exclude_property("SECRET");
    let json = run(vec![Arc::new(device)], element);
    let value = parse(&json);

    let device = value.get("device").expect("device present");
    assert_eq!(device.get("products"), Some(&Value::from("written")));
    assert_eq!(device.get("properties"), Some(&Value::from("written")));
    assert!(
        device.get("secret").is_none(),
        "secret excluded by configuration (case-insensitive)"
    );
    assert_eq!(device.get("ismobile"), Some(&Value::Bool(true)));
}

#[test]
fn emits_nullreason_for_no_value_property() {
    let device = TestElement::new("device")
        .with_property(PropertyMetaData::new(
            "platformname",
            "device",
            PropertyValueType::String,
        ))
        .with_no_value("platformname", "No matching profile was found.");

    let json = run(vec![Arc::new(device)], JsonBuilderElement::builder());
    let value = parse(&json);

    let device = value.get("device").expect("device present");
    assert_eq!(device.get("platformname"), Some(&Value::Null));
    assert_eq!(
        device.get("platformnamenullreason"),
        Some(&Value::String("No matching profile was found.".to_owned()))
    );
}

#[test]
fn serialises_engine_aspect_data() {
    // Exercise the engines dependency: an aspect engine writes AspectDataBase,
    // and the builder serializes the values it actually stored. A property the
    // engine never set is absent from the data and is simply not emitted,
    // because the builder iterates the stored value dictionary rather than the
    // declared metadata.
    struct EngineElement {
        filter: EvidenceKeyFilterWhitelist,
        properties: Vec<PropertyMetaData>,
    }
    impl EngineElement {
        const KEY: TypedKey<AspectDataBase> = TypedKey::new("device");
    }
    impl FlowElement for EngineElement {
        fn process(&self, data: &mut FlowData) -> Result<()> {
            data.get_or_add(Self::KEY, || {
                AspectDataBase::new("device").set("ismobile", true)
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

    let element = EngineElement {
        filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        properties: vec![
            PropertyMetaData::new("ismobile", "device", PropertyValueType::Bool),
            PropertyMetaData::new("platformname", "device", PropertyValueType::String),
        ],
    };

    let json = run(vec![Arc::new(element)], JsonBuilderElement::builder());
    let value = parse(&json);
    let device = value.get("device").expect("device present");

    // The value the engine set serializes normally.
    assert_eq!(device.get("ismobile"), Some(&Value::Bool(true)));
    // The property the engine never stored is absent, with no nullreason.
    assert!(
        device.get("platformname").is_none(),
        "unset property absent"
    );
    assert!(
        device.get("platformnamenullreason").is_none(),
        "no nullreason for an unstored property"
    );
}

#[test]
fn emits_delayexecution_and_evidenceproperties() {
    // The value property `screenpixelswidth` names its JavaScript evidence
    // property `screenpixelswidthjavascript`, which is delayed-execution. The
    // builder flags the JS property with a delayexecution sibling and lists it as
    // the value property's evidenceproperties. Only delayed-execution evidence is
    // listed, as required by the specification.
    let device = TestElement::new("device")
        .with_property(
            PropertyMetaData::new("screenpixelswidth", "device", PropertyValueType::Integer)
                .with_evidence_properties(["screenpixelswidthjavascript"]),
        )
        .with_property(
            PropertyMetaData::new(
                "screenpixelswidthjavascript",
                "device",
                PropertyValueType::JavaScript,
            )
            .with_delay_execution(true),
        )
        .with_value("screenpixelswidth", 1080i64)
        .with_value(
            "screenpixelswidthjavascript",
            PropertyValue::JavaScript("document.write('x');".to_owned()),
        );

    let json = run(vec![Arc::new(device)], JsonBuilderElement::builder());
    let value = parse(&json);
    let device = value.get("device").expect("device present");

    // The JavaScript property is flagged as delayed-execution.
    assert_eq!(
        device.get("screenpixelswidthjavascriptdelayexecution"),
        Some(&Value::Bool(true)),
        "delayexecution sibling present on the JavaScript property"
    );
    // The value property lists its delayed-execution JavaScript evidence.
    let evidence = device
        .get("screenpixelswidthevidenceproperties")
        .and_then(Value::as_array)
        .expect("evidenceproperties on the value property");
    assert_eq!(
        evidence,
        &vec![Value::String(
            "device.screenpixelswidthjavascript".to_owned()
        )]
    );
    // The JavaScript property carries no evidenceproperties of its own.
    assert!(
        device
            .get("screenpixelswidthjavascriptevidenceproperties")
            .is_none(),
        "the JavaScript property has no evidenceproperties of its own"
    );
}

#[test]
fn evidence_property_that_is_not_delayed_execution_is_dropped() {
    // The named evidence property exists but is an ordinary (non-delayed,
    // non-JavaScript) property, so it must not be listed and the
    // evidenceproperties key must be omitted entirely (no empty array).
    let device = TestElement::new("device")
        .with_property(
            PropertyMetaData::new("screenpixelswidth", "device", PropertyValueType::Integer)
                .with_evidence_properties(["plainprop"]),
        )
        .with_property(PropertyMetaData::new(
            "plainprop",
            "device",
            PropertyValueType::String,
        ))
        .with_value("screenpixelswidth", 1080i64)
        .with_value("plainprop", "x");

    let json = run(vec![Arc::new(device)], JsonBuilderElement::builder());
    let value = parse(&json);
    let device = value.get("device").expect("device present");
    assert!(
        device.get("screenpixelswidthevidenceproperties").is_none(),
        "a non-delayed evidence property is not listed and the key is omitted"
    );
}

#[test]
fn evidenceproperties_keeps_only_delayed_execution_entries() {
    // A property naming two evidence properties, only one of which is a
    // delayed-execution JavaScript property: only that one survives.
    let device = TestElement::new("device")
        .with_property(
            PropertyMetaData::new("screenpixelswidth", "device", PropertyValueType::Integer)
                .with_evidence_properties(["screenpixelswidthjavascript", "plainprop"]),
        )
        .with_property(
            PropertyMetaData::new(
                "screenpixelswidthjavascript",
                "device",
                PropertyValueType::JavaScript,
            )
            .with_delay_execution(true),
        )
        .with_property(PropertyMetaData::new(
            "plainprop",
            "device",
            PropertyValueType::String,
        ))
        .with_value("screenpixelswidth", 1080i64)
        .with_value(
            "screenpixelswidthjavascript",
            PropertyValue::JavaScript("x".to_owned()),
        )
        .with_value("plainprop", "y");

    let json = run(vec![Arc::new(device)], JsonBuilderElement::builder());
    let value = parse(&json);
    let device = value.get("device").expect("device present");
    let evidence = device
        .get("screenpixelswidthevidenceproperties")
        .and_then(Value::as_array)
        .expect("evidenceproperties present");
    assert_eq!(
        evidence,
        &vec![Value::String(
            "device.screenpixelswidthjavascript".to_owned()
        )],
        "only the delayed-execution evidence property is listed"
    );
}

#[test]
fn evidenceproperties_match_is_case_insensitive() {
    // The evidence property is named in mixed case in the metadata but matches
    // the lowercased delayed-execution path.
    let device = TestElement::new("device")
        .with_property(
            PropertyMetaData::new("screenpixelswidth", "device", PropertyValueType::Integer)
                .with_evidence_properties(["ScreenPixelsWidthJavaScript"]),
        )
        .with_property(
            PropertyMetaData::new(
                "screenpixelswidthjavascript",
                "device",
                PropertyValueType::JavaScript,
            )
            .with_delay_execution(true),
        )
        .with_value("screenpixelswidth", 1080i64)
        .with_value(
            "screenpixelswidthjavascript",
            PropertyValue::JavaScript("x".to_owned()),
        );

    let json = run(vec![Arc::new(device)], JsonBuilderElement::builder());
    let value = parse(&json);
    let device = value.get("device").expect("device present");
    let evidence = device
        .get("screenpixelswidthevidenceproperties")
        .and_then(Value::as_array)
        .expect("evidenceproperties present");
    assert_eq!(
        evidence,
        &vec![Value::String(
            "device.screenpixelswidthjavascript".to_owned()
        )]
    );
}

#[test]
fn collects_javascript_properties_when_below_cap() {
    let device = TestElement::new("device")
        .with_property(PropertyMetaData::new(
            "screenpixelswidthjavascript",
            "device",
            PropertyValueType::JavaScript,
        ))
        .with_value(
            "screenpixelswidthjavascript",
            PropertyValue::JavaScript("x".to_owned()),
        );

    let json = run(vec![Arc::new(device)], JsonBuilderElement::builder());
    let value = parse(&json);

    let js = value
        .get("javascriptProperties")
        .and_then(Value::as_array)
        .expect("javascriptProperties present below cap");
    assert_eq!(
        js,
        &vec![Value::String(
            "device.screenpixelswidthjavascript".to_owned()
        )]
    );
}

#[test]
fn an_unavailable_property_is_neither_javascript_nor_delayed() {
    // An engine lists the properties its data set was opened without as
    // unavailable. The JavaScript and delayed-execution lists come from the
    // available properties only, as .NET's come from the pipeline's available
    // properties, so the client is never asked to run a script the engine
    // gives no value for.
    let device = TestElement::new("device")
        .with_property(PropertyMetaData::new(
            "screenpixelswidthjavascript",
            "device",
            PropertyValueType::JavaScript,
        ))
        .with_property(
            PropertyMetaData::new(
                "screenpixelsheightjavascript",
                "device",
                PropertyValueType::JavaScript,
            )
            .with_delay_execution(true)
            .with_available(false),
        )
        .with_property(
            PropertyMetaData::new("screenpixelsheight", "device", PropertyValueType::Integer)
                .with_evidence_properties(["screenpixelsheightjavascript"]),
        )
        .with_value(
            "screenpixelswidthjavascript",
            PropertyValue::JavaScript("x".to_owned()),
        )
        .with_value("screenpixelsheight", 1920i64);

    let json = run(vec![Arc::new(device)], JsonBuilderElement::builder());
    let value = parse(&json);

    let js = value
        .get("javascriptProperties")
        .and_then(Value::as_array)
        .expect("javascriptProperties present below cap");
    assert_eq!(
        js,
        &vec![Value::String(
            "device.screenpixelswidthjavascript".to_owned()
        )]
    );
    let device = value.get("device").expect("device present");
    assert!(device.get("screenpixelsheightevidenceproperties").is_none());
}

#[test]
fn suppresses_javascript_properties_at_cap() {
    let device = TestElement::new("device")
        .with_property(PropertyMetaData::new(
            "screenpixelswidthjavascript",
            "device",
            PropertyValueType::JavaScript,
        ))
        .with_value(
            "screenpixelswidthjavascript",
            PropertyValue::JavaScript("x".to_owned()),
        );

    // A request sent with sequence 9 is request 10, which is
    // MAX_JAVASCRIPT_ITERATIONS, so the list is dropped.
    let json = run_with_sequence(
        vec![Arc::new(device)],
        JsonBuilderElement::builder(),
        Some("9"),
    );
    let value = parse(&json);
    assert!(
        value.get("javascriptProperties").is_none(),
        "javascriptProperties suppressed at the cap"
    );

    // A higher sequence number also suppresses it.
    let device = TestElement::new("device")
        .with_property(PropertyMetaData::new(
            "screenpixelswidthjavascript",
            "device",
            PropertyValueType::JavaScript,
        ))
        .with_value(
            "screenpixelswidthjavascript",
            PropertyValue::JavaScript("x".to_owned()),
        );
    let json = run_with_sequence(
        vec![Arc::new(device)],
        JsonBuilderElement::builder(),
        Some("25"),
    );
    assert!(parse(&json).get("javascriptProperties").is_none());
}

#[test]
fn flow_errors_are_a_list_of_messages() {
    // The specification puts the flow errors under `errors` as a list of
    // message strings, which is what the client script reads it as.
    let device = TestElement::new("device").with_value("ismobile", true);
    let failing = FailingElement::new("badengine");

    let json = run(
        vec![Arc::new(device), Arc::new(failing)],
        JsonBuilderElement::builder(),
    );
    let value = parse(&json);

    let errors = value
        .get("errors")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("errors is not a list of messages: {json}"));
    let messages: Vec<&str> = errors
        .iter()
        .map(|error| error.as_str().expect("each error is a message"))
        .collect();
    assert_eq!(messages.len(), 1);
    assert!(
        messages[0].contains("deliberate test failure"),
        "error message carried through, got {messages:?}"
    );
}

#[test]
fn a_failed_cloud_request_lists_no_javascript_property_and_writes_the_message() {
    // When the cloud request fails, a cloud engine stores empty data marked
    // as failed and records that it did not process. Its metadata still names
    // a JavaScript property, but the JSON holds no value for it, so
    // `javascriptProperties` leaves it out. The error is the message alone,
    // with no words naming the error's kind.
    struct FailedCloudEngine {
        filter: EvidenceKeyFilterWhitelist,
        properties: Vec<PropertyMetaData>,
    }
    impl FailedCloudEngine {
        const KEY: TypedKey<AspectDataBase> = TypedKey::new("device");
    }
    impl FlowElement for FailedCloudEngine {
        fn process(&self, data: &mut FlowData) -> Result<()> {
            data.get_or_add(Self::KEY, || {
                let mut device = AspectDataBase::new("device");
                device.mark_cloud_request_failed();
                device
            })?;
            CloudRequestEngine::record_not_processed(
                data,
                "DeviceDetectionCloudEngine",
                "device",
                module_path!(),
            );
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

    let engine = FailedCloudEngine {
        filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        properties: vec![
            PropertyMetaData::new("ismobile", "device", PropertyValueType::Bool),
            PropertyMetaData::new(
                "javascripthardwareprofile",
                "device",
                PropertyValueType::JavaScript,
            ),
        ],
    };

    let json = run(vec![Arc::new(engine)], JsonBuilderElement::builder());
    let value = parse(&json);

    assert_eq!(
        value
            .get("device")
            .and_then(Value::as_object)
            .map(|device| device.len()),
        Some(0),
        "the device data is written with no properties, got {json}"
    );
    assert!(
        value.get("javascriptProperties").is_none(),
        "no JavaScript property is listed, got {json}"
    );
    assert_eq!(
        value.get("errors"),
        Some(&serde_json::json!([
            "The 'DeviceDetectionCloudEngine' did not process because the JSON response \
             from the CloudRequestEngine was null or empty. This indicates that the cloud \
             request failed."
        ])),
        "the error is the message alone, got {json}"
    );
}

#[test]
fn output_is_deterministic() {
    // Build the same pipeline twice and confirm byte-identical JSON, including
    // the key order, so a downstream ETag is stable.
    fn make_device() -> TestElement {
        TestElement::new("device")
            .with_property(PropertyMetaData::new(
                "zebra",
                "device",
                PropertyValueType::String,
            ))
            .with_property(PropertyMetaData::new(
                "alpha",
                "device",
                PropertyValueType::Bool,
            ))
            .with_value("zebra", "z")
            .with_value("alpha", true)
            .with_value("middle", 42i64)
    }

    let first = run(vec![Arc::new(make_device())], JsonBuilderElement::builder());
    let second = run(vec![Arc::new(make_device())], JsonBuilderElement::builder());
    assert_eq!(first, second, "identical inputs yield identical JSON");

    // Properties are written in the order the element data lists them, not
    // in name order.
    let zebra = first.find("\"zebra\"").expect("zebra present");
    let alpha = first.find("\"alpha\"").expect("alpha present");
    let middle = first.find("\"middle\"").expect("middle present");
    assert!(
        zebra < alpha && alpha < middle,
        "properties in the order the element data lists them: {first}"
    );
}

#[test]
fn text_is_in_the_form_dotnet_writes() {
    let device = TestElement::new("device")
        .with_value("text", "a\"b\\c\u{1}\u{2028}<&\u{e9}")
        .with_value("one", 1.0f64)
        .with_value("tenth", 0.1f64)
        .with_value("small", 1e-7f64)
        .with_value("large", 1e21f64)
        .with_value("float", PropertyValue::Float(0.3));

    let json = run(vec![Arc::new(device)], JsonBuilderElement::builder());

    let expected = [
        "{",
        "  \"device\": {",
        "    \"text\": \"a\\\"b\\\\c\\u0001\\u2028<&\u{e9}\",",
        "    \"one\": 1.0,",
        "    \"tenth\": 0.1,",
        "    \"small\": 1E-07,",
        "    \"large\": 1E+21,",
        "    \"float\": 0.3",
        "  }",
        "}",
    ]
    .join(NEW_LINE);
    assert_eq!(json, expected);
}

/// A WKT value is written as an object holding its text under `value`, as
/// .NET's JSON builder writes its `WktString` and the cloud service sends it.
#[test]
fn a_wkt_value_is_an_object_holding_its_text() {
    let ip = TestElement::new("ip").with_value("areas", WktString::new("POINT(2 4)"));

    let json = run(vec![Arc::new(ip)], JsonBuilderElement::builder());

    assert_eq!(
        parse(&json)["ip"]["areas"],
        serde_json::json!({"value": "POINT(2 4)"}),
        "got {json}"
    );
}

#[test]
fn elements_are_written_in_the_order_their_data_was_added() {
    let zulu = TestElement::new("zulu").with_value("one", 1i64);
    let mike = TestElement::new("mike").with_value("two", 2i64);
    let alpha = TestElement::new("alpha").with_value("three", 3i64);

    let json = run(
        vec![Arc::new(zulu), Arc::new(mike), Arc::new(alpha)],
        JsonBuilderElement::builder(),
    );

    let zulu = json.find("\"zulu\"").expect("zulu present");
    let mike = json.find("\"mike\"").expect("mike present");
    let alpha = json.find("\"alpha\"").expect("alpha present");
    assert!(
        zulu < mike && mike < alpha,
        "elements in the order their data was added: {json}"
    );
}

#[test]
fn javascript_properties_follow_element_order_then_property_order() {
    // The values are set in the opposite order to the metadata, to show the
    // list follows the metadata.
    let zulu = TestElement::new("zulu")
        .with_property(PropertyMetaData::new(
            "zjavascript",
            "zulu",
            PropertyValueType::JavaScript,
        ))
        .with_property(PropertyMetaData::new(
            "ajavascript",
            "zulu",
            PropertyValueType::JavaScript,
        ))
        .with_value("ajavascript", PropertyValue::JavaScript("a".to_owned()))
        .with_value("zjavascript", PropertyValue::JavaScript("z".to_owned()));
    let alpha = TestElement::new("alpha")
        .with_property(PropertyMetaData::new(
            "mjavascript",
            "alpha",
            PropertyValueType::JavaScript,
        ))
        .with_value("mjavascript", PropertyValue::JavaScript("m".to_owned()));

    let json = run(
        vec![Arc::new(zulu), Arc::new(alpha)],
        JsonBuilderElement::builder(),
    );
    let value = parse(&json);

    let paths: Vec<&str> = value
        .get("javascriptProperties")
        .and_then(Value::as_array)
        .expect("javascriptProperties present below cap")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(
        paths,
        ["zulu.zjavascript", "zulu.ajavascript", "alpha.mjavascript"]
    );
}

#[test]
fn javascript_value_serialised_as_string() {
    let device = TestElement::new("device")
        .with_property(PropertyMetaData::new(
            "hint",
            "device",
            PropertyValueType::JavaScript,
        ))
        .with_value("hint", PropertyValue::JavaScript("alert(1)".to_owned()));

    let json = run(vec![Arc::new(device)], JsonBuilderElement::builder());
    let value = parse(&json);
    let device = value.get("device").expect("device present");
    assert_eq!(
        device.get("hint"),
        Some(&Value::String("alert(1)".to_owned())),
        "javascript value serialized as its source string"
    );
}

/// Process a pipeline of one JavaScript property and the JSON builder, with
/// no Sequence element, and check that processing fails with the builder's
/// error and that no JSON is written.
fn fails_without_a_sequence_element(evidence: Evidence) {
    let device = TestElement::new("device")
        .with_property(PropertyMetaData::new(
            "hint",
            "device",
            PropertyValueType::JavaScript,
        ))
        .with_value("hint", PropertyValue::JavaScript("x".to_owned()));

    let pipeline = Pipeline::builder()
        .add_element(Arc::new(device))
        .add_element(Arc::new(JsonBuilderElement::new()))
        .build()
        .expect("pipeline builds");
    let mut data = pipeline.create_flow_data_with(evidence);
    data.process().expect_err("processing fails");

    assert!(
        data.get(JSON_BUILDER_DATA_KEY).is_none(),
        "no JSON is written"
    );
    let errors = data.errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        matches!(errors[0].source, Error::Pipeline { .. }),
        "{errors:?}"
    );
    let message = errors[0].source.message();
    assert!(
        message.starts_with(
            "The value for the 'sequence' parameter could not be found. \
             A Sequence element needs to be placed before the JSON builder \
             element in the pipeline. See "
        ),
        "{message}"
    );
    let link = "https://51degrees.com/documentation/4.5/_services__cloud__error_messages.html?";
    assert!(message.contains(link), "{message}");
    assert!(message.ends_with(" for more information."), "{message}");

    // The client-side endpoints serve json_for when the builder left no
    // data, and it takes the request as the first, as .NET's fallback does.
    let json = JsonBuilderElement::new().json_for(&data);
    assert!(parse(&json).get("javascriptProperties").is_some(), "{json}");
}

#[test]
fn no_sequence_number_fails_and_writes_no_json() {
    // With no Sequence element and no sequence evidence, .NET's builder
    // fails with this message and Java's with a configuration error, so
    // neither writes JSON.
    fails_without_a_sequence_element(Evidence::builder().build());
}

#[test]
fn a_request_sequence_without_a_sequence_element_fails() {
    // A request's own query.sequence is the Sequence element's input, not a
    // sequence number, so without a Sequence element the builder still
    // fails. .NET and Java fail too, as their builders take only an integer
    // from the evidence and a query string gives them text.
    fails_without_a_sequence_element(Evidence::builder().add("query.sequence", "3").build());
}

/// Run a pipeline of the sequence element, one JavaScript property and the
/// JSON builder, as the web integration orders them, and say whether the
/// `javascriptProperties` list was emitted.
fn javascript_properties_after_sequence_element(sequence: &str) -> bool {
    let device = TestElement::new("device")
        .with_property(PropertyMetaData::new(
            "hint",
            "device",
            PropertyValueType::JavaScript,
        ))
        .with_value("hint", PropertyValue::JavaScript("x".to_owned()));

    let pipeline = Pipeline::builder()
        .add_element(Arc::new(SequenceElement::new()))
        .add_element(Arc::new(device))
        .add_element(Arc::new(JsonBuilderElement::new()))
        .build()
        .expect("pipeline builds");
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("query.sequence", sequence).build());
    data.process().expect("processing succeeds");
    let json = data.get(JSON_BUILDER_DATA_KEY).unwrap().json().to_owned();
    parse(&json).get("javascriptProperties").is_some()
}

#[test]
fn cap_uses_the_sequence_after_the_sequence_element_adds_one() {
    // The sequence element turns a request's sequence of 9 into 10, and the
    // .NET and Java JSON builders compare that 10 with the cap, so they leave
    // the list out. Reading the evidence alone compared 9 and kept it.
    assert!(
        !javascript_properties_after_sequence_element("9"),
        "a request sent with sequence 9 is request 10, which is at the cap"
    );
    assert!(
        javascript_properties_after_sequence_element("8"),
        "a request sent with sequence 8 is request 9, which is below the cap"
    );
}

/// The builder and the element are `Send`, `Sync`, `UnwindSafe` and
/// `RefUnwindSafe`, as removing an auto trait from a public type breaks
/// callers. This test fails to compile if either loses one.
#[test]
fn the_builder_and_element_keep_their_auto_traits() {
    fn has_auto_traits<T: Send + Sync + std::panic::UnwindSafe + std::panic::RefUnwindSafe>() {}
    has_auto_traits::<fiftyone_json_builder::JsonBuilderElementBuilder>();
    has_auto_traits::<JsonBuilderElement>();
}
