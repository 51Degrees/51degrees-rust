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

//! The cloud request engine logs no deprecation warning while the old name of
//! the endpoint variable goes unread. Kept apart from `endpoint_variable.rs`
//! because the warning is logged once for the whole process. Only a process
//! that never takes its endpoint from the old name can show that nothing is
//! logged, so this binary holds one test and never sets the old name alone
//! without an endpoint on the builder.

use std::sync::{Arc, Mutex};

use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine, CLOUD_URI_DEFAULT,
};

/// The variable read first.
const VARIABLE: &str = "_51DEGREES_CLOUD_ENDPOINT";

/// The deprecated name the variable had before.
const OLD_VARIABLE: &str = "51DEGREES_CLOUD_ENDPOINT";

const NEW_BASE: &str = "https://new.example.test/api/v4/";
const OLD_BASE: &str = "https://old.example.test/api/v4/";
const GIVEN_BASE: &str = "https://given.example.test/api/v4/";

/// Every record from this crate at warning level or above, as "LEVEL
/// message", since the process started.
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

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

/// The data endpoint of an engine built with the two variables set as given,
/// and with `given` as the builder's own endpoint when there is one. Both
/// variables are cleared again afterwards.
fn data_endpoint_with(new: Option<&str>, old: Option<&str>, given: Option<&str>) -> String {
    set(VARIABLE, new);
    set(OLD_VARIABLE, old);
    let mut builder = CloudRequestEngine::builder()
        .resource_key("test-resource-key")
        .http_client(Arc::new(FakeCloud));
    if let Some(given) = given {
        builder = builder.endpoint(given);
    }
    let built = builder.build();
    set(VARIABLE, None);
    set(OLD_VARIABLE, None);
    built.expect("the engine builds").data_endpoint().to_owned()
}

#[test]
fn nothing_is_logged_while_the_old_variable_is_not_read() {
    log::set_logger(&LOGGER).expect("no other logger is installed");
    log::set_max_level(log::LevelFilter::Warn);

    // The new name wins over the old one.
    assert_eq!(
        data_endpoint_with(Some(NEW_BASE), Some(OLD_BASE), None),
        format!("{NEW_BASE}json")
    );
    // The builder's endpoint wins, and the environment is not read at all,
    // so the old name being the only one set changes nothing.
    assert_eq!(
        data_endpoint_with(None, Some(OLD_BASE), Some(GIVEN_BASE)),
        format!("{GIVEN_BASE}json")
    );
    // Neither name is set.
    assert_eq!(
        data_endpoint_with(None, None, None),
        format!("{CLOUD_URI_DEFAULT}json")
    );

    let deprecations: Vec<String> = LOGGED
        .lock()
        .unwrap()
        .iter()
        .filter(|record| record.contains("deprecated"))
        .cloned()
        .collect();
    assert_eq!(deprecations, Vec::<String>::new());
}
