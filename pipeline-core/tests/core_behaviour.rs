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

//! Behavioral tests for the pipeline core.
//!
//! These cover the specification rules the plan calls out: evidence
//! case-insensitivity and precedence, `generate_key` determinism, typed vs
//! string element-data access, no-value behavior, sequential pipeline
//! processing and suppress-vs-propagate error handling.

use std::any::Any;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Weak};

use fiftyone_pipeline_core::{
    compare_keys, ElementData, Error, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist,
    EvidencePrefix, FlowData, FlowElement, MapElementData, NoValueError, Pipeline,
    PropertyMetaData, PropertyValue, PropertyValueType, Result, TypedKey,
};

// ---------------------------------------------------------------------------
// Test element data and elements.
// ---------------------------------------------------------------------------

/// Element data that records which element produced it, used to assert
/// processing order and typed access.
struct RecordingData {
    bag: MapElementData,
}

impl RecordingData {
    fn new(label: &str) -> Self {
        RecordingData {
            bag: MapElementData::new().set("label", label),
        }
    }
}

impl ElementData for RecordingData {
    fn get(&self, name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        self.bag.get(name)
    }
    fn keys(&self) -> Vec<String> {
        self.bag.keys()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A second, distinct element-data type to prove typed access discriminates by
/// type, not just by key.
struct OtherData;

impl ElementData for OtherData {
    fn get(&self, _name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        Err(NoValueError::new("other has no values"))
    }
    fn keys(&self) -> Vec<String> {
        Vec::new()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// An element that appends its label to a shared order log and writes
/// recording data under its key.
struct OrderElement {
    key: &'static str,
    order_log: Arc<std::sync::Mutex<Vec<&'static str>>>,
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

impl OrderElement {
    fn new(
        key: &'static str,
        order_log: Arc<std::sync::Mutex<Vec<&'static str>>>,
        evidence_keys: &[&str],
    ) -> Self {
        OrderElement {
            key,
            order_log,
            filter: EvidenceKeyFilterWhitelist::new(evidence_keys.iter().copied()),
            properties: vec![PropertyMetaData::new(
                "label",
                key,
                PropertyValueType::String,
            )],
        }
    }

    fn typed_key(&self) -> TypedKey<RecordingData> {
        TypedKey::new(self.key)
    }
}

impl FlowElement for OrderElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        self.order_log.lock().unwrap().push(self.key);
        let key = self.key;
        data.get_or_add(TypedKey::<RecordingData>::new(key), || {
            RecordingData::new(key)
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

/// An element that always fails, to drive the error-handling tests.
struct FailingElement {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

impl FailingElement {
    fn new() -> Self {
        FailingElement {
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            properties: Vec::new(),
        }
    }
}

impl FlowElement for FailingElement {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
        Err(Error::configuration("deliberate failure"))
    }
    fn data_key(&self) -> &str {
        "failing"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

/// An element that keeps each pipeline it is added to, with the number of
/// elements that pipeline held at the time, and refuses every pipeline when
/// `refuse` is set.
struct AddedElement {
    added: std::sync::Mutex<Vec<(Weak<Pipeline>, usize)>>,
    refuse: bool,
    filter: EvidenceKeyFilterWhitelist,
}

impl AddedElement {
    fn new(refuse: bool) -> Self {
        AddedElement {
            added: std::sync::Mutex::new(Vec::new()),
            refuse,
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        }
    }
}

impl FlowElement for AddedElement {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
        Ok(())
    }
    fn data_key(&self) -> &str {
        "added"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
    fn add_pipeline(&self, pipeline: &Arc<Pipeline>) -> Result<()> {
        self.added
            .lock()
            .unwrap()
            .push((Arc::downgrade(pipeline), pipeline.flow_elements().len()));
        if self.refuse {
            return Err(Error::configuration("refused"));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Evidence: case-insensitivity and precedence.
// ---------------------------------------------------------------------------

#[test]
fn evidence_keys_are_case_insensitive_values_are_not() {
    let evidence = Evidence::builder()
        .add("Header.User-Agent", "Mozilla/5.0")
        .build();

    // Key lookups fold case both ways.
    assert_eq!(evidence.get("header.user-agent"), Some("Mozilla/5.0"));
    assert_eq!(evidence.get("HEADER.USER-AGENT"), Some("Mozilla/5.0"));
    assert!(evidence.contains_key("Header.User-Agent"));

    // The value is preserved verbatim (case-sensitive).
    assert_eq!(evidence.get("header.user-agent"), Some("Mozilla/5.0"));
    assert_ne!(evidence.get("header.user-agent"), Some("mozilla/5.0"));
}

#[test]
fn duplicate_keys_overwrite_after_case_folding() {
    let evidence = Evidence::builder()
        .add("query.name", "first")
        .add("QUERY.NAME", "second")
        .build();
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence.get("query.name"), Some("second"));
}

#[test]
fn evidence_prefix_precedence_matches_spec_order() {
    assert!(EvidencePrefix::Query.precedence() < EvidencePrefix::Header.precedence());
    assert!(EvidencePrefix::Header.precedence() < EvidencePrefix::Cookie.precedence());
    assert!(EvidencePrefix::Cookie.precedence() < EvidencePrefix::Server.precedence());
    assert!(EvidencePrefix::Server.precedence() < EvidencePrefix::FiftyOne.precedence());
    assert!(EvidencePrefix::FiftyOne.precedence() < EvidencePrefix::Location.precedence());
}

#[test]
fn compare_keys_orders_known_prefixes_before_unknown_then_alphabetical() {
    // query beats header.
    assert_eq!(
        compare_keys("query.user-agent", "header.user-agent"),
        std::cmp::Ordering::Less
    );
    // Any known prefix beats an unknown one.
    assert_eq!(
        compare_keys("location.x", "custom.x"),
        std::cmp::Ordering::Less
    );
    // Two unknown prefixes are alphabetical by full key.
    assert_eq!(compare_keys("alpha.x", "beta.x"), std::cmp::Ordering::Less);
    // Same prefix falls back to alphabetical on the whole key.
    assert_eq!(
        compare_keys("header.a", "header.b"),
        std::cmp::Ordering::Less
    );
}

// ---------------------------------------------------------------------------
// generate_key determinism.
// ---------------------------------------------------------------------------

#[test]
fn generate_key_is_deterministic_regardless_of_insertion_order() {
    let filter = EvidenceKeyFilterWhitelist::new([
        "query.user-agent",
        "header.user-agent",
        "cookie.session",
    ]);

    let a = Evidence::builder()
        .add("cookie.session", "abc")
        .add("header.user-agent", "ua")
        .add("query.user-agent", "qua")
        .build();
    let b = Evidence::builder()
        .add("query.user-agent", "qua")
        .add("cookie.session", "abc")
        .add("header.user-agent", "ua")
        .build();

    let key_a = a.generate_key(&filter);
    let key_b = b.generate_key(&filter);

    // Equal evidence, different insertion order, produces equal keys.
    assert_eq!(key_a, key_b);

    // The entries are ordered by precedence: query, then header, then cookie.
    let names: Vec<&str> = key_a.entries().iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        names,
        vec!["query.user-agent", "header.user-agent", "cookie.session"]
    );
}

#[test]
fn generate_key_excludes_keys_not_in_filter() {
    let filter = EvidenceKeyFilterWhitelist::new(["query.included"]);
    let evidence = Evidence::builder()
        .add("query.included", "yes")
        .add("query.excluded", "no")
        .build();
    let key = evidence.generate_key(&filter);
    assert_eq!(key.entries().len(), 1);
    assert_eq!(key.entries()[0].0, "query.included");
}

#[test]
fn generate_key_respects_explicit_filter_order() {
    // Lower order means higher precedence, so 'second' (order 0) must come
    // before 'first' (order 1) even though it sorts later alphabetically.
    let filter = EvidenceKeyFilterWhitelist::with_orders([("query.first", 1), ("query.second", 0)]);
    let evidence = Evidence::builder()
        .add("query.first", "1")
        .add("query.second", "2")
        .build();
    let key = evidence.generate_key(&filter);
    let names: Vec<&str> = key.entries().iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(names, vec!["query.second", "query.first"]);
}

// ---------------------------------------------------------------------------
// Whitelist filter case-insensitivity.
// ---------------------------------------------------------------------------

#[test]
fn whitelist_filter_is_case_insensitive() {
    let filter = EvidenceKeyFilterWhitelist::new(["Query.User-Agent"]);
    assert!(filter.include("query.user-agent"));
    assert!(filter.include("QUERY.USER-AGENT"));
    assert!(!filter.include("header.user-agent"));
    assert_eq!(filter.order("query.user-agent"), Some(0));
    assert_eq!(filter.order("header.user-agent"), None);
}

#[test]
fn element_filter_accepts_only_whitelisted_evidence_keys() {
    // The element advertises a single accepted key. Its evidence filter must
    // accept that key (in any case) and reject every other key, which is the
    // advertise-accepted-evidence MUST.
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let element = OrderElement::new("e", Arc::clone(&log), &["query.user-agent"]);
    let filter = element.evidence_key_filter();

    // The whitelisted key is accepted, folding case both ways.
    assert!(filter.include("query.user-agent"));
    assert!(filter.include("QUERY.USER-AGENT"));
    assert_eq!(filter.order("query.user-agent"), Some(0));

    // Keys outside the whitelist are rejected and carry no order.
    assert!(!filter.include("header.user-agent"));
    assert!(!filter.include("cookie.session"));
    assert_eq!(filter.order("header.user-agent"), None);
}

#[test]
fn flow_data_exposes_only_filtered_evidence_to_an_element() {
    // An element only "sees" the evidence its filter accepts: `generate_key`
    // selects exactly the whitelisted keys from the flow data's evidence and
    // drops the rest. This is the same selection the pipeline uses to decide
    // which evidence an element may consume.
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let element = OrderElement::new("e", Arc::clone(&log), &["query.user-agent"]);
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(element))
        .build()
        .unwrap();

    let data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("query.user-agent", "kept")
            .add("header.user-agent", "dropped")
            .add("cookie.session", "dropped-too")
            .build(),
    );

    // Drive the element's filter over the flow data's evidence directly.
    let filter = pipeline.flow_elements()[0].evidence_key_filter();
    let key = data.evidence().generate_key(filter);

    // Only the whitelisted key survives, so the element is exposed to one value.
    let names: Vec<&str> = key.entries().iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(names, vec!["query.user-agent"]);
    assert_eq!(key.entries()[0].1, "kept");
}

// ---------------------------------------------------------------------------
// Typed vs string element-data access, and no-value behavior.
// ---------------------------------------------------------------------------

fn single_element_pipeline() -> (Arc<Pipeline>, Arc<std::sync::Mutex<Vec<&'static str>>>) {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let element = OrderElement::new("recording", Arc::clone(&log), &["query.name"]);
    // Exercise the element-exposed typed key accessor (the intended pattern an
    // element offers callers for strongly-typed retrieval).
    assert_eq!(element.typed_key().name(), "recording");
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(element))
        .build()
        .unwrap();
    (pipeline, log)
}

#[test]
fn typed_get_and_string_get_return_the_same_data() {
    let (pipeline, _log) = single_element_pipeline();
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();

    // String access (mechanism 1).
    let by_str = data.get_str("recording").expect("data by key");
    assert_eq!(by_str.get("label").unwrap().as_str(), Some("recording"));

    // Typed access (mechanism 2) recovers the concrete type.
    let key = TypedKey::<RecordingData>::new("recording");
    let by_type = data.get(key).expect("data by type");
    assert_eq!(by_type.get("label").unwrap().as_str(), Some("recording"));

    // Case-insensitive key access works too.
    assert!(data.get_str("RECORDING").is_some());
}

#[test]
fn typed_get_returns_none_for_wrong_type() {
    let (pipeline, _log) = single_element_pipeline();
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();

    // The key matches but the requested type does not, so downcast fails.
    let wrong = TypedKey::<OtherData>::new("recording");
    assert!(data.get(wrong).is_none());
}

#[test]
fn no_value_is_distinct_from_missing_data() {
    let (pipeline, _log) = single_element_pipeline();
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();

    let element_data = data.get_str("recording").unwrap();
    // A property the element data does not hold yields a NoValueError.
    let result = element_data.get("does-not-exist");
    assert!(matches!(result, Err(NoValueError { .. })));

    // A data key for an element that produced nothing yields no data at all.
    assert!(data.get_str("absent-element").is_none());
}

#[test]
fn get_evidence_or_property_prefers_element_data_then_falls_back_to_evidence() {
    let (pipeline, _log) = single_element_pipeline();
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("query.name", "Ada").build());
    data.process().unwrap();

    // 'label' is in element data, so it is returned from there.
    let from_data = data.get_evidence_or_property("label").unwrap();
    assert_eq!(from_data.as_str(), Some("recording"));

    // 'query.name' is only in evidence, so it falls back.
    let from_evidence = data.get_evidence_or_property("query.name").unwrap();
    assert_eq!(from_evidence.as_str(), Some("Ada"));

    // Neither source has this, so it is an error.
    assert!(data.get_evidence_or_property("missing").is_err());
}

// ---------------------------------------------------------------------------
// Pipeline sequential processing.
// ---------------------------------------------------------------------------

#[test]
fn pipeline_runs_elements_in_order() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(OrderElement::new("first", Arc::clone(&log), &[])))
        .add_element(Arc::new(OrderElement::new("second", Arc::clone(&log), &[])))
        .add_element(Arc::new(OrderElement::new("third", Arc::clone(&log), &[])))
        .build()
        .unwrap();

    let mut data = pipeline.create_flow_data();
    assert!(!data.is_processed());
    data.process().unwrap();
    assert!(data.is_processed());

    assert_eq!(*log.lock().unwrap(), vec!["first", "second", "third"]);
    // Each element added its data.
    assert!(data.get_str("first").is_some());
    assert!(data.get_str("second").is_some());
    assert!(data.get_str("third").is_some());
}

#[test]
fn a_flow_data_is_processed_once() {
    let (pipeline, log) = single_element_pipeline();
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();

    let second = data.process();
    assert!(
        matches!(second, Err(Error::PipelineConfiguration { .. })),
        "a second call fails, got {second:?}"
    );
    assert_eq!(
        *log.lock().unwrap(),
        vec!["recording"],
        "no element runs a second time"
    );
    assert!(data.is_processed());
}

#[test]
fn a_failed_flow_data_is_not_processed_again() {
    // A flow data whose processing failed is refused as well, so its errors
    // are those of its one run.
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(FailingElement::new()))
        .add_element(Arc::new(OrderElement::new("after", Arc::clone(&log), &[])))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    assert!(matches!(data.process(), Err(Error::Aggregate(_))));

    assert!(matches!(
        data.process(),
        Err(Error::PipelineConfiguration { .. })
    ));
    assert_eq!(data.errors().len(), 1, "the errors are not recorded twice");
    assert_eq!(*log.lock().unwrap(), vec!["after"]);
}

#[test]
fn pipeline_evidence_filter_unions_element_filters() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(OrderElement::new(
            "a",
            Arc::clone(&log),
            &["query.one"],
        )))
        .add_element(Arc::new(OrderElement::new(
            "b",
            Arc::clone(&log),
            &["header.two"],
        )))
        .build()
        .unwrap();

    let filter = pipeline.evidence_key_filter();
    // Both elements' keys are accepted by the union filter.
    assert!(filter.include("query.one"));
    assert!(filter.include("header.two"));
    assert!(!filter.include("cookie.three"));
}

#[test]
fn pipeline_reports_accepted_evidence_and_declared_properties() {
    // A pipeline is introspectable: a caller can ask which evidence it accepts
    // and which properties its elements declare, without processing anything.
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(OrderElement::new(
            "alpha",
            Arc::clone(&log),
            &["query.alpha"],
        )))
        .add_element(Arc::new(OrderElement::new(
            "beta",
            Arc::clone(&log),
            &["header.beta"],
        )))
        .build()
        .unwrap();

    // Two elements, in the order they were added.
    let elements = pipeline.flow_elements();
    assert_eq!(elements.len(), 2);
    assert_eq!(elements[0].data_key(), "alpha");
    assert_eq!(elements[1].data_key(), "beta");

    // The pipeline-wide filter is the union of the elements' accepted keys.
    let filter = pipeline.evidence_key_filter();
    assert!(filter.include("query.alpha"));
    assert!(filter.include("header.beta"));
    assert!(!filter.include("cookie.gamma"));

    // Each element declares its `label` property, owned by its own data key, and
    // declared as a string. Walking the elements gathers the full property set.
    let mut declared: Vec<(String, String)> = Vec::new();
    for element in elements {
        for property in element.properties() {
            assert_eq!(property.value_type, PropertyValueType::String);
            assert!(property.available);
            declared.push((property.name.clone(), property.element_data_key.clone()));
        }
    }
    declared.sort();
    assert_eq!(
        declared,
        vec![
            ("label".to_owned(), "alpha".to_owned()),
            ("label".to_owned(), "beta".to_owned()),
        ]
    );
}

// ---------------------------------------------------------------------------
// Error handling: suppress vs propagate.
// ---------------------------------------------------------------------------

#[test]
fn errors_propagate_when_not_suppressed() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(FailingElement::new()))
        .add_element(Arc::new(OrderElement::new("after", Arc::clone(&log), &[])))
        // Default is not suppressed, but be explicit for the test.
        .suppress_process_exceptions(false)
        .build()
        .unwrap();

    let mut data = pipeline.create_flow_data();
    let result = data.process();

    // Processing returns an aggregate error.
    match result {
        Err(Error::Aggregate(errors)) => assert_eq!(errors.len(), 1),
        other => panic!("expected an aggregate error, got {other:?}"),
    }

    // Even though an element failed, later elements still ran (errors gathered,
    // not short-circuited), per the spec.
    assert_eq!(*log.lock().unwrap(), vec!["after"]);
    // The error is also recorded on the flow data.
    assert_eq!(data.errors().len(), 1);
    assert_eq!(data.errors()[0].element_data_key, "failing");
}

#[test]
fn the_returned_error_names_the_element_and_the_cause() {
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(FailingElement::new()))
        .build()
        .unwrap();
    let shown = pipeline
        .create_flow_data()
        .process()
        .unwrap_err()
        .to_string();
    assert_eq!(
        shown,
        "1 error(s) occurred during pipeline processing (error in element 'failing': \
         pipeline configuration error: deliberate failure)"
    );
}

#[test]
fn errors_are_collected_and_ok_returned_when_suppressed() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(FailingElement::new()))
        .add_element(Arc::new(OrderElement::new("after", Arc::clone(&log), &[])))
        .suppress_process_exceptions(true)
        .build()
        .unwrap();

    let mut data = pipeline.create_flow_data();
    let result = data.process();

    // With suppression, processing succeeds.
    assert!(result.is_ok());
    // The error is still recorded for inspection.
    assert_eq!(data.errors().len(), 1);
    assert_eq!(data.errors()[0].element_data_key, "failing");
    // Subsequent element still ran.
    assert_eq!(*log.lock().unwrap(), vec!["after"]);
}

#[test]
fn empty_pipeline_fails_to_build() {
    let result = Pipeline::builder().build();
    assert!(matches!(result, Err(Error::PipelineConfiguration { .. })));
}

#[test]
fn an_element_is_told_each_pipeline_it_is_built_into() {
    let shared = Arc::new(AddedElement::new(false));
    let first = Pipeline::builder()
        .add_element(Arc::clone(&shared) as Arc<dyn FlowElement>)
        .add_element(Arc::clone(&shared) as Arc<dyn FlowElement>)
        .build()
        .unwrap();
    let second = Pipeline::builder()
        .add_element(Arc::clone(&shared) as Arc<dyn FlowElement>)
        .build()
        .unwrap();

    // Once for each place in each pipeline, in build order, and each time with
    // every element of the pipeline in place.
    let added = shared.added.lock().unwrap();
    let expected = [(&first, 2), (&first, 2), (&second, 1)];
    assert_eq!(added.len(), expected.len());
    for ((weak, count), (pipeline, elements)) in added.iter().zip(expected) {
        assert!(Weak::ptr_eq(weak, &Arc::downgrade(pipeline)));
        assert_eq!(*count, elements);
    }
}

#[test]
fn an_element_refusing_its_pipeline_fails_the_build() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let refusing = Arc::new(AddedElement::new(true));
    let result = Pipeline::builder()
        .add_element(Arc::new(OrderElement::new("first", log, &[])))
        .add_element(Arc::clone(&refusing) as Arc<dyn FlowElement>)
        .build();

    let Err(Error::PipelineConfiguration { message }) = result else {
        panic!("the build should fail with the element's error");
    };
    assert_eq!(message, "refused");
    // The refused pipeline is not kept anywhere.
    let added = refusing.added.lock().unwrap();
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].0.strong_count(), 0);
}

#[test]
fn weighted_value_weighting_is_normalised() {
    use fiftyone_pipeline_core::WeightedValue;
    let full = WeightedValue::new(u16::MAX, "a");
    assert!((full.weighting() - 1.0).abs() < f32::EPSILON);
    let none = WeightedValue::new(0, "b");
    assert_eq!(none.weighting(), 0.0);
}

#[test]
fn get_or_add_inserts_once_and_is_idempotent() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let (pipeline, _log) = single_element_pipeline();
    let mut data = pipeline.create_flow_data();

    let counter = Arc::clone(&call_count);
    let key = TypedKey::<RecordingData>::new("manual");
    data.get_or_add(key, || {
        counter.fetch_add(1, Ordering::SeqCst);
        RecordingData::new("manual")
    })
    .unwrap();

    let counter = Arc::clone(&call_count);
    data.get_or_add(key, || {
        counter.fetch_add(1, Ordering::SeqCst);
        RecordingData::new("manual-again")
    })
    .unwrap();

    // The create closure ran only on the first call.
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
    // The original value is retained.
    assert_eq!(
        data.get(key).unwrap().get("label").unwrap().as_str(),
        Some("manual")
    );
}

#[test]
fn type_name_names_the_concrete_element_through_the_pipeline() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(OrderElement::new("order", log, &[])))
        .add_element(Arc::new(FailingElement::new()))
        .build()
        .unwrap();

    let names: Vec<&str> = pipeline
        .flow_elements()
        .iter()
        .map(|element| element.type_name())
        .collect();
    assert_eq!(
        names,
        vec![
            "core_behaviour::OrderElement",
            "core_behaviour::FailingElement"
        ]
    );
}

/// A resource attached to the builder lives exactly as long as the pipeline
/// and is found again by its type.
#[test]
fn an_attached_resource_lives_with_the_pipeline_and_is_found_by_type() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let resource: Arc<String> = Arc::new(String::from("kept"));
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(OrderElement::new("order", log, &[])))
        .add_resource(resource.clone())
        .build()
        .unwrap();
    assert_eq!(Arc::strong_count(&resource), 2);
    assert_eq!(
        pipeline.resource::<String>().as_deref(),
        Some(&String::from("kept"))
    );
    assert!(pipeline.resource::<u32>().is_none());
    drop(pipeline);
    assert_eq!(Arc::strong_count(&resource), 1);
}

/// A whitelist lists its keys, an aggregator lists the union of its parts and
/// nothing when a part has no list, and the pipeline-wide filter lists every
/// element's keys.
#[test]
fn named_keys_are_listed_by_whitelists_and_their_unions() {
    use fiftyone_pipeline_core::EvidenceKeyFilterAggregator;

    let whitelist = EvidenceKeyFilterWhitelist::new(["header.User-Agent", "query.sequence"]);
    let mut keys = whitelist.named_keys().expect("a whitelist lists its keys");
    keys.sort();
    assert_eq!(keys, ["header.user-agent", "query.sequence"]);

    let mut union = EvidenceKeyFilterAggregator::new();
    union.add_filter(Box::new(whitelist));
    union.add_filter(Box::new(EvidenceKeyFilterWhitelist::new([
        "header.user-agent",
        "header.host",
    ])));
    assert_eq!(
        union.named_keys().expect("every part lists its keys"),
        ["header.host", "header.user-agent", "query.sequence"]
    );

    struct AnyKey;
    impl EvidenceKeyFilter for AnyKey {
        fn include(&self, _key: &str) -> bool {
            true
        }
    }
    union.add_filter(Box::new(AnyKey));
    assert!(union.named_keys().is_none());

    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(OrderElement::new(
            "order",
            log,
            &["header.user-agent"],
        )))
        .build()
        .unwrap();
    assert_eq!(
        pipeline.evidence_key_filter().named_keys(),
        Some(vec![String::from("header.user-agent")])
    );
}
