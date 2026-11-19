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

//! Evidence an element adds with `FlowData::add_evidence`, read by the
//! elements after it and by the caller, refused from an element of a parallel
//! group, and left out of `FlowData::supplied_evidence`.

use std::any::Any;
use std::sync::Arc;

use fiftyone_pipeline_core::{
    ElementData, Error, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData,
    FlowElement, FlowError, MapElementData, NoValueError, Pipeline, PipelineBuilder,
    PropertyMetaData, PropertyValue, Result, TypedKey,
};

/// Element data holding what an element read from the evidence.
struct SeenData(MapElementData);

impl ElementData for SeenData {
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

type Action = dyn Fn(&mut FlowData) + Send + Sync;

/// An element that runs its action under its own data key.
struct TestElement {
    key: &'static str,
    action: Box<Action>,
    filter: EvidenceKeyFilterWhitelist,
    adds_evidence: bool,
}

impl FlowElement for TestElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        (self.action)(data);
        Ok(())
    }
    fn data_key(&self) -> &str {
        self.key
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
    fn adds_evidence(&self) -> bool {
        self.adds_evidence
    }
}

fn element(
    key: &'static str,
    action: impl Fn(&mut FlowData) + Send + Sync + 'static,
) -> Arc<dyn FlowElement> {
    test_element(key, false, action)
}

fn test_element(
    key: &'static str,
    adds_evidence: bool,
    action: impl Fn(&mut FlowData) + Send + Sync + 'static,
) -> Arc<dyn FlowElement> {
    Arc::new(TestElement {
        key,
        action: Box::new(action),
        filter: EvidenceKeyFilterWhitelist::default(),
        adds_evidence,
    })
}

/// An element that adds each of `added` to the evidence, without saying so
/// with `FlowElement::adds_evidence`.
fn adder(
    key: &'static str,
    added: &'static [(&'static str, &'static str)],
) -> Arc<dyn FlowElement> {
    element(key, move |data| add_each(data, added))
}

/// An element that adds each of `added` to the evidence, and says so.
fn declared_adder(
    key: &'static str,
    added: &'static [(&'static str, &'static str)],
) -> Arc<dyn FlowElement> {
    test_element(key, true, move |data| add_each(data, added))
}

fn add_each(data: &mut FlowData, added: &[(&str, &str)]) {
    for (name, value) in added {
        data.add_evidence(*name, *value);
    }
}

/// Record under `key` what the evidence holds for each of `names`, or
/// "absent".
fn record_seen(data: &mut FlowData, key: &'static str, names: &[&str]) {
    let mut seen = MapElementData::new();
    for name in names {
        let value = data.evidence().get(name).unwrap_or("absent").to_owned();
        seen = seen.set(*name, value);
    }
    data.get_or_add(TypedKey::<SeenData>::new(key), || SeenData(seen))
        .unwrap();
}

/// An element that records what the evidence holds for each of `names`.
fn reader(key: &'static str, names: &'static [&'static str]) -> Arc<dyn FlowElement> {
    element(key, move |data| record_seen(data, key, names))
}

/// What the element under `key` recorded for `name`.
fn seen(data: &FlowData, key: &str, name: &str) -> String {
    match data.get_str(key).unwrap().get(name) {
        Ok(PropertyValue::String(value)) => value,
        other => panic!("no string for {name}: {other:?}"),
    }
}

/// The evidence as each key was first spelled, with its value, in order.
fn entries(evidence: &Evidence) -> Vec<(String, String)> {
    evidence
        .entries()
        .map(|entry| (entry.key_as_given().to_owned(), entry.value().to_owned()))
        .collect()
}

fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
    expected
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

fn supplied() -> Evidence {
    Evidence::builder()
        .add("header.User-Agent", "test")
        .add("query.Sequence", "5")
        .build()
}

fn process(builder: PipelineBuilder) -> FlowData {
    let mut data = builder.build().unwrap().create_flow_data_with(supplied());
    data.process().unwrap();
    data
}

#[test]
fn a_new_key_is_added_last() {
    let data = process(
        Pipeline::builder().add_element(adder("adder", &[("query.session-id", "new session")])),
    );

    assert_eq!(
        entries(data.evidence()),
        pairs(&[
            ("header.User-Agent", "test"),
            ("query.Sequence", "5"),
            ("query.session-id", "new session"),
        ])
    );
}

#[test]
fn a_key_in_another_case_keeps_its_spelling_and_place_and_takes_the_new_value() {
    let data = process(Pipeline::builder().add_element(adder("adder", &[("query.sequence", "6")])));

    assert_eq!(
        entries(data.evidence()),
        pairs(&[("header.User-Agent", "test"), ("query.Sequence", "6")])
    );
    assert_eq!(data.evidence().get("QUERY.SEQUENCE"), Some("6"));
}

#[test]
fn a_later_element_reads_the_added_value() {
    let data = process(
        Pipeline::builder()
            .add_element(reader("before", &["query.session-id"]))
            .add_element(adder("adder", &[("query.session-id", "new session")]))
            .add_element(reader("after", &["query.session-id"])),
    );

    assert_eq!(seen(&data, "before", "query.session-id"), "absent");
    assert_eq!(seen(&data, "after", "query.session-id"), "new session");
}

#[test]
fn the_caller_can_add_evidence_before_processing() {
    let pipeline = Pipeline::builder()
        .add_element(reader("reader", &["query.added"]))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    data.add_evidence("query.added", "by the caller");
    data.process().unwrap();

    assert_eq!(seen(&data, "reader", "query.added"), "by the caller");
}

/// The message of an error recorded for evidence an element of a group tried
/// to add under `key`.
fn refused(key: &str) -> String {
    format!(
        "An element of a parallel group tried to add the evidence '{key}', which was refused, \
         because the other elements of the group read the evidence at the same time. Add the \
         element on its own, before the elements that read the value."
    )
}

fn configuration_message(error: &FlowError) -> String {
    match &error.source {
        Error::PipelineConfiguration { message } => message.clone(),
        other => panic!("not a configuration error: {other}"),
    }
}

/// An element of a group that adds evidence without saying so adds nothing,
/// so it, the other element of the group and the element after the group all
/// see the evidence from before the group. The pipeline records a
/// configuration error against it for each key, in the order it tried them,
/// and with errors suppressed processing carries on.
#[test]
fn evidence_an_element_of_a_group_adds_is_refused_with_an_error_for_each_key() {
    const NAMES: &[&str] = &["query.added", "query.sequence"];
    let group = [
        element("adder", |data| {
            data.add_evidence("query.added", "1");
            data.add_evidence("QUERY.SEQUENCE", "6");
            record_seen(data, "adder", NAMES);
        }),
        reader("other", NAMES),
    ];
    let pipeline = Pipeline::builder()
        .add_parallel_elements(group)
        .add_element(reader("after", NAMES))
        .suppress_process_exceptions(true)
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data_with(supplied());
    data.process().unwrap();

    for key in ["adder", "other", "after"] {
        assert_eq!(seen(&data, key, "query.added"), "absent", "{key}");
        assert_eq!(seen(&data, key, "query.sequence"), "5", "{key}");
    }
    assert_eq!(
        entries(data.evidence()),
        pairs(&[("header.User-Agent", "test"), ("query.Sequence", "5")])
    );
    let errors: Vec<(&str, String)> = data
        .errors()
        .iter()
        .map(|error| {
            (
                error.element_data_key.as_str(),
                configuration_message(error),
            )
        })
        .collect();
    assert_eq!(
        errors,
        [
            ("adder", refused("query.added")),
            ("adder", refused("QUERY.SEQUENCE")),
        ]
    );
}

/// With errors not suppressed, refused evidence fails processing with the
/// error, once every element has run.
#[test]
fn refused_evidence_fails_processing_unless_errors_are_suppressed() {
    let pipeline = Pipeline::builder()
        .add_parallel_elements([
            adder("adder", &[("query.added", "1")]),
            reader("other", &["query.added"]),
        ])
        .add_element(reader("after", &["query.added"]))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data_with(supplied());

    match data.process() {
        Err(Error::Aggregate(errors)) => {
            assert_eq!(errors.len(), 1, "{errors:?}");
            assert_eq!(errors[0].element_data_key, "adder");
            assert_eq!(configuration_message(&errors[0]), refused("query.added"));
        }
        other => panic!("not an aggregate error: {other:?}"),
    }
    assert_eq!(seen(&data, "after", "query.added"), "absent");
    assert_eq!(data.evidence().get("query.added"), None);
}

/// A pipeline refuses a group holding an element that says it adds
/// evidence, wherever it is in the group. The element builds on its own, and
/// as a group of one, which is the element on its own.
#[test]
fn a_group_holding_an_element_that_adds_evidence_is_refused() {
    const ADDED: &[(&str, &str)] = &[("query.added", "1")];
    const EXPECTED: &str = "An element of a parallel group with the data key 'Adder' adds \
                            evidence, which the other elements of the group read at the same \
                            time. Add it on its own, before the elements that read what it adds.";
    for group in [
        vec![declared_adder("Adder", ADDED), reader("other", &[])],
        vec![reader("other", &[]), declared_adder("Adder", ADDED)],
    ] {
        match Pipeline::builder()
            .add_parallel_elements(group)
            .build()
            .err()
        {
            Some(Error::PipelineConfiguration { message }) => assert_eq!(message, EXPECTED),
            Some(other) => panic!("not a configuration error: {other}"),
            None => panic!("the pipeline was built"),
        }
    }

    for builder in [
        Pipeline::builder().add_element(declared_adder("adder", ADDED)),
        Pipeline::builder().add_parallel_elements([declared_adder("adder", ADDED)]),
    ] {
        let data = process(builder.add_element(reader("after", &["query.added"])));
        assert_eq!(seen(&data, "after", "query.added"), "1");
    }
}

/// Record under `key` what the supplied evidence holds for each of `names`,
/// or "absent".
fn record_supplied(data: &mut FlowData, key: &'static str, names: &[&str]) {
    let mut seen = MapElementData::new();
    for name in names {
        let value = data
            .supplied_evidence()
            .get(name)
            .unwrap_or("absent")
            .to_owned();
        seen = seen.set(*name, value);
    }
    data.get_or_add(TypedKey::<SeenData>::new(key), || SeenData(seen))
        .unwrap();
}

/// The supplied evidence leaves out a key an element added and keeps the
/// value a key had before an element replaced it.
#[test]
fn the_supplied_evidence_leaves_out_what_elements_added_and_replaced() {
    let data = process(Pipeline::builder().add_element(adder(
        "adder",
        &[("query.session-id", "new session"), ("query.sequence", "6")],
    )));

    assert_eq!(
        entries(data.supplied_evidence()),
        pairs(&[("header.User-Agent", "test"), ("query.Sequence", "5")])
    );
    assert_eq!(
        entries(data.evidence()),
        pairs(&[
            ("header.User-Agent", "test"),
            ("query.Sequence", "6"),
            ("query.session-id", "new session"),
        ])
    );
}

#[test]
fn the_supplied_evidence_is_the_evidence_when_nothing_is_added() {
    let data = process(Pipeline::builder().add_element(reader("reader", &[])));

    assert_eq!(entries(data.supplied_evidence()), entries(&supplied()));
    assert_eq!(entries(data.evidence()), entries(&supplied()));
}

/// Evidence the caller adds before processing is supplied, and evidence it
/// adds after processing is not.
#[test]
fn the_supplied_evidence_holds_what_the_caller_added_before_processing() {
    let pipeline = Pipeline::builder()
        .add_element(reader("reader", &[]))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data_with(supplied());
    data.add_evidence("query.before", "1");
    assert_eq!(data.supplied_evidence().get("query.before"), Some("1"));

    data.process().unwrap();
    data.add_evidence("query.after", "2");
    data.add_evidence("query.sequence", "7");

    assert_eq!(
        entries(data.supplied_evidence()),
        pairs(&[
            ("header.User-Agent", "test"),
            ("query.Sequence", "5"),
            ("query.before", "1"),
        ])
    );
    assert_eq!(data.evidence().get("query.after"), Some("2"));
    assert_eq!(data.evidence().get("query.sequence"), Some("7"));
}

/// An element after the one that added evidence reads the supplied evidence
/// without the added values, on its own and in a parallel group, and the
/// flow data still holds it once the group has finished.
#[test]
fn every_later_element_reads_the_supplied_evidence() {
    const NAMES: &[&str] = &["query.session-id", "query.sequence"];
    let data = process(
        Pipeline::builder()
            .add_element(adder(
                "adder",
                &[("query.session-id", "new session"), ("query.sequence", "6")],
            ))
            .add_element(element("alone", |data| {
                record_supplied(data, "alone", NAMES)
            }))
            .add_parallel_elements([
                element("first", |data| record_supplied(data, "first", NAMES)),
                element("second", |data| record_supplied(data, "second", NAMES)),
            ]),
    );

    for key in ["alone", "first", "second"] {
        assert_eq!(seen(&data, key, "query.session-id"), "absent", "{key}");
        assert_eq!(seen(&data, key, "query.sequence"), "5", "{key}");
    }
    assert_eq!(
        entries(data.supplied_evidence()),
        pairs(&[("header.User-Agent", "test"), ("query.Sequence", "5")])
    );
}
