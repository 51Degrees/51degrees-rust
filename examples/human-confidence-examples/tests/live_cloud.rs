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

//! Tests for the Human Confidence web demo against the cloud service. They
//! are ignored, run with `cargo test -p human-confidence-examples --
//! --ignored`, and check nothing when their key or cloud is not set.
//!
//! The production cloud does not serve `HumanConfidenceSource` yet, and
//! answers a passive call with the level `Unknown`. So the tests of the
//! source and of the passive level ask the staging cloud that
//! `CLOUD_STAGING_ENDPOINT` names, with the Bespoke key in
//! `RESOURCE_KEY_BESPOKE_STAGING`, which the staging cloud resolves. Only CI
//! sets those two names. A key outside the Bespoke product, in
//! `_51DEGREES_RESOURCE_KEY_BASIC`, gets no source there. The tests of
//! `HumanConfidenceSource` name the properties in the request, so the answer
//! carries them whatever the key's own property list holds.
//!
//! The test that the key includes `HumanConfidence` asks the cloud
//! `_51DEGREES_CLOUD_ENDPOINT` names, or the default when it is unset, with
//! the key that carries every property, in `_51DEGREES_RESOURCE_KEY_BESPOKE`.

use std::net::SocketAddr;

use human_confidence_examples::{api_from, app, has_human_confidence, Settings};

/// The staging cloud's API base, a name only CI sets.
const STAGING_ENDPOINT: &str = "CLOUD_STAGING_ENDPOINT";

/// A Bespoke Resource Key the staging cloud resolves, a name only CI sets.
const STAGING_RESOURCE_KEY: &str = "RESOURCE_KEY_BESPOKE_STAGING";

/// The Resource Key that carries every property.
fn resource_key() -> Option<String> {
    examples_shared::live_test_resource_key()
}

fn api() -> String {
    api_from(examples_shared::cloud_endpoint_from_env().as_deref())
}

/// The value of `name`, trimmed, or `None` when it is unset or blank.
fn non_blank_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// The staging cloud's API base, ending in one slash, or `None` when
/// `CLOUD_STAGING_ENDPOINT` is unset. There is no default address.
fn staging_api() -> Option<String> {
    non_blank_env(STAGING_ENDPOINT).map(|value| api_from(Some(&value)))
}

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

/// The properties the direct passive call names, so the answer carries both
/// whatever the key's own property list holds.
const VALUES: &str = "derived.humanconfidence,derived.humanconfidencesource";

/// The passive call the demo makes, a browser's User-Agent posted to the
/// key's JSON path, naming the two properties. Returns the answer's derived
/// object. The key is taken out of any message.
async fn passive_naming_the_properties(api: &str, resource_key: &str) -> serde_json::Value {
    let redact = |text: String| text.replace(resource_key, "<key>");
    let response = reqwest::Client::new()
        .post(format!("{api}{resource_key}.json"))
        .form(&[("user-agent", USER_AGENT), ("values", VALUES)])
        .send()
        .await
        .unwrap_or_else(|e| panic!("the passive call failed: {}", redact(e.to_string())));
    let status = response.status();
    let body = redact(response.text().await.unwrap_or_default());
    assert_eq!(status, 200, "the passive call failed: {body}");
    let answer: serde_json::Value = serde_json::from_str(&body).expect("the answer is JSON");
    answer["derived"].clone()
}

#[tokio::test]
#[ignore = "requires the staging cloud (set CLOUD_STAGING_ENDPOINT and RESOURCE_KEY_BESPOKE_STAGING)"]
async fn the_bespoke_key_gets_the_source_passive() {
    let Some(api) = staging_api() else {
        eprintln!("skipping: {STAGING_ENDPOINT} is not set");
        return;
    };
    let Some(resource_key) = non_blank_env(STAGING_RESOURCE_KEY) else {
        eprintln!("skipping: {STAGING_RESOURCE_KEY} is not set");
        return;
    };
    let derived = passive_naming_the_properties(&api, &resource_key).await;
    assert_eq!(
        derived["humanconfidencesource"], "Passive",
        "a request with no browser tests is passive, got {derived}"
    );
    let level = derived["humanconfidence"].as_str().unwrap_or_default();
    assert!(
        ["High", "Medium", "Low"].contains(&level),
        "the level was {level:?}"
    );
}

#[tokio::test]
#[ignore = "requires the staging cloud (set CLOUD_STAGING_ENDPOINT and _51DEGREES_RESOURCE_KEY_BASIC)"]
async fn a_key_outside_bespoke_gets_no_source() {
    let Some(api) = staging_api() else {
        eprintln!("skipping: {STAGING_ENDPOINT} is not set");
        return;
    };
    let Some(resource_key) = examples_shared::live_test_basic_resource_key() else {
        eprintln!("skipping: _51DEGREES_RESOURCE_KEY_BASIC is not set");
        return;
    };
    let derived = passive_naming_the_properties(&api, &resource_key).await;
    assert!(
        derived["humanconfidencesource"].is_null(),
        "a key outside Bespoke gets no source, got {derived}"
    );
}

#[tokio::test]
#[ignore = "requires a live cloud resource key (set _51DEGREES_RESOURCE_KEY_BESPOKE)"]
async fn the_key_carries_human_confidence() {
    let Some(resource_key) = resource_key() else {
        eprintln!("skipping: _51DEGREES_RESOURCE_KEY_BESPOKE is not set");
        return;
    };
    let found = has_human_confidence(&api(), &resource_key)
        .await
        .expect("accessibleproperties answers");
    assert!(found, "the Resource Key does not include HumanConfidence");
}

#[tokio::test]
#[ignore = "requires the staging cloud (set CLOUD_STAGING_ENDPOINT and RESOURCE_KEY_BESPOKE_STAGING)"]
async fn the_demos_passive_call_answers_a_level() {
    let Some(api) = staging_api() else {
        eprintln!("skipping: {STAGING_ENDPOINT} is not set");
        return;
    };
    let Some(resource_key) = non_blank_env(STAGING_RESOURCE_KEY) else {
        eprintln!("skipping: {STAGING_RESOURCE_KEY} is not set");
        return;
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = app(Settings {
        resource_key,
        license_key: None,
        api,
        human_confidence: true,
    });
    tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });

    let answer: serde_json::Value = reqwest::Client::new()
        .get(format!("http://{address}/passive"))
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(answer.get("error").is_none(), "the passive call failed");
    // The demo names no properties, so the source is here only when the
    // key's own property list brings it. The tests above name it.
    let level = answer["humanConfidence"].as_str().unwrap_or_default();
    assert!(
        ["High", "Medium", "Low"].contains(&level),
        "the level was {level:?}"
    );
}
