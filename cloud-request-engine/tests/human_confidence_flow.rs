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

//! The sealed Human Confidence result in a flow data's json call, as the
//! specification's required tests list them, against an in-process client.

use std::sync::{Arc, Mutex};

use std::future::Future;

use fiftyone_cloud_request_engine::{
    human_confidence, AsyncCloudHttpClient, Cache, CloudEngineState, CloudHttpClient,
    CloudHttpRequest, CloudHttpResponse, CloudRequestEngine, CloudRequestEngineBuilder,
    EvidenceKeyEntry, HumanConfidence, HumanConfidenceSource, HumanConfidenceTransactions,
    LicensedProducts, LocalBoxFuture, LruCache, PAGE_EVIDENCE, SEALED_RESULT_EVIDENCE,
};
use fiftyone_pipeline_core::{
    Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, Pipeline,
    PropertyMetaData,
};

const ENDPOINT: &str = "https://cloud.example.test/api/v4/";
const RESOURCE_KEY: &str = "test-resource-key";
const LICENSE_KEY: &str = "LK-0123456789-WXYZ";
const SEALED: &str = "c2VhbGVkLXJlc3VsdC1mb3ItdGhpcy1wYWdl";

/// A client that answers the json call with a scripted answer and keeps
/// what it was sent.
struct Cloud {
    answer: Mutex<Result<(u16, String), String>>,
    requests: Mutex<Vec<CloudHttpRequest>>,
}

impl Cloud {
    fn answering(status: u16, body: &str) -> Arc<Self> {
        Arc::new(Self {
            answer: Mutex::new(Ok((status, body.to_owned()))),
            requests: Mutex::new(Vec::new()),
        })
    }

    fn failing() -> Arc<Self> {
        Arc::new(Self {
            answer: Mutex::new(Err("timed out".to_owned())),
            requests: Mutex::new(Vec::new()),
        })
    }

    fn requests(&self) -> Vec<CloudHttpRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl CloudHttpClient for Cloud {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        self.requests.lock().unwrap().push(request.clone());
        self.answer
            .lock()
            .unwrap()
            .clone()
            .map(|(status, body)| CloudHttpResponse {
                status,
                body,
                retry_after: None,
            })
    }
}

/// An element that is not a cloud request engine.
struct Other(EvidenceKeyFilterWhitelist);

impl FlowElement for Other {
    fn process(&self, _: &mut FlowData) -> fiftyone_pipeline_core::Result<()> {
        Ok(())
    }

    fn data_key(&self) -> &str {
        "other"
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.0
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

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

fn licensed(cloud: &Arc<Cloud>) -> Arc<CloudRequestEngine> {
    Arc::new(builder(cloud).license_key(LICENSE_KEY).build().unwrap())
}

fn pipeline(engine: &Arc<CloudRequestEngine>) -> Arc<Pipeline> {
    Pipeline::builder()
        .add_element(engine.clone())
        .suppress_process_exceptions(true)
        .build()
        .unwrap()
}

/// Process a flow data with this evidence through the engine.
fn process(engine: &Arc<CloudRequestEngine>, evidence: &[(&str, &str)]) -> FlowData {
    let mut builder = Evidence::builder();
    for (key, value) in evidence {
        builder = builder.add(*key, *value);
    }
    let mut data = pipeline(engine).create_flow_data_with(builder.build());
    let _ = data.process();
    data
}

fn field<'a>(request: &'a CloudHttpRequest, name: &str) -> Vec<&'a str> {
    request
        .form
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
        .collect()
}

fn derived(value: &str) -> String {
    format!(
        r#"{{"derived":{{"humanconfidence":{value},"humanconfidencesource":"Active"}},"device":{{"ismobile":true}}}}"#
    )
}

/// A page the engine issued: its id, and the transaction its script URL
/// carries to the remote server.
fn page(engine: &CloudRequestEngine) -> (String, String) {
    let page = engine.human_confidence_page().unwrap();
    let url = page.script_url(ENDPOINT, RESOURCE_KEY);
    let transaction = url.split("transaction=").nth(1).unwrap().to_owned();
    (page.id().to_owned(), transaction)
}

/// Process the page's sealed result, with any other evidence given.
fn redeem(engine: &Arc<CloudRequestEngine>, page_id: &str, other: &[(&str, &str)]) -> FlowData {
    let mut evidence = vec![(SEALED_RESULT_EVIDENCE, SEALED), (PAGE_EVIDENCE, page_id)];
    evidence.extend_from_slice(other);
    process(engine, &evidence)
}

fn assert_medium_and_active(data: &FlowData, case: &str) {
    let properties = human_confidence(data).expect("the request carries a sealed result");
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

#[test]
fn a_sealed_result_that_redeems_gives_the_value_and_active() {
    for (word, value) in [
        ("High", HumanConfidence::High),
        ("Medium", HumanConfidence::Medium),
        ("Low", HumanConfidence::Low),
    ] {
        let cloud = Cloud::answering(200, &derived(&format!("\"{word}\"")));
        let engine = licensed(&cloud);
        let (id, transaction) = page(&engine);
        let data = redeem(&engine, &id, &[]);
        let properties = human_confidence(&data).unwrap();
        assert_eq!(properties.human_confidence(), value, "{word}");
        assert_eq!(
            properties.human_confidence_source(),
            HumanConfidenceSource::Active
        );
        assert!(data.errors().is_empty());
        let request = &cloud.requests()[0];
        assert_eq!(field(request, "humanconfidence"), [SEALED]);
        assert_eq!(field(request, "transaction"), [transaction.as_str()]);
        assert_eq!(field(request, "license"), [LICENSE_KEY]);
        assert!(
            field(request, "humanconfidencepage").is_empty(),
            "the page id is never sent"
        );
    }
}

#[test]
fn a_request_with_no_sealed_result_is_unchanged() {
    let cloud = Cloud::answering(200, &derived("\"High\""));
    let engine = licensed(&cloud);
    let (id, _) = page(&engine);
    let data = process(
        &engine,
        &[
            ("header.user-agent", "agent"),
            ("query.transaction", "the-page-sent-this"),
            (PAGE_EVIDENCE, &id),
        ],
    );
    assert_eq!(human_confidence(&data), None);
    let request = &cloud.requests()[0];
    assert!(field(request, "values").is_empty(), "nothing is added");
    assert_eq!(field(request, "transaction"), ["the-page-sent-this"]);
    assert!(field(request, "humanconfidencepage").is_empty());
}

#[test]
fn a_sealed_result_under_any_other_key_is_not_sent_nor_is_a_transaction() {
    for other in [
        "header.humanconfidence",
        "cookie.HumanConfidence",
        "server.humanconfidence",
        "humanconfidence",
    ] {
        let cloud = Cloud::answering(200, &derived("\"High\""));
        let engine = licensed(&cloud);
        let (id, _) = page(&engine);
        let data = process(
            &engine,
            &[
                (other, SEALED),
                (PAGE_EVIDENCE, &id),
                ("query.transaction", "from-the-page"),
                ("header.transaction", "from-a-header"),
                ("cookie.challenge", "from-a-cookie"),
            ],
        );
        assert_eq!(human_confidence(&data), None, "{other}");
        assert!(data.errors().is_empty(), "{other}");
        let request = &cloud.requests()[0];
        for name in ["humanconfidence", "transaction", "challenge"] {
            assert!(field(request, name).is_empty(), "{other}: {request:?}");
        }
    }
}

#[test]
fn a_sealed_result_under_another_key_is_not_part_of_the_cache_key() {
    let cloud = Cloud::answering(200, r#"{"device":{"ismobile":true}}"#);
    let cache: Arc<LruCache<String, String>> = Arc::new(LruCache::new(10, 1));
    // A service that lists the header among the evidence it accepts.
    let mut accepting = state();
    accepting.evidence_keys.push(EvidenceKeyEntry {
        key: "header.humanconfidence".to_owned(),
        order: 1,
    });
    let engine = Arc::new(
        builder(&cloud)
            .set_state(accepting)
            .license_key(LICENSE_KEY)
            .cache(cache.clone())
            .build()
            .unwrap(),
    );
    process(&engine, &[("header.user-agent", "agent")]);
    process(
        &engine,
        &[
            ("header.user-agent", "agent"),
            ("header.humanconfidence", SEALED),
        ],
    );
    assert_eq!(
        cloud.requests().len(),
        1,
        "served from the cache as the same request"
    );
}

#[test]
fn an_empty_human_confidence_changes_nothing() {
    let cloud = Cloud::answering(200, &derived("\"High\""));
    let engine = licensed(&cloud);
    let data = process(
        &engine,
        &[
            (SEALED_RESULT_EVIDENCE, ""),
            ("header.humanconfidence", ""),
            ("query.transaction", "from-the-page"),
        ],
    );
    assert_eq!(human_confidence(&data), None);
    assert_eq!(
        field(&cloud.requests()[0], "transaction"),
        ["from-the-page"]
    );
}

#[test]
fn each_case_where_the_sealed_result_is_not_redeemed_gives_medium_and_active() {
    let answers: Vec<(&str, Arc<Cloud>)> = vec![
        ("the request fails or times out", Cloud::failing()),
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
    ];
    for (case, cloud) in answers {
        let engine = licensed(&cloud);
        let (id, _) = page(&engine);
        let data = redeem(&engine, &id, &[]);
        let errors = data.errors().len();
        assert_medium_and_active(&data, case);
        human_confidence(&data);
        assert_eq!(
            data.errors().len(),
            errors,
            "{case}: the read adds no error"
        );
        assert!(errors <= 1, "{case}: a failure is recorded once at most");
    }

    // No cloud request engine in the Pipeline.
    let other = Pipeline::builder()
        .add_element(Arc::new(Other(EvidenceKeyFilterWhitelist::new(Vec::<
            String,
        >::new(
        )))))
        .build()
        .unwrap();
    let mut data = other.create_flow_data_with(
        Evidence::builder()
            .add(SEALED_RESULT_EVIDENCE, SEALED)
            .build(),
    );
    data.process().unwrap();
    assert_medium_and_active(&data, "no cloud request engine");

    // An engine with no License Key, which leaves the sealed result out.
    let cloud = Cloud::answering(200, &derived("\"High\""));
    let engine = Arc::new(builder(&cloud).build().unwrap());
    let (id, _) = page(&engine);
    let data = redeem(&engine, &id, &[]);
    assert_medium_and_active(&data, "no License Key");
    assert!(field(&cloud.requests()[0], "humanconfidence").is_empty());

    // No transaction kept for the page: none issued, an id the engine never
    // gave, or one used already. The sealed result is not sent.
    let cloud = Cloud::answering(200, &derived("\"High\""));
    let engine = licensed(&cloud);
    let (used, _) = page(&engine);
    redeem(&engine, &used, &[]);
    for (case, data) in [
        (
            "no page named",
            process(&engine, &[(SEALED_RESULT_EVIDENCE, SEALED)]),
        ),
        ("an unknown page", redeem(&engine, "not-a-page", &[])),
        ("a page already redeemed", redeem(&engine, &used, &[])),
        (
            "a transaction from the request only",
            process(
                &engine,
                &[
                    (SEALED_RESULT_EVIDENCE, SEALED),
                    ("query.transaction", "from-the-page"),
                ],
            ),
        ),
    ] {
        assert_medium_and_active(&data, case);
    }
    for request in &cloud.requests()[1..] {
        assert!(field(request, "humanconfidence").is_empty(), "{request:?}");
        assert!(field(request, "transaction").is_empty(), "{request:?}");
    }

    // No request for the flow data, as in a recovery period.
    let cloud = Cloud::failing();
    let engine = Arc::new(
        builder(&cloud)
            .license_key(LICENSE_KEY)
            .failures_to_enter_recovery(1)
            .build()
            .unwrap(),
    );
    process(&engine, &[("header.user-agent", "agent")]);
    *cloud.answer.lock().unwrap() = Ok((200, derived("\"High\"")));
    let (id, _) = page(&engine);
    let data = redeem(&engine, &id, &[]);
    assert_eq!(
        cloud.requests().len(),
        1,
        "the recovery period holds it back"
    );
    assert_medium_and_active(&data, "recovery period");
}

#[test]
fn two_reads_for_one_request_make_one_request() {
    let cloud = Cloud::answering(200, &derived("\"Low\""));
    let engine = licensed(&cloud);
    let (id, _) = page(&engine);
    let data = redeem(&engine, &id, &[]);
    let first = human_confidence(&data);
    let second = human_confidence(&data);
    assert_eq!(first, second);
    assert_eq!(first.unwrap().human_confidence(), HumanConfidence::Low);
    assert_eq!(cloud.requests().len(), 1);
}

#[test]
fn a_request_with_a_sealed_result_is_neither_stored_in_nor_served_from_the_cache() {
    let cloud = Cloud::answering(200, &derived("\"High\""));
    let cache: Arc<LruCache<String, String>> = Arc::new(LruCache::new(10, 1));
    let engine = Arc::new(
        builder(&cloud)
            .license_key(LICENSE_KEY)
            .cache(cache.clone())
            .build()
            .unwrap(),
    );
    let (id, _) = page(&engine);
    redeem(&engine, &id, &[("header.user-agent", "agent")]);
    assert_eq!(cache.len(), 0, "nothing is stored");

    // A stored answer for the same accepted evidence is not served either.
    process(&engine, &[("header.user-agent", "agent")]);
    assert_eq!(cache.len(), 1);
    let (id, _) = page(&engine);
    redeem(&engine, &id, &[("header.user-agent", "agent")]);
    assert_eq!(
        cloud.requests().len(),
        3,
        "every sealed result goes to the cloud"
    );
}

#[test]
fn the_call_holds_one_transaction_being_the_one_kept_for_the_page() {
    let cloud = Cloud::answering(200, &derived("\"High\""));
    let engine = licensed(&cloud);
    let (id, transaction) = page(&engine);
    redeem(
        &engine,
        &id,
        &[
            ("query.transaction", "forged"),
            ("header.Transaction", "forged"),
            ("cookie.challenge", "forged"),
            ("query.CHALLENGE", "forged"),
        ],
    );
    let request = &cloud.requests()[0];
    assert_eq!(field(request, "transaction"), [transaction.as_str()]);
    assert!(field(request, "challenge").is_empty());
}

#[test]
fn each_page_keeps_its_own_transaction() {
    // Two tabs: the older page still redeems after the newer one is served.
    let cloud = Cloud::answering(200, &derived("\"High\""));
    let engine = licensed(&cloud);
    let (older, older_transaction) = page(&engine);
    let (newer, newer_transaction) = page(&engine);
    assert_ne!(older, newer);
    assert_ne!(older_transaction, newer_transaction);
    redeem(&engine, &older, &[]);
    redeem(&engine, &newer, &[]);
    let sent: Vec<String> = cloud
        .requests()
        .iter()
        .map(|request| field(request, "transaction")[0].to_owned())
        .collect();
    assert_eq!(sent, [older_transaction, newer_transaction]);
}

#[test]
fn a_store_shared_with_the_server_issues_the_pages_the_engine_redeems() {
    let cloud = Cloud::answering(200, &derived("\"High\""));
    let transactions = Arc::new(HumanConfidenceTransactions::default());
    let engine = Arc::new(
        builder(&cloud)
            .license_key(LICENSE_KEY)
            .human_confidence_transactions(transactions.clone())
            .build()
            .unwrap(),
    );
    let page = transactions.issue().unwrap();
    let data = redeem(&engine, page.id(), &[]);
    assert_eq!(
        human_confidence(&data).unwrap().human_confidence(),
        HumanConfidence::High
    );
    let url = page.script_url(ENDPOINT, RESOURCE_KEY);
    assert!(url.ends_with(field(&cloud.requests()[0], "transaction")[0]));
}

/// The same client, awaited, as on a host that can only await.
struct AsyncCloud(Arc<Cloud>);

impl AsyncCloudHttpClient for AsyncCloud {
    fn send<'a>(
        &'a self,
        request: &'a CloudHttpRequest,
    ) -> LocalBoxFuture<'a, Result<CloudHttpResponse, String>> {
        Box::pin(async move { self.0.send(request) })
    }
}

#[test]
fn a_sealed_result_redeems_through_fetch_on_an_asynchronous_client() {
    let cloud = Cloud::answering(200, &derived("\"Low\""));
    let engine = Arc::new(
        CloudRequestEngine::builder()
            .resource_key(RESOURCE_KEY)
            .license_key(LICENSE_KEY)
            .endpoint(ENDPOINT)
            .async_http_client(Arc::new(AsyncCloud(cloud.clone())))
            .set_state(state())
            .build()
            .unwrap(),
    );
    let (id, transaction) = page(&engine);
    let mut data = pipeline(&engine).create_flow_data_with(
        Evidence::builder()
            .add(SEALED_RESULT_EVIDENCE, SEALED)
            .add(PAGE_EVIDENCE, id.as_str())
            .build(),
    );
    {
        let mut fetch = std::pin::pin!(engine.fetch(&mut data));
        let waker = std::task::Waker::noop();
        while fetch
            .as_mut()
            .poll(&mut std::task::Context::from_waker(waker))
            .is_pending()
        {}
    }
    data.process().unwrap();
    assert_eq!(
        human_confidence(&data).unwrap().human_confidence(),
        HumanConfidence::Low
    );
    assert_eq!(
        field(&cloud.requests()[0], "transaction"),
        [transaction.as_str()]
    );
}

#[test]
fn the_license_key_is_never_taken_from_evidence() {
    let cloud = Cloud::answering(200, &derived("\"High\""));
    let engine = licensed(&cloud);
    let (id, _) = page(&engine);
    redeem(
        &engine,
        &id,
        &[
            ("query.license", "from-the-page"),
            ("query.licence", "from-the-page"),
        ],
    );
    let request = &cloud.requests()[0];
    assert_eq!(field(request, "license"), [LICENSE_KEY]);
    assert!(field(request, "licence").is_empty());
}

#[test]
fn derived_human_confidence_is_added_but_not_reported_missing() {
    let cloud = Cloud::answering(200, r#"{"device":{"ismobile":true}}"#);
    let engine = Arc::new(
        builder(&cloud)
            .license_key(LICENSE_KEY)
            .values(["device.ismobile"])
            .build()
            .unwrap(),
    );
    let (id, _) = page(&engine);
    let data = redeem(&engine, &id, &[]);
    assert_eq!(
        field(&cloud.requests()[0], "values"),
        ["device.ismobile,derived.HumanConfidence"]
    );
    let warnings = data
        .get(CloudRequestEngine::DATA_KEY)
        .unwrap()
        .warnings()
        .join(" ");
    assert!(
        !warnings.to_ascii_lowercase().contains("humanconfidence"),
        "the added name is not reported: {warnings}"
    );

    // A name the list already holds is not added again.
    let cloud = Cloud::answering(200, &derived("\"High\""));
    let engine = Arc::new(
        builder(&cloud)
            .license_key(LICENSE_KEY)
            .values(["Derived.HumanConfidence"])
            .build()
            .unwrap(),
    );
    let (id, _) = page(&engine);
    redeem(&engine, &id, &[]);
    assert_eq!(
        field(&cloud.requests()[0], "values"),
        ["Derived.HumanConfidence"]
    );
}

/// Fail where `text` holds any run of four characters of `secret`, so a
/// value kept in part fails as well as one kept whole.
fn assert_no_part(text: &str, secret: &str) {
    let characters: Vec<char> = secret.chars().collect();
    for part in characters.windows(4) {
        let part: String = part.iter().collect();
        assert!(!text.contains(&part), "'{part}' of a secret is in: {text}");
    }
}

/// Every record logged at any level, from any crate.
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Recorder;

impl log::Log for Recorder {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        LOGGED.lock().unwrap().push(record.args().to_string());
    }

    fn flush(&self) {}
}

static RECORDER: Recorder = Recorder;

#[test]
fn no_log_entry_or_warning_holds_the_sealed_result_the_license_key_or_the_transaction() {
    let _ = log::set_logger(&RECORDER);
    log::set_max_level(log::LevelFilter::Trace);
    // The service repeats everything it was sent in its warnings.
    let cloud = Cloud::answering(200, "");
    let engine = licensed(&cloud);
    let (id, transaction) = page(&engine);
    *cloud.answer.lock().unwrap() = Ok((
        200,
        format!(
            r#"{{"derived":{{"humanconfidence":"High"}},"warnings":["saw {SEALED} {LICENSE_KEY} {transaction}"]}}"#
        ),
    ));
    let data = redeem(&engine, &id, &[]);
    let warnings = data
        .get(CloudRequestEngine::DATA_KEY)
        .unwrap()
        .warnings()
        .join(" ");
    assert!(warnings.contains("saw"), "the warning is kept: {warnings}");
    let logged = LOGGED.lock().unwrap().join("\n");
    assert!(logged.contains("saw"), "the warning is logged: {logged}");
    for secret in [SEALED, LICENSE_KEY, transaction.as_str()] {
        assert_no_part(&logged, secret);
        assert_no_part(&warnings, secret);
    }
}

#[test]
fn no_error_message_holds_the_sealed_result_the_license_key_or_the_transaction() {
    let cloud = Cloud::answering(200, "");
    let engine = licensed(&cloud);
    let (id, transaction) = page(&engine);
    // The service repeats everything it was sent in its errors.
    *cloud.answer.lock().unwrap() = Ok((
        400,
        format!(r#"{{"errors":["bad {SEALED} {LICENSE_KEY} {transaction}"]}}"#),
    ));
    let data = redeem(&engine, &id, &[]);
    assert!(!data.errors().is_empty(), "the failure is recorded");
    for error in data.errors() {
        let message = format!("{error:?}");
        assert!(message.contains("bad"), "{message}");
        for secret in [SEALED, LICENSE_KEY, transaction.as_str()] {
            assert_no_part(&message, secret);
        }
    }
}

#[test]
fn a_long_error_body_cut_short_keeps_no_part_of_a_secret() {
    for which in 0..3 {
        for at in [990, 995, 999] {
            let cloud = Cloud::answering(200, "");
            let engine = licensed(&cloud);
            let (id, transaction) = page(&engine);
            let secret = [SEALED, LICENSE_KEY, transaction.as_str()][which].to_owned();
            // JSON with data and no errors list, under a failing status, long
            // enough to be cut, with the value across the point it is cut.
            let body = format!(r#"{{"a":"{}{secret}"}}"#, "x".repeat(at - 6));
            *cloud.answer.lock().unwrap() = Ok((502, body));
            let data = redeem(&engine, &id, &[]);
            assert!(!data.errors().is_empty(), "the failure is recorded");
            for error in data.errors() {
                let message = format!("{error:?}");
                assert!(message.contains("xxxx"), "the body is quoted: {message}");
                assert_no_part(&message, &secret);
            }
        }
    }
}

#[test]
fn a_conflict_warning_holds_no_part_of_a_sealed_result_or_a_transaction() {
    let _ = log::set_logger(&RECORDER);
    log::set_max_level(log::LevelFilter::Trace);
    const FORGED: &str = "QQ-FORGED-0001";
    let cloud = Cloud::answering(200, &derived("\"High\""));
    let engine = licensed(&cloud);
    let (id, _) = page(&engine);
    redeem(
        &engine,
        &id,
        &[
            ("header.humanconfidence", "from-a-header"),
            ("cookie.humanconfidence", "from-a-cookie"),
            ("header.e", "first"),
            ("cookie.e", "second"),
            ("query.transaction", FORGED),
            ("header.transaction", "from-a-header"),
            ("cookie.transaction", "from-a-cookie"),
        ],
    );
    let logged = LOGGED.lock().unwrap().join("\n");
    assert!(logged.contains("conflicts with"), "{logged}");
    assert_no_part(&logged, SEALED);
    assert_no_part(&logged, FORGED);
}
