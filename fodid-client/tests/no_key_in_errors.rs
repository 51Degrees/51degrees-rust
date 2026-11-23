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

//! A failing 51Did call must never print the resource key or the licence key.
//!
//! The 51Did routes carry the resource key as part of the route, so the address
//! quotes it, and the service repeats the key back inside its own message when
//! it cannot read the key. Both the `Display` and the `Debug` form are checked,
//! because `unwrap` and `expect` print the `Debug` one.

use std::sync::{Arc, Mutex};

use fodid::{Creator, Crypto, FodId};
use fodid_client::{DidClient, DidHttpClient, DidHttpRequest, DidHttpResponse, LocalBoxFuture};

/// The only key-shaped value written anywhere in this repository. It is not a
/// real resource key and the service refuses it.
const NOT_A_KEY: &str = "AQ-NOT-A-REAL-KEY-000000";

/// A key that does not have the shape of a 51Degrees resource key, used to
/// prove the client removes the value it holds and not just the shape.
const KEY_OF_NO_PARTICULAR_SHAPE: &str = "totally-ordinary-looking-value-1234";

/// The payload header and match key lengths, taken from the specification at
/// <https://github.com/51Degrees/specifications/blob/main/did-specification/identifier-layout.md>
/// rather than from the crate.
const HEADER_LENGTH: usize = 5;
const MATCH_KEY_LENGTH: usize = 32;

/// The flags byte of the payload below. Payload version 0, identifier type
/// probabilistic and usage bit 0 set, which is the smallest flags byte the
/// specification calls a 51Did. A byte of all zeros states no usage at all
/// and is not one.
const FLAGS: u8 = 0b0000_0001;

/// A 51Did that parses, so a verify or redeem call reaches the transport
/// rather than being refused before the call is made. Nothing here depends on
/// the signature, only on the value being a 51Did at all.
fn a_51did() -> String {
    let creator = Creator::new("51degrees.com", Crypto::new()).expect("create a creator");
    let mut payload = vec![0u8; HEADER_LENGTH + MATCH_KEY_LENGTH];
    payload[0] = FLAGS;
    let owid = creator.create(payload).expect("sign the envelope");
    FodId::from_owid(owid)
        .expect("a 51Did")
        .as_base64()
        .expect("encode the 51Did")
}

/// The reply the live service gives to a key it cannot read, taken from a real
/// call made with the fake key above.
fn refusal_body(key: &str) -> String {
    format!(
        "{{ \"errors\":[\"'{key}' could not be read as a valid resource key. Check \
         that you copied the full key correctly, or create a new one at \
         https://configure.51degrees.com.\"]}}"
    )
}

/// Stands in for the network with one scripted answer.
struct Scripted {
    answer: Mutex<Result<DidHttpResponse, String>>,
}

impl Scripted {
    fn answering(status: u16, body: String) -> Arc<Self> {
        Arc::new(Scripted {
            answer: Mutex::new(Ok(DidHttpResponse { status, body })),
        })
    }

    fn failing(message: String) -> Arc<Self> {
        Arc::new(Scripted {
            answer: Mutex::new(Err(message)),
        })
    }
}

impl DidHttpClient for Scripted {
    fn send<'a>(
        &'a self,
        _request: &'a DidHttpRequest,
    ) -> LocalBoxFuture<'a, Result<DidHttpResponse, String>> {
        Box::pin(async move { self.answer.lock().unwrap().clone() })
    }
}

fn client_for(key: &str, transport: Arc<Scripted>) -> DidClient {
    DidClient::builder(key)
        .endpoint("https://cloud.example.test/api/v4/")
        .http_client(transport)
        .build()
        .expect("the client builds")
}

/// Run `call` on the current thread and give back its error as `Display` and
/// as `Debug`.
fn failure(client: DidClient, call: Call) -> (String, String) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a current-thread runtime starts");
    let encoded = a_51did();
    let result = runtime.block_on(async move {
        match call {
            Call::Keys => client.public_keys().await.map(|_| ()),
            Call::Verify => client.verify_encoded(&encoded).await.map(|_| ()),
            Call::Redeem => client
                .redeem_encoded(&encoded, "sealed", None)
                .await
                .map(|_| ()),
        }
    });
    let Err(error) = result else {
        panic!("the transport was scripted to fail, so the call must fail");
    };
    (error.to_string(), format!("{error:?}"))
}

/// Which call the test makes. Without a licence key each one puts the
/// resource key somewhere different, being the route for the key and verify
/// calls and the form body for redeem.
enum Call {
    Keys,
    Verify,
    Redeem,
}

#[test]
fn a_key_fetch_refused_with_the_key_quoted_does_not_print_it() {
    let transport = Scripted::answering(400, refusal_body(NOT_A_KEY));
    let (shown, debugged) = failure(client_for(NOT_A_KEY, transport), Call::Keys);

    assert!(!shown.contains(NOT_A_KEY), "the key was displayed: {shown}");
    assert!(
        !debugged.contains(NOT_A_KEY),
        "the key was in the debug form, which is what unwrap prints: {debugged}"
    );
    assert!(shown.contains("400"), "the status was lost: {shown}");
    assert!(
        shown.contains("could not be read as a valid resource key"),
        "the service's reason was lost: {shown}"
    );
    assert!(
        shown.contains("key endpoint"),
        "the operation was lost: {shown}"
    );
}

#[test]
fn a_transport_failure_quoting_the_route_does_not_print_the_key() {
    let transport = Scripted::failing(format!(
        "failed to send request to 'https://cloud.example.test/api/v4/id/key/{NOT_A_KEY}': \
         connection refused"
    ));
    let (shown, debugged) = failure(client_for(NOT_A_KEY, transport), Call::Keys);

    assert!(!shown.contains(NOT_A_KEY), "the key was displayed: {shown}");
    assert!(
        !debugged.contains(NOT_A_KEY),
        "the key was debugged: {debugged}"
    );
    assert!(
        shown.contains("connection refused"),
        "the transport's reason was lost: {shown}"
    );
}

#[test]
fn a_verify_refusal_that_quotes_the_key_does_not_print_it() {
    let transport = Scripted::answering(400, refusal_body(NOT_A_KEY));
    let (shown, debugged) = failure(client_for(NOT_A_KEY, transport), Call::Verify);

    assert!(!shown.contains(NOT_A_KEY), "the key was displayed: {shown}");
    assert!(
        !debugged.contains(NOT_A_KEY),
        "the key was debugged: {debugged}"
    );
    assert!(
        shown.contains("could not be read as a valid resource key"),
        "the service's reason was lost: {shown}"
    );
}

#[test]
fn a_key_of_no_particular_shape_is_removed_by_value() {
    // The shape rules cannot see this one, so only the client knowing its own
    // credentials keeps it out of the error.
    let transport = Scripted::answering(400, refusal_body(KEY_OF_NO_PARTICULAR_SHAPE));
    let (shown, debugged) = failure(
        client_for(KEY_OF_NO_PARTICULAR_SHAPE, transport),
        Call::Verify,
    );

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
    let transport = Scripted::answering(
        400,
        format!("{{\"errors\":[\"the licence '{licence}' is not valid for this resource\"]}}"),
    );
    let client = DidClient::builder(NOT_A_KEY)
        .licence_key(licence)
        .endpoint("https://cloud.example.test/api/v4/")
        .http_client(transport)
        .build()
        .expect("the client builds");
    let (shown, debugged) = failure(client, Call::Redeem);

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
fn a_key_fetch_on_a_licence_key_refused_with_it_quoted_does_not_print_it() {
    // With a licence key the key fetch sends it in a header, and a 401 is
    // how the service refuses a licence key it will not accept.
    let licence = "licence-value-that-looks-like-nothing";
    let transport = Scripted::answering(
        401,
        format!(r#"{{"errors":["the licence '{licence}' is not valid"]}}"#),
    );
    let client = DidClient::builder(NOT_A_KEY)
        .licence_key(licence)
        .endpoint("https://cloud.example.test/api/v4/")
        .http_client(transport)
        .build()
        .expect("the client builds");
    let (shown, debugged) = failure(client, Call::Keys);

    assert!(
        !shown.contains(licence),
        "the licence was displayed: {shown}"
    );
    assert!(
        !debugged.contains(licence),
        "the licence was debugged: {debugged}"
    );
    assert!(shown.contains("401"), "the status was lost: {shown}");
    assert!(
        shown.contains("key endpoint"),
        "the operation was lost: {shown}"
    );
    assert!(
        shown.contains("is not valid"),
        "the service's reason was lost: {shown}"
    );
}

// Catching the panic needs unwinding, which wasm32-wasip1 does not have,
// so there a panic aborts the test binary.
#[cfg(panic = "unwind")]
#[test]
fn the_panic_from_unwrapping_a_failed_call_carries_no_key() {
    // The client holds trait objects that carry no unwind-safety promise, so
    // it is built inside the closure rather than carried into it.
    let outcome = std::panic::catch_unwind(|| {
        let transport = Scripted::answering(400, refusal_body(NOT_A_KEY));
        let client = client_for(NOT_A_KEY, transport);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("a current-thread runtime starts");
        runtime
            .block_on(async move { client.public_keys().await })
            .expect("the signing keys are fetched")
    });
    let Err(panic) = outcome else {
        panic!("the fetch was scripted to fail, so expect must panic");
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
        message.contains("the signing keys are fetched"),
        "the test's own wording was lost: {message}"
    );
}
