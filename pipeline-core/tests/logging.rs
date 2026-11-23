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
use std::thread;
use std::time::Duration;

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

/// How long the first of two failing group members waits before it fails,
/// so that the second finishes first.
const FIRST_MEMBER_WAIT_MILLISECONDS: u64 = 100;

/// With errors returned, two failing members of a group are logged at error
/// level and returned in the order they were given, even when the first
/// finishes last.
#[test]
fn failing_group_members_are_logged_in_order_when_errors_are_returned() {
    capture_logs();
    let group: [Arc<dyn FlowElement>; 2] = [
        Arc::new(FailingElement::new("ordered-first", || {
            thread::sleep(Duration::from_millis(FIRST_MEMBER_WAIT_MILLISECONDS));
            Error::configuration("first failure")
        })),
        Arc::new(FailingElement::new("ordered-second", || {
            Error::configuration("second failure")
        })),
    ];
    let result = Pipeline::builder()
        .add_parallel_elements(group)
        .build()
        .unwrap()
        .create_flow_data()
        .process();

    assert_eq!(
        logged_about("'ordered-"),
        vec![
            "ERROR An error occurred in element 'ordered-first': pipeline \
             configuration error: first failure",
            "ERROR An error occurred in element 'ordered-second': pipeline \
             configuration error: second failure",
        ]
    );
    let errors = match result {
        Err(Error::Aggregate(errors)) => errors,
        other => panic!("expected the aggregate error, got {other:?}"),
    };
    let keys: Vec<&str> = errors
        .iter()
        .map(|error| error.element_data_key.as_str())
        .collect();
    assert_eq!(keys, ["ordered-first", "ordered-second"]);
}

/// Filling every thread parallel groups share, which needs threads, so
/// WebAssembly leaves it out.
#[cfg(not(target_family = "wasm"))]
mod shared_thread_limit {
    use std::sync::Condvar;

    use super::*;

    /// More elements than there are threads shared by parallel groups.
    const GROUP_SIZE: usize = 80;

    /// A gate the elements of a group wait at until the first opens it.
    #[derive(Default)]
    struct Gate {
        open: Mutex<bool>,
        opened: Condvar,
    }

    /// An element that opens its gate, or waits until the gate is open.
    struct GateElement {
        key: String,
        gate: Arc<Gate>,
        opens: bool,
        filter: EvidenceKeyFilterWhitelist,
    }

    impl FlowElement for GateElement {
        fn process(&self, _data: &mut FlowData) -> Result<()> {
            let mut open = self.gate.open.lock().unwrap();
            if self.opens {
                *open = true;
                self.gate.opened.notify_all();
            }
            while !*open {
                open = self.gate.opened.wait(open).unwrap();
            }
            Ok(())
        }
        fn data_key(&self) -> &str {
            &self.key
        }
        fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
            &self.filter
        }
        fn properties(&self) -> &[PropertyMetaData] {
            &[]
        }
    }

    /// Process a group whose first element opens a gate every other element
    /// waits at. The first runs only after every other element has been
    /// offered to the shared threads, and none of those can finish before
    /// it, so a group larger than the shared threads finds them all in use.
    fn process_a_group_larger_than_the_shared_threads() {
        let gate = Arc::new(Gate::default());
        let group = (0..GROUP_SIZE).map(|index| {
            Arc::new(GateElement {
                key: format!("gate-{index}"),
                gate: Arc::clone(&gate),
                opens: index == 0,
                filter: EvidenceKeyFilterWhitelist::default(),
            }) as Arc<dyn FlowElement>
        });
        Pipeline::builder()
            .add_parallel_elements(group)
            .build()
            .unwrap()
            .create_flow_data()
            .process()
            .unwrap();
    }

    /// The first time every shared thread is in use, one warning names the
    /// limit, and filling them again logs nothing more.
    #[test]
    fn reaching_the_shared_thread_limit_is_logged_once() {
        capture_logs();
        process_a_group_larger_than_the_shared_threads();
        process_a_group_larger_than_the_shared_threads();

        let records = logged_about("threads shared by parallel groups");
        assert_eq!(records.len(), 1, "{records:?}");
        assert!(
            records[0].starts_with("WARN All 64 threads shared by parallel groups"),
            "{records:?}"
        );
    }
}

/// .NET's pipeline logs one error-level record for an element that fails
/// while errors are not suppressed, and still throws.
#[test]
fn a_returned_error_is_logged_at_error_level() {
    capture_logs();
    let result = process(
        FailingElement::new("returned-failure", || {
            Error::configuration("deliberate failure")
        }),
        false,
    );

    assert!(matches!(result, Err(Error::Aggregate(_))));
    assert_eq!(
        logged_about("'returned-failure'"),
        vec![
            "ERROR An error occurred in element 'returned-failure': pipeline \
             configuration error: deliberate failure"
        ]
    );
}

#[test]
fn a_returned_error_is_logged_without_credentials() {
    const NOT_A_KEY: &str = "AQ-NOT-A-REAL-KEY-000001";
    capture_logs();
    let result = process(
        FailingElement::new("returned-cloud", || Error::CloudRequest {
            status_code: 400,
            retry_after_seconds: None,
            message: format!("'{NOT_A_KEY}' could not be read as a valid resource key."),
        }),
        false,
    );

    assert!(matches!(result, Err(Error::Aggregate(_))));
    let records = logged_about("'returned-cloud'");
    assert_eq!(records.len(), 1, "{records:?}");
    assert!(records[0].starts_with("ERROR "), "{records:?}");
    assert!(!records[0].contains(NOT_A_KEY), "{records:?}");
    assert!(records[0].contains("status 400"), "{records:?}");
}

/// Adding evidence logs the key at debug level and never the value, because
/// evidence can carry credentials.
#[test]
fn added_evidence_is_logged_by_key_without_its_value() {
    const KEY: &str = "query.logged-added-key";
    const VALUE: &str = "value-that-stays-out-of-logs";
    capture_logs();
    let mut data = Pipeline::builder()
        .add_element(Arc::new(FailingElement::new("added-evidence", || {
            Error::configuration("never processed")
        })))
        .build()
        .unwrap()
        .create_flow_data();
    data.add_evidence(KEY, VALUE);

    assert_eq!(data.evidence().get(KEY), Some(VALUE));
    assert_eq!(
        logged_about(KEY),
        vec![format!("DEBUG Evidence '{KEY}' was added to a flow data.")]
    );
    assert!(logged_about(VALUE).is_empty());
}
