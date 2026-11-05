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

//! Tests of the JSON builder's selected properties, the specification's
//! `SetProperties` option, with at least one test for each part of its
//! definition.

use std::any::Any;
use std::collections::BTreeMap;
use std::sync::Arc;

use fiftyone_json_builder::{JsonBuilderElement, JsonBuilderElementBuilder, JSON_BUILDER_DATA_KEY};
use fiftyone_pipeline_core::{
    ElementData, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    MapElementData, NoValueError, Pipeline, PropertyMetaData, PropertyValue, PropertyValueType,
    Result, TypedKey,
};
use fiftyone_pipeline_engines_fiftyone::SequenceElement;
use serde_json::{json, Value};

/// Element data holding values and properties that have no value.
struct TestData {
    values: MapElementData,
    no_value: Vec<(String, String)>,
}

impl ElementData for TestData {
    fn get(&self, name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        match self
            .no_value
            .iter()
            .find(|(no_value, _)| no_value.eq_ignore_ascii_case(name))
        {
            Some((_, message)) => Err(NoValueError::new(message.clone())),
            None => self.values.get(name),
        }
    }

    fn keys(&self) -> Vec<String> {
        let mut keys = self.values.keys();
        keys.extend(self.no_value.iter().map(|(name, _)| name.clone()));
        keys
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// An element that stores fixed values and metadata, and optionally adds a
/// warning and fails.
struct TestElement {
    key: &'static str,
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
    values: Vec<(&'static str, PropertyValue)>,
    no_value: Vec<(String, String)>,
    warning: Option<&'static str>,
    fail: bool,
}

impl TestElement {
    fn new(key: &'static str) -> Self {
        TestElement {
            key,
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            properties: Vec::new(),
            values: Vec::new(),
            no_value: Vec::new(),
            warning: None,
            fail: false,
        }
    }

    fn value(mut self, name: &'static str, value: impl Into<PropertyValue>) -> Self {
        self.values.push((name, value.into()));
        self
    }

    fn no_value(mut self, name: &str, message: &str) -> Self {
        self.no_value.push((name.to_owned(), message.to_owned()));
        self
    }

    fn meta(mut self, meta: PropertyMetaData) -> Self {
        self.properties.push(meta);
        self
    }

    /// A JavaScript property, by its metadata and by its value.
    fn javascript(self, name: &'static str) -> Self {
        let key = self.key;
        self.meta(PropertyMetaData::new(
            name,
            key,
            PropertyValueType::JavaScript,
        ))
        .value(name, PropertyValue::JavaScript("var a = 1;".to_owned()))
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

impl FlowElement for TestElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        let mut values = MapElementData::new();
        for (name, value) in &self.values {
            values.insert(name, value.clone());
        }
        let no_value = self.no_value.clone();
        data.get_or_add(TypedKey::<TestData>::new(self.key), || TestData {
            values,
            no_value,
        })?;
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

/// Run the Sequence element, the elements and then the JSON builder, and give
/// the JSON text. The Sequence element's own data is left out of the JSON.
fn run(elements: Vec<Arc<dyn FlowElement>>, json_builder: JsonBuilderElementBuilder) -> String {
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
    let mut data = pipeline.create_flow_data();
    data.process().expect("exceptions are suppressed");
    data.get(JSON_BUILDER_DATA_KEY)
        .expect("json builder data present")
        .json()
        .to_owned()
}

fn parse(json: &str) -> Value {
    serde_json::from_str(json).expect("output is valid JSON")
}

/// The keys of a JSON object, sorted.
fn keys(value: &Value) -> Vec<&str> {
    let mut keys: Vec<&str> = value
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    keys
}

/// A device element with two values, one property with no value and a
/// JavaScript property.
fn device() -> Arc<dyn FlowElement> {
    Arc::new(
        TestElement::new("device")
            .value("ismobile", true)
            .value("browsername", "Chrome")
            .no_value("platformname", "No platform.")
            .javascript("javascriptscript"),
    )
}

/// A hardware element with a value and a list of two records.
fn hardware() -> Arc<dyn FlowElement> {
    let record = |name: &str, vendor: &str| {
        let mut record = BTreeMap::new();
        record.insert(
            "HardwareName".to_owned(),
            PropertyValue::String(name.to_owned()),
        );
        record.insert(
            "HardwareVendor".to_owned(),
            PropertyValue::String(vendor.to_owned()),
        );
        record
    };
    Arc::new(
        TestElement::new("hardware")
            .value("profilecount", 2_i64)
            .value(
                "profiles",
                PropertyValue::KeyValueList(vec![
                    record("Pixel 8", "Google"),
                    record("iPhone", "Apple"),
                ]),
            ),
    )
}

/// "By default, the list is empty and all Properties will be included in
/// the JSON."
#[test]
fn an_empty_list_writes_every_property() {
    let default = run(vec![device(), hardware()], JsonBuilderElement::builder());
    let empty = run(
        vec![device(), hardware()],
        JsonBuilderElement::builder().set_properties(Vec::<String>::new()),
    );
    assert_eq!(empty, default);

    let value = parse(&default);
    assert_eq!(
        keys(&value["device"]),
        [
            "browsername",
            "ismobile",
            "javascriptscript",
            "platformname",
            "platformnamenullreason",
        ]
    );
    assert_eq!(keys(&value["hardware"]), ["profilecount", "profiles"]);
}

/// "A list of the Properties to include in the JSON."
#[test]
fn only_the_listed_properties_are_written() {
    let value = parse(&run(
        vec![device(), hardware()],
        JsonBuilderElement::builder().set_properties(["device.ismobile", "hardware.profilecount"]),
    ));
    assert_eq!(
        keys(&value["device"]),
        ["ismobile", "javascriptscript", "platformnamenullreason"]
    );
    assert_eq!(value["device"]["ismobile"], json!(true));
    assert_eq!(value["hardware"], json!({ "profilecount": 2 }));
}

/// A later list replaces an earlier one.
#[test]
fn a_later_list_replaces_an_earlier_one() {
    let value = parse(&run(
        vec![device()],
        JsonBuilderElement::builder()
            .set_properties(["device.ismobile"])
            .set_properties(["device.browsername"]),
    ));
    assert_eq!(
        keys(&value["device"]),
        ["browsername", "javascriptscript", "platformnamenullreason"]
    );
}

/// "Property names will need to be fully qualified." A bare property name
/// selects nothing, and names are matched without regard to case.
#[test]
fn names_are_complete_and_matched_ignoring_case() {
    let bare = parse(&run(
        vec![device()],
        JsonBuilderElement::builder().set_properties(["ismobile"]),
    ));
    assert_eq!(
        keys(&bare["device"]),
        ["javascriptscript", "platformnamenullreason"]
    );

    let mixed_case = parse(&run(
        vec![device()],
        JsonBuilderElement::builder().set_properties(["Device.IsMobile"]),
    ));
    assert_eq!(
        keys(&mixed_case["device"]),
        ["ismobile", "javascriptscript", "platformnamenullreason"]
    );

    // Part of a name is not the name.
    let part = parse(&run(
        vec![device()],
        JsonBuilderElement::builder().set_properties(["device.is"]),
    ));
    assert_eq!(
        keys(&part["device"]),
        ["javascriptscript", "platformnamenullreason"]
    );
}

/// The selection rule, through the element's own check.
#[test]
fn is_property_selected_follows_the_rule() {
    let all = JsonBuilderElement::new();
    assert!(all.is_property_selected("device.ismobile"));
    assert!(all.is_property_selected("anything.at.all"));

    let some = JsonBuilderElement::builder()
        .set_properties(["Device.IsMobile", "hardware.profiles.hardwarename"])
        .build();
    assert!(some.is_property_selected("device.ismobile"));
    assert!(some.is_property_selected("DEVICE.ISMOBILE"));
    assert!(!some.is_property_selected("device.ismobilex"));
    assert!(!some.is_property_selected("device.is"));
    assert!(!some.is_property_selected("ismobile"));
    // A list that holds a listed name is kept, and so is the listed name.
    assert!(some.is_property_selected("hardware.profiles"));
    assert!(some.is_property_selected("hardware.profiles.hardwarename"));
    assert!(!some.is_property_selected("hardware.profiles.hardwarevendor"));

    // Everything under a listed name is selected.
    let list = JsonBuilderElement::builder()
        .set_properties(["hardware.profiles"])
        .build();
    assert!(list.is_property_selected("hardware.profiles.hardwarevendor"));
    assert!(!list.is_property_selected("hardware.profilecount"));
}

/// "JavaScript Properties ... MUST always be included, regardless of this
/// setting." That holds for one known by its metadata, one known only by its
/// value and one with no value, and the `javascriptProperties` list is the
/// same as with no list.
#[test]
fn javascript_properties_are_always_written() {
    let element = || -> Arc<dyn FlowElement> {
        Arc::new(
            TestElement::new("device")
                .value("ismobile", true)
                .javascript("javascriptscript")
                .value(
                    "javascriptbyvalue",
                    PropertyValue::JavaScript("var b = 2;".to_owned()),
                )
                .meta(PropertyMetaData::new(
                    "javascriptlater",
                    "device",
                    PropertyValueType::JavaScript,
                ))
                .no_value("javascriptlater", "Not yet."),
        )
    };
    let all = parse(&run(vec![element()], JsonBuilderElement::builder()));
    let value = parse(&run(
        vec![element()],
        JsonBuilderElement::builder().set_properties(["device.ismobile"]),
    ));
    assert_eq!(value, all);
    assert_eq!(value["device"]["javascriptbyvalue"], json!("var b = 2;"));
    assert_eq!(value["device"]["javascriptlater"], Value::Null);
    assert_eq!(
        value["device"]["javascriptlaternullreason"],
        json!("Not yet.")
    );
    assert_eq!(
        value["javascriptProperties"],
        json!(["device.javascriptscript", "device.javascriptlater"])
    );

    // With nothing else selected, the JavaScript properties are still there.
    let none = parse(&run(
        vec![element()],
        JsonBuilderElement::builder().set_properties(["location.town"]),
    ));
    assert_eq!(
        keys(&none["device"]),
        [
            "javascriptbyvalue",
            "javascriptlater",
            "javascriptlaternullreason",
            "javascriptscript",
        ]
    );
    assert_eq!(none["javascriptProperties"], all["javascriptProperties"]);
}

/// "Note that some Properties, such as JavaScript Properties and meta
/// properties, MUST always be included, regardless of this setting." Every
/// property's `nullreason`, `delayexecution` and `evidenceproperties` keys
/// are written whether or not the property is listed, and are the same as
/// with no list, while an unlisted property's value is left out.
#[test]
fn meta_properties_are_always_written() {
    let element = || -> Arc<dyn FlowElement> {
        Arc::new(
            TestElement::new("device")
                .meta(
                    PropertyMetaData::new(
                        "screenpixelswidth",
                        "device",
                        PropertyValueType::Integer,
                    )
                    .with_evidence_properties(["screenpixelswidthjavascript"]),
                )
                .meta(
                    PropertyMetaData::new(
                        "screenpixelsheight",
                        "device",
                        PropertyValueType::Integer,
                    )
                    .with_evidence_properties(["screenpixelswidthjavascript"]),
                )
                .meta(
                    PropertyMetaData::new(
                        "screenpixelswidthjavascript",
                        "device",
                        PropertyValueType::JavaScript,
                    )
                    .with_delay_execution(true),
                )
                .value("screenpixelswidth", 1080_i64)
                .value("screenpixelsheight", 2400_i64)
                .value(
                    "screenpixelswidthjavascript",
                    PropertyValue::JavaScript("var w = 1;".to_owned()),
                )
                .no_value("platformname", "No platform.")
                .no_value("browsername", "No browser."),
        )
    };
    let all = parse(&run(vec![element()], JsonBuilderElement::builder()));
    let value = parse(&run(
        vec![element()],
        JsonBuilderElement::builder()
            .set_properties(["device.screenpixelswidth", "device.platformname"]),
    ));
    assert_eq!(
        keys(&value["device"]),
        [
            "browsernamenullreason",
            "platformname",
            "platformnamenullreason",
            "screenpixelsheightevidenceproperties",
            "screenpixelswidth",
            "screenpixelswidthevidenceproperties",
            "screenpixelswidthjavascript",
            "screenpixelswidthjavascriptdelayexecution",
        ]
    );
    for meta in [
        "browsernamenullreason",
        "platformnamenullreason",
        "screenpixelsheightevidenceproperties",
        "screenpixelswidthevidenceproperties",
        "screenpixelswidthjavascriptdelayexecution",
    ] {
        assert_eq!(value["device"][meta], all["device"][meta], "{meta}");
    }
    assert_eq!(
        value["device"]["browsernamenullreason"],
        json!("No browser.")
    );
    assert_eq!(
        value["device"]["screenpixelsheightevidenceproperties"],
        json!(["device.screenpixelswidthjavascript"])
    );

    // With nothing listed from this element, only the JavaScript property
    // and the meta properties are written.
    let none = parse(&run(
        vec![element()],
        JsonBuilderElement::builder().set_properties(["location.town"]),
    ));
    assert_eq!(
        keys(&none["device"]),
        [
            "browsernamenullreason",
            "platformnamenullreason",
            "screenpixelsheightevidenceproperties",
            "screenpixelswidthevidenceproperties",
            "screenpixelswidthjavascript",
            "screenpixelswidthjavascriptdelayexecution",
        ]
    );
}

/// "e.g. ... `devices.profiles.hardwarename`." A name inside a list of
/// records keeps the list with only that property in each record.
#[test]
fn a_name_inside_a_list_keeps_that_property_of_each_record() {
    let value = parse(&run(
        vec![hardware()],
        JsonBuilderElement::builder().set_properties(["hardware.profiles.hardwarename"]),
    ));
    assert_eq!(
        value["hardware"],
        json!({
            "profiles": [
                { "hardwarename": "Pixel 8" },
                { "hardwarename": "iPhone" }
            ]
        })
    );
}

/// Listing a list of records keeps every property of each record.
#[test]
fn a_listed_list_keeps_every_property_of_each_record() {
    let value = parse(&run(
        vec![hardware()],
        JsonBuilderElement::builder().set_properties(["hardware.profiles"]),
    ));
    assert_eq!(
        value["hardware"],
        json!({
            "profiles": [
                { "hardwarename": "Pixel 8", "hardwarevendor": "Google" },
                { "hardwarename": "iPhone", "hardwarevendor": "Apple" }
            ]
        })
    );
}

/// A JavaScript property in a list of records is always written, so a list
/// that is not listed is kept with only that property in each record. That
/// holds for one known by its metadata and one known only by its value.
#[test]
fn a_javascript_property_in_a_list_keeps_the_list() {
    let profiles = |javascript: PropertyValue| {
        let record = |name: &str| {
            let mut record = BTreeMap::new();
            record.insert(
                "HardwareName".to_owned(),
                PropertyValue::String(name.to_owned()),
            );
            record.insert("ProfileJavaScript".to_owned(), javascript.clone());
            record
        };
        PropertyValue::KeyValueList(vec![record("Pixel 8"), record("iPhone")])
    };
    let expected = |javascript: &str| {
        json!({
            "profilecount": 2,
            "profiles": [
                { "profilejavascript": javascript },
                { "profilejavascript": javascript }
            ]
        })
    };

    // Known by its metadata, with a value that is plain text.
    let by_metadata: Arc<dyn FlowElement> = Arc::new(
        TestElement::new("hardware")
            .meta(
                PropertyMetaData::new("profiles", "hardware", PropertyValueType::KeyValueList)
                    .with_item_properties([PropertyMetaData::new(
                        "profilejavascript",
                        "hardware",
                        PropertyValueType::JavaScript,
                    )]),
            )
            .value("profilecount", 2_i64)
            .value(
                "profiles",
                profiles(PropertyValue::String("var p = 1;".to_owned())),
            ),
    );
    let value = parse(&run(
        vec![by_metadata],
        JsonBuilderElement::builder().set_properties(["hardware.profilecount"]),
    ));
    assert_eq!(value["hardware"], expected("var p = 1;"));

    // Known only by its value.
    let by_value: Arc<dyn FlowElement> = Arc::new(
        TestElement::new("hardware")
            .value("profilecount", 2_i64)
            .value(
                "profiles",
                profiles(PropertyValue::JavaScript("var p = 2;".to_owned())),
            ),
    );
    let value = parse(&run(
        vec![by_value],
        JsonBuilderElement::builder().set_properties(["hardware.profilecount"]),
    ));
    assert_eq!(value["hardware"], expected("var p = 2;"));
}

/// "There MUST be a top-level entry for each Element Data", so an element
/// with nothing selected keeps an empty entry, and the errors and warnings
/// are written as they are with no list.
#[test]
fn an_element_with_nothing_selected_keeps_an_empty_entry() {
    let elements = || -> Vec<Arc<dyn FlowElement>> {
        vec![
            device(),
            Arc::new(
                TestElement::new("location")
                    .value("town", "Reading")
                    .warning("location warning")
                    .failing(),
            ),
        ]
    };
    let all = parse(&run(elements(), JsonBuilderElement::builder()));
    let value = parse(&run(
        elements(),
        JsonBuilderElement::builder().set_properties(["device.ismobile"]),
    ));
    assert_eq!(value["location"], json!({}));
    assert_eq!(value["errors"], all["errors"]);
    assert_eq!(value["warnings"], json!(["location warning"]));
    assert_eq!(
        keys(&value),
        [
            "device",
            "errors",
            "javascriptProperties",
            "location",
            "warnings",
        ]
    );
}
