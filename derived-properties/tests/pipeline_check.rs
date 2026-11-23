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

//! The check the element makes when a pipeline adds it, as the
//! specification's "The Pipeline check" sets it out. Each message is the
//! one .NET's FiftyOne.Pipeline.DerivedProperty 4.5.129 gives, which joins
//! its lines with the platform's line ending where the crate always uses
//! `\n`.

use std::sync::Arc;

use fiftyone_derived_properties::{BuiltInScript, DerivedPropertyElement, Script};
use fiftyone_pipeline_core::{
    Error, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, Pipeline,
    PropertyMetaData, PropertyValueType, Result,
};

/// An element that lists properties and writes nothing, standing in for an
/// engine.
struct Supplier {
    key: &'static str,
    properties: Vec<PropertyMetaData>,
    loaded: bool,
    filter: EvidenceKeyFilterWhitelist,
}

impl FlowElement for Supplier {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
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

    fn properties_loaded(&self) -> bool {
        self.loaded
    }
}

/// An element with the data key `key` listing `names`.
fn supplier(
    key: &'static str,
    names: impl IntoIterator<Item = impl Into<String>>,
) -> Arc<dyn FlowElement> {
    Arc::new(Supplier {
        key,
        properties: names
            .into_iter()
            .map(|name| PropertyMetaData::new(name, key, PropertyValueType::String))
            .collect(),
        loaded: true,
        filter: EvidenceKeyFilterWhitelist::new(Vec::<&str>::new()),
    })
}

/// An element with the data key `key` that has yet to load its properties,
/// as a cloud engine is when the request for its property list failed at
/// start-up.
fn not_loaded(key: &'static str) -> Arc<dyn FlowElement> {
    Arc::new(Supplier {
        key,
        properties: Vec::new(),
        loaded: false,
        filter: EvidenceKeyFilterWhitelist::new(Vec::<&str>::new()),
    })
}

/// An element that wraps another and passes every call on to it, as a host
/// does to change when the element runs.
struct Wrapped(Arc<dyn FlowElement>);

impl FlowElement for Wrapped {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        self.0.process(data)
    }

    fn data_key(&self) -> &str {
        self.0.data_key()
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        self.0.evidence_key_filter()
    }

    fn is_evidence_key_filter_complete(&self) -> bool {
        self.0.is_evidence_key_filter_complete()
    }

    fn properties(&self) -> &[PropertyMetaData] {
        self.0.properties()
    }

    fn properties_loaded(&self) -> bool {
        self.0.properties_loaded()
    }

    fn is_concurrent(&self) -> bool {
        self.0.is_concurrent()
    }

    fn type_name(&self) -> &'static str {
        self.0.type_name()
    }

    fn add_pipeline(&self, pipeline: &Arc<Pipeline>) -> Result<()> {
        self.0.add_pipeline(pipeline)
    }
}

fn human_confidence() -> Script {
    BuiltInScript::HumanConfidence
        .compile()
        .expect("the shipped script is sound")
}

/// The properties the shipped HumanConfidence reads under `key`, in the
/// order the script first names them.
fn read_by_human_confidence(key: &str) -> Vec<String> {
    human_confidence()
        .source_properties()
        .iter()
        .filter(|property| property.element_key == key)
        .map(|property| property.property_name.clone())
        .collect()
}

/// An element running the shipped HumanConfidence.
fn human_confidence_element() -> Arc<dyn FlowElement> {
    Arc::new(
        DerivedPropertyElement::builder()
            .add_built_in(BuiltInScript::HumanConfidence)
            .build()
            .expect("the shipped script is sound"),
    )
}

/// A script producing `name` from `source`, written as `element.Property`,
/// or from nothing where `source` is empty.
fn script(name: &str, source: &str) -> String {
    let mut text = format!(
        "Format: 1\nName: {name}\nVersion: 1.0.0\nOutput:\n  Name: {name}\n  Description: A \
         demonstration.\n  ValueType: string\n  IsList: false\nRules:\n"
    );
    if !source.is_empty() {
        text.push_str(&format!(
            "  - When: {{ Property: {source}, Lt: 2 }}\n    Then: \"yes\"\n"
        ));
    }
    text.push_str("  - Else: \"no\"\n");
    text
}

/// An element running one script for each name and source given.
fn derived(scripts: &[(&str, &str)]) -> Arc<dyn FlowElement> {
    let builder = scripts.iter().fold(
        DerivedPropertyElement::builder(),
        |builder, (name, source)| builder.add_script(name, &script(name, source)),
    );
    Arc::new(builder.build().expect("the scripts are sound"))
}

fn message(result: Result<Arc<Pipeline>>) -> String {
    match result {
        Err(Error::PipelineConfiguration { message }) => message,
        Err(other) => panic!("not a configuration error: {other}"),
        Ok(_) => panic!("the pipeline was built"),
    }
}

/// .NET's message for a pipeline the element cannot be used in.
fn refused(lines: &[String]) -> String {
    let mut message = "The derived property element cannot be used in this pipeline.".to_owned();
    for line in lines {
        message.push('\n');
        message.push_str(line);
    }
    message
}

/// .NET's line for a source property no element supplies.
fn needs(script: &str, property: &str) -> String {
    format!(
        "The script '{script}' needs the property '{property}', and no element in the pipeline \
         supplies it. Either add the element that supplies '{property}', or change the script so \
         that it does not name '{property}'."
    )
}

/// .NET's line for a derived property another element also writes.
fn writes(script: &str, property: &str) -> String {
    format!(
        "The script '{script}' writes the derived property '{property}', and another element in \
         the pipeline already writes a property of that name. One pipeline writes each derived \
         property once."
    )
}

/// The lines for every property HumanConfidence reads under `key`.
fn needs_from(key: &str) -> Vec<String> {
    read_by_human_confidence(key)
        .iter()
        .map(|name| needs("HumanConfidence", &format!("{key}.{name}")))
        .collect()
}

#[test]
fn a_pipeline_supplying_no_source_property_names_each_one() {
    let result = Pipeline::builder()
        .add_element(human_confidence_element())
        .build();
    let lines: Vec<String> = human_confidence()
        .source_properties()
        .iter()
        .map(|property| needs("HumanConfidence", &property.name))
        .collect();
    assert_eq!(lines.len(), 10);
    assert_eq!(message(result), refused(&lines));
}

#[test]
fn suppliers_may_sit_anywhere_in_the_pipeline() {
    let device = || supplier("device", read_by_human_confidence("device"));
    let ip = || supplier("ip", read_by_human_confidence("ip"));

    Pipeline::builder()
        .add_element(device())
        .add_element(human_confidence_element())
        .add_element(ip())
        .build()
        .expect("one supplier before and one after");
    Pipeline::builder()
        .add_parallel_elements([device(), ip()])
        .add_element(human_confidence_element())
        .build()
        .expect("both suppliers in a parallel group before");
    Pipeline::builder()
        .add_element(human_confidence_element())
        .add_parallel_elements([device(), ip()])
        .build()
        .expect("both suppliers in a parallel group after");
}

#[test]
fn keys_and_names_are_compared_ignoring_case() {
    let lower = |key: &str| -> Vec<String> {
        read_by_human_confidence(key)
            .iter()
            .map(|name| name.to_lowercase())
            .collect()
    };
    Pipeline::builder()
        .add_element(supplier("DEVICE", lower("device")))
        .add_element(supplier("Ip", lower("ip")))
        .add_element(human_confidence_element())
        .build()
        .expect("the same keys and names in other case");
}

#[test]
fn a_property_listed_under_another_key_is_not_supplied() {
    let result = Pipeline::builder()
        .add_element(supplier("device", read_by_human_confidence("device")))
        .add_element(supplier("location", read_by_human_confidence("ip")))
        .add_element(human_confidence_element())
        .build();
    assert_eq!(message(result), refused(&needs_from("ip")));
}

#[test]
fn an_element_yet_to_load_its_properties_may_supply_any_of_them() {
    Pipeline::builder()
        .add_element(supplier("device", read_by_human_confidence("device")))
        .add_element(not_loaded("ip"))
        .add_element(human_confidence_element())
        .build()
        .expect("the ip element may list its properties once loaded");

    // It supplies nothing under any other key.
    let result = Pipeline::builder()
        .add_element(not_loaded("ip"))
        .add_element(human_confidence_element())
        .build();
    assert_eq!(message(result), refused(&needs_from("device")));
}

#[test]
fn two_elements_writing_one_derived_property_are_refused() {
    let result = Pipeline::builder()
        .add_element(supplier("src", ["N"]))
        .add_element(derived(&[("Demo", "src.N")]))
        .add_element(derived(&[("Demo", "src.N")]))
        .build();
    assert_eq!(message(result), refused(&[writes("Demo", "Demo")]));

    // Letter case does not make two properties of one.
    let result = Pipeline::builder()
        .add_element(derived(&[("Demo", "")]))
        .add_element(derived(&[("DEMO", "")]))
        .build();
    assert_eq!(message(result), refused(&[writes("Demo", "Demo")]));

    Pipeline::builder()
        .add_element(supplier("src", ["N"]))
        .add_element(derived(&[("Demo", "src.N")]))
        .add_element(derived(&[("Other", "src.N")]))
        .build()
        .expect("two elements writing different properties");

    // Only an element with the data key `derived` writes a derived property.
    Pipeline::builder()
        .add_element(supplier("src", ["N", "Demo"]))
        .add_element(derived(&[("Demo", "src.N")]))
        .build()
        .expect("a property of the same name under another key");
}

#[test]
fn a_script_may_read_what_an_earlier_script_of_its_element_writes() {
    Pipeline::builder()
        .add_element(supplier("src", ["N"]))
        .add_element(derived(&[
            ("Band", "src.N"),
            ("BandIsHigh", "derived.Band"),
        ]))
        .build()
        .expect("derived.Band is written by an earlier script");

    // A later script's property has no value when an earlier one reads it.
    let result = Pipeline::builder()
        .add_element(supplier("src", ["N"]))
        .add_element(derived(&[
            ("BandIsHigh", "derived.Band"),
            ("Band", "src.N"),
        ]))
        .build();
    assert_eq!(
        message(result),
        refused(&[needs("BandIsHigh", "derived.Band")])
    );

    // A script's own property is not there when it runs either.
    let result = Pipeline::builder()
        .add_element(derived(&[("Demo", "derived.Demo")]))
        .build();
    assert_eq!(message(result), refused(&[needs("Demo", "derived.Demo")]));

    let result = Pipeline::builder()
        .add_element(derived(&[("Demo", "derived.Missing")]))
        .build();
    assert_eq!(
        message(result),
        refused(&[needs("Demo", "derived.Missing")])
    );
}

#[test]
fn another_derived_property_element_supplies_what_it_writes_wherever_it_sits() {
    Pipeline::builder()
        .add_element(supplier("src", ["N"]))
        .add_element(derived(&[("BandIsHigh", "derived.Band")]))
        .add_element(derived(&[("Band", "src.N")]))
        .build()
        .expect("derived.Band is written by another element");
}

#[test]
fn an_element_inside_a_wrapper_passing_calls_on_is_not_another_writer() {
    Pipeline::builder()
        .add_element(supplier("device", read_by_human_confidence("device")))
        .add_element(supplier("ip", read_by_human_confidence("ip")))
        .add_element(Arc::new(Wrapped(human_confidence_element())))
        .build()
        .expect("the wrapper gives the element's own properties");

    // Through the wrapper, earlier scripts still supply and another element
    // writing the same property is still refused.
    Pipeline::builder()
        .add_element(supplier("src", ["N"]))
        .add_element(Arc::new(Wrapped(derived(&[
            ("Band", "src.N"),
            ("BandIsHigh", "derived.Band"),
        ]))))
        .build()
        .expect("derived.Band is written by an earlier script");
    let result = Pipeline::builder()
        .add_element(supplier("src", ["N"]))
        .add_element(Arc::new(Wrapped(derived(&[("Demo", "src.N")]))))
        .add_element(derived(&[("Demo", "src.N")]))
        .build();
    assert_eq!(message(result), refused(&[writes("Demo", "Demo")]));
}

#[test]
fn the_message_gives_properties_written_twice_before_those_not_supplied() {
    let result = Pipeline::builder()
        .add_element(derived(&[("Demo", "gone.N")]))
        .add_element(derived(&[("Demo", "")]))
        .build();
    assert_eq!(
        message(result),
        refused(&[writes("Demo", "Demo"), needs("Demo", "gone.N")])
    );
}
