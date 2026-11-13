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

//! A key that reaches a log record or an error message from the cloud
//! request engine shows only its last 4 characters, with every character
//! before them replaced. Kept in its own test binary because a logger is
//! installed once for the whole process, and each test holds [`SERIAL`] so
//! the records it reads are its own.

use std::sync::{Arc, Mutex, MutexGuard};

use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine,
};
use fiftyone_pipeline_core::{Evidence, Pipeline};

const ENDPOINT: &str = "https://cloud.example.test/api/v4/";

/// The licence key the engine is given. It is not a real key.
const LICENCE: &str = "LICENCEKEY0123456789";

/// [`LICENCE`] as a message may show it.
const LICENCE_MASKED: &str = "****************6789";

/// A resource key a caller passes as evidence. It is not a real key.
const RESOURCE_EVIDENCE: &str = "RESOURCE-VALUE-0001";

/// A licence key a caller passes as evidence under the spelling `licence`.
/// It is not a real key.
const LICENCE_EVIDENCE: &str = "LICENCE-VALUE-0002";

/// [`LICENCE_EVIDENCE`] as a message may show it.
const LICENCE_EVIDENCE_MASKED: &str = "**************0002";

/// [`RESOURCE_EVIDENCE`] as a message may show it.
const RESOURCE_EVIDENCE_MASKED: &str = "***************0001";

/// Every log record, as "LEVEL message".
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Held by each test for its whole run.
static SERIAL: Mutex<()> = Mutex::new(());

/// Keeps every record in [`LOGGED`].
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

/// Install the logger, wait for any other test to finish and clear the
/// records it left.
fn start() -> MutexGuard<'static, ()> {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Trace);
    let guard = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    LOGGED.lock().unwrap().clear();
    guard
}

/// Take the records logged since [`start`].
fn take_logged() -> Vec<String> {
    std::mem::take(&mut *LOGGED.lock().unwrap())
}

/// A fake cloud that accepts the evidence keys given and answers the data
/// request with a fixed status and body.
struct FakeCloud {
    evidence_keys: &'static str,
    data: (u16, String),
}

impl CloudHttpClient for FakeCloud {
    fn send(&self, request: &CloudHttpRequest) -> std::result::Result<CloudHttpResponse, String> {
        let (status, body) = if request.url.contains("evidencekeys") {
            (200, self.evidence_keys.to_owned())
        } else if request.url.contains("accessibleproperties") {
            (200, r#"{"Products":{}}"#.to_owned())
        } else {
            self.data.clone()
        };
        Ok(CloudHttpResponse {
            status,
            body,
            retry_after: None,
        })
    }
}

/// Process `evidence` once through an engine holding [`LICENCE`], and return
/// each error the flow data records, as `Display` and as `Debug`.
fn process(cloud: FakeCloud, evidence: &[(&str, &str)]) -> Vec<String> {
    let engine = CloudRequestEngine::builder()
        .resource_key("test-resource-key")
        .license_key(LICENCE)
        .endpoint(ENDPOINT)
        .http_client(Arc::new(cloud))
        .build()
        .unwrap();
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(engine))
        .suppress_process_exceptions(true)
        .build()
        .unwrap();
    let mut builder = Evidence::builder();
    for (key, value) in evidence {
        builder = builder.add(*key, *value);
    }
    let mut data = pipeline.create_flow_data_with(builder.build());
    data.process().unwrap();
    data.errors()
        .iter()
        .flat_map(|error| [error.source.to_string(), format!("{:?}", error.source)])
        .collect()
}

/// Fail when any text shows `key`.
fn assert_none_shows(texts: &[String], key: &str) {
    for text in texts {
        assert!(!text.contains(key), "the key is shown in full: {text}");
    }
}

/// A failed request carrying a licence key leaves only its masked form in
/// the error and in every log record, as the specification's logging page
/// asks a test to show.
#[test]
fn a_licence_key_the_service_repeats_in_an_error_shows_only_its_last_four_characters() {
    let _serial = start();
    let errors = process(
        FakeCloud {
            evidence_keys: r#"["header.user-agent"]"#,
            data: (
                400,
                format!(
                    "{{\"errors\":[\"the licence '{LICENCE}' is not valid for this resource\"]}}"
                ),
            ),
        },
        &[("header.user-agent", "UA")],
    );
    let logged = take_logged();

    assert!(!errors.is_empty(), "the failed request recorded no error");
    assert_none_shows(&errors, LICENCE);
    assert_none_shows(&logged, LICENCE);
    for error in &errors {
        assert!(
            error.contains(&format!("the licence '{LICENCE_MASKED}' is not valid")),
            "the masked key or the reason was lost: {error}"
        );
    }
}

/// A warning the service sends back is logged with the engine's own keys
/// masked, because the shape of a licence key is not enough to find it.
#[test]
fn a_licence_key_the_service_repeats_in_a_warning_shows_only_its_last_four_characters() {
    let _serial = start();
    let errors = process(
        FakeCloud {
            evidence_keys: r#"["header.user-agent"]"#,
            data: (
                200,
                format!(
                    "{{\"device\":{{\"ismobile\":true}},\
                     \"warnings\":[\"the licence '{LICENCE}' expires soon\"]}}"
                ),
            ),
        },
        &[("header.user-agent", "UA")],
    );
    let logged = take_logged();

    assert_eq!(errors, Vec::<String>::new());
    assert_none_shows(&logged, LICENCE);
    assert!(
        logged
            .iter()
            .any(|record| record.contains(&format!("the licence '{LICENCE_MASKED}' expires soon"))),
        "the warning was not logged with the masked key: {logged:?}"
    );
}

/// Evidence that clashes is logged with every other entry ending in the same
/// field. A resource key given as evidence can be one of those, so its value
/// is masked.
#[test]
fn a_resource_key_given_as_evidence_is_masked_in_a_conflict_warning() {
    let _serial = start();
    process(
        FakeCloud {
            evidence_keys: r#"["header.source","cookie.source","query.resource"]"#,
            data: (200, r#"{"device":{"ismobile":true}}"#.to_owned()),
        },
        &[
            ("header.source", "first"),
            ("cookie.source", "second"),
            ("query.resource", RESOURCE_EVIDENCE),
        ],
    );
    let logged = take_logged();

    assert_none_shows(&logged, RESOURCE_EVIDENCE);
    assert!(
        logged
            .iter()
            .any(|record| record.contains("evidence conflicts with")
                && record.contains(&format!("query.resource={RESOURCE_EVIDENCE_MASKED}"))),
        "the conflict was not logged with the masked key: {logged:?}"
    );
}

/// A licence key given as evidence under the spelling `licence` is never
/// sent, and a conflict warning that names it shows it masked, as for the
/// spelling `license`.
#[test]
fn a_licence_key_given_as_evidence_is_masked_in_a_conflict_warning() {
    let _serial = start();
    process(
        FakeCloud {
            evidence_keys: r#"["header.ence","cookie.ence","query.licence"]"#,
            data: (200, r#"{"device":{"ismobile":true}}"#.to_owned()),
        },
        &[
            ("header.ence", "first"),
            ("cookie.ence", "second"),
            ("query.licence", LICENCE_EVIDENCE),
        ],
    );
    let logged = take_logged();

    assert_none_shows(&logged, LICENCE_EVIDENCE);
    assert!(
        logged
            .iter()
            .any(|record| record.contains("evidence conflicts with")
                && record.contains(&format!("query.licence={LICENCE_EVIDENCE_MASKED}"))),
        "the conflict was not logged with the masked key: {logged:?}"
    );
}
