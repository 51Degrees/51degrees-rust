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

//! Tests for the Human Confidence web demo, run against a fake cloud on a
//! local port. No real keys are used.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::extract::{Form, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use human_confidence_examples::{
    app, has_human_confidence, port_from, Settings, CONTACT_URL, DEFAULT_PORT, FIND_OUT_MORE,
};

const RESOURCE_KEY: &str = "resource-key-placeholder";
const LICENSE_KEY: &str = "license-key-placeholder";

/// The crate's README, whose links are held to the same tags as the code's.
const README: &str = include_str!("../README.md");

/// The tags every link in this crate starts with, up to the file it is in.
const TAGS: &str = "utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-human-confidence-examples-";

/// What the fake cloud was sent, and how it answers redeem.
#[derive(Default)]
struct Cloud {
    passive: Mutex<Option<HashMap<String, String>>>,
    redeem: Mutex<Option<HashMap<String, String>>>,
    redeem_status: u16,
    redeem_body: &'static str,
    has_property: bool,
}

async fn evidence_keys() -> Json<serde_json::Value> {
    Json(serde_json::json!([
        "header.user-agent",
        "header.sec-ch-ua",
        "query.51D_example",
        "cookie.51D_example",
        "server.client-ip"
    ]))
}

async fn accessible_properties(
    State(cloud): State<Arc<Cloud>>,
    Query(query): Query<HashMap<String, String>>,
) -> Json<serde_json::Value> {
    assert_eq!(
        query.get("resource").map(String::as_str),
        Some(RESOURCE_KEY)
    );
    let mut properties = vec![serde_json::json!({ "Name": "IsMobile" })];
    if cloud.has_property {
        properties.push(serde_json::json!({ "Name": "HumanConfidence" }));
    }
    Json(serde_json::json!({
        "Products": { "device": { "DataTier": "CloudV5Pro", "Properties": properties } }
    }))
}

async fn json(
    State(cloud): State<Arc<Cloud>>,
    Path(file): Path<String>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    assert_eq!(file, format!("{RESOURCE_KEY}.json"));
    *cloud.passive.lock().unwrap() = Some(form);
    Json(serde_json::json!({
        "device": { "ismobile": false },
        "derived": { "humanconfidencesource": "Passive", "humanconfidence": "Medium" }
    }))
    .into_response()
}

async fn redeem(
    State(cloud): State<Arc<Cloud>>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    *cloud.redeem.lock().unwrap() = Some(form);
    (
        StatusCode::from_u16(cloud.redeem_status).unwrap(),
        cloud.redeem_body,
    )
        .into_response()
}

async fn serve(router: Router) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    address
}

/// Start a fake cloud and return it with its API base.
async fn fake_cloud(cloud: Cloud) -> (Arc<Cloud>, String) {
    let cloud = Arc::new(cloud);
    let router = Router::new()
        .route("/api/v4/evidencekeys", get(evidence_keys))
        .route("/api/v4/accessibleproperties", get(accessible_properties))
        .route("/api/v4/hc/redeem", post(redeem))
        .route("/api/v4/{file}", post(json))
        .with_state(cloud.clone());
    let address = serve(router).await;
    (cloud, format!("http://{address}/api/v4/"))
}

/// Start a fake cloud and the demo in front of it. Returns the cloud and the
/// demo's base address.
async fn demo(cloud: Cloud) -> (Arc<Cloud>, String) {
    let (cloud, api) = fake_cloud(cloud).await;
    let human_confidence = has_human_confidence(&api, RESOURCE_KEY).await.unwrap();
    let address = serve(app(Settings {
        resource_key: RESOURCE_KEY.to_owned(),
        license_key: Some(LICENSE_KEY.to_owned()),
        api,
        human_confidence,
    }))
    .await;
    (cloud, format!("http://{address}/"))
}

fn with_property() -> Cloud {
    Cloud {
        has_property: true,
        redeem_status: 200,
        redeem_body: "{}",
        ..Cloud::default()
    }
}

#[test]
fn default_port_is_5151() {
    assert_eq!(DEFAULT_PORT, 5151);
    assert_eq!(port_from(None), 5151);
    assert_eq!(port_from(Some("not a number")), 5151);
    assert_eq!(port_from(Some("6000")), 6000);
}

#[tokio::test]
async fn page_is_filled_in_and_never_cached() {
    let (_, base) = demo(with_property()).await;
    let response = reqwest::get(&base).await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    let page = response.text().await.unwrap();
    assert!(page.contains(&format!("var RESOURCE = \"{RESOURCE_KEY}\"")));
    assert!(page.contains("var API = \"http://127.0.0.1:"));
    assert!(!page.contains("__RESOURCE__") && !page.contains("__API__"));
    assert!(!page.contains(&CONTACT_URL.replace('&', "&amp;")));
    let css = reqwest::get(format!("{base}examples-main.min.css"))
        .await
        .unwrap();
    assert_eq!(css.status(), 200);
}

#[tokio::test]
async fn page_ends_with_the_find_out_more_section() {
    let (_, base) = demo(with_property()).await;
    let page = reqwest::get(&base).await.unwrap().text().await.unwrap();
    let section = page
        .rfind(examples_shared::FIND_OUT_MORE_HEADING)
        .expect("the page has a Find out more section");
    assert!(section > page.find("</table>").unwrap());
    let after = &page[section..];
    assert_eq!(FIND_OUT_MORE.len(), 3);
    for (label, url) in FIND_OUT_MORE {
        let link = format!("<a href=\"{}\">{label}</a>", url.replace('&', "&amp;"));
        assert!(after.contains(&link), "no {label} link in {after}");
    }
    for repository in examples_shared::CLOUD_REPOSITORIES {
        assert!(after.contains(repository), "no {repository} in {after}");
    }
    assert!(
        after.replace("\r\n", "\n").trim_end().ends_with(
            "</body>
</html>"
        ),
        "got {after}"
    );
}

#[test]
fn links_carry_the_repository_tags() {
    for (label, url) in FIND_OUT_MORE {
        assert!(
            url.ends_with(&format!("?{TAGS}src-lib.rs&utm_term=find-out-more")),
            "{label} is tagged {url}"
        );
    }
    let links: Vec<&str> = README
        .split("](")
        .skip(1)
        .filter_map(|rest| rest.split(')').next())
        .filter(|url| url.contains("51degrees.com"))
        .collect();
    assert!(links.len() >= 4, "found {links:?}");
    for url in links {
        assert!(
            url.contains(&format!("?{TAGS}readme.md&utm_term=")),
            "the README link is tagged {url}"
        );
    }
}

#[tokio::test]
async fn passive_sends_only_the_clouds_headers_and_the_client_ip() {
    let (cloud, base) = demo(with_property()).await;
    let answer: serde_json::Value = reqwest::Client::new()
        .get(format!("{base}passive"))
        .header("User-Agent", "test-agent")
        .header("Sec-CH-UA", "\"Test\";v=\"1\"")
        .header("Authorization", "Bearer placeholder")
        .header("X-Other", "other")
        .header("client-ip", "203.0.113.9")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(answer["source"], "Passive");
    assert_eq!(answer["humanConfidence"], "Medium");

    let sent = cloud.passive.lock().unwrap().clone().unwrap();
    assert_eq!(
        sent.get("user-agent").map(String::as_str),
        Some("test-agent")
    );
    assert_eq!(
        sent.get("sec-ch-ua").map(String::as_str),
        Some("\"Test\";v=\"1\"")
    );
    assert_eq!(sent.get("client-ip").map(String::as_str), Some("127.0.0.1"));
    assert!(!sent.contains_key("authorization"));
    assert!(!sent.contains_key("x-other"));
    assert_eq!(sent.len(), 3, "unexpected evidence {sent:?}");
}

#[tokio::test]
async fn redeem_passes_the_clouds_answer_on_unchanged() {
    let body = r#"{"result":"replayed","extra":[1,2]}"#;
    let (cloud, base) = demo(Cloud {
        redeem_status: 409,
        redeem_body: body,
        ..with_property()
    })
    .await;
    let response = reqwest::Client::new()
        .post(format!("{base}redeem"))
        .form(&[("value", "sealed-value")])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 409);
    assert_eq!(response.text().await.unwrap(), body);

    let sent = cloud.redeem.lock().unwrap().clone().unwrap();
    assert_eq!(sent.get("value").map(String::as_str), Some("sealed-value"));
    assert_eq!(sent.get("license").map(String::as_str), Some(LICENSE_KEY));
}

#[tokio::test]
async fn redeem_404_says_the_service_does_not_redeem() {
    let (_, base) = demo(Cloud {
        redeem_status: 404,
        redeem_body: "not found",
        ..with_property()
    })
    .await;
    let answer: serde_json::Value = reqwest::Client::new()
        .post(format!("{base}redeem"))
        .form(&[("value", "sealed-value")])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let error = answer["error"].as_str().unwrap();
    assert!(
        error.contains("doesn't redeem Human Confidence"),
        "got {error}"
    );
}

#[tokio::test]
async fn a_key_without_the_property_links_to_contact_us() {
    let (_, base) = demo(Cloud {
        has_property: false,
        ..with_property()
    })
    .await;
    let page = reqwest::get(&base).await.unwrap().text().await.unwrap();
    assert!(page.contains(&format!(
        "<a href=\"{}\">Contact us</a> to add it.",
        CONTACT_URL.replace('&', "&amp;")
    )));
    assert!(page.contains("HumanConfidence"));
    assert_eq!(
        CONTACT_URL,
        format!("https://51degrees.com/contact-us?{TAGS}src-lib.rs&utm_term=human-confidence")
    );
}
