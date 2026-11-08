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

//! The element end to end, through a pipeline, with the key directories
//! served from memory.

mod common;

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use fiftyone_agent_signature::constants::*;
use fiftyone_agent_signature::AgentSignatureElement;
use fiftyone_pipeline_core::{ElementData, FlowElement, PropertyValue};

use common::*;

/// The request line evidence of a GET for `/articles/rust` with no query.
const REQUEST_LINE: [(&str, &str); 3] = [
    ("server.request-method", "GET"),
    ("server.request-path", "/articles/rust"),
    ("server.request-query", ""),
];

/// The eight evidence keys the element accepts, as the specification lists
/// them.
const ACCEPTED_KEYS: [&str; 8] = [
    "header.signature",
    "header.signature-input",
    "header.signature-agent",
    "header.host",
    "header.protocol",
    "server.request-method",
    "server.request-path",
    "server.request-query",
];

/// A header none of the accepted keys names, with the value it is signed
/// with.
const X_TEST: (&str, &str) = ("header.x-test", "covered");

/// A signer that also covers the `x-test` header.
fn x_test_signer() -> Signer {
    Signer {
        extra: vec![("\"x-test\"".to_owned(), X_TEST.1.to_owned())],
        ..Signer::default()
    }
}

/// A signer that also covers the method and the path.
fn request_line_signer() -> Signer {
    Signer {
        nonce: Some("n0nce".to_owned()),
        extra: vec![
            ("\"@method\"".to_owned(), "GET".to_owned()),
            ("\"@path\"".to_owned(), "/articles/rust".to_owned()),
        ],
        ..Signer::default()
    }
}

#[test]
fn the_rfc_9421_ed25519_key_with_a_local_directory_is_verified() {
    let harness = Harness::new(
        FakeClient::default().with(
            DIRECTORY_URL,
            FakeResponse::directory(test_key_directory(Some("search"))),
        ),
        |builder| builder,
    );
    let data = harness.process_signed(&request_line_signer().sign(), &REQUEST_LINE);

    assert_eq!(
        outcome(&data),
        (STATUS_VERIFIED.to_owned(), REASON_VERIFIED.to_owned())
    );
    assert_eq!(
        text_or_reason(data.agent_signature_agent()),
        Ok(AGENT.to_owned())
    );
    assert_eq!(
        text_or_reason(data.agent_signature_key_id()),
        Ok(ED25519_THUMBPRINT.to_owned())
    );
    assert_eq!(
        text_or_reason(data.agent_signature_algorithm()),
        Ok(ALGORITHM_ED25519.to_owned())
    );
    assert_eq!(
        data.agent_signature_created().value().ok(),
        DateTime::<Utc>::from_timestamp(CREATED, 0).as_ref()
    );
    assert_eq!(
        data.agent_signature_expires().value().ok(),
        DateTime::<Utc>::from_timestamp(EXPIRES, 0).as_ref()
    );
    assert_eq!(
        text_or_reason(data.agent_signature_nonce()),
        Ok("n0nce".to_owned())
    );
    assert_eq!(
        text_or_reason(data.agent_signature_purpose()),
        Ok("search".to_owned())
    );
    for card in [
        data.agent_signature_name(),
        data.agent_signature_product_token(),
        data.agent_signature_card_url(),
    ] {
        assert_eq!(
            text_or_reason(card),
            Err("No agent card available".to_owned())
        );
    }
    // The times read as text through the property bag.
    assert_eq!(
        data.get("AgentSignatureCreated"),
        Ok(PropertyValue::String(
            "2025-01-01T00:00:00+00:00".to_owned()
        ))
    );
    assert_eq!(harness.client.calls(DIRECTORY_URL), 1);
}

#[test]
fn a_changed_path_is_a_signature_mismatch() {
    let harness = Harness::with_test_key();
    let signed = request_line_signer().sign();
    let changed = [
        ("server.request-method", "GET"),
        ("server.request-path", "/articles/other"),
        ("server.request-query", ""),
    ];
    let data = harness.process_signed(&signed, &changed);
    assert_eq!(
        outcome(&data),
        (
            STATUS_INVALID.to_owned(),
            REASON_SIGNATURE_MISMATCH.to_owned()
        )
    );
    // The directory was read, so the details are still reported.
    assert_eq!(
        text_or_reason(data.agent_signature_algorithm()),
        Ok(ALGORITHM_ED25519.to_owned())
    );
    assert_eq!(
        text_or_reason(data.agent_signature_purpose()),
        Err("The agent does not say what its keys are for".to_owned())
    );
    // The same signature over the path it was made for checks out.
    let data = harness.process_signed(&signed, &REQUEST_LINE);
    assert_eq!(outcome(&data).1, REASON_VERIFIED);
}

#[test]
fn a_directory_slower_than_the_wait_budget_is_timeout_and_is_found_later() {
    let slow_agent = "https://slow-agent.test";
    let slow_directory = "https://slow-agent.test/.well-known/http-message-signatures-directory";
    let mut response = FakeResponse::directory(test_key_directory(None));
    response.delay = Duration::from_millis(1500);
    let harness = Harness::new(
        FakeClient::default().with(slow_directory, response),
        |builder| builder.set_wait_budget(Duration::from_millis(100)),
    );
    let signed = Signer {
        agent: Some(slow_agent.to_owned()),
        ..Signer::default()
    }
    .sign();

    let started = Instant::now();
    let data = harness.process_signed(&signed, &[]);
    assert_eq!(
        outcome(&data),
        (
            STATUS_TIMEOUT.to_owned(),
            REASON_DIRECTORY_PENDING.to_owned()
        )
    );
    // The request waited for the budget, not for the fetch.
    assert!(started.elapsed() < Duration::from_millis(1400));
    assert_eq!(
        text_or_reason(data.agent_signature_purpose()),
        Err("The signature was not verified so the directory was not read".to_owned())
    );

    // The fetch carried on, so a later request from the agent finds the
    // keys, and no second fetch was started.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let data = harness.process_signed(&signed, &[]);
        if outcome(&data).0 == STATUS_VERIFIED {
            break;
        }
        assert_eq!(outcome(&data).1, REASON_DIRECTORY_PENDING);
        assert!(Instant::now() < deadline, "the fetch never finished");
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(harness.client.calls(slow_directory), 1);
}

#[test]
fn many_requests_at_once_for_one_agent_cause_one_fetch() {
    let mut response = FakeResponse::directory(test_key_directory(None));
    response.delay = Duration::from_millis(300);
    let harness = Arc::new(Harness::new(
        FakeClient::default().with(DIRECTORY_URL, response),
        |builder| builder.set_wait_budget(Duration::from_secs(20)),
    ));
    let signed = Signer::default().sign();
    let workers: Vec<_> = (0..20)
        .map(|_| {
            let harness = Arc::clone(&harness);
            let signed = signed.clone();
            thread::spawn(move || outcome(&harness.process_signed(&signed, &[])).1)
        })
        .collect();
    for worker in workers {
        assert_eq!(worker.join().unwrap(), REASON_VERIFIED);
    }
    assert_eq!(harness.client.calls(DIRECTORY_URL), 1);
}

#[test]
fn a_request_with_no_signature_says_why_each_detail_has_no_value() {
    let harness = Harness::with_test_key();
    let data = harness.process(&[("header.host", "example.com")]);
    assert_eq!(
        outcome(&data),
        (STATUS_ABSENT.to_owned(), REASON_NO_SIGNATURE.to_owned())
    );
    let absent = "No signature headers were present in the request";
    for name in [
        PROPERTY_AGENT,
        PROPERTY_KEY_ID,
        PROPERTY_ALGORITHM,
        PROPERTY_CREATED,
        PROPERTY_EXPIRES,
        PROPERTY_NONCE,
    ] {
        assert_eq!(data.get(name).unwrap_err().message, absent, "{name}");
    }
    assert_eq!(
        data.get(PROPERTY_PURPOSE).unwrap_err().message,
        "The signature was not verified so the directory was not read"
    );
    for name in [PROPERTY_NAME, PROPERTY_PRODUCT_TOKEN, PROPERTY_CARD_URL] {
        assert_eq!(
            data.get(name).unwrap_err().message,
            "No agent card available",
            "{name}"
        );
    }
    assert_eq!(harness.client.total(), 0);
}

#[test]
fn one_signature_header_without_the_other_is_malformed() {
    let harness = Harness::with_test_key();
    let signed = Signer::default().sign();
    let data = harness.process(&[("header.signature", signed.signature.as_str())]);
    assert_eq!(
        outcome(&data),
        (STATUS_INVALID.to_owned(), REASON_MALFORMED.to_owned())
    );
    assert_eq!(
        text_or_reason(data.agent_signature_key_id()),
        Err("The signature headers could not be read".to_owned())
    );
    let data = harness.process(&[
        ("header.signature", signed.signature.as_str()),
        ("header.signature-input", "sig1=(\"@authority\""),
    ]);
    assert_eq!(outcome(&data).1, REASON_MALFORMED);
}

#[test]
fn another_tag_is_a_tag_mismatch() {
    let harness = Harness::with_test_key();
    let signed = Signer {
        tag: Some("other".to_owned()),
        ..Signer::default()
    }
    .sign();
    let data = harness.process_signed(&signed, &[]);
    assert_eq!(
        outcome(&data),
        (STATUS_INVALID.to_owned(), REASON_TAG_MISMATCH.to_owned())
    );
    // The agent header was read, so the agent is reported.
    assert_eq!(
        text_or_reason(data.agent_signature_agent()),
        Ok(AGENT.to_owned())
    );
    assert_eq!(
        text_or_reason(data.agent_signature_key_id()),
        Err("No signature in the request was made for automated agent traffic".to_owned())
    );
    assert_eq!(harness.client.total(), 0);
}

#[test]
fn a_missing_key_id_is_a_missing_parameter() {
    let harness = Harness::with_test_key();
    let signed = Signer {
        key_id: None,
        ..Signer::default()
    }
    .sign();
    assert_eq!(
        outcome(&harness.process_signed(&signed, &[])),
        (
            STATUS_INVALID.to_owned(),
            REASON_MISSING_PARAMETER.to_owned()
        )
    );
}

#[test]
fn a_time_outside_the_years_one_to_9999_is_malformed() {
    let harness = Harness::with_test_key();
    let signed = Signer {
        created: Some(253_402_300_800),
        ..Signer::default()
    }
    .sign();
    assert_eq!(
        outcome(&harness.process_signed(&signed, &[])),
        (STATUS_INVALID.to_owned(), REASON_MALFORMED.to_owned())
    );
}

#[test]
fn an_agent_the_signature_does_not_cover_is_no_agent() {
    let harness = Harness::with_test_key();
    let signed = Signer {
        agent: None,
        ..Signer::default()
    }
    .sign();
    // The header is sent, but the signature does not cover it, and anyone
    // can add a header to a request.
    let data = harness.process_signed(
        &signed,
        &[(
            "header.signature-agent",
            "agent1=\"https://signature-agent.test\"",
        )],
    );
    assert_eq!(
        outcome(&data),
        (STATUS_UNVERIFIED.to_owned(), REASON_NO_AGENT.to_owned())
    );
    assert_eq!(harness.client.total(), 0);
}

#[test]
fn the_whole_header_covered_names_its_only_agent() {
    let harness = Harness::with_test_key();
    // The older bare string form, covered whole.
    let legacy = Signer {
        agent_label: None,
        ..Signer::default()
    }
    .sign();
    assert_eq!(
        outcome(&harness.process_signed(&legacy, &[])).1,
        REASON_VERIFIED
    );
    // With two members there is no telling which one signed.
    let two = Signed {
        signature_agent: Some(format!("a=\"{AGENT}\", b=\"https://other.test\"")),
        ..legacy.clone()
    };
    assert_eq!(
        outcome(&harness.process_signed(&two, &[])).1,
        REASON_NO_AGENT
    );
    // The older form is refused where it is turned off.
    let strict = Harness::new(FakeClient::default(), |builder| {
        builder.set_allow_legacy_signature_agent(false)
    });
    assert_eq!(
        outcome(&strict.process_signed(&legacy, &[])).1,
        REASON_MALFORMED
    );
}

#[test]
fn a_signature_covering_neither_authority_nor_target_uri_is_unbound() {
    let harness = Harness::with_test_key();
    let signed = Signer {
        cover_authority: false,
        ..Signer::default()
    }
    .sign();
    assert_eq!(
        outcome(&harness.process_signed(&signed, &[])),
        (
            STATUS_INVALID.to_owned(),
            REASON_UNBOUND_SIGNATURE.to_owned()
        )
    );
    assert_eq!(harness.client.total(), 0);
}

#[test]
fn request_line_components_without_the_evidence_are_unavailable() {
    let harness = Harness::with_test_key();
    let data = harness.process_signed(&request_line_signer().sign(), &[]);
    assert_eq!(
        outcome(&data),
        (
            STATUS_UNVERIFIED.to_owned(),
            REASON_COMPONENT_UNAVAILABLE.to_owned()
        )
    );
}

#[test]
fn a_signature_covering_the_target_uri_and_query_verifies() {
    let harness = Harness::with_test_key();
    let signed = Signer {
        cover_authority: false,
        extra: vec![
            (
                "\"@target-uri\"".to_owned(),
                "https://example.com/search?q=a%20b&x".to_owned(),
            ),
            ("\"@query\"".to_owned(), "?q=a%20b&x".to_owned()),
            ("\"@scheme\"".to_owned(), "https".to_owned()),
        ],
        ..Signer::default()
    }
    .sign();
    let line = [
        ("server.request-method", "GET"),
        ("server.request-path", "/search"),
        ("server.request-query", "q=a%20b&x"),
    ];
    assert_eq!(
        outcome(&harness.process_signed(&signed, &line)).1,
        REASON_VERIFIED
    );
    let changed = [
        ("server.request-method", "GET"),
        ("server.request-path", "/search"),
        ("server.request-query", "q=a+b&x"),
    ];
    assert_eq!(
        outcome(&harness.process_signed(&signed, &changed)).1,
        REASON_SIGNATURE_MISMATCH
    );
}

#[test]
fn an_inline_key_set_is_refused_unless_allowed() {
    let inline = format!(
        "data:application/http-message-signatures-directory+json;base64,{}",
        base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            test_key_directory(None)
        )
    );
    let signed = Signer {
        agent: Some(inline),
        ..Signer::default()
    }
    .sign();
    let refused = Harness::new(FakeClient::default(), |builder| builder);
    let data = refused.process_signed(&signed, &[]);
    assert_eq!(
        outcome(&data),
        (
            STATUS_UNVERIFIED.to_owned(),
            REASON_INLINE_DIRECTORY.to_owned()
        )
    );
    assert_eq!(
        text_or_reason(data.agent_signature_nonce()),
        Err(
            "The agent sent its key with the request rather than publishing it, which \
             this element does not accept."
                .to_owned()
        )
    );
    let allowed = Harness::new(FakeClient::default(), |builder| {
        builder.set_allow_inline_directory(true)
    });
    assert_eq!(
        outcome(&allowed.process_signed(&signed, &[])).1,
        REASON_VERIFIED
    );
    assert_eq!(allowed.client.total(), 0);
}

#[test]
fn a_directory_without_the_named_key_is_an_unknown_key() {
    let harness = Harness::with_test_key();
    let signed = Signer {
        key_id: Some("withdrawn".to_owned()),
        ..Signer::default()
    }
    .sign();
    assert_eq!(
        outcome(&harness.process_signed(&signed, &[])),
        (STATUS_INVALID.to_owned(), REASON_UNKNOWN_KEY.to_owned())
    );
}

#[test]
fn a_key_that_expired_before_the_signature_was_made_is_key_expired() {
    let directory = format!(
        "{{\"keys\":[{{\"kty\":\"OKP\",\"crv\":\"Ed25519\",\"x\":\"{ED25519_X}\",\
         \"exp\":{}}}]}}",
        CREATED - 1
    );
    let harness = Harness::new(
        FakeClient::default().with(DIRECTORY_URL, FakeResponse::directory(directory)),
        |builder| builder,
    );
    assert_eq!(
        outcome(&harness.process_signed(&Signer::default().sign(), &[])),
        (STATUS_INVALID.to_owned(), REASON_KEY_EXPIRED.to_owned())
    );
}

#[test]
fn a_shared_secret_key_is_an_unsupported_algorithm() {
    let directory = format!(
        "{{\"keys\":[{{\"kty\":\"oct\",\"kid\":\"{ED25519_THUMBPRINT}\",\"k\":\"c2VjcmV0\"}}]}}"
    );
    let harness = Harness::new(
        FakeClient::default().with(DIRECTORY_URL, FakeResponse::directory(directory)),
        |builder| builder,
    );
    let signed = Signer {
        algorithm: None,
        ..Signer::default()
    }
    .sign();
    let data = harness.process_signed(&signed, &[]);
    assert_eq!(
        outcome(&data),
        (
            STATUS_UNVERIFIED.to_owned(),
            REASON_UNSUPPORTED_ALGORITHM.to_owned()
        )
    );
    assert_eq!(
        text_or_reason(data.agent_signature_algorithm()),
        Ok(ALGORITHM_HMAC_SHA256.to_owned())
    );
}

#[test]
fn a_directory_served_as_plain_json_is_unavailable() {
    let mut response = FakeResponse::directory(test_key_directory(None));
    response.content_type = Some("application/json".to_owned());
    let harness = Harness::new(
        FakeClient::default().with(DIRECTORY_URL, response),
        |builder| builder,
    );
    assert_eq!(
        outcome(&harness.process_signed(&Signer::default().sign(), &[])),
        (
            STATUS_UNVERIFIED.to_owned(),
            REASON_DIRECTORY_UNAVAILABLE.to_owned()
        )
    );
}

#[test]
fn a_directory_longer_than_the_limit_is_unavailable() {
    let harness = Harness::new(
        FakeClient::default().with(
            DIRECTORY_URL,
            FakeResponse::directory(test_key_directory(None)),
        ),
        |builder| builder.set_max_response_bytes(50),
    );
    assert_eq!(
        outcome(&harness.process_signed(&Signer::default().sign(), &[])).1,
        REASON_DIRECTORY_UNAVAILABLE
    );
}

#[test]
fn an_agent_card_gives_the_keys_and_the_card_properties() {
    let card_url = "https://signature-agent.test/card";
    let card = format!(
        "{{\"client_id\":\"{card_url}\",\"client_name\":\"Test Agent\",\
         \"jwks\":{},\"web_bot_auth\":{{\"rfc9309-product-token\":\"TestBot\",\
         \"purpose\":\"research\"}}}}",
        test_key_directory(None)
    );
    let mut response = FakeResponse::directory(card);
    response.content_type = Some("application/json".to_owned());
    let harness = Harness::new(FakeClient::default().with(card_url, response), |builder| {
        builder
    });
    let signed = Signer {
        agent: Some(card_url.to_owned()),
        agent_parameters: ";type=cimd".to_owned(),
        ..Signer::default()
    }
    .sign();
    let data = harness.process_signed(&signed, &[]);
    assert_eq!(outcome(&data).1, REASON_VERIFIED);
    assert_eq!(
        text_or_reason(data.agent_signature_name()),
        Ok("Test Agent".to_owned())
    );
    assert_eq!(
        text_or_reason(data.agent_signature_product_token()),
        Ok("TestBot".to_owned())
    );
    assert_eq!(
        text_or_reason(data.agent_signature_card_url()),
        Ok(card_url.to_owned())
    );
    assert_eq!(
        text_or_reason(data.agent_signature_purpose()),
        Ok("research".to_owned())
    );
}

#[test]
fn a_query_string_cannot_stand_in_for_the_request() {
    let harness = Harness::with_test_key();
    let signed = Signer::default().sign();
    let agent = signed.signature_agent.clone().unwrap();
    let evidence = [
        ("query.signature", signed.signature.as_str()),
        ("query.signature-input", signed.signature_input.as_str()),
        ("query.signature-agent", agent.as_str()),
        ("query.host", "example.com"),
        ("header.host", "example.com"),
    ];
    assert_eq!(
        outcome(&harness.process(&evidence)),
        (STATUS_ABSENT.to_owned(), REASON_NO_SIGNATURE.to_owned())
    );

    let trusted = Harness::new(
        FakeClient::default().with(
            DIRECTORY_URL,
            FakeResponse::directory(test_key_directory(None)),
        ),
        |builder| builder.set_trust_forwarded_evidence(true),
    );
    assert_eq!(outcome(&trusted.process(&evidence)).1, REASON_VERIFIED);
}

#[test]
fn the_element_accepts_its_eight_keys_and_no_others() {
    let element = AgentSignatureElement::builder()
        .set_http_client(Arc::new(FakeClient::default()))
        .build()
        .unwrap();
    let filter = element.evidence_key_filter();
    for key in ACCEPTED_KEYS {
        assert!(filter.include(key), "{key}");
        assert!(filter.include(&key.to_uppercase()), "{key}");
    }
    for key in [
        "header.x-test",
        "header.content-type",
        "header.user-agent",
        "server.http-request",
        "server.client-ip",
        "query.signature",
        "cookie.signature",
    ] {
        assert!(!filter.include(key), "{key}");
    }
    let mut named = filter.named_keys().unwrap();
    named.sort();
    let mut expected: Vec<String> = ACCEPTED_KEYS.iter().map(|key| key.to_string()).collect();
    expected.sort();
    assert_eq!(named, expected);
    let names: Vec<&str> = element
        .properties()
        .iter()
        .map(|property| property.name.as_str())
        .collect();
    assert_eq!(names.len(), 12);
    assert_eq!(names[0], PROPERTY_STATUS);
    assert_eq!(element.data_key(), DEFAULT_ELEMENT_DATA_KEY);
}

#[test]
fn trusting_forwarded_evidence_adds_the_query_keys_and_no_headers() {
    let element = AgentSignatureElement::builder()
        .set_http_client(Arc::new(FakeClient::default()))
        .set_trust_forwarded_evidence(true)
        .build()
        .unwrap();
    let filter = element.evidence_key_filter();
    for key in ACCEPTED_KEYS {
        assert!(filter.include(key), "{key}");
    }
    assert!(filter.include("query.signature"));
    assert!(filter.include("query.x-test"));
    assert!(!filter.include(X_TEST.0));
}

#[test]
fn the_pipeline_does_not_accept_a_header_only_a_signature_covers() {
    let harness = Harness::with_test_key();
    let filter = harness.pipeline.evidence_key_filter();
    assert!(!filter.include(X_TEST.0));
    assert!(!filter.include("server.http-request"));
    for key in ACCEPTED_KEYS {
        assert!(filter.include(key), "{key}");
    }
}

#[test]
fn a_covered_header_in_the_evidence_verifies() {
    let harness = Harness::with_test_key();
    let data = harness.process_signed(&x_test_signer().sign(), &[X_TEST]);
    assert_eq!(
        outcome(&data),
        (STATUS_VERIFIED.to_owned(), REASON_VERIFIED.to_owned())
    );
}

#[test]
fn a_covered_header_only_in_the_web_request_verifies() {
    let harness = Harness::with_test_key();
    let data =
        process_signed_with_request(&harness.pipeline, &x_test_signer().sign(), &[], &[X_TEST]);
    assert_eq!(
        outcome(&data),
        (STATUS_VERIFIED.to_owned(), REASON_VERIFIED.to_owned())
    );
}

#[test]
fn a_covered_header_in_neither_place_is_unavailable() {
    let harness = Harness::with_test_key();
    let signed = x_test_signer().sign();
    let unavailable = (
        STATUS_UNVERIFIED.to_owned(),
        REASON_COMPONENT_UNAVAILABLE.to_owned(),
    );
    assert_eq!(outcome(&harness.process_signed(&signed, &[])), unavailable);
    assert_eq!(
        outcome(&process_signed_with_request(
            &harness.pipeline,
            &signed,
            &[],
            &[("header.x-other", "covered")],
        )),
        unavailable
    );
    // The signature base is rebuilt before the key directory is looked up.
    assert_eq!(harness.client.total(), 0);
}

#[test]
fn a_covered_header_takes_the_evidence_value_before_the_web_request() {
    let harness = Harness::with_test_key();
    let signed = x_test_signer().sign();
    let other = (X_TEST.0, "other");
    let data = process_signed_with_request(&harness.pipeline, &signed, &[X_TEST], &[other]);
    assert_eq!(outcome(&data).1, REASON_VERIFIED);
    let data = process_signed_with_request(&harness.pipeline, &signed, &[other], &[X_TEST]);
    assert_eq!(
        outcome(&data),
        (
            STATUS_INVALID.to_owned(),
            REASON_SIGNATURE_MISMATCH.to_owned()
        )
    );
}

#[test]
fn an_accepted_key_is_not_read_from_the_web_request() {
    let harness = Harness::with_test_key();
    let signed = Signer::default().sign();
    let agent = signed.signature_agent.clone().unwrap();
    let mut evidence = vec![
        ("header.signature", signed.signature.as_str()),
        ("header.signature-input", signed.signature_input.as_str()),
        ("header.signature-agent", agent.as_str()),
        ("header.protocol", "https"),
    ];
    let mut request = evidence.clone();
    request.push(("header.host", "example.com"));
    let data = process_with_request(&harness.pipeline, &evidence, &request);
    assert_eq!(
        outcome(&data),
        (
            STATUS_UNVERIFIED.to_owned(),
            REASON_COMPONENT_UNAVAILABLE.to_owned()
        )
    );
    evidence.push(("header.host", "example.com"));
    let data = process_with_request(&harness.pipeline, &evidence, &request);
    assert_eq!(outcome(&data).1, REASON_VERIFIED);
}

#[test]
fn a_period_longer_than_a_year_is_refused_when_the_element_is_built() {
    let client = Arc::new(FakeClient::default());
    let too_long = Duration::from_secs(366 * 24 * 60 * 60);
    assert!(AgentSignatureElement::builder()
        .set_http_client(client.clone())
        .set_wait_budget(too_long)
        .build()
        .is_err());
    assert!(AgentSignatureElement::builder()
        .set_http_client(client.clone())
        .set_cache_size(0)
        .build()
        .is_err());
    assert!(AgentSignatureElement::builder()
        .set_http_client(client)
        .set_cache_lifetime(MAXIMUM_PERIOD)
        .build()
        .is_ok());
}
