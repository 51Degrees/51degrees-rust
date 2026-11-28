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
    app, has_human_confidence, Settings, CONTACT_URL, DEFAULT_PORT, FIND_OUT_MORE,
};

const RESOURCE_KEY: &str = "resource-key-placeholder";
const LICENSE_KEY: &str = "license-key-placeholder";

/// The crate's README, whose links are held to the same tags as the code's.
const README: &str = include_str!("../README.md");

/// The tags every link in this crate starts with, up to the file it is in.
const TAGS: &str = "utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-human-confidence-examples-";

/// What the fake cloud was sent, and how it answers a json call that
/// carries a sealed value.
#[derive(Default)]
struct Cloud {
    passive: Mutex<Option<HashMap<String, String>>>,
    redeem: Mutex<Option<HashMap<String, String>>>,
    /// Set if anything called `hc/redeem`, which a package never does.
    hc_redeem: Mutex<bool>,
    /// The engine's calls that sent no sealed value.
    unredeemed: Mutex<usize>,
    redeem_status: u16,
    redeem_body: &'static str,
    has_property: bool,
    /// Answer the passive call as for a key without the properties.
    no_source: bool,
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

/// The accessible properties for a License Key alone, which the engine asks
/// for in the body of a POST.
async fn licensed_properties(Form(form): Form<HashMap<String, String>>) -> Json<serde_json::Value> {
    assert_eq!(form.get("license").map(String::as_str), Some(LICENSE_KEY));
    Json(serde_json::json!({
        "Products": { "derived": { "DataTier": "CloudV5Pro", "Properties": [
            { "Name": "HumanConfidence" }
        ] } }
    }))
}

async fn json(
    State(cloud): State<Arc<Cloud>>,
    Path(file): Path<String>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    // The cloud request engine's call, which redeems where it sends a
    // sealed value.
    if file == "json" {
        if !form.contains_key("humanconfidence") {
            *cloud.unredeemed.lock().unwrap() += 1;
            return Json(serde_json::json!({ "derived": {} })).into_response();
        }
        *cloud.redeem.lock().unwrap() = Some(form);
        return (
            StatusCode::from_u16(cloud.redeem_status).unwrap(),
            cloud.redeem_body,
        )
            .into_response();
    }
    assert_eq!(file, format!("{RESOURCE_KEY}.json"));
    *cloud.passive.lock().unwrap() = Some(form);
    if cloud.no_source {
        return Json(serde_json::json!({
            "derived": {
                "humanconfidence": null,
                "humanconfidencenullreason": "Not in this key.",
                "humanconfidencesource": null,
                "humanconfidencesourcenullreason": "Not in this key."
            }
        }))
        .into_response();
    }
    Json(serde_json::json!({
        "device": { "ismobile": false },
        "derived": { "humanconfidencesource": "Passive", "humanconfidence": "Medium" }
    }))
    .into_response()
}

async fn hc_redeem(State(cloud): State<Arc<Cloud>>) -> StatusCode {
    *cloud.hc_redeem.lock().unwrap() = true;
    StatusCode::OK
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
        .route(
            "/api/v4/accessibleproperties",
            get(accessible_properties).post(licensed_properties),
        )
        .route("/api/v4/hc/redeem", post(hc_redeem))
        .route("/api/v4/{file}", post(json))
        .with_state(cloud.clone());
    let address = serve(router).await;
    (cloud, format!("http://{address}/api/v4/"))
}

/// Start a fake cloud and the demo in front of it. Returns the cloud and the
/// demo's base address.
async fn demo(cloud: Cloud) -> (Arc<Cloud>, String) {
    demo_with(cloud, Some(LICENSE_KEY)).await
}

/// As [`demo`], with the License Key given or none.
async fn demo_with(cloud: Cloud, license_key: Option<&str>) -> (Arc<Cloud>, String) {
    let (cloud, api) = fake_cloud(cloud).await;
    let human_confidence = has_human_confidence(&api, RESOURCE_KEY).await.unwrap();
    let address = serve(app(Settings {
        resource_key: RESOURCE_KEY.to_owned(),
        license_key: license_key.map(str::to_owned),
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
}

#[tokio::test]
async fn page_is_filled_in_and_never_cached() {
    let (_, base) = demo(with_property()).await;
    let response = reqwest::get(&base).await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    let page = response.text().await.unwrap();
    let (script, page_id) = script_and_page(&page);
    assert!(
        script.starts_with("http://127.0.0.1:"),
        "the script comes from the cloud: {script}"
    );
    assert!(script.contains(&format!("/api/v4/{RESOURCE_KEY}.js?transaction=")));
    assert!(!page_id.is_empty());
    assert!(!page.contains("__SCRIPT__") && !page.contains("__PAGE__"));
    assert!(!page.contains(LICENSE_KEY));

    // Each load is a new page, with its own id and transaction.
    let again = reqwest::get(&base).await.unwrap().text().await.unwrap();
    let (other_script, other_page) = script_and_page(&again);
    assert_ne!(other_page, page_id);
    assert_ne!(other_script, script);
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
async fn passive_without_the_source_reports_none() {
    let (_, base) = demo(Cloud {
        no_source: true,
        ..with_property()
    })
    .await;
    let answer: serde_json::Value = reqwest::Client::new()
        .get(format!("{base}passive"))
        .header("User-Agent", "test-agent")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(answer.get("error").is_none(), "the passive call failed");
    assert!(answer["source"].is_null(), "no source is made up: {answer}");
    assert!(
        answer["humanConfidence"].is_null(),
        "no level is made up: {answer}"
    );
}

/// The client script's address and the page id the demo put in a page.
fn script_and_page(page: &str) -> (String, String) {
    let value = |name: &str| {
        let start = page
            .find(&format!("var {name} = \""))
            .unwrap_or_else(|| panic!("no {name} in the page"))
            + name.len()
            + 8;
        page[start..start + page[start..].find('"').unwrap()].to_owned()
    };
    (value("SCRIPT"), value("PAGE"))
}

/// Load the demo's page, as a browser does, and return its page id and the
/// transaction its script address carries.
async fn load_page(base: &str) -> (String, String) {
    let page = reqwest::get(base).await.unwrap().text().await.unwrap();
    let (script, page_id) = script_and_page(&page);
    let transaction = script.split("transaction=").nth(1).unwrap().to_owned();
    (page_id, transaction)
}

/// Send the demo a sealed value and a page id as the page does, and return
/// its answer.
async fn read_active(base: &str, sealed: &str, page_id: &str) -> serde_json::Value {
    reqwest::Client::new()
        .post(format!("{base}active"))
        .form(&[
            ("humanconfidence", sealed),
            ("humanconfidencepage", page_id),
        ])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

/// A json answer for a sealed value, as the cloud gives it.
fn sealed_answer(level: &str) -> &'static str {
    match level {
        "High" => r#"{"derived":{"humanconfidence":"High","humanconfidencesource":"Active"}}"#,
        _ => r#"{"derived":{"humanconfidence":"Medium","humanconfidencesource":"Active"}}"#,
    }
}

fn medium_and_active() -> serde_json::Value {
    serde_json::json!({ "source": "Active", "humanConfidence": "Medium" })
}

#[tokio::test]
async fn active_shows_the_two_properties_the_package_gives() {
    let (cloud, base) = demo(Cloud {
        redeem_body: sealed_answer("High"),
        ..with_property()
    })
    .await;
    let (page_id, transaction) = load_page(&base).await;
    let answer = read_active(&base, "sealed-value", &page_id).await;
    assert_eq!(
        answer,
        serde_json::json!({ "source": "Active", "humanConfidence": "High" })
    );

    // The package redeemed it through the json call, with the License Key
    // in the body and the transaction kept for this page, and nothing
    // called hc/redeem.
    let sent = cloud.redeem.lock().unwrap().clone().unwrap();
    assert_eq!(
        sent.get("humanconfidence").map(String::as_str),
        Some("sealed-value")
    );
    assert_eq!(sent.get("transaction"), Some(&transaction));
    assert_eq!(sent.get("license").map(String::as_str), Some(LICENSE_KEY));
    assert_eq!(
        sent.get("values").map(String::as_str),
        Some("derived.HumanConfidence")
    );
    assert!(!sent.contains_key("humanconfidencepage"));
    assert!(!sent.contains_key("sealedResult") && !sent.contains_key("value"));
    assert!(
        !*cloud.hc_redeem.lock().unwrap(),
        "the example never redeems"
    );
}

#[tokio::test]
async fn a_sealed_value_that_did_not_redeem_shows_medium_and_active() {
    for (status, body) in [
        (200, sealed_answer("Medium")),
        (200, r#"{"derived":{"humanconfidence":null}}"#),
        (500, r#"{"errors":["down"]}"#),
    ] {
        let (_, base) = demo(Cloud {
            redeem_status: status,
            redeem_body: body,
            ..with_property()
        })
        .await;
        let (page_id, _) = load_page(&base).await;
        assert_eq!(
            read_active(&base, "sealed-value", &page_id).await,
            medium_and_active(),
            "{status} {body}"
        );
    }
}

#[tokio::test]
async fn a_page_the_server_did_not_give_or_already_used_is_not_redeemed() {
    let (cloud, base) = demo(Cloud {
        redeem_body: sealed_answer("High"),
        ..with_property()
    })
    .await;
    assert_eq!(
        read_active(&base, "sealed-value", "made-up").await,
        medium_and_active()
    );
    assert!(
        cloud.redeem.lock().unwrap().is_none(),
        "nothing sealed sent"
    );

    let (page_id, _) = load_page(&base).await;
    read_active(&base, "sealed-value", &page_id).await;
    *cloud.redeem.lock().unwrap() = None;
    assert_eq!(
        read_active(&base, "sealed-value", &page_id).await,
        medium_and_active(),
        "a page's transaction is used once"
    );
    assert!(cloud.redeem.lock().unwrap().is_none());
    assert_eq!(*cloud.unredeemed.lock().unwrap(), 2);
}

#[tokio::test]
async fn without_a_license_key_nothing_is_redeemed_and_the_level_is_medium() {
    let (cloud, base) = demo_with(
        Cloud {
            redeem_body: sealed_answer("High"),
            ..with_property()
        },
        None,
    )
    .await;
    let (page_id, _) = load_page(&base).await;
    assert_eq!(
        read_active(&base, "sealed-value", &page_id).await,
        medium_and_active()
    );
    assert!(cloud.redeem.lock().unwrap().is_none());
}

#[tokio::test]
async fn active_without_a_sealed_value_says_so() {
    let (cloud, base) = demo(with_property()).await;
    let (page_id, _) = load_page(&base).await;
    let answer = read_active(&base, "", &page_id).await;
    assert_eq!(answer["error"], "The page sent no sealed value.");
    assert!(cloud.redeem.lock().unwrap().is_none());
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
