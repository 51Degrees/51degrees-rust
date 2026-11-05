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

//! The checks that protect a key directory fetch and what it returns. Each
//! test's client would serve the input the check refuses, so a refusal
//! proves the check rather than a client that failed.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use fiftyone_agent_signature::constants::*;
use fiftyone_agent_signature::{AgentSignatureElement, HttpClient, HttpRequest, HttpResponse};
use serde_json::Value;

use common::*;

/// The signed key directory response published with the Web Bot Auth test
/// vectors, made with the Ed25519 test key of RFC 9421 Appendix B.1.4.
const DIRECTORY_RESPONSE: &str = include_str!("fixtures/web_bot_auth_directory_response_v1.json");

/// Where the test agent's card is served.
const CARD_URL: &str = "https://signature-agent.test/card";

/// The address a cloud machine answers its own credentials on.
const METADATA_HOST: &str = "169.254.169.254";

/// The address a redirect leads to.
const ELSEWHERE_URL: &str = "https://elsewhere.test/keys";

/// The unverified status with the reason a directory could not be had.
fn directory_unavailable() -> (String, String) {
    (
        STATUS_UNVERIFIED.to_owned(),
        REASON_DIRECTORY_UNAVAILABLE.to_owned(),
    )
}

#[test]
fn an_address_written_as_a_private_ip_address_is_never_fetched() {
    for (agent, parameters, key_url) in [
        (
            "https://10.0.0.1",
            "",
            "https://10.0.0.1/.well-known/http-message-signatures-directory",
        ),
        (
            "https://[::1]",
            "",
            "https://[::1]/.well-known/http-message-signatures-directory",
        ),
        (
            "https://169.254.169.254/latest/meta-data/keys",
            ";type=jwks_uri",
            "https://169.254.169.254/latest/meta-data/keys",
        ),
    ] {
        let harness = Harness::new(
            FakeClient::default().with(key_url, FakeResponse::directory(test_key_directory(None))),
            |builder| builder,
        );
        let signed = Signer {
            agent: Some(agent.to_owned()),
            agent_parameters: parameters.to_owned(),
            ..Signer::default()
        }
        .sign();
        assert_eq!(
            outcome(&harness.process_signed(&signed, &[])),
            directory_unavailable(),
            "{agent}"
        );
        assert_eq!(harness.client.total(), 0, "{agent} was asked for");
    }
}

#[test]
fn an_agent_card_naming_an_address_that_may_not_be_fetched_is_not_followed() {
    for jwks_uri in [
        format!("http://{METADATA_HOST}/latest/meta-data/iam"),
        format!("https://{METADATA_HOST}/latest/meta-data/iam"),
    ] {
        let card = format!(
            "{{\"client_id\":\"{CARD_URL}\",\"client_name\":\"Example Bot\",\
             \"jwks_uri\":\"{jwks_uri}\"}}"
        );
        let harness = Harness::new(
            FakeClient::default()
                .with(CARD_URL, FakeResponse::json(card))
                .with(&jwks_uri, FakeResponse::json(test_key_directory(None))),
            |builder| builder,
        );
        let signed = Signer {
            agent: Some(CARD_URL.to_owned()),
            agent_parameters: ";type=cimd".to_owned(),
            ..Signer::default()
        }
        .sign();
        assert_eq!(
            outcome(&harness.process_signed(&signed, &[])),
            directory_unavailable(),
            "{jwks_uri}"
        );
        assert_eq!(
            harness.client.calls(&jwks_uri),
            0,
            "{jwks_uri} was asked for"
        );
        assert_eq!(harness.client.calls(CARD_URL), 1);
    }
}

#[test]
fn a_directory_that_arrives_from_another_address_is_refused() {
    let signed = Signer::default().sign();
    // An empty address is a client that does not say where its answer came
    // from, which is refused as well.
    for final_url in [ELSEWHERE_URL, ""] {
        let mut response = FakeResponse::directory(test_key_directory(None));
        response.final_url = Some(final_url.to_owned());
        let harness = Harness::new(
            FakeClient::default().with(DIRECTORY_URL, response),
            |builder| builder,
        );
        assert_eq!(
            outcome(&harness.process_signed(&signed, &[])),
            directory_unavailable(),
            "{final_url:?}"
        );
        assert_eq!(harness.client.calls(DIRECTORY_URL), 1);
    }
    // The same directory from the address asked for verifies, so the
    // refusals above were for the address it came from.
    let harness = Harness::with_test_key();
    assert_eq!(
        outcome(&harness.process_signed(&signed, &[])).1,
        REASON_VERIFIED
    );
}

#[test]
fn an_agent_card_that_arrives_from_another_address_is_refused() {
    let card = format!(
        "{{\"client_id\":\"{CARD_URL}\",\"client_name\":\"Example Bot\",\"jwks\":{}}}",
        test_key_directory(None)
    );
    let signed = Signer {
        agent: Some(CARD_URL.to_owned()),
        agent_parameters: ";type=cimd".to_owned(),
        ..Signer::default()
    }
    .sign();
    let mut response = FakeResponse::json(card.clone());
    response.final_url = Some("https://elsewhere.test/card".to_owned());
    let harness = Harness::new(FakeClient::default().with(CARD_URL, response), |builder| {
        builder
    });
    let data = harness.process_signed(&signed, &[]);
    assert_eq!(outcome(&data), directory_unavailable());
    assert_eq!(
        text_or_reason(data.agent_signature_name()),
        Err("No agent card available".to_owned())
    );
    let harness = Harness::new(
        FakeClient::default().with(CARD_URL, FakeResponse::json(card)),
        |builder| builder,
    );
    assert_eq!(
        outcome(&harness.process_signed(&signed, &[])).1,
        REASON_VERIFIED
    );
}

/// The signed directory response, with its body and its `Signature` header
/// changed as given.
fn signed_directory_response(
    change_body: impl Fn(&str) -> String,
    change_signature: impl Fn(&str) -> String,
) -> FakeResponse {
    let vectors: Value = serde_json::from_str(DIRECTORY_RESPONSE).unwrap();
    let vector = &vectors[0];
    assert_eq!(vector["request"]["target_url"], DIRECTORY_URL);
    let headers = vector["response"]["headers"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, value)| {
            let value = value.as_str().unwrap();
            let value = if name.eq_ignore_ascii_case("signature") {
                change_signature(value)
            } else {
                value.to_owned()
            };
            (name.clone(), value)
        })
        .collect();
    let body = vector["response"]["body"].as_str().unwrap();
    FakeResponse {
        content_type: None,
        headers,
        ..FakeResponse::directory(change_body(body))
    }
}

#[test]
fn a_signed_directory_response_is_accepted_and_verifies() {
    let response = signed_directory_response(str::to_owned, str::to_owned);
    let harness = Harness::new(
        FakeClient::default().with(DIRECTORY_URL, response),
        |builder| builder,
    );
    assert_eq!(
        outcome(&harness.process_signed(&Signer::default().sign(), &[])),
        (STATUS_VERIFIED.to_owned(), REASON_VERIFIED.to_owned())
    );
}

#[test]
fn a_directory_body_that_does_not_match_its_digest_is_unavailable() {
    // One space inside the JSON, so the keys are the same and the digest is
    // the only thing that can fail.
    let response = signed_directory_response(
        |body| {
            let changed = body.replacen("{\"keys\"", "{ \"keys\"", 1);
            assert_ne!(changed, body);
            changed
        },
        str::to_owned,
    );
    let harness = Harness::new(
        FakeClient::default().with(DIRECTORY_URL, response),
        |builder| builder,
    );
    assert_eq!(
        outcome(&harness.process_signed(&Signer::default().sign(), &[])),
        directory_unavailable()
    );
}

#[test]
fn a_directory_response_whose_signature_does_not_check_out_is_unavailable() {
    // The body and its digest are untouched, so the signature over the
    // response is the only thing that can fail.
    let response = signed_directory_response(str::to_owned, |signature| {
        corrupt(Signed {
            signature: signature.to_owned(),
            signature_input: String::new(),
            signature_agent: None,
        })
        .signature
    });
    let harness = Harness::new(
        FakeClient::default().with(DIRECTORY_URL, response),
        |builder| builder,
    );
    assert_eq!(
        outcome(&harness.process_signed(&Signer::default().sign(), &[])),
        directory_unavailable()
    );
}

/// A request signed under the label given.
fn labelled(label: &str) -> Signer {
    Signer {
        label: label.to_owned(),
        ..Signer::default()
    }
}

#[test]
fn a_second_tagged_signature_verifies_when_the_first_does_not() {
    let harness = Harness::with_test_key();
    let merged = merge(&[corrupt(labelled("sig1").sign()), labelled("sig2").sign()]);
    assert_eq!(
        outcome(&harness.process_signed(&merged, &[])),
        (STATUS_VERIFIED.to_owned(), REASON_VERIFIED.to_owned())
    );
}

#[test]
fn the_first_tagged_signatures_outcome_is_reported_when_none_verifies() {
    let unbound = |label: &str| {
        Signer {
            cover_authority: false,
            ..labelled(label)
        }
        .sign()
    };
    let harness = Harness::with_test_key();
    let merged = merge(&[corrupt(labelled("sig1").sign()), unbound("sig2")]);
    assert_eq!(
        outcome(&harness.process_signed(&merged, &[])).1,
        REASON_SIGNATURE_MISMATCH
    );
    let merged = merge(&[unbound("sig1"), corrupt(labelled("sig2").sign())]);
    assert_eq!(
        outcome(&harness.process_signed(&merged, &[])).1,
        REASON_UNBOUND_SIGNATURE
    );
}

#[test]
fn a_signature_with_another_tag_does_not_stop_the_tagged_one_verifying() {
    let harness = Harness::with_test_key();
    let other = Signer {
        tag: Some("some-other-purpose".to_owned()),
        ..labelled("sig1")
    }
    .sign();
    let merged = merge(&[other, labelled("sig2").sign()]);
    assert_eq!(
        outcome(&harness.process_signed(&merged, &[])).1,
        REASON_VERIFIED
    );
}

#[test]
fn only_the_first_three_tagged_signatures_are_checked() {
    let harness = Harness::with_test_key();
    let third = merge(&[
        corrupt(labelled("sig1").sign()),
        corrupt(labelled("sig2").sign()),
        labelled("sig3").sign(),
    ]);
    assert_eq!(
        outcome(&harness.process_signed(&third, &[])).1,
        REASON_VERIFIED
    );
    let fourth = merge(&[
        corrupt(labelled("sig1").sign()),
        corrupt(labelled("sig2").sign()),
        corrupt(labelled("sig3").sign()),
        labelled("sig4").sign(),
    ]);
    assert_eq!(
        outcome(&harness.process_signed(&fourth, &[])),
        (
            STATUS_INVALID.to_owned(),
            REASON_SIGNATURE_MISMATCH.to_owned()
        )
    );
}

#[test]
fn a_failure_under_one_member_type_does_not_answer_another() {
    // A set of keys is a valid answer for the jwks_uri type and an
    // unreadable one for the cimd type, because an agent card is a
    // different document.
    let keys_url = "https://signature-agent.test/keys";
    let harness = Harness::new(
        FakeClient::default().with(keys_url, FakeResponse::json(test_key_directory(None))),
        |builder| builder,
    );
    let signed_as = |agent_type: &str| {
        Signer {
            agent: Some(keys_url.to_owned()),
            agent_parameters: format!(";type={agent_type}"),
            ..Signer::default()
        }
        .sign()
    };
    assert_eq!(
        outcome(&harness.process_signed(&signed_as("cimd"), &[])),
        directory_unavailable()
    );
    assert_eq!(
        outcome(&harness.process_signed(&signed_as("jwks_uri"), &[])).1,
        REASON_VERIFIED
    );
    assert_eq!(harness.client.calls(keys_url), 2);
}

#[test]
fn a_registry_that_is_not_https_is_refused_when_the_element_is_built() {
    let build = |registry: &str| {
        AgentSignatureElement::builder()
            .set_http_client(Arc::new(FakeClient::default()))
            .set_registry(registry)
            .build()
    };
    for registry in [
        "http://registry.example/cards",
        "https://user@registry.example/cards",
        "registry.example/cards",
    ] {
        assert!(build(registry).is_err(), "{registry}");
    }
    assert!(build("https://registry.example/cards").is_ok());
}

/// How long the holding client holds a request at most, so a failed test
/// does not leave its threads waiting for ever.
const HOLD_LIMIT: Duration = Duration::from_secs(60);

/// The wait budget for the test of the fetch limit.
const WAIT_BUDGET: Duration = Duration::from_millis(20);

/// How much longer than the wait budget a request may take on a busy
/// machine.
const SLACK: Duration = Duration::from_secs(2);

/// What the holding client has seen.
#[derive(Default)]
struct Held {
    open: bool,
    running: usize,
    most: usize,
    calls: usize,
}

/// A client that holds every request until it is opened, then serves the
/// test key's directory, and records the most requests it held at once.
#[derive(Default)]
struct HoldingClient {
    held: Mutex<Held>,
    opened: Condvar,
}

impl HoldingClient {
    fn open(&self) {
        self.held.lock().unwrap().open = true;
        self.opened.notify_all();
    }

    fn most(&self) -> usize {
        self.held.lock().unwrap().most
    }

    fn calls(&self) -> usize {
        self.held.lock().unwrap().calls
    }
}

impl HttpClient for HoldingClient {
    fn get(&self, request: &HttpRequest<'_>) -> Result<HttpResponse, String> {
        let mut held = self.held.lock().unwrap();
        held.calls += 1;
        held.running += 1;
        held.most = held.most.max(held.running);
        let deadline = Instant::now() + HOLD_LIMIT;
        while !held.open {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            held = self.opened.wait_timeout(held, left).unwrap().0;
        }
        held.running -= 1;
        drop(held);
        Ok(HttpResponse {
            status: 200,
            url: request.url.to_owned(),
            headers: vec![("Content-Type".to_owned(), DIRECTORY_MEDIA_TYPE.to_owned())],
            body: test_key_directory(None).into_bytes(),
        })
    }
}

#[test]
fn no_more_key_directory_fetches_run_at_once_than_the_limit() {
    let client = Arc::new(HoldingClient::default());
    let pipeline = pipeline_with(client.clone(), |builder| {
        builder.set_wait_budget(WAIT_BUDGET)
    });
    let beyond = 10;
    let agents = MAXIMUM_FETCHES_RUNNING + beyond;
    let signed_by = |index: usize| {
        Signer {
            agent: Some(format!("https://agent-{index}.test")),
            ..Signer::default()
        }
        .sign()
    };
    let mut reasons: HashMap<String, usize> = HashMap::new();
    for index in 0..agents {
        let signed = signed_by(index);
        let started = Instant::now();
        let reason = outcome(&process_signed(&pipeline, &signed, &[])).1;
        assert!(
            started.elapsed() < WAIT_BUDGET + SLACK,
            "agent {index} waited {:?}",
            started.elapsed()
        );
        *reasons.entry(reason).or_default() += 1;
    }
    // Every fetch is held, so the first ones started wait out the budget and
    // the rest are refused at once.
    assert_eq!(
        reasons.get(REASON_DIRECTORY_PENDING),
        Some(&MAXIMUM_FETCHES_RUNNING),
        "{reasons:?}"
    );
    assert_eq!(
        reasons.get(REASON_DIRECTORY_UNAVAILABLE),
        Some(&beyond),
        "{reasons:?}"
    );
    assert_eq!(client.calls(), MAXIMUM_FETCHES_RUNNING);
    assert!(client.most() <= MAXIMUM_FETCHES_RUNNING);

    // Once the fetches finish, an agent that was refused gets a fetch of its
    // own on a later request.
    client.open();
    let refused = signed_by(agents - 1);
    let deadline = Instant::now() + Duration::from_secs(30);
    while outcome(&process_signed(&pipeline, &refused, &[])).1 != REASON_VERIFIED {
        assert!(
            Instant::now() < deadline,
            "the refused agent was never fetched"
        );
        thread::sleep(WAIT_BUDGET);
    }
}
