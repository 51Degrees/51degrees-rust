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

//! A failing cloud request must never print the resource key.
//!
//! Two ways the key reaches an error are covered here. The address of the
//! accessible-properties request carries the key as a query parameter, and the
//! service repeats the key back inside its own message when it cannot read the
//! key. Both paths run through the builder, which is what every live test and
//! every example calls, and through the first process call, which retries a
//! failure that is not a 4xx. Both the `Display` and the `Debug` form are
//! checked because `unwrap` and `expect` print the `Debug` one.

use std::sync::{Arc, Mutex};

use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine,
};
use fiftyone_pipeline_core::{Evidence, Pipeline};

/// The only key-shaped value written anywhere in this repository. It is not a
/// real resource key and the service refuses it.
const NOT_A_KEY: &str = "AQ-NOT-A-REAL-KEY-000000";

/// A key that does not have the shape of a 51Degrees resource key, used to
/// prove that the engine removes the value it holds and not just the shape.
const KEY_OF_NO_PARTICULAR_SHAPE: &str = "totally-ordinary-looking-value-1234";

/// The reply the live service gives to a key it cannot read, taken from a real
/// call made with the fake key above.
fn refusal_body(key: &str) -> String {
    format!(
        "{{ \"status\":\"400\", \"errors\": [\"'{key}' could not be read as a valid \
         resource key. Check that you copied the full key correctly, or create a new \
         one at https://configure.51degrees.com.\"] }}"
    )
}

/// A transport that answers the evidence-keys request and then fails the
/// accessible-properties one in whichever way the test asked for.
struct FailingProperties {
    failure: Mutex<Result<CloudHttpResponse, String>>,
}

impl FailingProperties {
    fn answering(failure: Result<CloudHttpResponse, String>) -> Arc<Self> {
        Arc::new(FailingProperties {
            failure: Mutex::new(failure),
        })
    }
}

impl CloudHttpClient for FailingProperties {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        if request.url.contains("evidencekeys") {
            return Ok(CloudHttpResponse {
                status: 200,
                body: r#"["header.user-agent"]"#.to_owned(),
                retry_after: None,
            });
        }
        self.failure.lock().unwrap().clone()
    }
}

/// Build an engine for `key` against a transport that fails the discovery
/// request, and return the error as `Display` and as `Debug`. A 4xx answer
/// fails the build. Any other failure is retried by the first process call,
/// so the error is the one that call records.
fn build_failure(key: &str, failure: Result<CloudHttpResponse, String>) -> (String, String) {
    let built = CloudRequestEngine::builder()
        .resource_key(key)
        .endpoint("https://cloud.example.test/api/v4/")
        .http_client(FailingProperties::answering(failure))
        .build();
    match built {
        Err(error) => (error.to_string(), format!("{error:?}")),
        Ok(engine) => first_process_failure(engine),
    }
}

/// Process one flow data through `engine` and return the one error it
/// records, as `Display` and as `Debug`.
fn first_process_failure(engine: CloudRequestEngine) -> (String, String) {
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(engine))
        .suppress_process_exceptions(true)
        .build()
        .unwrap();
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("header.user-agent", "UA").build());
    data.process().unwrap();
    let errors = data.errors();
    assert_eq!(errors.len(), 1, "the first process call records one error");
    let error = &errors[0].source;
    (error.to_string(), format!("{error:?}"))
}

#[test]
fn a_service_refusal_that_quotes_the_key_does_not_print_it() {
    let failure = Ok(CloudHttpResponse {
        status: 400,
        body: refusal_body(NOT_A_KEY),
        retry_after: None,
    });
    let (shown, debugged) = build_failure(NOT_A_KEY, failure);

    assert!(!shown.contains(NOT_A_KEY), "the key was displayed: {shown}");
    assert!(
        !debugged.contains(NOT_A_KEY),
        "the key was in the debug form, which is what unwrap prints: {debugged}"
    );
    // The diagnosis has to survive, or the redaction has cost more than it saved.
    assert!(
        shown.contains("could not be read as a valid resource key"),
        "the service's reason was lost: {shown}"
    );
    assert!(shown.contains("status 400"), "the status was lost: {shown}");
}

#[test]
fn an_address_carrying_the_key_does_not_print_it() {
    // A transport failure, where the only thing quoting the key is the address
    // the engine built for the accessible-properties request.
    let failure = Err(format!(
        "failed to send request to 'https://cloud.example.test/api/v4/\
         accessibleproperties?resource={NOT_A_KEY}': connection refused"
    ));
    let (shown, debugged) = build_failure(NOT_A_KEY, failure);

    assert!(!shown.contains(NOT_A_KEY), "the key was displayed: {shown}");
    assert!(
        !debugged.contains(NOT_A_KEY),
        "the key was debugged: {debugged}"
    );
    assert!(
        shown.contains("connection refused"),
        "the transport's reason was lost: {shown}"
    );
    assert!(
        shown.contains("accessibleproperties"),
        "the failing operation was lost: {shown}"
    );
}

#[test]
fn a_key_of_no_particular_shape_is_removed_by_value() {
    // The shape rules cannot see this one, so only the engine knowing its own
    // credentials keeps it out of the error.
    let failure = Ok(CloudHttpResponse {
        status: 400,
        body: refusal_body(KEY_OF_NO_PARTICULAR_SHAPE),
        retry_after: None,
    });
    let (shown, debugged) = build_failure(KEY_OF_NO_PARTICULAR_SHAPE, failure);

    assert!(
        !shown.contains(KEY_OF_NO_PARTICULAR_SHAPE),
        "the key was displayed: {shown}"
    );
    assert!(
        !debugged.contains(KEY_OF_NO_PARTICULAR_SHAPE),
        "the key was debugged: {debugged}"
    );
    assert!(
        shown.contains("could not be read as a valid resource key"),
        "the service's reason was lost: {shown}"
    );
}

#[test]
fn a_licence_key_the_service_repeats_back_is_removed_by_value() {
    let licence = "licence-value-that-looks-like-nothing";
    let failure = Ok(CloudHttpResponse {
        status: 400,
        body: format!(
            "{{\"errors\":[\"the licence '{licence}' is not valid for this resource\"]}}"
        ),
        retry_after: None,
    });
    let built = CloudRequestEngine::builder()
        .resource_key(NOT_A_KEY)
        .license_key(licence)
        .endpoint("https://cloud.example.test/api/v4/")
        .http_client(FailingProperties::answering(failure))
        .build();
    let Err(error) = built else {
        panic!("the discovery fetch was scripted to fail, so the build must fail");
    };
    let shown = error.to_string();
    let debugged = format!("{error:?}");

    assert!(
        !shown.contains(licence),
        "the licence was displayed: {shown}"
    );
    assert!(
        !debugged.contains(licence),
        "the licence was debugged: {debugged}"
    );
    assert!(
        shown.contains("is not valid for this resource"),
        "the service's reason was lost: {shown}"
    );
}

#[test]
fn the_panic_from_unwrapping_a_failed_build_carries_no_key() {
    // This is exactly what the live tests and the example binaries do.
    let failure = Ok(CloudHttpResponse {
        status: 400,
        body: refusal_body(NOT_A_KEY),
        retry_after: None,
    });
    let client = FailingProperties::answering(failure);
    let outcome = std::panic::catch_unwind(move || {
        CloudRequestEngine::builder()
            .resource_key(NOT_A_KEY)
            .endpoint("https://cloud.example.test/api/v4/")
            .http_client(client)
            .build()
            .expect("request engine builds")
    });
    let Err(panic) = outcome else {
        panic!("the build was scripted to fail, so expect must panic");
    };
    let message = panic
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_else(|| "the panic payload was not a string".to_owned());

    assert!(
        !message.contains(NOT_A_KEY),
        "the key reached the panic message: {message}"
    );
    assert!(
        message.contains("request engine builds"),
        "the test's own wording was lost: {message}"
    );
}
