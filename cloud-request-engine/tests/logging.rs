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

//! What the cloud request engine logs at warning level or above, for the
//! cases where .NET's engine logs. Kept in its own test binary because a
//! logger is installed once for the whole process, and each test holds
//! [`SERIAL`] so the records it reads are its own.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine, LruCache,
};
use fiftyone_pipeline_core::{Evidence, Pipeline, Result};

const ENDPOINT: &str = "https://cloud.example.test/api/v4/";

/// Records from this crate at warning level or above, as "LEVEL message".
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Held by each test for its whole run.
static SERIAL: Mutex<()> = Mutex::new(());

/// Keeps this crate's records at warning level or above in [`LOGGED`]. Other
/// crates' records, such as the pipeline's own record of an element's error,
/// are left out so only the engine's are counted.
struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
            && metadata
                .target()
                .starts_with("fiftyone_cloud_request_engine")
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

/// Take the records logged since [`start`] or the last call.
fn take_logged() -> Vec<String> {
    std::mem::take(&mut *LOGGED.lock().unwrap())
}

/// A fake cloud answering each endpoint with a fixed status and body.
struct FakeCloud {
    evidence_keys: (u16, &'static str),
    properties: (u16, &'static str),
    data: (u16, &'static str),
    data_requests: Arc<AtomicUsize>,
}

impl Default for FakeCloud {
    fn default() -> Self {
        FakeCloud {
            evidence_keys: (
                200,
                r#"["header.user-agent","cookie.user-agent","query.user-agent","server.client-ip","custom.client-ip"]"#,
            ),
            properties: (200, r#"{"Products":{}}"#),
            data: (200, r#"{"device":{"ismobile":true}}"#),
            data_requests: Arc::default(),
        }
    }
}

impl CloudHttpClient for FakeCloud {
    fn send(&self, request: &CloudHttpRequest) -> std::result::Result<CloudHttpResponse, String> {
        let (status, body) = if request.url.contains("evidencekeys") {
            self.evidence_keys
        } else if request.url.contains("accessibleproperties") {
            self.properties
        } else {
            self.data_requests.fetch_add(1, Ordering::Relaxed);
            self.data
        };
        Ok(CloudHttpResponse {
            status,
            body: body.to_owned(),
            retry_after: None,
        })
    }
}

fn build(cloud: FakeCloud) -> Result<CloudRequestEngine> {
    CloudRequestEngine::builder()
        .resource_key("test-resource-key")
        .endpoint(ENDPOINT)
        .http_client(Arc::new(cloud))
        .build()
}

fn pipeline(cloud: FakeCloud, suppress: bool) -> Arc<Pipeline> {
    Pipeline::builder()
        .add_element(Arc::new(build(cloud).unwrap()))
        .suppress_process_exceptions(suppress)
        .build()
        .unwrap()
}

fn process(pipeline: &Arc<Pipeline>, evidence: &[(&str, &str)]) {
    let mut builder = Evidence::builder();
    for (key, value) in evidence {
        builder = builder.add(*key, *value);
    }
    let _ = pipeline.create_flow_data_with(builder.build()).process();
}

#[test]
fn each_warning_in_a_response_is_logged_on_every_request() {
    let _serial = start();
    let pipeline = pipeline(
        FakeCloud {
            data: (
                200,
                r#"{"device":{"ismobile":true},"warnings":["first warning","second warning"]}"#,
            ),
            ..FakeCloud::default()
        },
        false,
    );
    assert_eq!(take_logged(), Vec::<String>::new());
    for request in 1..=2 {
        process(&pipeline, &[("header.user-agent", "A")]);
        assert_eq!(
            take_logged(),
            vec!["WARN first warning", "WARN second warning"],
            "request {request}"
        );
    }
}

#[test]
fn a_cached_response_logs_its_warnings_as_a_live_one_does() {
    let _serial = start();
    let cloud = FakeCloud {
        data: (
            200,
            r#"{"device":{"ismobile":true},"warnings":["first warning"]}"#,
        ),
        ..FakeCloud::default()
    };
    let data_requests = Arc::clone(&cloud.data_requests);
    let engine = CloudRequestEngine::builder()
        .resource_key("test-resource-key")
        .endpoint(ENDPOINT)
        .http_client(Arc::new(cloud))
        .cache(Arc::new(LruCache::new(16, 1)))
        .build()
        .unwrap();
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(engine))
        .build()
        .unwrap();
    for request in 1..=2 {
        process(&pipeline, &[("header.user-agent", "A")]);
        assert_eq!(
            take_logged(),
            vec!["WARN first warning"],
            "request {request}"
        );
    }
    assert_eq!(data_requests.load(Ordering::Relaxed), 1, "second was a hit");
}

#[test]
fn a_warning_in_the_accessible_properties_is_logged_at_build() {
    let _serial = start();
    let built = build(FakeCloud {
        properties: (200, r#"{"Products":{},"warnings":["properties warning"]}"#),
        ..FakeCloud::default()
    });
    assert!(built.is_ok());
    assert_eq!(take_logged(), vec!["WARN properties warning"]);

    // Beside an error that fails the build, the warning is logged first.
    let built = build(FakeCloud {
        properties: (
            403,
            r#"{"errors":["refused"],"warnings":["properties warning"]}"#,
        ),
        ..FakeCloud::default()
    });
    assert!(built.is_err());
    assert_eq!(
        take_logged(),
        vec!["WARN properties warning", PROPERTIES_ERROR]
    );
}

#[test]
fn a_warning_beside_an_error_in_a_data_response_is_logged() {
    let _serial = start();
    let pipeline = pipeline(
        FakeCloud {
            data: (
                400,
                r#"{"errors":["evidence refused"],"warnings":["data warning"]}"#,
            ),
            ..FakeCloud::default()
        },
        true,
    );
    assert_eq!(take_logged(), Vec::<String>::new());
    for request in 1..=2 {
        process(&pipeline, &[("header.user-agent", "A")]);
        assert_eq!(
            take_logged(),
            vec!["WARN data warning"],
            "request {request}"
        );
    }
}

#[test]
fn header_evidence_replacing_cookie_evidence_logs_a_warning() {
    let _serial = start();
    let pipeline = pipeline(FakeCloud::default(), false);
    process(
        &pipeline,
        &[("header.user-agent", "A"), ("cookie.user-agent", "B")],
    );
    assert_eq!(
        take_logged(),
        vec!["WARN 'header.user-agent=A' evidence conflicts with 'cookie.user-agent=B'"]
    );
}

#[test]
fn query_evidence_replacing_header_evidence_logs_nothing() {
    let _serial = start();
    let pipeline = pipeline(FakeCloud::default(), false);
    process(
        &pipeline,
        &[("query.user-agent", "A"), ("header.user-agent", "B")],
    );
    assert_eq!(take_logged(), Vec::<String>::new());
}

#[test]
fn two_other_prefixes_sharing_a_name_log_a_warning() {
    let _serial = start();
    let pipeline = pipeline(FakeCloud::default(), false);
    process(
        &pipeline,
        &[
            ("server.client-ip", "1.1.1.1"),
            ("custom.client-ip", "2.2.2.2"),
        ],
    );
    assert_eq!(
        take_logged(),
        vec!["WARN 'server.client-ip=1.1.1.1' evidence conflicts with 'custom.client-ip=2.2.2.2'"]
    );
}

#[test]
fn a_failed_data_request_is_left_to_the_pipeline_to_log() {
    for suppress in [false, true] {
        let _serial = start();
        let pipeline = pipeline(
            FakeCloud {
                data: (500, r#"{"errors":["server fault"]}"#),
                ..FakeCloud::default()
            },
            suppress,
        );
        process(&pipeline, &[("header.user-agent", "A")]);
        assert_eq!(take_logged(), Vec::<String>::new(), "suppress {suppress}");
    }
}

#[test]
fn a_data_response_with_no_body_is_left_to_the_pipeline_to_log() {
    let _serial = start();
    let pipeline = pipeline(
        FakeCloud {
            data: (200, ""),
            ..FakeCloud::default()
        },
        false,
    );
    process(&pipeline, &[("header.user-agent", "A")]);
    assert_eq!(take_logged(), Vec::<String>::new());
}

const EVIDENCE_KEYS_ERROR: &str = "ERROR Failed to retrieve evidence keys from cloud service \
     at https://cloud.example.test/api/v4/evidencekeys.";
const PROPERTIES_ERROR: &str = "ERROR Failed to retrieve available properties from cloud \
     service at https://cloud.example.test/api/v4/accessibleproperties.";
const WARM_UP_WARNING: &str = "WARN Failed to warm up the cloud request engine due to a \
     transient error. The engine will retry the discovery requests when it is first used.";

#[test]
fn evidence_keys_refused_with_a_4xx_log_one_error_and_fail_the_build() {
    let _serial = start();
    let built = build(FakeCloud {
        evidence_keys: (403, r#"{"errors":["refused"]}"#),
        ..FakeCloud::default()
    });
    assert!(built.is_err());
    assert_eq!(take_logged(), vec![EVIDENCE_KEYS_ERROR]);
}

#[test]
fn evidence_keys_failing_for_a_transient_reason_log_an_error_and_a_warning() {
    for answer in [(500, "fault"), (200, "")] {
        let _serial = start();
        let built = build(FakeCloud {
            evidence_keys: answer,
            ..FakeCloud::default()
        });
        assert!(built.is_ok(), "{answer:?}");
        assert_eq!(
            take_logged(),
            vec![EVIDENCE_KEYS_ERROR, WARM_UP_WARNING],
            "{answer:?}"
        );
    }
}

#[test]
fn properties_refused_with_a_4xx_log_one_error_and_fail_the_build() {
    let _serial = start();
    let built = build(FakeCloud {
        properties: (403, r#"{"errors":["refused"]}"#),
        ..FakeCloud::default()
    });
    assert!(built.is_err());
    assert_eq!(take_logged(), vec![PROPERTIES_ERROR]);
}

#[test]
fn properties_failing_for_a_transient_reason_log_an_error_and_a_warning() {
    let _serial = start();
    let built = build(FakeCloud {
        properties: (500, "fault"),
        ..FakeCloud::default()
    });
    assert!(built.is_ok());
    assert_eq!(take_logged(), vec![PROPERTIES_ERROR, WARM_UP_WARNING]);
}

#[test]
fn discovery_json_of_the_wrong_shape_logs_only_the_warning() {
    for cloud in [
        FakeCloud {
            evidence_keys: (200, r#"{"a":1}"#),
            ..FakeCloud::default()
        },
        FakeCloud {
            properties: (200, r#"{"Products":5}"#),
            ..FakeCloud::default()
        },
    ] {
        let _serial = start();
        assert!(build(cloud).is_ok());
        assert_eq!(take_logged(), vec![WARM_UP_WARNING]);
    }
}

#[test]
fn each_retry_on_first_use_that_fails_logs_only_the_error() {
    for (cloud, error) in [
        (
            FakeCloud {
                evidence_keys: (500, "fault"),
                ..FakeCloud::default()
            },
            EVIDENCE_KEYS_ERROR,
        ),
        (
            FakeCloud {
                properties: (500, "fault"),
                ..FakeCloud::default()
            },
            PROPERTIES_ERROR,
        ),
    ] {
        let _serial = start();
        let data_requests = Arc::clone(&cloud.data_requests);
        let pipeline = pipeline(cloud, true);
        take_logged();
        for request in 1..=2 {
            process(&pipeline, &[("header.user-agent", "A")]);
            assert_eq!(take_logged(), vec![error], "request {request}");
        }
        assert_eq!(data_requests.load(Ordering::Relaxed), 0);
    }
}
