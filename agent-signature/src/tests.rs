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

//! Tests that need the crate's own parts: the worked examples of RFC 9421
//! Appendix B, and the behaviour that depends on the clock.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use chrono::{DateTime, Utc};
use fiftyone_pipeline_core::{Evidence, Pipeline};
use ring::signature::{Ed25519KeyPair, KeyPair as _};
use serde_json::Value;

use crate::constants::*;
use crate::http::{HttpClient, HttpRequest, HttpResponse};
use crate::keys::KeyDirectory;
use crate::signature_base::{build_signature_base, ComponentResolver};
use crate::signature_input::SignatureCandidate;
use crate::structured_field::{parse_dictionary, serialize_item, Item};
use crate::verify::{resolve_algorithm, verify};
use crate::AgentSignatureElement;

/// The worked examples of RFC 9421 Appendix B.2, with the keys of
/// Appendix B.1.
const APPENDIX_B: &str = include_str!("../tests/fixtures/rfc9421_appendix_b.json");

/// The private part of the Ed25519 test key of RFC 9421 Appendix B.1.4.
const ED25519_SEED: [u8; 32] = [
    0x9f, 0x83, 0x62, 0xf8, 0x7a, 0x48, 0x4a, 0x95, 0x4e, 0x6e, 0x74, 0x0c, 0x5b, 0x4c, 0x0e, 0x84,
    0x22, 0x91, 0x39, 0xa2, 0x0a, 0xa8, 0xab, 0x56, 0xff, 0x66, 0x58, 0x6f, 0x6a, 0x7d, 0x29, 0xc5,
];

/// The thumbprint of that key.
const ED25519_THUMBPRINT: &str = "poqkLGiymh_W0uP6PZFw-dvez3QJT5SolqXBCW38r0U";

/// The origin the test agent publishes its keys at.
const AGENT: &str = "https://signature-agent.test";

/// Where the test agent's key directory is fetched from.
const DIRECTORY_URL: &str =
    "https://signature-agent.test/.well-known/http-message-signatures-directory";

/// A time the test signatures are made at, 1 January 2025.
const SIGNED_AT: i64 = 1_735_689_600;

/// Resolves components from the values Appendix B prints for them.
struct PrintedValues(HashMap<String, String>);

impl ComponentResolver for PrintedValues {
    fn resolve(&self, _name: &str, component: &Item) -> Option<String> {
        self.0.get(&serialize_item(component)).cloned()
    }
}

fn first_candidate(input: &str, signature: &str) -> SignatureCandidate {
    let input = parse_dictionary(input).unwrap();
    let signature = parse_dictionary(signature).unwrap();
    SignatureCandidate::build_all(&input, &signature)
        .unwrap()
        .remove(0)
}

#[test]
fn every_appendix_b_signature_base_is_rebuilt_and_checks_out() {
    let examples: Vec<Value> = serde_json::from_str(APPENDIX_B).unwrap();
    assert_eq!(examples.len(), 6);
    for example in &examples {
        let section = example["section"].as_str().unwrap();
        let candidate = first_candidate(
            example["signature_input_header"].as_str().unwrap(),
            example["signature_header"].as_str().unwrap(),
        );
        assert_eq!(
            candidate.signature_params,
            example["signature_params"].as_str().unwrap(),
            "{section}"
        );
        let printed = PrintedValues(
            example["components"]
                .as_array()
                .unwrap()
                .iter()
                .map(|component| {
                    (
                        component["identifier"].as_str().unwrap().to_owned(),
                        component["value"].as_str().unwrap().to_owned(),
                    )
                })
                .collect(),
        );
        let base = build_signature_base(
            &candidate.covered_components,
            &candidate.signature_params,
            &printed,
        )
        .unwrap();
        assert_eq!(
            base,
            example["signature_base"].as_str().unwrap(),
            "{section}"
        );

        let directory =
            KeyDirectory::parse_text(&format!("{{\"keys\":[{}]}}", example["key"])).unwrap();
        let key = &directory.keys[0];
        let algorithm = example["algorithm"].as_str().unwrap();
        let supported = example["supported"].as_bool().unwrap();
        assert_eq!(
            verify(algorithm, key, base.as_bytes(), &candidate.signature),
            supported,
            "{section}"
        );
        if supported {
            let mut changed = candidate.signature.clone();
            changed[0] ^= 1;
            assert!(
                !verify(algorithm, key, base.as_bytes(), &changed),
                "{section}"
            );
            let changed_base = base.replace("example.com", "example.org");
            let changed_base = if changed_base == base {
                format!("{base} ")
            } else {
                changed_base
            };
            assert!(
                !verify(
                    algorithm,
                    key,
                    changed_base.as_bytes(),
                    &candidate.signature
                ),
                "{section}"
            );
        }
    }
}

#[test]
fn the_algorithm_is_settled_from_the_key_and_the_signature() {
    let examples: Vec<Value> = serde_json::from_str(APPENDIX_B).unwrap();
    let key_for = |section: &str| {
        let example = examples
            .iter()
            .find(|example| example["section"] == section)
            .unwrap();
        KeyDirectory::parse_text(&format!("{{\"keys\":[{}]}}", example["key"]))
            .unwrap()
            .keys
            .remove(0)
    };
    let ed25519 = key_for("B.2.6");
    let rsa = key_for("B.2.1");
    let ecdsa = key_for("B.2.4");
    let shared = key_for("B.2.5");
    let settle = |key: &crate::keys::JsonWebKey, alg: Option<&str>| {
        let resolution = resolve_algorithm(key, alg);
        (resolution.name, resolution.supported)
    };
    assert_eq!(settle(&ed25519, None), (ALGORITHM_ED25519.to_owned(), true));
    assert_eq!(
        settle(&ecdsa, None),
        (ALGORITHM_ECDSA_P256_SHA256.to_owned(), true)
    );
    // An RSA key does not say which RSA algorithm it is for.
    assert_eq!(settle(&rsa, None), ("RSA".to_owned(), false));
    assert_eq!(
        settle(&rsa, Some("RSA-PSS-SHA512")),
        (ALGORITHM_RSA_PSS_SHA512.to_owned(), true)
    );
    // The key and the signature disagree, so the signature's name is given.
    assert_eq!(
        settle(&ed25519, Some("rsa-pss-sha512")),
        (ALGORITHM_RSA_PSS_SHA512.to_owned(), false)
    );
    assert_eq!(
        settle(&shared, None),
        (ALGORITHM_HMAC_SHA256.to_owned(), false)
    );
}

/// A directory served by the test and the clock the element reads.
struct Harness {
    now: Arc<AtomicI64>,
    fetches: Arc<AtomicUsize>,
    available: Arc<Mutex<bool>>,
    pipeline: Arc<Pipeline>,
}

/// Serves the test key's directory while `available` is true, and fails
/// otherwise.
struct Directory {
    fetches: Arc<AtomicUsize>,
    available: Arc<Mutex<bool>>,
}

impl HttpClient for Directory {
    fn get(&self, request: &HttpRequest<'_>) -> Result<HttpResponse, String> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        if !*self.available.lock().unwrap() || request.url != DIRECTORY_URL {
            return Err("the directory is down".to_owned());
        }
        let public = Ed25519KeyPair::from_seed_unchecked(&ED25519_SEED).unwrap();
        let x = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public.public_key());
        Ok(HttpResponse {
            status: 200,
            url: Some(request.url.to_owned()),
            headers: vec![("Content-Type".to_owned(), DIRECTORY_MEDIA_TYPE.to_owned())],
            body: format!("{{\"keys\":[{{\"kty\":\"OKP\",\"crv\":\"Ed25519\",\"x\":\"{x}\"}}]}}")
                .into_bytes(),
        })
    }
}

impl Harness {
    fn new(
        configure: impl FnOnce(
            crate::AgentSignatureElementBuilder,
        ) -> crate::AgentSignatureElementBuilder,
    ) -> Self {
        let now = Arc::new(AtomicI64::new(SIGNED_AT + 100));
        let fetches = Arc::new(AtomicUsize::new(0));
        let available = Arc::new(Mutex::new(true));
        let clock = Arc::clone(&now);
        let builder = AgentSignatureElement::builder()
            .set_http_client(Arc::new(Directory {
                fetches: Arc::clone(&fetches),
                available: Arc::clone(&available),
            }))
            .set_wait_budget(Duration::from_secs(10))
            .set_clock(move || {
                DateTime::<Utc>::from_timestamp(clock.load(Ordering::SeqCst), 0).unwrap()
            });
        let element = configure(builder).build().unwrap();
        let pipeline = Pipeline::builder()
            .add_element(Arc::new(element))
            .build()
            .unwrap();
        Harness {
            now,
            fetches,
            available,
            pipeline,
        }
    }

    /// Sign a request covering the authority and the agent, made at
    /// `created` and valid for `lifetime` seconds, and process it.
    fn process(&self, created: i64, lifetime: i64) -> (String, String) {
        let params = format!(
            "(\"@authority\" \"signature-agent\";key=\"agent1\");created={created};\
             expires={};keyid=\"{ED25519_THUMBPRINT}\";tag=\"web-bot-auth\"",
            created + lifetime
        );
        let base = format!(
            "\"@authority\": example.com\n\"signature-agent\";key=\"agent1\": \"{AGENT}\"\n\
             \"@signature-params\": {params}"
        );
        let pair = Ed25519KeyPair::from_seed_unchecked(&ED25519_SEED).unwrap();
        let signature =
            base64::engine::general_purpose::STANDARD.encode(pair.sign(base.as_bytes()));
        let evidence = Evidence::builder()
            .add(EVIDENCE_SIGNATURE_KEY, format!("sig1=:{signature}:"))
            .add(EVIDENCE_SIGNATURE_INPUT_KEY, format!("sig1={params}"))
            .add(EVIDENCE_SIGNATURE_AGENT_KEY, format!("agent1=\"{AGENT}\""))
            .add(EVIDENCE_HOST_KEY, "example.com")
            .build();
        let mut data = self.pipeline.create_flow_data_with(evidence);
        data.process().unwrap();
        let result = data.get(AgentSignatureElement::KEY).unwrap();
        (
            result.agent_signature().value().unwrap().clone(),
            result.agent_signature_reason().value().unwrap().clone(),
        )
    }

    /// The reason for a request signed just now, valid for an hour.
    fn reason(&self) -> String {
        let now = self.now.load(Ordering::SeqCst);
        self.process(now - 10, 3600).1
    }

    fn advance(&self, seconds: i64) {
        self.now.fetch_add(seconds, Ordering::SeqCst);
    }

    fn set_available(&self, available: bool) {
        *self.available.lock().unwrap() = available;
    }

    fn fetches(&self) -> usize {
        self.fetches.load(Ordering::SeqCst)
    }
}

#[test]
fn a_signature_created_beyond_the_clock_skew_is_not_yet_valid() {
    let harness = Harness::new(|builder| builder);
    assert_eq!(
        harness.process(SIGNED_AT + 100 + 59, 3600).1,
        REASON_VERIFIED
    );
    assert_eq!(
        harness.process(SIGNED_AT + 100 + 61, 3600).1,
        REASON_NOT_YET_VALID
    );
}

#[test]
fn a_signature_past_its_expiry_and_the_skew_is_expired() {
    let harness = Harness::new(|builder| builder);
    let now = SIGNED_AT + 100;
    let created = SIGNED_AT - 1000;
    assert_eq!(
        harness.process(created, now - 59 - created).1,
        REASON_VERIFIED
    );
    assert_eq!(
        harness.process(created, now - 61 - created).1,
        REASON_EXPIRED
    );
}

#[test]
fn a_signature_valid_for_longer_than_the_maximum_lifetime_is_expired() {
    let harness = Harness::new(|builder| builder.set_max_lifetime(Duration::from_secs(600)));
    assert_eq!(harness.process(SIGNED_AT, 600).1, REASON_VERIFIED);
    assert_eq!(harness.process(SIGNED_AT, 601).1, REASON_EXPIRED);
}

#[test]
fn a_failed_fetch_is_remembered_for_the_negative_cache_lifetime() {
    let harness = Harness::new(|builder| builder);
    harness.set_available(false);
    assert_eq!(harness.reason(), REASON_DIRECTORY_UNAVAILABLE);
    assert_eq!(harness.fetches(), 1);
    harness.set_available(true);
    harness.advance(299);
    assert_eq!(harness.reason(), REASON_DIRECTORY_UNAVAILABLE);
    assert_eq!(harness.fetches(), 1);
    harness.advance(2);
    // The failure is now older than five minutes, so a fresh fetch starts
    // and this request waits for it.
    assert_eq!(harness.reason(), REASON_VERIFIED);
    assert_eq!(harness.fetches(), 2);
}

#[test]
fn keys_held_answer_whilst_refreshes_fail_for_one_further_lifetime() {
    let day = 24 * 60 * 60;
    let harness = Harness::new(|builder| builder);
    assert_eq!(harness.reason(), REASON_VERIFIED);
    assert_eq!(harness.fetches(), 1);
    harness.set_available(false);
    harness.advance(day + 1);
    // The directory is stale, so a refresh starts and the copy held answers.
    assert_eq!(harness.reason(), REASON_VERIFIED);
    wait_for_fetches(&harness, 2);
    harness.advance(day - 10);
    assert_eq!(harness.reason(), REASON_VERIFIED);
    harness.advance(20);
    // The keys were read more than two lifetimes ago and every refresh since
    // has failed, so they no longer answer.
    assert_eq!(harness.reason(), REASON_DIRECTORY_UNAVAILABLE);
}

/// Wait for the background fetches to reach `count`.
fn wait_for_fetches(harness: &Harness, count: usize) {
    for _ in 0..1000 {
        if harness.fetches() >= count {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("only {} fetches were made", harness.fetches());
}
