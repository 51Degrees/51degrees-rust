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

//! The environment variables the cloud request engine reads its base endpoint
//! from. `_51DEGREES_CLOUD_ENDPOINT` is read first. The name it replaced is
//! deprecated and is read only when the first is unset or empty, with one
//! warning. Kept in its own test binary because the environment and the
//! logger belong to the whole process, and each test holds [`SERIAL`] so no
//! other test changes the variables under it. That nothing is logged while
//! the old name goes unread is tested in `endpoint_variable_quiet.rs`, which
//! needs a process the warning has never been logged in.

use std::sync::{Arc, Mutex, MutexGuard};

use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine, CLOUD_URI_DEFAULT,
};

/// The variable read first.
const VARIABLE: &str = "_51DEGREES_CLOUD_ENDPOINT";

/// The deprecated name the variable had before.
const OLD_VARIABLE: &str = "51DEGREES_CLOUD_ENDPOINT";

const NEW_BASE: &str = "https://new.example.test/api/v4/";
const OLD_BASE: &str = "https://old.example.test/api/v4/";

/// The one warning the engine logs for the deprecated name.
const WARNING: &str = "WARN The environment variable '51DEGREES_CLOUD_ENDPOINT' is deprecated \
                       and '_51DEGREES_CLOUD_ENDPOINT' replaces it.";

/// Every record from this crate at warning level or above, as "LEVEL
/// message", since the process started.
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Held by each test for its whole run.
static SERIAL: Mutex<()> = Mutex::new(());

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

/// Install the logger and wait for any other test to finish. The records are
/// never cleared, because the warning is logged once for the whole process.
fn start() -> MutexGuard<'static, ()> {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Warn);
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The records logged so far that call a variable deprecated.
fn deprecation_warnings() -> Vec<String> {
    LOGGED
        .lock()
        .unwrap()
        .iter()
        .filter(|record| record.contains("deprecated"))
        .cloned()
        .collect()
}

/// A fake cloud that answers the requests the build makes.
struct FakeCloud;

impl CloudHttpClient for FakeCloud {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        let body = if request.url.contains("evidencekeys") {
            r#"["header.user-agent"]"#
        } else if request.url.contains("accessibleproperties") {
            r#"{"Products":{}}"#
        } else {
            r#"{"device":{"ismobile":true}}"#
        };
        Ok(CloudHttpResponse {
            status: 200,
            body: body.to_owned(),
            retry_after: None,
        })
    }
}

fn set(name: &str, value: Option<&str>) {
    match value {
        Some(value) => std::env::set_var(name, value),
        None => std::env::remove_var(name),
    }
}

/// The data endpoint of an engine built with no endpoint of its own, with
/// the two variables set as given. Both are cleared again afterwards.
fn data_endpoint_with(new: Option<&str>, old: Option<&str>) -> String {
    set(VARIABLE, new);
    set(OLD_VARIABLE, old);
    let built = CloudRequestEngine::builder()
        .resource_key("test-resource-key")
        .http_client(Arc::new(FakeCloud))
        .build();
    set(VARIABLE, None);
    set(OLD_VARIABLE, None);
    built.expect("the engine builds").data_endpoint().to_owned()
}

#[test]
fn only_the_old_variable_gives_its_endpoint() {
    let _serial = start();
    assert_eq!(
        data_endpoint_with(None, Some(OLD_BASE)),
        format!("{OLD_BASE}json")
    );
}

#[test]
fn an_empty_new_variable_leaves_the_old_one_read() {
    let _serial = start();
    assert_eq!(
        data_endpoint_with(Some(""), Some(OLD_BASE)),
        format!("{OLD_BASE}json")
    );
}

#[test]
fn both_variables_give_the_new_one() {
    let _serial = start();
    assert_eq!(
        data_endpoint_with(Some(NEW_BASE), Some(OLD_BASE)),
        format!("{NEW_BASE}json")
    );
}

#[test]
fn neither_variable_gives_the_default() {
    let _serial = start();
    assert_eq!(
        data_endpoint_with(None, None),
        format!("{CLOUD_URI_DEFAULT}json")
    );
}

#[test]
fn an_empty_old_variable_gives_the_default() {
    let _serial = start();
    assert_eq!(
        data_endpoint_with(None, Some("")),
        format!("{CLOUD_URI_DEFAULT}json")
    );
}

#[test]
fn an_empty_new_variable_alone_is_refused_as_before() {
    let _serial = start();
    set(VARIABLE, Some(""));
    set(OLD_VARIABLE, None);
    let built = CloudRequestEngine::builder()
        .resource_key("test-resource-key")
        .http_client(Arc::new(FakeCloud))
        .build();
    set(VARIABLE, None);
    let message = match built {
        Err(error) => error.message().into_owned(),
        Ok(_) => panic!("the engine was built"),
    };
    assert!(message.contains("is not a valid absolute URL"), "{message}");
}

#[test]
fn the_builders_endpoint_wins_over_both_variables() {
    let _serial = start();
    set(VARIABLE, Some(NEW_BASE));
    set(OLD_VARIABLE, Some(OLD_BASE));
    let built = CloudRequestEngine::builder()
        .resource_key("test-resource-key")
        .endpoint("https://given.example.test/api/v4/")
        .http_client(Arc::new(FakeCloud))
        .build();
    set(VARIABLE, None);
    set(OLD_VARIABLE, None);
    assert_eq!(
        built.expect("the engine builds").data_endpoint(),
        "https://given.example.test/api/v4/json"
    );
}

#[test]
fn the_old_variable_is_warned_about_once() {
    let _serial = start();
    for _ in 0..2 {
        assert_eq!(
            data_endpoint_with(None, Some(OLD_BASE)),
            format!("{OLD_BASE}json")
        );
    }
    assert_eq!(deprecation_warnings(), vec![WARNING.to_owned()]);
}

#[test]
fn the_new_constant_holds_the_new_name() {
    assert_eq!(
        fiftyone_cloud_request_engine::CLOUD_ENDPOINT_VARIABLE,
        VARIABLE
    );
}

#[test]
#[allow(deprecated)]
fn the_old_constant_still_holds_the_old_name() {
    assert_eq!(
        fiftyone_cloud_request_engine::CLOUD_ENDPOINT_ENV_VAR,
        OLD_VARIABLE
    );
}
