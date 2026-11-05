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

//! What the element logs at warning level or above, and how often, for the
//! cases where .NET's derived property element logs. Kept in its own test
//! binary because a logger is installed once for the whole process, and each
//! test holds [`SERIAL`] so the records it reads are its own.

use std::sync::{Arc, Mutex, MutexGuard};

use fiftyone_derived_properties::{DerivedPropertyElement, DERIVED_DEFAULT_ELEMENT_DATA_KEY};
use fiftyone_pipeline_core::{
    Error, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, MapElementData,
    Pipeline, PropertyMetaData, PropertyValueType, Result, TypedKey,
};

/// Log records at warning level or above, as "LEVEL message".
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Held by each test for its whole run.
static SERIAL: Mutex<()> = Mutex::new(());

/// Keeps every record at warning level or above in [`LOGGED`].
struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            LOGGED
                .lock()
                .unwrap()
                .push(format!("{} {}", record.level(), record.args()));
        }
    }

    fn flush(&self) {}
}

static LOGGER: TestLogger = TestLogger;

/// Install the logger, wait for any other test to finish and clear the
/// records it left.
fn start() -> MutexGuard<'static, ()> {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Warn);
    let guard = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    LOGGED.lock().unwrap().clear();
    guard
}

/// The records logged since [`start`] or the last call, which clears them.
fn take() -> Vec<String> {
    std::mem::take(&mut *LOGGED.lock().unwrap())
}

/// A script named `name` producing a property of the same name from the
/// source property `source`, written as `element.Property`.
fn script(name: &str, source: &str, deprecation: &str) -> String {
    format!(
        "Format: 1\n\
         Name: {name}\n\
         Version: 1.0.0\n\
         {deprecation}\
         Output:\n  Name: {name}\n  Description: A demonstration.\n  ValueType: string\n  \
         IsList: false\n\
         Checks:\n  Current:\n    Property: {source}\n    Lt: 2\n\
         Rules:\n  - When:\n      Check: Current\n    Then: \"yes\"\n  - Else: \"no\"\n"
    )
}

/// An element with one script reading `source`.
fn derived(source: &str) -> Arc<DerivedPropertyElement> {
    Arc::new(
        DerivedPropertyElement::builder()
            .add_script("Demo", &script("Demo", source, ""))
            .build()
            .expect("the script is sound"),
    )
}

/// Writes `N` = 1 under its key, or data with no properties, or fails.
struct SourceElement {
    key: &'static str,
    fail: bool,
    empty: bool,
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

impl SourceElement {
    /// An element writing `src`.
    fn new(fail: bool) -> Arc<Self> {
        Self::writing("src", fail)
    }

    /// An element writing data with no properties under `src`, as a cloud
    /// engine does when its request failed.
    fn storing_nothing() -> Arc<Self> {
        Arc::new(SourceElement {
            key: "src",
            fail: false,
            empty: true,
            filter: EvidenceKeyFilterWhitelist::new(Vec::<&str>::new()),
            properties: vec![PropertyMetaData::new(
                "N",
                "src",
                PropertyValueType::Integer,
            )],
        })
    }

    fn writing(key: &'static str, fail: bool) -> Arc<Self> {
        Arc::new(SourceElement {
            key,
            fail,
            empty: false,
            filter: EvidenceKeyFilterWhitelist::new(Vec::<&str>::new()),
            properties: vec![PropertyMetaData::new("N", key, PropertyValueType::Integer)],
        })
    }
}

impl FlowElement for SourceElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        if self.fail {
            return Err(Error::configuration("the source failed"));
        }
        let key: TypedKey<MapElementData> = TypedKey::new(self.key);
        let stored = data.get_or_add(key, MapElementData::default)?;
        if !self.empty {
            stored.insert("N", 1i64);
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

/// Process `requests` empty requests and give the records each logged.
fn run(pipeline: &Arc<Pipeline>, requests: usize) -> Vec<Vec<String>> {
    (0..requests)
        .map(|_| {
            pipeline.create_flow_data().process().unwrap();
            take()
        })
        .collect()
}

/// The value or no value message of `Demo` for one request.
fn demo(pipeline: &Arc<Pipeline>) -> std::result::Result<String, String> {
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();
    let derived = data.get_str(DERIVED_DEFAULT_ELEMENT_DATA_KEY).unwrap();
    derived
        .get("Demo")
        .map(|value| format!("{value:?}"))
        .map_err(|no_value| no_value.message)
}

const AFTER: &str = "ERROR The script 'Demo' reads 'src.N', and the request carried no \
    element data 'src' at all. An element in the pipeline writes 'src', so that element is \
    running after this one rather than before it, or it failed or was skipped on this \
    request. Add the derived property element after it. This is reported once.";

#[test]
fn a_deprecated_script_logs_one_warning_when_the_element_is_built() {
    let _serial = start();
    let build = |deprecation: &str| {
        DerivedPropertyElement::builder()
            .add_script("Demo", &script("Demo", "src.N", deprecation))
            .build()
            .unwrap()
    };

    build("Deprecated: true\nDeprecationNote: Use Other instead.\n");
    assert_eq!(
        take(),
        vec!["WARN Derived property script 'Demo' is deprecated. Use Other instead."]
    );
    build("Deprecated: false\n");
    build("");
    assert_eq!(take(), Vec::<String>::new());

    // A build that fails logs nothing, because there is no element.
    let failed = DerivedPropertyElement::builder()
        .add_script(
            "Demo",
            &script("Demo", "src.N", "Deprecated: true\nDeprecationNote: x\n"),
        )
        .add_script("Other", "not a script")
        .build();
    assert!(failed.is_err());
    assert_eq!(take(), Vec::<String>::new());
}

#[test]
fn a_source_element_after_this_one_logs_one_error_on_the_first_request() {
    let _serial = start();
    let pipeline = Pipeline::builder()
        .add_element(derived("src.N"))
        .add_element(SourceElement::new(false))
        .build()
        .unwrap();
    assert_eq!(take(), Vec::<String>::new());
    assert_eq!(
        run(&pipeline, 3),
        vec![vec![AFTER.to_owned()], vec![], vec![]]
    );
    assert!(demo(&pipeline).is_err());
    assert_eq!(take(), Vec::<String>::new());
}

#[test]
fn a_source_element_before_this_one_logs_nothing() {
    let _serial = start();
    let pipeline = Pipeline::builder()
        .add_element(SourceElement::new(false))
        .add_element(derived("src.N"))
        .build()
        .unwrap();
    assert_eq!(run(&pipeline, 3), vec![Vec::<String>::new(); 3]);
    assert_eq!(demo(&pipeline), Ok("String(\"yes\")".to_owned()));
}

/// A source element that runs first and stores data holding no properties,
/// as a cloud engine does when its request failed, logs nothing, because
/// the data under its key is there.
#[test]
fn a_source_element_that_stores_no_properties_logs_nothing() {
    let _serial = start();
    let pipeline = Pipeline::builder()
        .add_element(SourceElement::storing_nothing())
        .add_element(derived("src.N"))
        .build()
        .unwrap();
    assert_eq!(run(&pipeline, 3), vec![Vec::<String>::new(); 3]);
}

#[test]
fn a_source_element_that_fails_logs_one_error_on_the_first_request() {
    let _serial = start();
    // The failure itself is logged at debug level, below what is kept here.
    let pipeline = Pipeline::builder()
        .add_element(SourceElement::new(true))
        .add_element(derived("src.N"))
        .suppress_process_exceptions(true)
        .build()
        .unwrap();
    assert_eq!(
        run(&pipeline, 3),
        vec![vec![AFTER.to_owned()], vec![], vec![]]
    );
}

#[test]
fn a_source_element_not_in_the_pipeline_logs_nothing() {
    let _serial = start();
    let pipeline = Pipeline::builder()
        .add_element(derived("src.N"))
        .build()
        .unwrap();
    assert_eq!(run(&pipeline, 2), vec![Vec::<String>::new(); 2]);
    assert!(demo(&pipeline).is_err());
}

#[test]
fn each_script_reports_once_and_the_element_once_across_pipelines() {
    let _serial = start();
    let element = Arc::new(
        DerivedPropertyElement::builder()
            .add_script("Demo", &script("Demo", "src.N", ""))
            .add_script("Other", &script("Other", "src.N", ""))
            .build()
            .unwrap(),
    );
    let pipeline = |element: &Arc<DerivedPropertyElement>| {
        Pipeline::builder()
            .add_element(element.clone())
            .add_element(SourceElement::new(false))
            .build()
            .unwrap()
    };
    let first = pipeline(&element);
    let records = run(&first, 2);
    assert_eq!(records[0].len(), 2, "{records:?}");
    assert_eq!(records[0][0], AFTER);
    assert!(
        records[0][1].starts_with("ERROR The script 'Other' reads 'src.N',"),
        "{records:?}"
    );
    assert_eq!(records[1], Vec::<String>::new());

    let second = pipeline(&element);
    assert_eq!(run(&second, 2), vec![Vec::<String>::new(); 2]);
}

#[test]
fn a_script_reading_another_derived_property_logs_nothing() {
    let _serial = start();
    let pipeline = Pipeline::builder()
        .add_element(derived("derived.Other"))
        .build()
        .unwrap();
    assert_eq!(run(&pipeline, 2), vec![Vec::<String>::new(); 2]);
}

#[test]
fn a_script_whose_source_was_carried_logs_nothing_after_one_whose_source_was_not() {
    let _serial = start();
    let element = Arc::new(
        DerivedPropertyElement::builder()
            .add_script("Demo", &script("Demo", "src.N", ""))
            .add_script("Other", &script("Other", "pre.N", ""))
            .build()
            .unwrap(),
    );
    let pipeline = Pipeline::builder()
        .add_element(SourceElement::writing("pre", false))
        .add_element(element)
        .add_element(SourceElement::new(false))
        .build()
        .unwrap();
    assert_eq!(run(&pipeline, 2), vec![vec![AFTER.to_owned()], vec![]]);
}

#[test]
fn a_script_reading_two_missing_elements_names_the_first_it_reads() {
    let _serial = start();
    let text = "Format: 1\n\
        Name: Demo\n\
        Version: 1.0.0\n\
        Output:\n  Name: Demo\n  Description: A demonstration.\n  ValueType: string\n  \
        IsList: false\n\
        Checks:\n  First:\n    Property: src.N\n    Lt: 2\n  \
        Second:\n    Property: late.N\n    Lt: 2\n\
        Rules:\n  - When:\n      Check: First\n    Then: \"first\"\n  \
        - When:\n      Check: Second\n    Then: \"second\"\n  - Else: \"no\"\n";
    let element = Arc::new(
        DerivedPropertyElement::builder()
            .add_script("Demo", text)
            .build()
            .expect("the script is sound"),
    );
    let pipeline = Pipeline::builder()
        .add_element(element)
        .add_element(SourceElement::new(false))
        .add_element(SourceElement::writing("late", false))
        .build()
        .unwrap();
    assert_eq!(run(&pipeline, 2), vec![vec![AFTER.to_owned()], vec![]]);
}
