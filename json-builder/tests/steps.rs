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

//! Tests of the JSON builder's steps and converters, the points at which a
//! caller changes the output.

use std::any::Any;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use fiftyone_json_builder::{
    default_steps, DefaultJsonBuilderSteps, JsonBuilderContext, JsonBuilderElement,
    JsonBuilderSteps, JsonObject, JsonValue, JsonValueConverter, JSON_BUILDER_DATA_KEY,
};
use fiftyone_pipeline_core::{
    ElementData, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    MapElementData, NoValueError, Pipeline, PropertyMetaData, PropertyValue, PropertyValueType,
    Result, TypedKey,
};
use serde_json::{json, Value};

/// An element that stores fixed values, and optionally fails or adds a
/// warning.
struct ValuesElement {
    key: &'static str,
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
    values: Vec<(&'static str, PropertyValue)>,
    warning: Option<&'static str>,
    fail: bool,
}

impl ValuesElement {
    fn new(key: &'static str) -> Self {
        ValuesElement {
            key,
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            properties: Vec::new(),
            values: Vec::new(),
            warning: None,
            fail: false,
        }
    }

    fn value(mut self, name: &'static str, value: impl Into<PropertyValue>) -> Self {
        self.values.push((name, value.into()));
        self
    }

    fn javascript(mut self, name: &'static str) -> Self {
        self.properties.push(PropertyMetaData::new(
            name,
            self.key,
            PropertyValueType::JavaScript,
        ));
        self.values
            .push((name, PropertyValue::JavaScript("var a = 1;".to_owned())));
        self
    }

    /// Declare a JavaScript property that the data holds no value for.
    fn declared_javascript(mut self, name: &'static str) -> Self {
        self.properties.push(PropertyMetaData::new(
            name,
            self.key,
            PropertyValueType::JavaScript,
        ));
        self
    }

    fn warning(mut self, message: &'static str) -> Self {
        self.warning = Some(message);
        self
    }

    fn failing(mut self) -> Self {
        self.fail = true;
        self
    }
}

impl FlowElement for ValuesElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        let mut bag = MapElementData::new();
        for (name, value) in &self.values {
            bag.insert(name, value.clone());
        }
        data.get_or_add(TypedKey::<MapElementData>::new(self.key), || bag)?;
        if let Some(message) = self.warning {
            data.add_warning(message, None);
        }
        if self.fail {
            return Err(fiftyone_pipeline_core::Error::NotProcessed {
                message: format!("{} failed", self.key),
            });
        }
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

/// Element data that counts how many values are read from it.
struct CountingData {
    values: MapElementData,
    reads: Arc<AtomicUsize>,
}

impl ElementData for CountingData {
    fn get(&self, name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.values.get(name)
    }

    fn has_property(&self, name: &str) -> bool {
        self.values.has_property(name)
    }

    fn keys(&self) -> Vec<String> {
        self.values.keys()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// An element whose data counts its reads.
struct CountingElement {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
    reads: Arc<AtomicUsize>,
}

impl FlowElement for CountingElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        let values = MapElementData::new()
            .set("ismobile", true)
            .set("browsername", "Chrome")
            .set("platformname", "Android");
        let reads = Arc::clone(&self.reads);
        data.get_or_add(TypedKey::<CountingData>::new("device"), || CountingData {
            values,
            reads,
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

/// Run the elements and then the JSON builder, and give the JSON text.
fn run(elements: Vec<Arc<dyn FlowElement>>, json_builder: JsonBuilderElement) -> String {
    let mut builder = Pipeline::builder().suppress_process_exceptions(true);
    for element in elements {
        builder = builder.add_element(element);
    }
    let pipeline = builder
        .add_element(Arc::new(json_builder))
        .build()
        .expect("pipeline builds");
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("query.sequence", "1").build());
    data.process().expect("exceptions are suppressed");
    data.get(JSON_BUILDER_DATA_KEY)
        .expect("json builder data present")
        .json()
        .to_owned()
}

fn parse(json: &str) -> Value {
    serde_json::from_str(json).expect("output is valid JSON")
}

/// The strings in a JSON array.
fn messages(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .expect("an array")
        .iter()
        .map(|message| message.as_str().expect("a string"))
        .collect()
}

/// A device element with two values and a JavaScript property.
fn device() -> Arc<dyn FlowElement> {
    Arc::new(
        ValuesElement::new("device")
            .value("ismobile", true)
            .value("browsername", "Chrome")
            .javascript("javascriptscript"),
    )
}

/// Steps that replace nothing.
struct NoChanges;

impl JsonBuilderSteps for NoChanges {}

#[test]
fn steps_that_replace_nothing_give_the_builders_json() {
    let expected = run(vec![device()], JsonBuilderElement::new());
    let explicit = run(
        vec![device()],
        JsonBuilderElement::builder()
            .set_steps(DefaultJsonBuilderSteps)
            .build(),
    );
    let none = run(
        vec![device()],
        JsonBuilderElement::builder().set_steps(NoChanges).build(),
    );
    assert_eq!(explicit, expected);
    assert_eq!(none, expected);
}

/// Replaces the whole document.
struct FixedDocument;

impl JsonBuilderSteps for FixedDocument {
    fn build_json(&self, _context: &JsonBuilderContext<'_>) -> String {
        "{}".to_owned()
    }
}

#[test]
fn build_json_can_be_replaced() {
    let json = run(
        vec![device()],
        JsonBuilderElement::builder()
            .set_steps(FixedDocument)
            .build(),
    );
    assert_eq!(json, "{}");
}

/// Writes the top-level keys it is given, separated by commas.
struct KeysOnly;

impl JsonBuilderSteps for KeysOnly {
    fn write_json(&self, _context: &JsonBuilderContext<'_>, all_properties: &JsonObject) -> String {
        all_properties.keys().collect::<Vec<_>>().join(",")
    }
}

#[test]
fn write_json_is_given_the_collected_values() {
    let json = run(
        vec![device()],
        JsonBuilderElement::builder().set_steps(KeysOnly).build(),
    );
    assert_eq!(json, "device,javascriptProperties");
}

/// Adds an entry after the default's.
struct ExtraEntry;

impl JsonBuilderSteps for ExtraEntry {
    fn get_all_properties(&self, context: &JsonBuilderContext<'_>) -> JsonObject {
        let mut all_properties = default_steps::get_all_properties(self, context);
        let mut extra = JsonObject::new();
        extra.insert("count", 2_i64);
        all_properties.insert("extra", extra);
        all_properties
    }
}

#[test]
fn get_all_properties_can_add_to_the_default() {
    let value = parse(&run(
        vec![device()],
        JsonBuilderElement::builder().set_steps(ExtraEntry).build(),
    ));
    assert_eq!(value["extra"], json!({ "count": 2 }));
    assert_eq!(value["device"]["ismobile"], json!(true));
}

/// Reads only `ismobile`.
struct IsMobileOnly;

impl JsonBuilderSteps for IsMobileOnly {
    fn get_values(
        &self,
        context: &JsonBuilderContext<'_>,
        data_path: &str,
        source: &dyn ElementData,
    ) -> JsonObject {
        let mut values = JsonObject::new();
        self.add_json_values_for_property(
            context,
            &mut values,
            data_path,
            "ismobile",
            source.get("ismobile"),
            false,
        );
        values
    }
}

#[test]
fn get_values_can_read_only_the_properties_needed() {
    let reads = Arc::new(AtomicUsize::new(0));
    let element: Arc<dyn FlowElement> = Arc::new(CountingElement {
        filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        properties: Vec::new(),
        reads: Arc::clone(&reads),
    });
    let value = parse(&run(
        vec![element],
        JsonBuilderElement::builder()
            .set_steps(IsMobileOnly)
            .build(),
    ));
    assert_eq!(value["device"], json!({ "ismobile": true }));
    assert_eq!(reads.load(Ordering::SeqCst), 1, "one value read");
}

#[test]
fn the_default_reads_every_property() {
    let reads = Arc::new(AtomicUsize::new(0));
    let element: Arc<dyn FlowElement> = Arc::new(CountingElement {
        filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        properties: Vec::new(),
        reads: Arc::clone(&reads),
    });
    run(vec![element], JsonBuilderElement::new());
    assert_eq!(reads.load(Ordering::SeqCst), 3);
}

/// Writes `ismobile` as `mobile`, and everything else as the default does.
struct RenameIsMobile;

impl JsonBuilderSteps for RenameIsMobile {
    fn add_json_values_for_property(
        &self,
        context: &JsonBuilderContext<'_>,
        json_values: &mut JsonObject,
        data_path: &str,
        name: &str,
        value: std::result::Result<PropertyValue, NoValueError>,
        include_value_data_only: bool,
    ) {
        let name = if name == "ismobile" { "mobile" } else { name };
        default_steps::add_json_values_for_property(
            self,
            context,
            json_values,
            data_path,
            name,
            value,
            include_value_data_only,
        );
    }
}

#[test]
fn add_json_values_for_property_can_change_a_key() {
    let value = parse(&run(
        vec![device()],
        JsonBuilderElement::builder()
            .set_steps(RenameIsMobile)
            .build(),
    ));
    assert_eq!(value["device"]["mobile"], json!(true));
    assert!(value["device"].get("ismobile").is_none());
    assert_eq!(value["device"]["browsername"], json!("Chrome"));
}

/// Lists one JavaScript property of its own.
struct OwnJavaScript;

impl JsonBuilderSteps for OwnJavaScript {
    fn get_javascript_properties(
        &self,
        _context: &JsonBuilderContext<'_>,
        available_properties: &[String],
    ) -> Vec<String> {
        assert!(available_properties.contains(&"device.javascriptscript".to_owned()));
        vec!["device.own".to_owned()]
    }
}

#[test]
fn get_javascript_properties_can_be_replaced() {
    let value = parse(&run(
        vec![device()],
        JsonBuilderElement::builder()
            .set_steps(OwnJavaScript)
            .build(),
    ));
    assert_eq!(value["javascriptProperties"], json!(["device.own"]));
}

#[test]
fn javascript_properties_are_those_written() {
    // Of the three JavaScript properties the element declares, one is
    // excluded and one is not in the data, so neither is written or listed.
    let element: Arc<dyn FlowElement> = Arc::new(
        ValuesElement::new("device")
            .javascript("javascriptscript")
            .javascript("javascripthidden")
            .declared_javascript("javascriptmissing"),
    );
    let value = parse(&run(
        vec![element],
        JsonBuilderElement::builder()
            .exclude_property("javascripthidden")
            .build(),
    ));
    assert_eq!(
        value["javascriptProperties"],
        json!(["device.javascriptscript"])
    );
}

/// Leaves the errors out.
struct NoErrors;

impl JsonBuilderSteps for NoErrors {
    fn add_errors(&self, _context: &JsonBuilderContext<'_>, _all_properties: &mut JsonObject) {}
}

#[test]
fn add_errors_can_be_replaced() {
    let failing: Arc<dyn FlowElement> = Arc::new(ValuesElement::new("broken").failing());
    let default = parse(&run(vec![Arc::clone(&failing)], JsonBuilderElement::new()));
    assert!(messages(&default["errors"])[0].contains("broken failed"));

    let replaced = parse(&run(
        vec![failing],
        JsonBuilderElement::builder().set_steps(NoErrors).build(),
    ));
    assert!(replaced.get("errors").is_none());
}

/// Writes a warning of its own before the flow data's.
struct OwnWarningFirst;

impl JsonBuilderSteps for OwnWarningFirst {
    fn add_warnings(&self, context: &JsonBuilderContext<'_>, all_properties: &mut JsonObject) {
        all_properties.insert(
            "warnings",
            JsonValue::Array(vec![JsonValue::from("own warning")]),
        );
        default_steps::add_warnings(self, context, all_properties);
    }
}

#[test]
fn add_warnings_adds_to_warnings_already_written() {
    let element: Arc<dyn FlowElement> =
        Arc::new(ValuesElement::new("device").warning("flow warning"));
    let value = parse(&run(
        vec![element],
        JsonBuilderElement::builder()
            .set_steps(OwnWarningFirst)
            .build(),
    ));
    assert_eq!(value["warnings"], json!(["own warning", "flow warning"]));
}

#[test]
fn warnings_are_written_after_errors() {
    let warning: Arc<dyn FlowElement> =
        Arc::new(ValuesElement::new("first").warning("first warning"));
    let failing: Arc<dyn FlowElement> = Arc::new(ValuesElement::new("second").failing());
    let json = run(vec![warning, failing], JsonBuilderElement::new());
    let value = parse(&json);
    assert_eq!(value["warnings"], json!(["first warning"]));
    let errors = messages(&value["errors"]);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("second failed"));
    let errors = json.find("\"errors\"").expect("errors written");
    let warnings = json.find("\"warnings\"").expect("warnings written");
    assert!(errors < warnings, "errors before warnings: {json}");
}

#[test]
fn no_warnings_writes_no_warnings_key() {
    let value = parse(&run(vec![device()], JsonBuilderElement::new()));
    assert!(value.get("warnings").is_none());
}

/// Writes a boolean as "yes" or "no".
struct YesNo;

impl JsonValueConverter for YesNo {
    fn convert(&self, _element: &JsonBuilderElement, value: &PropertyValue) -> Option<JsonValue> {
        match value {
            PropertyValue::Bool(true) => Some(JsonValue::from("yes")),
            PropertyValue::Bool(false) => Some(JsonValue::from("no")),
            _ => None,
        }
    }
}

/// Writes every whole number as the given number.
struct Always(i64);

impl JsonValueConverter for Always {
    fn convert(&self, _element: &JsonBuilderElement, value: &PropertyValue) -> Option<JsonValue> {
        match value {
            PropertyValue::Integer(_) => Some(JsonValue::Integer(self.0)),
            _ => None,
        }
    }
}

#[test]
fn a_converter_changes_how_a_value_is_written() {
    let value = parse(&run(
        vec![device()],
        JsonBuilderElement::builder()
            .add_json_converter(YesNo)
            .build(),
    ));
    assert_eq!(value["device"]["ismobile"], json!("yes"));
    assert_eq!(value["device"]["browsername"], json!("Chrome"));
}

#[test]
fn converters_are_asked_in_the_order_added() {
    let element: Arc<dyn FlowElement> =
        Arc::new(ValuesElement::new("device").value("count", 5_i64));
    let value = parse(&run(
        vec![element],
        JsonBuilderElement::builder()
            .add_json_converter(Always(1))
            .add_json_converter(Always(2))
            .build(),
    ));
    assert_eq!(value["device"]["count"], json!(1));
}

#[test]
fn set_json_converters_replaces_them() {
    let element: Arc<dyn FlowElement> =
        Arc::new(ValuesElement::new("device").value("count", 5_i64));
    let converters: Vec<Arc<dyn JsonValueConverter>> = vec![Arc::new(Always(2))];
    let value = parse(&run(
        vec![element],
        JsonBuilderElement::builder()
            .add_json_converter(Always(1))
            .set_json_converters(converters)
            .build(),
    ));
    assert_eq!(value["device"]["count"], json!(2));
}

#[test]
fn a_closure_is_a_converter() {
    let value = parse(&run(
        vec![device()],
        JsonBuilderElement::builder()
            .add_json_converter(
                |_: &JsonBuilderElement, value: &PropertyValue| match value {
                    PropertyValue::String(s) => Some(JsonValue::String(s.to_uppercase())),
                    _ => None,
                },
            )
            .build(),
    ));
    assert_eq!(value["device"]["browsername"], json!("CHROME"));
}

/// A device element with a list of two records.
fn profiles() -> Arc<dyn FlowElement> {
    let record = |id: i64| {
        let mut record = BTreeMap::new();
        record.insert("ProfileId".to_owned(), PropertyValue::Integer(id));
        record
    };
    Arc::new(ValuesElement::new("hardware").value(
        "profiles",
        PropertyValue::KeyValueList(vec![record(1), record(2)]),
    ))
}

#[test]
fn a_converter_sees_the_values_in_records() {
    let value = parse(&run(
        vec![profiles()],
        JsonBuilderElement::builder()
            .add_json_converter(Always(7))
            .build(),
    ));
    assert_eq!(
        value["hardware"]["profiles"],
        json!([{ "profileid": 7 }, { "profileid": 7 }])
    );
}

/// Records the data path of every object whose values are collected.
struct RecordPaths(Arc<Mutex<Vec<String>>>);

impl JsonBuilderSteps for RecordPaths {
    fn get_values(
        &self,
        context: &JsonBuilderContext<'_>,
        data_path: &str,
        source: &dyn ElementData,
    ) -> JsonObject {
        self.0.lock().unwrap().push(data_path.to_owned());
        default_steps::get_values(self, context, data_path, source)
    }
}

#[test]
fn records_are_collected_by_get_values() {
    let paths = Arc::new(Mutex::new(Vec::new()));
    let value = parse(&run(
        vec![profiles()],
        JsonBuilderElement::builder()
            .set_steps(RecordPaths(Arc::clone(&paths)))
            .build(),
    ));
    assert_eq!(
        value["hardware"]["profiles"],
        json!([{ "profileid": 1 }, { "profileid": 2 }])
    );
    assert_eq!(
        *paths.lock().unwrap(),
        ["hardware", "hardware.profiles", "hardware.profiles"]
    );
}

#[test]
fn to_json_value_asks_the_converters_first() {
    let element = JsonBuilderElement::builder()
        .add_json_converter(YesNo)
        .build();
    assert_eq!(
        element.to_json_value(&PropertyValue::Bool(true)),
        JsonValue::from("yes")
    );
    assert_eq!(
        element.to_json_value(&PropertyValue::Integer(3)),
        JsonValue::Integer(3)
    );
}

#[test]
fn an_object_keeps_the_order_keys_were_added() {
    let mut object = JsonObject::new();
    object.insert("b", 1_i64);
    object.insert("a", 2_i64);
    object.insert("c", 3_i64);
    object.insert("b", 4_i64);
    assert_eq!(object.keys().collect::<Vec<_>>(), ["b", "a", "c"]);
    assert_eq!(object.get("b"), Some(&JsonValue::Integer(4)));

    object.remove("a");
    assert_eq!(object.keys().collect::<Vec<_>>(), ["b", "c"]);

    let mut reversed = JsonObject::new();
    reversed.insert("c", 3_i64);
    reversed.insert("b", 4_i64);
    assert_ne!(object, reversed, "objects in a different order differ");
}
