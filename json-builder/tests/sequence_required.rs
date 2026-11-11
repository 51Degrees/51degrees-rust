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

//! The JSON builder fails, and sets no JSON, when no sequence element has
//! produced a sequence number.
//!
//! See the [JSON Builder specification](https://github.com/51Degrees/specifications/blob/e89679e9/pipeline-specification/pipeline-elements/json-builder.md#sequence-number),
//! which says the element MUST fail with an error and MUST NOT set its JSON
//! value, that the error SHOULD say a sequence element needs to be placed
//! before it, and that a `query.sequence` value taken from a query string is
//! text rather than a sequence number.

use std::any::Any;
use std::sync::Arc;

use fiftyone_json_builder::{JsonBuilderElement, JSON_BUILDER_DATA_KEY};
use fiftyone_pipeline_core::{
    ElementData, Error, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData,
    FlowElement, MapElementData, NoValueError, Pipeline, PropertyMetaData, PropertyValue,
    PropertyValueType, Result, TypedKey,
};
use fiftyone_pipeline_engines_fiftyone::SequenceElement;

// ---------------------------------------------------------------------------
// A one-property element, so the JSON has something to carry.
// ---------------------------------------------------------------------------

struct DeviceData(MapElementData);

impl ElementData for DeviceData {
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

struct DeviceElement {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

impl DeviceElement {
    const KEY: TypedKey<DeviceData> = TypedKey::new("device");

    fn new() -> Self {
        DeviceElement {
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
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
        data.get_or_add(Self::KEY, || {
            DeviceData(MapElementData::new().set("ismobile", true))
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// What one request produced: whether the JSON builder set a JSON value, and
/// the error it recorded or returned.
struct Outcome {
    json: Option<String>,
    error: Option<String>,
}

/// Process one request through a pipeline of the device element and the JSON
/// builder, with a sequence element first when `with_sequence_element` is true,
/// and report what the JSON builder did.
///
/// The run is repeated with the pipeline's process exceptions suppressed and
/// not suppressed, because the specification leaves which of the two the
/// pipeline does to its exception handling, and the element has to behave the
/// same either way.
fn run(with_sequence_element: bool, evidence: &[(&str, &str)], suppress: bool) -> Outcome {
    let mut builder = Pipeline::builder().suppress_process_exceptions(suppress);
    if with_sequence_element {
        builder = builder.add_element(Arc::new(SequenceElement::new()));
    }
    let pipeline = builder
        .add_element(Arc::new(DeviceElement::new()))
        .add_element(Arc::new(JsonBuilderElement::new()))
        .build()
        .expect("pipeline builds");

    let mut ev = Evidence::builder();
    for (key, value) in evidence {
        ev = ev.add(*key, *value);
    }
    let mut data = pipeline.create_flow_data_with(ev.build());

    let returned = data.process();
    let error = match returned {
        Err(error) => Some(error.to_string()),
        Ok(()) => data.errors().first().map(|error| error.to_string()),
    };
    let json = data
        .get(JSON_BUILDER_DATA_KEY)
        .map(|element_data| element_data.json().to_owned());

    Outcome { json, error }
}

/// The message the specification gives, written out here rather than read
/// from the element, so that a change to the element's own text fails this
/// test instead of moving it along.
const SPECIFICATION_MESSAGE: &str = "The JSON Builder needs a sequence number. \
     Place a Sequence Element before the JSON Builder in the Pipeline.";

/// Assert that the request failed, that no JSON was set and that the error
/// carries the message the specification gives, word for word.
fn assert_failed(outcome: &Outcome) {
    let error = outcome
        .error
        .as_deref()
        .expect("the JSON builder ran with no sequence number");
    assert!(
        error.contains(SPECIFICATION_MESSAGE),
        "the error does not carry the message the specification gives, \
         got {error}"
    );
    assert!(
        outcome.json.is_none(),
        "a JSON value was set, got {:?}",
        outcome.json
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn fails_with_no_sequence_element_and_no_evidence() {
    for suppress in [true, false] {
        assert_failed(&run(false, &[], suppress));
    }
}

#[test]
fn fails_when_the_sequence_is_only_text_in_the_query_string() {
    // A query string carries text. The page says text is not a sequence
    // number, so the element fails just as it does with no evidence at all.
    for suppress in [true, false] {
        assert_failed(&run(false, &[("query.sequence", "4")], suppress));
    }
}

#[test]
fn fails_when_the_text_is_not_a_number_either() {
    for suppress in [true, false] {
        assert_failed(&run(false, &[("query.sequence", "abc")], suppress));
    }
}

#[test]
fn builds_the_json_with_a_sequence_element() {
    for suppress in [true, false] {
        let outcome = run(true, &[], suppress);
        assert!(
            outcome.error.is_none(),
            "a sequence element was present, got {:?}",
            outcome.error
        );
        let json = outcome.json.expect("a JSON value was set");
        assert!(
            json.contains("\"ismobile\""),
            "the JSON carries the device data, got {json}"
        );
    }
}

#[test]
fn the_error_names_the_json_builder_element() {
    // The pipeline records the error against the element that raised it, so
    // whoever reads the flow data's errors can tell which element failed.
    let mut builder = Pipeline::builder().suppress_process_exceptions(true);
    builder = builder.add_element(Arc::new(DeviceElement::new()));
    let pipeline = builder
        .add_element(Arc::new(JsonBuilderElement::new()))
        .build()
        .expect("pipeline builds");

    let mut data = pipeline.create_flow_data();
    data.process().expect("exceptions are suppressed");

    let recorded = data.errors().first().expect("one error recorded");
    assert_eq!(
        recorded.element_data_key,
        fiftyone_json_builder::JSON_BUILDER_ELEMENT_DATA_KEY
    );
    assert!(
        matches!(recorded.source, Error::PipelineConfiguration { .. }),
        "a missing sequence element is a configuration fault, got {:?}",
        recorded.source
    );
}
