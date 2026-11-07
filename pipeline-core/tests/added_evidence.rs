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
//! elements after it and by the caller, on its own and in a parallel group.

use std::any::Any;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use fiftyone_pipeline_core::{
    ElementData, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    MapElementData, NoValueError, Pipeline, PipelineBuilder, PropertyMetaData, PropertyValue,
    Result, TypedKey,
};

/// How long the first element of a group waits, so the second finishes
/// first.
const WAIT_MILLISECONDS: u64 = 200;

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
}

fn element(
    key: &'static str,
    action: impl Fn(&mut FlowData) + Send + Sync + 'static,
) -> Arc<dyn FlowElement> {
    Arc::new(TestElement {
        key,
        action: Box::new(action),
        filter: EvidenceKeyFilterWhitelist::default(),
    })
}

/// An element that adds each of `added` to the evidence.
fn adder(
    key: &'static str,
    added: &'static [(&'static str, &'static str)],
) -> Arc<dyn FlowElement> {
    element(key, move |data| {
        for (name, value) in added {
            data.add_evidence(*name, *value);
        }
    })
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

/// Each element of a group sees the evidence from before the group and what
/// it added itself, never what another adds. Afterwards the group's additions
/// are in the evidence in the order the elements were given, though the first
/// finishes last, and of two adding one key the later element's value is kept.
#[test]
fn each_element_of_a_group_sees_its_own_added_evidence_and_not_anothers() {
    const NAMES: &[&str] = &[
        "header.user-agent",
        "query.slower",
        "query.faster",
        "query.shared",
    ];
    let group = [
        element("slower", |data| {
            thread::sleep(Duration::from_millis(WAIT_MILLISECONDS));
            data.add_evidence("query.shared", "from slower");
            data.add_evidence("query.slower", "1");
            record_seen(data, "slower", NAMES);
        }),
        element("faster", |data| {
            record_seen(data, "faster-first", NAMES);
            data.add_evidence("query.faster", "2");
            data.add_evidence("query.shared", "from faster");
            record_seen(data, "faster", NAMES);
        }),
    ];
    let data = process(
        Pipeline::builder()
            .add_parallel_elements(group)
            .add_element(reader("after", NAMES)),
    );

    let seen_by =
        |key: &str| -> Vec<String> { NAMES.iter().map(|name| seen(&data, key, name)).collect() };
    assert_eq!(
        seen_by("faster-first"),
        ["test", "absent", "absent", "absent"]
    );
    assert_eq!(seen_by("slower"), ["test", "1", "absent", "from slower"]);
    assert_eq!(seen_by("faster"), ["test", "absent", "2", "from faster"]);
    assert_eq!(seen_by("after"), ["test", "1", "2", "from faster"]);
    assert_eq!(
        entries(data.evidence()),
        pairs(&[
            ("header.User-Agent", "test"),
            ("query.Sequence", "5"),
            ("query.shared", "from faster"),
            ("query.slower", "1"),
            ("query.faster", "2"),
        ])
    );
}

/// An element of a group that adds a key already supplied sees its value in
/// place of the supplied one, keeping the supplied spelling and place, and
/// the others see the supplied value.
#[test]
fn an_element_of_a_group_replacing_a_supplied_value_sees_its_own() {
    const NAMES: &[&str] = &["query.sequence"];
    let group = [
        element("changer", |data| {
            data.add_evidence("QUERY.SEQUENCE", "6");
            record_seen(data, "changer", NAMES);
            assert_eq!(
                entries(data.evidence()),
                pairs(&[("header.User-Agent", "test"), ("query.Sequence", "6")])
            );
        }),
        element("other", |data| {
            thread::sleep(Duration::from_millis(WAIT_MILLISECONDS));
            record_seen(data, "other", NAMES);
        }),
    ];
    let data = process(Pipeline::builder().add_parallel_elements(group));

    assert_eq!(seen(&data, "changer", "query.sequence"), "6");
    assert_eq!(seen(&data, "other", "query.sequence"), "5");
    assert_eq!(
        entries(data.evidence()),
        pairs(&[("header.User-Agent", "test"), ("query.Sequence", "6")])
    );
}
