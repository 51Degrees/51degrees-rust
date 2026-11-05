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

//! Parallel groups: elements added with
//! `PipelineBuilder::add_parallel_elements` run at the same time on one flow
//! data, and give the values and errors the same elements give in sequence.

use std::any::Any;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use fiftyone_pipeline_core::{
    ElementData, Error, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData,
    FlowElement, MapElementData, NoValueError, Pipeline, PipelineBuilder, PropertyMetaData,
    PropertyValue, PropertyValueType, Result, TypedKey,
};

/// How long each of the two slow elements waits.
const WAIT_MILLISECONDS: u64 = 200;

/// The elements of a group read the data added before the group from several
/// threads at once, so element data and flow data are `Send + Sync`.
#[test]
fn flow_data_and_element_data_are_send_and_sync() {
    fn send_and_sync<T: Send + Sync + ?Sized>() {}
    send_and_sync::<FlowData>();
    send_and_sync::<dyn ElementData>();
}

/// Element data holding string values.
struct ValueData(MapElementData);

impl ElementData for ValueData {
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

type Action = dyn Fn(&TestElement, &mut FlowData) -> Result<()> + Send + Sync;

/// An element that does whatever its action says, under its own data key.
struct TestElement {
    key: &'static str,
    action: Box<Action>,
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

/// A test element with the data key `key`, accepting `evidence_keys`.
fn element(
    key: &'static str,
    evidence_keys: &[&str],
    action: impl Fn(&TestElement, &mut FlowData) -> Result<()> + Send + Sync + 'static,
) -> Arc<dyn FlowElement> {
    Arc::new(TestElement {
        key,
        action: Box::new(action),
        filter: EvidenceKeyFilterWhitelist::new(evidence_keys.iter().copied()),
        properties: vec![PropertyMetaData::new(
            "value",
            key,
            PropertyValueType::String,
        )],
    })
}

impl TestElement {
    /// Add this element's data with one value.
    fn write(&self, data: &mut FlowData, value: impl Into<String>) -> Result<()> {
        let value = value.into();
        data.get_or_add(TypedKey::<ValueData>::new(self.key), || {
            ValueData(MapElementData::new().set("value", value))
        })?;
        Ok(())
    }
}

impl FlowElement for TestElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        (self.action)(self, data)
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

/// An element that writes `value` under `key`.
fn writer(key: &'static str, value: &'static str) -> Arc<dyn FlowElement> {
    element(key, &[], move |element, data| element.write(data, value))
}

/// An element that always fails with `message`.
fn failing(key: &'static str, message: &'static str) -> Arc<dyn FlowElement> {
    element(key, &[], move |_, _| Err(Error::configuration(message)))
}

/// A string value another element added, or a word saying why there is none.
fn read(data: &FlowData, key: &str) -> String {
    match data.get_str(key) {
        Some(found) => show(found.get("value")),
        None => "absent".to_owned(),
    }
}

/// A property value as text, a string as itself.
fn show(value: std::result::Result<PropertyValue, NoValueError>) -> String {
    match value {
        Ok(PropertyValue::String(value)) => value,
        Ok(other) => format!("{other:?}"),
        Err(error) => format!("no value: {error}"),
    }
}

/// Every element data's values, by data key and property name.
fn values(data: &FlowData) -> BTreeMap<String, BTreeMap<String, String>> {
    data.data_keys()
        .into_iter()
        .map(|key| {
            let found = data.get_str(&key).unwrap();
            let properties = found
                .keys()
                .into_iter()
                .map(|name| {
                    let value = show(found.get(&name));
                    (name, value)
                })
                .collect();
            (key, properties)
        })
        .collect()
}

/// Every error recorded, as the element's data key and the message.
fn errors(data: &FlowData) -> Vec<(String, String)> {
    data.errors()
        .iter()
        .map(|error| (error.element_data_key.clone(), error.source.to_string()))
        .collect()
}

/// What processing gave: its returned error, the values and the errors.
#[derive(Debug, PartialEq)]
struct Outcome {
    returned: Option<String>,
    values: BTreeMap<String, BTreeMap<String, String>>,
    errors: Vec<(String, String)>,
}

/// Process `evidence` with `first`, then `group` as a parallel group or in
/// sequence, then `last`.
fn run(
    first: &[Arc<dyn FlowElement>],
    group: &[Arc<dyn FlowElement>],
    last: &[Arc<dyn FlowElement>],
    parallel: bool,
    suppress: bool,
    evidence: Evidence,
) -> Outcome {
    let mut builder = Pipeline::builder().suppress_process_exceptions(suppress);
    for element in first {
        builder = builder.add_element(Arc::clone(element));
    }
    if parallel {
        builder = builder.add_parallel_elements(group.iter().cloned());
    } else {
        for element in group {
            builder = builder.add_element(Arc::clone(element));
        }
    }
    for element in last {
        builder = builder.add_element(Arc::clone(element));
    }
    let pipeline = builder.build().unwrap();
    assert_eq!(pipeline.is_concurrent(), parallel && group.len() > 1);
    let mut data = pipeline.create_flow_data_with(evidence);
    let returned = data.process().err().map(|error| error.to_string());
    Outcome {
        returned,
        values: values(&data),
        errors: errors(&data),
    }
}

fn evidence() -> Evidence {
    Evidence::builder()
        .add("query.colour", "blue")
        .add("header.user-agent", "test")
        .build()
}

/// Tests that need threads, which WebAssembly here cannot start. There the
/// group's elements run one after another, as
/// `without_threads_a_group_runs_on_the_calling_thread` shows.
#[cfg(not(target_family = "wasm"))]
mod at_the_same_time {
    use std::sync::{Condvar, Mutex};
    use std::time::Instant;

    use super::*;

    /// The longest two slow elements in a group may take together. A sleep
    /// never ends early, so in sequence they take at least this long, and the
    /// margin over one wait allows for a busy machine waking threads late.
    const GROUP_LIMIT_MILLISECONDS: u64 = 2 * WAIT_MILLISECONDS;
    /// How long an element waits for the others of its group to start, long
    /// enough that only elements run one after another reach it.
    const MEETING_SECONDS: u64 = 10;

    /// An element that waits, then writes.
    fn slow(key: &'static str) -> Arc<dyn FlowElement> {
        element(key, &[], |element, data| {
            thread::sleep(Duration::from_millis(WAIT_MILLISECONDS));
            element.write(data, "waited")
        })
    }

    #[test]
    fn the_elements_of_a_group_run_at_the_same_time() {
        let pipeline = Pipeline::builder()
            .add_parallel_elements([slow("one"), slow("two")])
            .build()
            .unwrap();
        let mut data = pipeline.create_flow_data();

        let started = Instant::now();
        data.process().unwrap();
        let took = started.elapsed();

        assert!(
            took < Duration::from_millis(GROUP_LIMIT_MILLISECONDS),
            "two elements that each wait {WAIT_MILLISECONDS} ms took {took:?}"
        );
        assert_eq!(read(&data, "one"), "waited");
        assert_eq!(read(&data, "two"), "waited");
    }

    /// A point that elements reach one at a time and wait at until all of them
    /// have reached it, as a .NET `Barrier` does.
    struct Meeting {
        expected: usize,
        arrived: Mutex<usize>,
        all_here: Condvar,
    }

    impl Meeting {
        fn new(expected: usize) -> Arc<Self> {
            Arc::new(Meeting {
                expected,
                arrived: Mutex::new(0),
                all_here: Condvar::new(),
            })
        }

        /// Arrive, then wait for the others. False when they did not all arrive
        /// in time, as happens when elements run one after another.
        fn arrive_and_wait(&self) -> bool {
            let mut arrived = self.arrived.lock().unwrap();
            *arrived += 1;
            self.all_here.notify_all();
            let (_arrived, waited) = self
                .all_here
                .wait_timeout_while(arrived, Duration::from_secs(MEETING_SECONDS), |arrived| {
                    *arrived < self.expected
                })
                .unwrap();
            !waited.timed_out()
        }
    }

    #[test]
    fn every_element_of_a_group_starts_before_any_finishes() {
        let keys = ["one", "two", "three"];
        let meeting = Meeting::new(keys.len());
        let group: Vec<_> = keys
            .into_iter()
            .map(|key| {
                let meeting = Arc::clone(&meeting);
                element(key, &[], move |element, data| {
                    if meeting.arrive_and_wait() {
                        element.write(data, "met")
                    } else {
                        Err(Error::configuration("the others never started"))
                    }
                })
            })
            .collect();
        let pipeline = Pipeline::builder()
            .add_parallel_elements(group)
            .build()
            .unwrap();
        let mut data = pipeline.create_flow_data();

        data.process().unwrap();

        assert!(data.errors().is_empty(), "{:?}", errors(&data));
        for key in keys {
            assert_eq!(read(&data, key), "met");
        }
    }
}

#[cfg(target_family = "wasm")]
#[test]
fn without_threads_a_group_runs_on_the_calling_thread() {
    let keys = ["one", "two", "three"];
    let caller = thread::current().id();
    let group: Vec<_> = keys
        .into_iter()
        .map(|key| {
            element(key, &[], move |element, data| {
                if thread::current().id() == caller {
                    element.write(data, "calling thread")
                } else {
                    element.write(data, "another thread")
                }
            })
        })
        .collect();
    let pipeline = Pipeline::builder()
        .add_parallel_elements(group)
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();

    data.process().unwrap();

    assert!(data.errors().is_empty(), "{:?}", errors(&data));
    for key in keys {
        assert_eq!(read(&data, key), "calling thread");
    }
}

#[test]
fn a_group_gives_the_values_and_errors_the_elements_give_in_sequence() {
    let first = [
        writer("configuration", "wanted"),
        failing("early", "before the group"),
    ];
    let group = [
        element("copy", &["query.colour"], |element, data| {
            let colour = data.evidence().get("query.colour").unwrap_or("none");
            let wanted = read(data, "configuration");
            element.write(data, format!("{colour} {wanted}"))
        }),
        element("count", &[], |element, data| {
            let seen = data.errors().len();
            element.write(data, format!("{seen} error(s) before"))
        }),
        failing("broken", "inside the group"),
    ];
    let last = [element("after", &[], |element, data| {
        let copied = read(data, "copy");
        let counted = read(data, "count");
        element.write(data, format!("{copied}, {counted}"))
    })];

    for suppress in [false, true] {
        let in_sequence = run(&first, &group, &last, false, suppress, evidence());
        let in_group = run(&first, &group, &last, true, suppress, evidence());
        assert_eq!(in_group, in_sequence, "suppress {suppress}");
        assert_eq!(
            read_outcome(&in_group, "after"),
            "blue wanted, 1 error(s) before"
        );
        assert_eq!(
            in_group.errors,
            vec![
                (
                    "early".to_owned(),
                    "pipeline configuration error: before the group".to_owned()
                ),
                (
                    "broken".to_owned(),
                    "pipeline configuration error: inside the group".to_owned()
                ),
            ]
        );
        assert_eq!(
            in_group.returned.is_some(),
            !suppress,
            "suppress {suppress}"
        );
    }
}

fn read_outcome(outcome: &Outcome, key: &str) -> String {
    outcome.values[key]["value"].clone()
}

#[test]
fn group_errors_are_in_the_order_the_elements_were_given() {
    let group = [
        element("slower", &[], |_, _| {
            thread::sleep(Duration::from_millis(WAIT_MILLISECONDS));
            Err(Error::configuration("finished last"))
        }),
        failing("faster", "finished first"),
    ];

    let outcome = run(&[], &group, &[], true, false, evidence());

    let keys: Vec<_> = outcome.errors.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(keys, ["slower", "faster"]);
    assert_eq!(
        outcome.returned.as_deref(),
        Some(
            "2 error(s) occurred during pipeline processing (error in element \
             'slower': pipeline configuration error: finished last) (error in \
             element 'faster': pipeline configuration error: finished first)"
        )
    );
}

/// An element that passes every call to another, and runs it only when the
/// element data added before it asks for it.
struct RunsWhenWanted {
    inner: Arc<dyn FlowElement>,
}

impl FlowElement for RunsWhenWanted {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        if read(data, "configuration") == "wanted" {
            self.inner.process(data)
        } else {
            Ok(())
        }
    }
    fn data_key(&self) -> &str {
        self.inner.data_key()
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        self.inner.evidence_key_filter()
    }
    fn properties(&self) -> &[PropertyMetaData] {
        self.inner.properties()
    }
    fn type_name(&self) -> &'static str {
        self.inner.type_name()
    }
}

#[test]
fn an_element_that_skips_another_works_in_a_group_as_in_sequence() {
    let group: [Arc<dyn FlowElement>; 2] = [
        Arc::new(RunsWhenWanted {
            inner: writer("inner", "ran"),
        }),
        writer("beside", "ran"),
    ];
    for wanted in ["wanted", "not wanted"] {
        let first = [writer("configuration", wanted)];
        for suppress in [false, true] {
            let in_sequence = run(&first, &group, &[], false, suppress, evidence());
            let in_group = run(&first, &group, &[], true, suppress, evidence());
            assert_eq!(in_group, in_sequence, "{wanted}, suppress {suppress}");
            assert_eq!(
                in_group.values.contains_key("inner"),
                wanted == "wanted",
                "{wanted}"
            );
            assert_eq!(read_outcome(&in_group, "beside"), "ran");
        }
    }
}

#[test]
fn an_element_of_a_group_does_not_see_what_another_adds() {
    let group = [
        writer("one", "first"),
        element("two", &[], |element, data| {
            // Give the first element time to finish, so a shared flow data
            // would hold its value by now.
            thread::sleep(Duration::from_millis(WAIT_MILLISECONDS));
            let seen = read(data, "one");
            element.write(data, seen)
        }),
    ];

    let outcome = run(&[], &group, &[], true, false, evidence());

    assert_eq!(read_outcome(&outcome, "two"), "absent");
    assert_eq!(read_outcome(&outcome, "one"), "first");
}

#[test]
fn an_element_of_a_group_cannot_change_data_added_before_it() {
    let first = [writer("shared", "before")];
    let group = [
        writer("other", "beside"),
        element("changer", &[], |_, data| {
            data.get_or_add(TypedKey::<ValueData>::new("shared"), || {
                ValueData(MapElementData::new().set("value", "changed"))
            })?;
            Ok(())
        }),
    ];

    let outcome = run(&first, &group, &[], true, true, evidence());

    assert_eq!(read_outcome(&outcome, "shared"), "before");
    assert_eq!(
        outcome.errors,
        vec![(
            "changer".to_owned(),
            "pipeline configuration error: Element data under key 'shared' was \
             added before the parallel group, so an element in the group \
             cannot change it."
                .to_owned()
        )]
    );
}

#[test]
fn only_the_first_of_two_elements_adding_one_key_is_kept() {
    let group = [
        element("first", &[], |_, data| {
            data.get_or_add(TypedKey::<ValueData>::new("same"), || {
                ValueData(MapElementData::new().set("value", "from first"))
            })?;
            Ok(())
        }),
        element("second", &[], |_, data| {
            data.get_or_add(TypedKey::<ValueData>::new("same"), || {
                ValueData(MapElementData::new().set("value", "from second"))
            })?;
            Ok(())
        }),
    ];

    let outcome = run(&[], &group, &[], true, true, evidence());

    assert_eq!(read_outcome(&outcome, "same"), "from first");
    assert_eq!(
        outcome.errors,
        vec![(
            "second".to_owned(),
            "pipeline configuration error: Element data under key 'same' was \
             added by more than one element of a parallel group, so only the \
             first is kept."
                .to_owned()
        )]
    );
}

/// What building `builder` gave, being "built" or the error's message.
fn build_outcome(builder: PipelineBuilder) -> String {
    match builder.build() {
        Ok(_) => "built".to_owned(),
        Err(error) => error.to_string(),
    }
}

#[test]
fn two_elements_of_a_group_with_one_data_key_are_refused_when_built() {
    let builder = Pipeline::builder().add_parallel_elements([
        writer("same", "one"),
        writer("other", "two"),
        writer("Same", "three"),
    ]);

    assert_eq!(
        build_outcome(builder),
        "pipeline configuration error: Two elements of a parallel group have \
         the data key 'Same', so only one of them could keep its data. Add one \
         of them on its own."
    );
}

#[test]
fn an_element_of_a_group_with_the_data_key_of_one_before_it_is_refused_when_built() {
    let message = |key: &str| {
        format!(
            "pipeline configuration error: An element of a parallel group has \
             the data key '{key}' of an element that runs before the group, so \
             it could not add its data once that element had. Add it on its own."
        )
    };
    let after_an_element = Pipeline::builder()
        .add_element(writer("shared", "before"))
        .add_parallel_elements([writer("other", "beside"), writer("SHARED", "in")]);
    let after_a_group = Pipeline::builder()
        .add_parallel_elements([writer("one", "a"), writer("shared", "b")])
        .add_parallel_elements([writer("Shared", "c"), writer("two", "d")]);

    assert_eq!(build_outcome(after_an_element), message("SHARED"));
    assert_eq!(build_outcome(after_a_group), message("Shared"));
}

/// Only an element of a group is refused, because an element on its own gets
/// the data already under its key, as it does in .NET.
#[test]
fn elements_on_their_own_may_share_a_data_key_with_any_element() {
    let pipeline = Pipeline::builder()
        .add_element(writer("first", "before"))
        .add_element(writer("First", "again"))
        .add_parallel_elements([writer("second", "in"), writer("third", "beside")])
        .add_element(writer("second", "after"))
        .build()
        .expect("the pipeline builds");
    let mut data = pipeline.create_flow_data_with(evidence());
    data.process().expect("processing succeeds");

    assert_eq!(read(&data, "first"), "before");
    assert_eq!(read(&data, "second"), "in");
    assert_eq!(read(&data, "third"), "beside");
    assert!(errors(&data).is_empty(), "{:?}", errors(&data));
}

#[cfg(panic = "unwind")]
#[test]
fn a_panic_in_a_group_reaches_the_caller_with_the_flow_data_intact() {
    use std::panic::{self, AssertUnwindSafe};

    let pipeline = Pipeline::builder()
        .add_element(writer("before", "kept"))
        .add_parallel_elements([
            writer("beside", "ran"),
            element("panics", &[], |_, _| panic!("deliberate panic")),
        ])
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data_with(evidence());

    let caught = panic::catch_unwind(AssertUnwindSafe(|| data.process()));

    let payload = caught.expect_err("the panic reaches the caller");
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"deliberate panic"));
    assert_eq!(data.evidence().get("query.colour"), Some("blue"));
    assert_eq!(read(&data, "before"), "kept");
}

#[test]
fn an_element_of_a_group_cannot_process_its_flow_data_again() {
    let group = [
        writer("beside", "ran"),
        element("again", &[], |_, data| data.process()),
    ];

    let outcome = run(&[], &group, &[], true, true, evidence());

    assert_eq!(outcome.errors.len(), 1);
    assert_eq!(outcome.errors[0].0, "again");
    assert!(
        outcome.errors[0].1.contains("already been processed"),
        "{:?}",
        outcome.errors
    );
}

#[test]
fn an_element_of_a_group_reads_its_pipeline_which_is_released_after() {
    let reader = element("reader", &[], |element, data| {
        let pipeline = data.pipeline();
        let seen = format!(
            "{:p} {} {}",
            Arc::as_ptr(pipeline),
            pipeline.flow_elements().len(),
            data.evidence_key_filter().include("query.after")
        );
        element.write(data, seen)
    });
    let pipeline = Pipeline::builder()
        .add_parallel_elements([writer("beside", "ran"), reader])
        .add_element(element("after", &["query.after"], |_, _| Ok(())))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();

    data.process().unwrap();

    assert_eq!(
        read(&data, "reader"),
        format!("{:p} 3 true", Arc::as_ptr(&pipeline))
    );
    assert_eq!(
        Arc::strong_count(&pipeline),
        2,
        "only `pipeline` and `data` hold the pipeline"
    );
}

#[test]
fn one_element_is_added_on_its_own_and_none_adds_nothing() {
    let one = Pipeline::builder()
        .add_parallel_elements([writer("only", "ran")])
        .build()
        .unwrap();
    assert!(!one.is_concurrent());
    assert_eq!(one.flow_elements().len(), 1);

    let none = Pipeline::builder()
        .add_parallel_elements(Vec::new())
        .build();
    assert!(matches!(none, Err(Error::PipelineConfiguration { .. })));
}

/// A group's elements are listed after every element added on its own, as
/// .NET's `Pipeline.FlowElements` lists them.
#[test]
fn a_pipeline_lists_a_groups_elements_after_the_others_with_their_evidence() {
    let pipeline = Pipeline::builder()
        .add_element(element("before", &["query.before"], |_, _| Ok(())))
        .add_parallel_elements([
            element("one", &["header.one"], |_, _| Ok(())),
            element("two", &["cookie.two"], |_, _| Ok(())),
        ])
        .add_element(element("after", &["query.after"], |_, _| Ok(())))
        .build()
        .unwrap();

    let keys: Vec<_> = pipeline
        .flow_elements()
        .iter()
        .map(|element| element.data_key())
        .collect();
    assert_eq!(keys, ["before", "after", "one", "two"]);
    assert!(pipeline.is_concurrent());
    let filter = pipeline.evidence_key_filter();
    for key in ["query.before", "header.one", "cookie.two", "query.after"] {
        assert!(filter.include(key), "{key}");
    }
    assert!(!filter.include("query.other"));
}

/// The reference count of the pipeline that the first element of a group
/// sees, which runs once every other member's flow data has been made.
fn pipeline_count_seen_by_the_first_of(size: usize) -> usize {
    const OTHERS: [&str; 3] = ["second", "third", "fourth"];
    let seen = Arc::new(AtomicUsize::new(0));
    let recorder = Arc::clone(&seen);
    let mut group = vec![element("first", &[], move |_, data| {
        recorder.store(Arc::strong_count(data.pipeline()), Ordering::SeqCst);
        Ok(())
    })];
    group.extend(
        OTHERS[..size - 1]
            .iter()
            .map(|&key| element(key, &[], |_, _| Ok(()))),
    );
    Pipeline::builder()
        .add_parallel_elements(group)
        .build()
        .unwrap()
        .create_flow_data()
        .process()
        .unwrap();
    seen.load(Ordering::SeqCst)
}

/// Every member of a group reads the pipeline through the view the group
/// shares, so the pipeline's reference count does not grow with the group.
/// If each member held a clone, a group of four would see two more than a
/// group of two.
#[test]
fn a_groups_members_share_one_reference_to_the_pipeline() {
    let two = pipeline_count_seen_by_the_first_of(2);
    let four = pipeline_count_seen_by_the_first_of(4);
    assert!(two > 1, "the first element ran and saw the pipeline");
    assert_eq!(two, four);
}

#[test]
fn a_groups_elements_are_released_with_the_pipeline() {
    let one = writer("one", "ran");
    let two = writer("two", "ran");
    let pipeline = Pipeline::builder()
        .add_parallel_elements([Arc::clone(&one), Arc::clone(&two)])
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();
    assert!(
        Arc::strong_count(&one) > 1,
        "the pipeline holds the element"
    );

    drop(data);
    drop(pipeline);

    assert_eq!(Arc::strong_count(&one), 1);
    assert_eq!(Arc::strong_count(&two), 1);
}
