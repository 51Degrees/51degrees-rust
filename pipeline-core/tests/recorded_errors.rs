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

//! An element that carries on past a failure records it on the flow data,
//! and whether that error fails processing is the element's choice.

use std::sync::{Arc, Mutex};
use std::thread::{self, ThreadId};

use fiftyone_pipeline_core::{
    Error, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, FlowError,
    Pipeline, PropertyMetaData, Result,
};

/// An element that records an error and carries on.
struct RecordingErrorElement {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
    should_throw: bool,
    should_log: bool,
}

impl RecordingErrorElement {
    fn new(should_throw: bool, should_log: bool) -> Self {
        RecordingErrorElement {
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            properties: Vec::new(),
            should_throw,
            should_log,
        }
    }
}

impl FlowElement for RecordingErrorElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        data.add_error(
            FlowError::with_should_throw(
                "recording",
                Error::Pipeline {
                    message: "carried on".to_owned(),
                },
                self.should_throw,
            ),
            self.should_log,
        );
        Ok(())
    }
    fn data_key(&self) -> &str {
        "recording"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

#[test]
fn an_element_can_record_an_error_that_does_not_fail_processing() {
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(RecordingErrorElement::new(false, false)))
        .suppress_process_exceptions(false)
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    assert!(data.process().is_ok());
    assert_eq!(data.errors().len(), 1);
    assert_eq!(data.errors()[0].element_data_key, "recording");
    assert!(!data.errors()[0].should_throw);
    assert_eq!(
        data.errors()[0].source.to_string(),
        "pipeline error: carried on"
    );
}

#[test]
fn an_error_an_element_records_to_throw_fails_processing() {
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(RecordingErrorElement::new(true, false)))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    match data.process() {
        Err(Error::Aggregate(errors)) => {
            assert_eq!(errors.len(), 1);
            assert!(matches!(errors[0].source, Error::Pipeline { .. }));
            assert_eq!(errors[0].element_data_key, "recording");
        }
        other => panic!("expected an aggregate error, got {other:?}"),
    }
}

/// Error-level records with the thread that logged each, because the tests in
/// this binary run in parallel and each reads only its own.
static ERRORS: Mutex<Vec<(ThreadId, String)>> = Mutex::new(Vec::new());

struct ErrorLogger;

impl log::Log for ErrorLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() == log::Level::Error
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            ERRORS
                .lock()
                .unwrap()
                .push((thread::current().id(), record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

static LOGGER: ErrorLogger = ErrorLogger;

/// The error-level records the current thread has logged, installing the
/// logger first if no test has yet.
fn errors_logged() -> Vec<String> {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Error);
    let me = thread::current().id();
    ERRORS
        .lock()
        .unwrap()
        .iter()
        .filter(|(id, _)| *id == me)
        .map(|(_, message)| message.clone())
        .collect()
}

/// An error an element records with `should_log` set is logged once at error
/// level whether or not the pipeline suppresses errors, as .NET's
/// `FlowData.AddError` does, and one recorded without it is not logged.
#[test]
fn an_error_recorded_to_log_is_logged_once_whether_or_not_suppressed() {
    for suppress in [false, true] {
        for should_log in [true, false] {
            let from = errors_logged().len();
            let pipeline = Pipeline::builder()
                .add_element(Arc::new(RecordingErrorElement::new(false, should_log)))
                .suppress_process_exceptions(suppress)
                .build()
                .unwrap();
            let mut data = pipeline.create_flow_data();
            assert!(data.process().is_ok());
            let logged = errors_logged().split_off(from);
            let expected = if should_log {
                vec![
                    "An error occurred in element 'recording': pipeline error: carried on"
                        .to_owned(),
                ]
            } else {
                Vec::new()
            };
            assert_eq!(
                logged, expected,
                "suppress {suppress}, should_log {should_log}"
            );
        }
    }
}

/// An element that does nothing, to make a group with another.
struct IdleElement {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

impl IdleElement {
    fn new() -> Self {
        IdleElement {
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            properties: Vec::new(),
        }
    }
}

impl FlowElement for IdleElement {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
        Ok(())
    }
    fn data_key(&self) -> &str {
        "idle"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

/// An error a member of a parallel group records reaches the flow data once
/// the group has finished, fails processing only when it should, and is logged
/// once when it should be, as in sequence. The recording element is the first
/// of the group, which runs on the calling thread, so its record is this
/// thread's.
#[test]
fn an_error_a_group_member_records_reaches_the_flow_data() {
    for should_throw in [false, true] {
        for should_log in [false, true] {
            let case = format!("should_throw {should_throw}, should_log {should_log}");
            let from = errors_logged().len();
            let group: [Arc<dyn FlowElement>; 2] = [
                Arc::new(RecordingErrorElement::new(should_throw, should_log)),
                Arc::new(IdleElement::new()),
            ];
            let pipeline = Pipeline::builder()
                .add_parallel_elements(group)
                .build()
                .unwrap();
            let mut data = pipeline.create_flow_data();
            let result = data.process();

            let keys: Vec<&str> = data
                .errors()
                .iter()
                .map(|error| error.element_data_key.as_str())
                .collect();
            assert_eq!(keys, ["recording"], "{case}");
            match result {
                Ok(()) => assert!(!should_throw, "{case}"),
                Err(Error::Aggregate(errors)) => {
                    assert!(should_throw, "{case}");
                    assert_eq!(errors.len(), 1, "{case}");
                    assert_eq!(errors[0].element_data_key, "recording", "{case}");
                }
                Err(other) => panic!("{case}: expected an aggregate error, got {other:?}"),
            }
            let logged = errors_logged().split_off(from);
            assert_eq!(logged.len(), usize::from(should_log), "{case}: {logged:?}");
        }
    }
}

/// An element that records an error and then fails.
struct RecordingThenFailingElement {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

impl FlowElement for RecordingThenFailingElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        data.add_error(
            FlowError::with_should_throw(
                "recording-then-failing",
                Error::Pipeline {
                    message: "recorded".to_owned(),
                },
                false,
            ),
            false,
        );
        Err(Error::Pipeline {
            message: "returned".to_owned(),
        })
    }
    fn data_key(&self) -> &str {
        "recording-then-failing"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

/// The errors an element of a group records come before the error it
/// returns, as they do in sequence.
#[test]
fn a_group_members_recorded_error_comes_before_its_returned_error() {
    let element = || -> Arc<dyn FlowElement> {
        Arc::new(RecordingThenFailingElement {
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            properties: Vec::new(),
        })
    };
    let errors = |pipeline: Arc<Pipeline>| -> Vec<String> {
        let mut data = pipeline.create_flow_data();
        data.process().expect("errors are suppressed");
        data.errors()
            .iter()
            .map(|error| error.source.to_string())
            .collect()
    };
    let in_sequence = errors(
        Pipeline::builder()
            .add_element(element())
            .suppress_process_exceptions(true)
            .build()
            .unwrap(),
    );
    let group: [Arc<dyn FlowElement>; 2] = [element(), Arc::new(IdleElement::new())];
    let in_a_group = errors(
        Pipeline::builder()
            .add_parallel_elements(group)
            .suppress_process_exceptions(true)
            .build()
            .unwrap(),
    );
    assert_eq!(
        in_sequence,
        ["pipeline error: recorded", "pipeline error: returned"]
    );
    assert_eq!(in_a_group, in_sequence);
}
