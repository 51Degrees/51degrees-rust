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

//! The 51Did client logs no deprecation warning while the old name of the
//! endpoint variable goes unread. Kept apart from `endpoint_variable.rs`
//! because the warning is logged once for the whole process. Only a process
//! that never takes its endpoint from the old name can show that nothing is
//! logged, so this binary holds one test and never sets the old name alone
//! without an endpoint on the builder.

use std::sync::{Arc, Mutex};

use fodid_client::{
    DidClient, DidHttpClient, DidHttpRequest, DidHttpResponse, LocalBoxFuture, DEFAULT_ENDPOINT,
};

/// The variable read first.
const VARIABLE: &str = "_51DEGREES_CLOUD_ENDPOINT";

/// The deprecated name the variable had before.
const OLD_VARIABLE: &str = "FOD_CLOUD_API_URL";

const NEW_BASE: &str = "https://new.example.test/api/v4/";
const OLD_BASE: &str = "https://old.example.test/api/v4/";
const GIVEN_BASE: &str = "https://given.example.test/api/v4/";

/// Every record from this crate at warning level or above, as "LEVEL
/// message", since the process started.
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn && metadata.target().starts_with("fodid_client")
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

/// A transport no test here reaches, because building a client sends nothing.
struct NoNetwork;

impl DidHttpClient for NoNetwork {
    fn send<'a>(
        &'a self,
        _request: &'a DidHttpRequest,
    ) -> LocalBoxFuture<'a, Result<DidHttpResponse, String>> {
        Box::pin(async { Err("no request is expected".to_owned()) })
    }
}

fn set(name: &str, value: Option<&str>) {
    match value {
        Some(value) => std::env::set_var(name, value),
        None => std::env::remove_var(name),
    }
}

/// The endpoint of a client built with the two variables set as given, and
/// with `given` as the builder's own endpoint when there is one. Both
/// variables are cleared again afterwards.
fn endpoint_with(new: Option<&str>, old: Option<&str>, given: Option<&str>) -> String {
    set(VARIABLE, new);
    set(OLD_VARIABLE, old);
    let mut builder = DidClient::builder("test-resource-key").http_client(Arc::new(NoNetwork));
    if let Some(given) = given {
        builder = builder.endpoint(given);
    }
    let built = builder.build();
    set(VARIABLE, None);
    set(OLD_VARIABLE, None);
    built.expect("the client builds").endpoint().to_owned()
}

#[test]
fn nothing_is_logged_while_the_old_variable_is_not_read() {
    log::set_logger(&LOGGER).expect("no other logger is installed");
    log::set_max_level(log::LevelFilter::Warn);

    // The new name wins over the old one.
    assert_eq!(
        endpoint_with(Some(NEW_BASE), Some(OLD_BASE), None),
        NEW_BASE
    );
    // The builder's endpoint wins, and the environment is not read at all,
    // so the old name being the only one set changes nothing.
    assert_eq!(
        endpoint_with(None, Some(OLD_BASE), Some(GIVEN_BASE)),
        GIVEN_BASE
    );
    // Neither name is set.
    assert_eq!(endpoint_with(None, None, None), DEFAULT_ENDPOINT);

    let deprecations: Vec<String> = LOGGED
        .lock()
        .unwrap()
        .iter()
        .filter(|record| record.contains("deprecated"))
        .cloned()
        .collect();
    assert_eq!(deprecations, Vec::<String>::new());
}
