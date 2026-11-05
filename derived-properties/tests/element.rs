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
 * ********************************************************************* */

//! The element in a pipeline, reading what earlier elements produced.

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use fiftyone_derived_properties::{
    BuiltInScript, DerivedPropertyElement, DERIVED_DEFAULT_ELEMENT_DATA_KEY,
};
use fiftyone_pipeline_core::{
    ElementData, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    MapElementData, NoValueError, Pipeline, PropertyMetaData, PropertyValue, PropertyValueType,
    Result, TypedKey,
};

/// Element data that can hold a value, a no value message, or neither, so a
/// test can put a source element into each of the three states a real one
/// reaches.
#[derive(Debug, Clone, Default)]
struct StubData {
    values: MapElementData,
    no_value: HashMap<String, String>,
    names: Vec<String>,
}

impl StubData {
    fn with(mut self, name: &str, value: impl Into<PropertyValue>) -> Self {
        self.names.push(name.to_owned());
        self.values.insert(name, value.into());
        self
    }

    fn with_no_value(mut self, name: &str, message: &str) -> Self {
        self.names.push(name.to_owned());
        self.no_value
            .insert(name.to_lowercase(), message.to_owned());
        self
    }
}

impl ElementData for StubData {
    fn get(&self, name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        if let Some(value) = self.values.get_value(name) {
            return Ok(value.clone());
        }
        if let Some(message) = self.no_value.get(&name.to_lowercase()) {
            return Err(NoValueError::new(message.clone()));
        }
        Err(NoValueError::new(format!(
            "No value for property '{name}'."
        )))
    }

    fn keys(&self) -> Vec<String> {
        self.names.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// An element that publishes the data it was built with, standing in for
/// device detection or IP intelligence.
struct StubElement {
    key: &'static str,
    data: StubData,
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

impl StubElement {
    fn new(key: &'static str, data: StubData) -> Self {
        let properties = data
            .names
            .iter()
            .map(|name| PropertyMetaData::new(name.clone(), key, PropertyValueType::String))
            .collect();
        StubElement {
            key,
            data,
            filter: EvidenceKeyFilterWhitelist::new(Vec::<&str>::new()),
            properties,
        }
    }
}

impl FlowElement for StubElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        // The key is a compile time constant on each element, which is what a
        // typed key needs.
        let key: TypedKey<StubData> = TypedKey::new(match self.key {
            "device" => "device",
            _ => "ip",
        });
        data.get_or_add(key, || self.data.clone())?;
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

fn device_data() -> StubData {
    StubData::default()
        .with("IsCrawler", false)
        .with("IsHeadless", false)
        .with("HasWebDriver", "False")
        .with("IsVisible", "True")
        .with("BrowserReleaseYear", 2026i64)
        .with("BrowserReleaseAge", 1i64)
}

fn ip_data() -> StubData {
    StubData::default()
        .with("HumanProbability", 9i64)
        .with("ConnectionType", "Broadband")
}

fn pipeline_with(device: StubData, ip: StubData) -> Arc<Pipeline> {
    let derived = DerivedPropertyElement::builder()
        .add_built_in(BuiltInScript::HumanConfidence)
        .build()
        .expect("the shipped script is sound");
    Pipeline::builder()
        .add_element(Arc::new(StubElement::new("device", device)))
        .add_element(Arc::new(StubElement::new("ip", ip)))
        .add_element(Arc::new(derived))
        .build()
        .expect("the pipeline")
}

#[test]
fn the_element_publishes_its_property_under_the_derived_key() {
    let pipeline = pipeline_with(device_data(), ip_data());
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();

    let derived = data
        .get_str(DERIVED_DEFAULT_ELEMENT_DATA_KEY)
        .expect("the derived element data");
    assert_eq!(
        derived.get("HumanConfidence").unwrap(),
        PropertyValue::String("High".to_owned())
    );
    // Property names are matched without regard to letter case, as they are
    // everywhere else in the pipeline.
    assert!(derived.get("humanconfidence").is_ok());
    assert_eq!(derived.keys(), vec!["HumanConfidence".to_owned()]);
}

#[test]
fn a_missing_source_property_leaves_the_output_with_no_value() {
    // The ip element is in the pipeline but supplies neither property, which is
    // what an engine configuration or a resource key that leaves them out looks
    // like.
    let pipeline = pipeline_with(device_data(), StubData::default());
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();

    let derived = data.get_str(DERIVED_DEFAULT_ELEMENT_DATA_KEY).unwrap();
    let error = derived
        .get("HumanConfidence")
        .expect_err("the property is present with no value");
    assert!(
        error.message.contains(
            "Derived property 'HumanConfidence' has no value because 2 source properties were \
             not available."
        ),
        "{}",
        error.message
    );
    assert!(
        error.message.contains("'ip.HumanProbability'"),
        "{}",
        error.message
    );
    assert!(
        error.message.contains("'ip.ConnectionType'"),
        "{}",
        error.message
    );
    // The property is still listed, because it is present with no value rather
    // than absent.
    assert_eq!(derived.keys(), vec!["HumanConfidence".to_owned()]);
}

#[test]
fn a_source_that_carries_its_own_no_value_message_has_it_repeated() {
    let device = StubData::default()
        .with("IsCrawler", false)
        .with("IsHeadless", false)
        .with("HasWebDriver", "False")
        .with("IsVisible", "True")
        .with_no_value("BrowserReleaseYear", "the browser was not matched")
        .with("BrowserReleaseAge", 1i64);
    let pipeline = pipeline_with(device, ip_data());
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();

    let derived = data.get_str(DERIVED_DEFAULT_ELEMENT_DATA_KEY).unwrap();
    let error = derived.get("HumanConfidence").expect_err("no value");
    assert!(
        error.message.contains(
            "'device.BrowserReleaseYear' (element 'device' has no value for \
             'BrowserReleaseYear': the browser was not matched)."
        ),
        "{}",
        error.message
    );
}

#[test]
fn an_element_that_is_not_in_the_pipeline_at_all_is_named() {
    let derived = DerivedPropertyElement::builder()
        .add_built_in(BuiltInScript::HumanConfidence)
        .build()
        .unwrap();
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(StubElement::new("device", device_data())))
        .add_element(Arc::new(derived))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();

    let derived = data.get_str(DERIVED_DEFAULT_ELEMENT_DATA_KEY).unwrap();
    let error = derived.get("HumanConfidence").expect_err("no value");
    assert!(
        error
            .message
            .contains("element 'ip' has no value for 'HumanProbability': property not present"),
        "{}",
        error.message
    );
}

#[test]
fn one_script_can_read_a_property_another_script_produced() {
    let first = "Format: 1\n\
                 Name: Band\n\
                 Version: \"1.0.0\"\n\
                 Output:\n\
                 \x20 Name: Band\n\
                 \x20 Description: The band a probability falls in.\n\
                 \x20 ValueType: string\n\
                 \x20 IsList: false\n\
                 Rules:\n\
                 \x20 - When: { Property: ip.HumanProbability, Ge: 7 }\n\
                 \x20   Then: High\n\
                 \x20 - Else: Low\n";
    let second = "Format: 1\n\
                  Name: BandIsHigh\n\
                  Version: \"1.0.0\"\n\
                  Output:\n\
                  \x20 Name: BandIsHigh\n\
                  \x20 Description: Whether the band is High.\n\
                  \x20 ValueType: bool\n\
                  \x20 IsList: false\n\
                  Rules:\n\
                  \x20 - When: { Property: derived.Band, Eq: \"High\" }\n\
                  \x20   Then: true\n\
                  \x20 - Else: false\n";
    let derived = DerivedPropertyElement::builder()
        .add_script("Band", first)
        .add_script("BandIsHigh", second)
        .build()
        .expect("both scripts");
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(StubElement::new("ip", ip_data())))
        .add_element(Arc::new(derived))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();

    let derived = data.get_str(DERIVED_DEFAULT_ELEMENT_DATA_KEY).unwrap();
    assert_eq!(
        derived.get("Band").unwrap(),
        PropertyValue::String("High".to_owned())
    );
    assert_eq!(
        derived.get("BandIsHigh").unwrap(),
        PropertyValue::Bool(true)
    );
}

#[test]
fn a_32_bit_float_is_read_as_a_number() {
    // A float on its own, and a float as the value of a weighted record, are
    // both read as the number they hold rather than as a list.
    let script = |name: &str, property: &str| {
        format!(
            "Format: 1\n\
             Name: {name}\n\
             Version: \"1.0.0\"\n\
             Output:\n\
             \x20 Name: {name}\n\
             \x20 Description: Whether the value is above 51.3.\n\
             \x20 ValueType: string\n\
             \x20 IsList: false\n\
             Rules:\n\
             \x20 - When: {{ Property: ip.{property}, Gt: 51.3 }}\n\
             \x20   Then: Above\n\
             \x20 - Else: Below\n"
        )
    };
    let weighted = |value: f32, weight: i64| {
        let mut record = std::collections::BTreeMap::new();
        record.insert("value".to_owned(), PropertyValue::Float(value));
        record.insert("weight".to_owned(), PropertyValue::Integer(weight));
        record
    };
    let ip = StubData::default()
        .with("Latitude", PropertyValue::Float(51.352))
        .with(
            "Weighted",
            PropertyValue::KeyValueList(vec![weighted(50.5, 1), weighted(51.5, 2)]),
        );
    let derived = DerivedPropertyElement::builder()
        .add_script("FloatAbove", &script("FloatAbove", "Latitude"))
        .add_script("WeightedAbove", &script("WeightedAbove", "Weighted"))
        .build()
        .expect("both scripts");
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(StubElement::new("ip", ip)))
        .add_element(Arc::new(derived))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();

    let derived = data.get_str(DERIVED_DEFAULT_ELEMENT_DATA_KEY).unwrap();
    assert_eq!(
        derived.get("FloatAbove").unwrap(),
        PropertyValue::String("Above".to_owned())
    );
    assert_eq!(
        derived.get("WeightedAbove").unwrap(),
        PropertyValue::String("Above".to_owned())
    );
}

#[test]
fn the_element_publishes_metadata_for_every_script() {
    let element = DerivedPropertyElement::builder()
        .add_built_in(BuiltInScript::HumanConfidence)
        .build()
        .unwrap();
    assert_eq!(element.data_key(), DERIVED_DEFAULT_ELEMENT_DATA_KEY);
    let properties = element.properties();
    assert_eq!(properties.len(), 1);
    assert_eq!(properties[0].name, "HumanConfidence");
    assert_eq!(properties[0].element_data_key, "derived");
    assert_eq!(properties[0].value_type, PropertyValueType::String);
    assert_eq!(properties[0].category, "Human Detection");
    // The element reads no evidence, so it advertises none.
    assert!(!element.evidence_key_filter().include("header.user-agent"));
}

#[test]
fn a_builder_reports_what_is_wrong_rather_than_building() {
    let faults = DerivedPropertyElement::builder()
        .build()
        .expect_err("an element with no scripts is a fault");
    assert!(
        faults.to_string().contains("no scripts were added"),
        "{faults}"
    );

    let same = "Format: 1\n\
                Name: Demo\n\
                Version: \"1.0.0\"\n\
                Output:\n\
                \x20 Name: Demo\n\
                \x20 Description: A demonstration.\n\
                \x20 ValueType: string\n\
                \x20 IsList: false\n\
                Rules:\n\
                \x20 - Else: only\n";
    let faults = DerivedPropertyElement::builder()
        .add_script("Demo", same)
        .add_script("Demo", same)
        .build()
        .expect_err("two scripts cannot produce one property");
    assert!(
        faults
            .to_string()
            .contains("two scripts both produce the property 'Demo'"),
        "{faults}"
    );

    let faults = DerivedPropertyElement::builder()
        .add_script("Demo", "Format: 2\n")
        .build()
        .expect_err("a script that does not validate");
    assert!(faults.to_string().contains("Format must be 1"), "{faults}");
}

// A test on WebAssembly has no file system to read from.
#[cfg(not(target_family = "wasm"))]
#[test]
fn a_script_can_be_read_from_a_file() {
    // The shipped scripts are compiled in, and this is the one path that needs
    // a file system, so it is worth proving on its own.
    let folder = std::env::temp_dir().join("fiftyone-derived-properties-test");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("Band.yaml");
    std::fs::write(
        &path,
        "Format: 1\n\
         Name: Band\n\
         Version: \"1.0.0\"\n\
         Output:\n\
         \x20 Name: Band\n\
         \x20 Description: The band a probability falls in.\n\
         \x20 ValueType: string\n\
         \x20 IsList: false\n\
         Rules:\n\
         \x20 - When: { Property: ip.HumanProbability, Ge: 7 }\n\
         \x20   Then: High\n\
         \x20 - Else: Low\n",
    )
    .unwrap();

    let derived = DerivedPropertyElement::builder()
        .add_script_file(&path)
        .build()
        .expect("the script file");
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(StubElement::new("ip", ip_data())))
        .add_element(Arc::new(derived))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();
    assert_eq!(
        data.get_str(DERIVED_DEFAULT_ELEMENT_DATA_KEY)
            .unwrap()
            .get("Band")
            .unwrap(),
        PropertyValue::String("High".to_owned())
    );

    // A file that is not there is reported rather than ignored, and the fault
    // names the file it tried to read.
    let missing = folder.join("NotThere.yaml");
    let faults = DerivedPropertyElement::builder()
        .add_script_file(&missing)
        .build()
        .expect_err("a file that is not there");
    assert!(
        faults
            .to_string()
            .contains("the script file could not be read"),
        "{faults}"
    );
    assert_eq!(faults.faults()[0].script.as_deref(), Some("NotThere"));

    std::fs::remove_file(&path).ok();
}
