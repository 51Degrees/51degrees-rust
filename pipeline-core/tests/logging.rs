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

//! What processing logs. Kept in its own test binary because a logger is
//! installed once for the whole process.

use std::sync::{Arc, Mutex};

use fiftyone_pipeline_core::{
    Error, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, Pipeline,
    PropertyMetaData, Result,
};

/// Log records from every test in this binary, as "LEVEL message".
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Keeps every log record in [`LOGGED`].
struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        LOGGED
            .lock()
            .unwrap()
            .push(format!("{} {}", record.level(), record.args()));
    }

    fn flush(&self) {}
}

static LOGGER: TestLogger = TestLogger;

/// Send log records to [`LOGGED`]. Safe to call from every test.
fn capture_logs() {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Trace);
}

/// The records that mention `text`, so tests running at once do not see each
/// other's.
fn logged_about(text: &str) -> Vec<String> {
    LOGGED
        .lock()
        .unwrap()
        .iter()
        .filter(|record| record.contains(text))
        .cloned()
        .collect()
}

/// An element that always fails with the error `make` builds.
struct FailingElement {
    key: &'static str,
    make: fn() -> Error,
    filter: EvidenceKeyFilterWhitelist,
}

impl FailingElement {
    fn new(key: &'static str, make: fn() -> Error) -> Self {
        FailingElement {
            key,
            make,
            filter: EvidenceKeyFilterWhitelist::default(),
        }
    }
}

impl FlowElement for FailingElement {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
        Err((self.make)())
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

fn process(element: FailingElement, suppress: bool) -> Result<()> {
    Pipeline::builder()
        .add_element(Arc::new(element))
        .suppress_process_exceptions(suppress)
        .build()
        .unwrap()
        .create_flow_data()
        .process()
}

#[test]
fn a_suppressed_error_is_logged() {
    capture_logs();
    process(
        FailingElement::new("suppressed-failure", || {
            Error::configuration("deliberate failure")
        }),
        true,
    )
    .unwrap();

    assert_eq!(
        logged_about("'suppressed-failure'"),
        vec![
            "DEBUG Suppressed an error in element 'suppressed-failure': pipeline \
             configuration error: deliberate failure"
        ]
    );
}

#[test]
fn a_suppressed_error_is_logged_without_credentials() {
    const NOT_A_KEY: &str = "AQ-NOT-A-REAL-KEY-000000";
    capture_logs();
    process(
        FailingElement::new("suppressed-cloud", || Error::CloudRequest {
            status_code: 400,
            retry_after_seconds: None,
            message: format!("'{NOT_A_KEY}' could not be read as a valid resource key."),
        }),
        true,
    )
    .unwrap();

    let records = logged_about("'suppressed-cloud'");
    assert_eq!(records.len(), 1, "{records:?}");
    assert!(!records[0].contains(NOT_A_KEY), "{records:?}");
    assert!(records[0].contains("status 400"), "{records:?}");
}

#[test]
fn a_suppressed_error_in_a_parallel_group_is_logged() {
    capture_logs();
    let group: [Arc<dyn FlowElement>; 2] = [
        Arc::new(FailingElement::new("grouped-failure", || {
            Error::configuration("deliberate failure")
        })),
        Arc::new(FailingElement::new("grouped-beside", || {
            Error::configuration("deliberate failure")
        })),
    ];
    Pipeline::builder()
        .add_parallel_elements(group)
        .suppress_process_exceptions(true)
        .build()
        .unwrap()
        .create_flow_data()
        .process()
        .unwrap();

    assert_eq!(
        logged_about("'grouped-failure'"),
        vec![
            "DEBUG Suppressed an error in element 'grouped-failure': pipeline \
             configuration error: deliberate failure"
        ]
    );
    assert_eq!(logged_about("'grouped-beside'").len(), 1);
}

#[test]
fn a_returned_error_is_left_to_the_caller() {
    capture_logs();
    let result = process(
        FailingElement::new("returned-failure", || {
            Error::configuration("deliberate failure")
        }),
        false,
    );

    assert!(matches!(result, Err(Error::Aggregate(_))));
    assert!(logged_about("'returned-failure'").is_empty());
}
