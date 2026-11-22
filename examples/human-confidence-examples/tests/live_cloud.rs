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
//! are ignored without a Resource Key, and run with
//! `cargo test -p human-confidence-examples -- --ignored`.
//!
//! The cloud is the one `_51DEGREES_CLOUD_ENDPOINT` names, or the default
//! when it is unset.

use std::net::SocketAddr;

use human_confidence_examples::{api_from, app, has_human_confidence, Settings};

/// The Resource Key that carries every property, or the one the caller set.
fn resource_key() -> Option<String> {
    [
        examples_shared::CI_RESOURCE_KEY_BESPOKE_ENV_VAR,
        examples_shared::RESOURCE_KEY_ENV_VAR,
    ]
    .into_iter()
    .filter_map(|name| std::env::var(name).ok())
    .map(|value| value.trim().to_owned())
    .find(|value| !value.is_empty())
}

fn api() -> String {
    api_from(examples_shared::cloud_endpoint_from_env().as_deref())
}

#[tokio::test]
#[ignore = "requires a live cloud resource key (set _51DEGREES_RESOURCE_KEY_BESPOKE)"]
async fn the_key_carries_human_confidence() {
    let Some(resource_key) = resource_key() else {
        eprintln!("skipping: no resource key set");
        return;
    };
    let found = has_human_confidence(&api(), &resource_key)
        .await
        .expect("accessibleproperties answers");
    assert!(found, "the Resource Key does not include HumanConfidence");
}

#[tokio::test]
#[ignore = "requires a live cloud resource key (set _51DEGREES_RESOURCE_KEY_BESPOKE)"]
async fn the_passive_call_answers_a_source_and_a_level() {
    let Some(resource_key) = resource_key() else {
        eprintln!("skipping: no resource key set");
        return;
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = app(Settings {
        resource_key,
        license_key: None,
        api: api(),
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
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36",
        )
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(answer.get("error").is_none(), "the passive call failed");
    assert_eq!(answer["source"], "Passive");
    let level = answer["humanConfidence"].as_str().unwrap_or_default();
    assert!(
        ["High", "Medium", "Low"].contains(&level),
        "the level was {level:?}"
    );
}
