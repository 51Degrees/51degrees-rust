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

//! Tests of `CloudRequestEngine::redeem_human_confidence` against an
//! in-process client, so nothing reaches the network.

use std::future::Future;
use std::pin::pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use fiftyone_cloud_request_engine::{
    AsyncCloudHttpClient, Cache, CloudEngineState, CloudHttpClient, CloudHttpRequest,
    CloudHttpResponse, CloudRequestEngine, CloudRequestEngineBuilder, EvidenceKeyEntry,
    HumanConfidence, HumanConfidenceSource, LicensedProducts, LocalBoxFuture, LruCache,
};

const ENDPOINT: &str = "https://cloud.example.test/api/v4/";
const RESOURCE_KEY: &str = "test-resource-key";
const LICENSE_KEY: &str = "test-license-key";
const SEALED: &str = "c2VhbGVkLXJlc3VsdA";
const TRANSACTION: &str = "kept-transaction";

/// Drive `future` to its end on this thread.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
    }
}

/// A client that answers every request with one scripted answer and keeps
/// the requests it was sent.
struct Cloud {
    answer: Result<(u16, String), String>,
    requests: Mutex<Vec<CloudHttpRequest>>,
}

impl Cloud {
    fn answering(status: u16, body: &str) -> Arc<Self> {
        Arc::new(Self {
            answer: Ok((status, body.to_owned())),
            requests: Mutex::new(Vec::new()),
        })
    }

    fn failing() -> Arc<Self> {
        Arc::new(Self {
            answer: Err("timed out".to_owned()),
            requests: Mutex::new(Vec::new()),
        })
    }

    fn requests(&self) -> Vec<CloudHttpRequest> {
        self.requests.lock().unwrap().clone()
    }

    fn reply(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        self.requests.lock().unwrap().push(request.clone());
        self.answer.clone().map(|(status, body)| CloudHttpResponse {
            status,
            body,
            retry_after: None,
        })
    }
}

impl CloudHttpClient for Cloud {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        self.reply(request)
    }
}

/// The same client, awaited.
struct AsyncCloud(Arc<Cloud>);

impl AsyncCloudHttpClient for AsyncCloud {
    fn send<'a>(
        &'a self,
        request: &'a CloudHttpRequest,
    ) -> LocalBoxFuture<'a, Result<CloudHttpResponse, String>> {
        Box::pin(async move { self.0.reply(request) })
    }
}

/// A saved state, so the engine builds without discovery requests.
fn state() -> CloudEngineState {
    CloudEngineState {
        evidence_keys: vec![EvidenceKeyEntry {
            key: "header.user-agent".to_owned(),
            order: 0,
        }],
        accessible_properties: LicensedProducts::default(),
    }
}

fn builder(cloud: &Arc<Cloud>) -> CloudRequestEngineBuilder {
    CloudRequestEngine::builder()
        .resource_key(RESOURCE_KEY)
        .endpoint(ENDPOINT)
        .http_client(cloud.clone())
        .set_state(state())
}

fn engine(cloud: &Arc<Cloud>) -> CloudRequestEngine {
    builder(cloud)
        .license_key(LICENSE_KEY)
        .build()
        .expect("the engine builds")
}

fn field<'a>(request: &'a CloudHttpRequest, name: &str) -> Vec<&'a str> {
    request
        .form
        .iter()
        .filter(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
        .collect()
}

fn derived(value: &str) -> String {
    format!(r#"{{"derived":{{"humanconfidence":{value},"humanconfidencesource":"Active"}}}}"#)
}

#[test]
fn a_sealed_result_that_redeems_gives_the_value_and_active() {
    for (word, value) in [
        ("High", HumanConfidence::High),
        ("Medium", HumanConfidence::Medium),
        ("Low", HumanConfidence::Low),
    ] {
        let cloud = Cloud::answering(200, &derived(&format!("\"{word}\"")));
        let properties =
            block_on(engine(&cloud).redeem_human_confidence(SEALED, Some(TRANSACTION)))
                .expect("a sealed result gives the Properties");
        assert_eq!(properties.human_confidence(), value, "{word}");
        assert_eq!(properties.human_confidence().as_str(), word);
        assert_eq!(
            properties.human_confidence_source(),
            HumanConfidenceSource::Active
        );
        assert_eq!(properties.human_confidence_source().as_str(), "Active");
    }
}

#[test]
fn the_redemption_is_one_json_call_with_the_sealed_result_and_the_kept_transaction() {
    let cloud = Cloud::answering(200, &derived("\"High\""));
    let engine = builder(&cloud)
        .license_key(LICENSE_KEY)
        .values(["device.ismobile"])
        .build()
        .unwrap();
    block_on(engine.redeem_human_confidence(SEALED, Some(TRANSACTION))).unwrap();
    let requests = cloud.requests();
    assert_eq!(requests.len(), 1, "one request");
    let request = &requests[0];
    assert_eq!(request.url, format!("{ENDPOINT}json"));
    assert_eq!(field(request, "humanconfidence"), [SEALED]);
    assert_eq!(field(request, "transaction"), [TRANSACTION]);
    assert!(field(request, "challenge").is_empty());
    assert_eq!(field(request, "license"), [LICENSE_KEY]);
    assert_eq!(field(request, "resource"), [RESOURCE_KEY]);
    assert_eq!(
        field(request, "values"),
        ["device.ismobile,derived.HumanConfidence"]
    );
    assert_eq!(request.form.len(), 5, "nothing else is sent: {request:?}");
    assert!(!request.url.contains(LICENSE_KEY));
    assert!(!request.url.contains(SEALED));
}

#[test]
fn a_named_property_is_not_added_twice_and_no_transaction_sends_none() {
    let cloud = Cloud::answering(200, &derived("\"Low\""));
    let engine = builder(&cloud)
        .license_key(LICENSE_KEY)
        .values(["Derived.HumanConfidence"])
        .build()
        .unwrap();
    block_on(engine.redeem_human_confidence(SEALED, None)).unwrap();
    block_on(engine.redeem_human_confidence(SEALED, Some(""))).unwrap();
    for request in cloud.requests() {
        assert_eq!(field(&request, "values"), ["Derived.HumanConfidence"]);
        assert!(field(&request, "transaction").is_empty());
    }
}

#[test]
fn each_case_where_the_sealed_result_is_not_redeemed_gives_medium_and_active() {
    let cases: Vec<(&str, Arc<Cloud>)> = vec![
        ("the request fails", Cloud::failing()),
        ("a 500", Cloud::answering(500, r#"{"errors":["down"]}"#)),
        (
            "errors say the credential does not cover it",
            Cloud::answering(
                200,
                r#"{"errors":["HumanConfidence is not covered by this License Key."]}"#,
            ),
        ),
        (
            "no derived member",
            Cloud::answering(200, r#"{"device":{}}"#),
        ),
        (
            "no humanconfidence",
            Cloud::answering(200, r#"{"derived":{}}"#),
        ),
        ("a null value", Cloud::answering(200, &derived("null"))),
        ("Unknown", Cloud::answering(200, &derived("\"Unknown\""))),
        ("another word", Cloud::answering(200, &derived("\"high\""))),
        ("not a string", Cloud::answering(200, &derived("3"))),
        ("not JSON", Cloud::answering(200, "not json")),
    ];
    for (case, cloud) in cases {
        let properties = block_on(engine(&cloud).redeem_human_confidence(SEALED, None))
            .expect("a sealed result gives the Properties");
        assert_eq!(
            properties.human_confidence(),
            HumanConfidence::Medium,
            "{case}"
        );
        assert_eq!(
            properties.human_confidence_source(),
            HumanConfidenceSource::Active,
            "{case}"
        );
    }
}

#[test]
fn without_a_license_key_nothing_is_sent_and_the_value_is_medium() {
    let cloud = Cloud::answering(200, &derived("\"High\""));
    let engine = builder(&cloud).build().unwrap();
    let properties = block_on(engine.redeem_human_confidence(SEALED, None)).unwrap();
    assert_eq!(properties.human_confidence(), HumanConfidence::Medium);
    assert_eq!(
        properties.human_confidence_source(),
        HumanConfidenceSource::Active
    );
    assert!(cloud.requests().is_empty());
}

#[test]
fn in_a_recovery_period_nothing_is_sent_and_the_value_is_medium() {
    let cloud = Cloud::failing();
    let engine = builder(&cloud)
        .license_key(LICENSE_KEY)
        .failures_to_enter_recovery(1)
        .build()
        .unwrap();
    block_on(engine.redeem_human_confidence(SEALED, None)).unwrap();
    assert_eq!(cloud.requests().len(), 1);
    let properties = block_on(engine.redeem_human_confidence(SEALED, None)).unwrap();
    assert_eq!(properties.human_confidence(), HumanConfidence::Medium);
    assert_eq!(
        cloud.requests().len(),
        1,
        "the recovery period holds it back"
    );
}

#[test]
fn an_empty_sealed_result_is_no_sealed_result() {
    let cloud = Cloud::answering(200, &derived("\"High\""));
    assert_eq!(
        block_on(engine(&cloud).redeem_human_confidence("", Some(TRANSACTION))),
        None
    );
    assert!(cloud.requests().is_empty());
}

#[test]
fn a_redemption_is_neither_served_from_nor_stored_in_the_cache() {
    let cloud = Cloud::answering(200, &derived("\"High\""));
    let cache: Arc<LruCache<String, String>> = Arc::new(LruCache::new(10, 1));
    let engine = builder(&cloud)
        .license_key(LICENSE_KEY)
        .cache(cache.clone())
        .build()
        .unwrap();
    for _ in 0..2 {
        block_on(engine.redeem_human_confidence(SEALED, None)).unwrap();
    }
    assert_eq!(
        cloud.requests().len(),
        2,
        "each redemption goes to the cloud"
    );
    assert_eq!(cache.len(), 0, "nothing is stored");
}

#[test]
fn the_asynchronous_client_is_used_where_there_is_one() {
    let cloud = Cloud::answering(200, &derived("\"Low\""));
    let engine = CloudRequestEngine::builder()
        .resource_key(RESOURCE_KEY)
        .license_key(LICENSE_KEY)
        .endpoint(ENDPOINT)
        .async_http_client(Arc::new(AsyncCloud(cloud.clone())))
        .set_state(state())
        .build()
        .unwrap();
    let properties = block_on(engine.redeem_human_confidence(SEALED, None)).unwrap();
    assert_eq!(properties.human_confidence(), HumanConfidence::Low);
    assert_eq!(cloud.requests().len(), 1);
}
