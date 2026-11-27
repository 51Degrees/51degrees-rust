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

//! The environment variable the cloud request engine reads its base endpoint
//! from, `_51DEGREES_CLOUD_ENDPOINT`. Kept in its own test binary because the
//! environment belongs to the whole process, and each test holds [`SERIAL`]
//! so no other test changes the variable under it.

use std::sync::{Arc, Mutex, MutexGuard};

use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine, CLOUD_URI_DEFAULT,
};

/// The variable the base endpoint is read from.
const VARIABLE: &str = "_51DEGREES_CLOUD_ENDPOINT";

const NEW_BASE: &str = "https://new.example.test/api/v4/";

/// Held by each test for its whole run.
static SERIAL: Mutex<()> = Mutex::new(());

/// Wait for any other test to finish.
fn start() -> MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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
/// the variable set as given. It is cleared again afterwards.
fn data_endpoint_with(value: Option<&str>) -> String {
    set(VARIABLE, value);
    let built = CloudRequestEngine::builder()
        .resource_key("test-resource-key")
        .http_client(Arc::new(FakeCloud))
        .build();
    set(VARIABLE, None);
    built.expect("the engine builds").data_endpoint().to_owned()
}

#[test]
fn the_variable_gives_its_endpoint() {
    let _serial = start();
    assert_eq!(
        data_endpoint_with(Some(NEW_BASE)),
        format!("{NEW_BASE}json")
    );
}

#[test]
fn no_variable_gives_the_default() {
    let _serial = start();
    assert_eq!(data_endpoint_with(None), format!("{CLOUD_URI_DEFAULT}json"));
}

#[test]
fn an_empty_variable_is_refused() {
    let _serial = start();
    set(VARIABLE, Some(""));
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
fn the_builders_endpoint_wins_over_the_variable() {
    let _serial = start();
    set(VARIABLE, Some(NEW_BASE));
    let built = CloudRequestEngine::builder()
        .resource_key("test-resource-key")
        .endpoint("https://given.example.test/api/v4/")
        .http_client(Arc::new(FakeCloud))
        .build();
    set(VARIABLE, None);
    assert_eq!(
        built.expect("the engine builds").data_endpoint(),
        "https://given.example.test/api/v4/json"
    );
}

#[test]
fn the_new_constant_holds_the_new_name() {
    assert_eq!(
        fiftyone_cloud_request_engine::CLOUD_ENDPOINT_VARIABLE,
        VARIABLE
    );
}
