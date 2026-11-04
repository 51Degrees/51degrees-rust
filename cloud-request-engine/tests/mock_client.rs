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

//! Engine-level tests driven against an in-process fake HTTP client, so the
//! full process flow (discovery, form building, response parsing, recovery)
//! is exercised without touching the network.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use fiftyone_cloud_request_engine::{
    Cache, CloudEngineState, CloudHttpClient, CloudHttpRequest, CloudHttpResponse,
    CloudRequestEngine, EvidenceKeyEntry, HttpMethod, LicensedProducts, PutCache,
};
use fiftyone_pipeline_core::{Error, Evidence, FlowElement, Pipeline};
use fiftyone_pipeline_engines::AspectData;

/// A scripted fake transport. Each endpoint is matched by a substring of the
/// URL, returning a queued response or recording a transport failure.
#[derive(Default)]
struct FakeClient {
    /// Recorded requests, for assertions.
    requests: Mutex<Vec<CloudHttpRequest>>,
    /// Response for the data (`json`) endpoint.
    data: Mutex<Option<Result<CloudHttpResponse, String>>>,
    /// Response for the `evidencekeys` endpoint.
    evidence_keys: Mutex<Option<Result<CloudHttpResponse, String>>>,
    /// Response for the `accessibleproperties` endpoint.
    properties: Mutex<Option<Result<CloudHttpResponse, String>>>,
}

impl FakeClient {
    fn ok(body: &str) -> Result<CloudHttpResponse, String> {
        Ok(CloudHttpResponse {
            status: 200,
            body: body.to_owned(),
            retry_after: None,
        })
    }

    fn with_status(status: u16, body: &str) -> Result<CloudHttpResponse, String> {
        Ok(CloudHttpResponse {
            status,
            body: body.to_owned(),
            retry_after: None,
        })
    }

    fn set_data(&self, response: Result<CloudHttpResponse, String>) {
        *self.data.lock().unwrap() = Some(response);
    }
    fn set_evidence_keys(&self, response: Result<CloudHttpResponse, String>) {
        *self.evidence_keys.lock().unwrap() = Some(response);
    }
    fn set_properties(&self, response: Result<CloudHttpResponse, String>) {
        *self.properties.lock().unwrap() = Some(response);
    }

    fn request_count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

impl CloudHttpClient for FakeClient {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        self.requests.lock().unwrap().push(request.clone());
        // The builder fetches evidencekeys and accessibleproperties as it builds
        // the engine. A test that does not care about discovery leaves those slots
        // unset and gets a sensible default, so the build still succeeds; a test
        // exercising discovery sets them explicitly (including to an error).
        if request.url.contains("evidencekeys") {
            self.evidence_keys
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| FakeClient::ok(r#"["header.user-agent","query.user-agent"]"#))
        } else if request.url.contains("accessibleproperties") {
            self.properties
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| FakeClient::ok(r#"{"Products":{}}"#))
        } else {
            self.data
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| Err("no scripted response".to_owned()))
        }
    }
}

fn engine_with(client: Arc<FakeClient>) -> CloudRequestEngine {
    CloudRequestEngine::builder()
        .resource_key("test-resource-key")
        .endpoint("https://cloud.example.test/api/v4/")
        .http_client(client)
        .build()
        .unwrap()
}

#[test]
fn happy_path_stores_raw_json_under_cloud_key() {
    let client = Arc::new(FakeClient::default());
    client.set_evidence_keys(FakeClient::ok(
        r#"["header.user-agent","query.user-agent"]"#,
    ));
    client.set_data(FakeClient::ok(r#"{"device":{"ismobile":true}}"#));

    let engine = engine_with(client.clone());
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(engine))
        .build()
        .unwrap();

    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", "Mozilla/5.0")
            .build(),
    );
    data.process().unwrap();

    let cloud = data.get(CloudRequestEngine::DATA_KEY).unwrap();
    assert_eq!(
        cloud.json_response(),
        Some(r#"{"device":{"ismobile":true}}"#)
    );
    assert!(cloud.process_started());

    // Three requests: evidencekeys and accessibleproperties discovery at build
    // time, then the data POST on process.
    assert_eq!(client.request_count(), 3);
}

#[test]
fn evidence_is_filtered_to_accepted_keys_and_prefixes_stripped() {
    let client = Arc::new(FakeClient::default());
    // The server only accepts the user-agent header.
    client.set_evidence_keys(FakeClient::ok(r#"["header.user-agent"]"#));
    client.set_data(FakeClient::ok(r#"{"device":{}}"#));

    let engine = engine_with(client.clone());
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(engine))
        .build()
        .unwrap();

    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", "UA")
            .add("cookie.not-accepted", "should-be-dropped")
            .build(),
    );
    data.process().unwrap();

    let requests = client.requests.lock().unwrap();
    let data_request = requests
        .iter()
        .find(|r| r.method == HttpMethod::Post)
        .expect("a POST to the data endpoint");

    // Resource key is present, the accepted header is stripped to `user-agent`,
    // and the non-accepted cookie is absent.
    assert!(data_request
        .form
        .iter()
        .any(|(k, v)| k == "resource" && v == "test-resource-key"));
    assert!(data_request
        .form
        .iter()
        .any(|(k, v)| k == "user-agent" && v == "UA"));
    assert!(!data_request.form.iter().any(|(k, _)| k == "not-accepted"));
}

#[test]
fn data_request_carries_resource_key_in_form_and_origin_header() {
    let client = Arc::new(FakeClient::default());
    client.set_evidence_keys(FakeClient::ok(r#"["header.user-agent"]"#));
    client.set_data(FakeClient::ok(r#"{"device":{}}"#));

    // Configure a cloud-request origin. The cloud service checks this against the
    // origins the resource key permits, so the engine must put it on the request.
    let engine = CloudRequestEngine::builder()
        .resource_key("test-resource-key")
        .endpoint("https://cloud.example.test/api/v4/")
        .http_client(client.clone())
        .cloud_request_origin("https://example.com")
        .build()
        .unwrap();
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(engine))
        .build()
        .unwrap();

    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("header.user-agent", "UA").build());
    data.process().unwrap();

    let requests = client.requests.lock().unwrap();
    let data_request = requests
        .iter()
        .find(|r| r.method == HttpMethod::Post)
        .expect("a POST to the data endpoint");

    // The resource key travels in the url-encoded form body under `resource`.
    assert!(
        data_request
            .form
            .iter()
            .any(|(k, v)| k == "resource" && v == "test-resource-key"),
        "the data POST should carry the resource key in the form body"
    );

    // The configured origin is set on the request so the transport sends it as
    // the `Origin` header.
    assert_eq!(
        data_request.origin.as_deref(),
        Some("https://example.com"),
        "the data POST should carry the configured cloud-request origin"
    );
}

#[test]
fn requests_omit_origin_when_no_cloud_request_origin_is_configured() {
    let client = Arc::new(FakeClient::default());
    client.set_evidence_keys(FakeClient::ok(r#"["header.user-agent"]"#));
    client.set_data(FakeClient::ok(r#"{"device":{}}"#));

    // The default `engine_with` configures no origin.
    let engine = engine_with(client.clone());
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(engine))
        .build()
        .unwrap();

    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("header.user-agent", "UA").build());
    data.process().unwrap();

    // With no origin configured, every request leaves the header unset so the
    // transport adds no `Origin`.
    let requests = client.requests.lock().unwrap();
    assert!(
        requests.iter().all(|r| r.origin.is_none()),
        "no request should carry an origin when none is configured"
    );
}

#[test]
fn cloud_error_response_raises_cloud_request_error_when_not_suppressed() {
    let client = Arc::new(FakeClient::default());
    client.set_evidence_keys(FakeClient::ok(r#"["header.user-agent"]"#));
    client.set_data(FakeClient::with_status(
        400,
        r#"{"errors":["invalid resource key"]}"#,
    ));

    let engine = engine_with(client);
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(engine))
        .build()
        .unwrap();

    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("header.user-agent", "UA").build());
    let err = data.process().unwrap_err();
    // The aggregate carries a single cloud-request error.
    match err {
        Error::Aggregate(errors) => {
            assert_eq!(errors.len(), 1);
            match &errors[0].source {
                Error::CloudRequest {
                    status_code,
                    message,
                    ..
                } => {
                    assert_eq!(*status_code, 400);
                    assert!(message.contains("invalid resource key"));
                }
                other => panic!("unexpected inner error {other:?}"),
            }
        }
        other => panic!("expected aggregate, got {other:?}"),
    }
}

/// How many requests `client` received whose URL contains `endpoint`.
fn requests_to(client: &FakeClient, endpoint: &str) -> usize {
    client
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|request| request.url.contains(endpoint))
        .count()
}

/// Process one flow data carrying a user agent through `pipeline`, returning
/// the errors it recorded.
fn errors_processing(pipeline: &Arc<Pipeline>) -> Vec<String> {
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("header.user-agent", "UA").build());
    data.process().unwrap();
    data.errors()
        .iter()
        .map(|error| error.source.to_string())
        .collect()
}

/// A discovery answer refused with `status`.
fn refused(status: u16) -> Result<CloudHttpResponse, String> {
    FakeClient::with_status(status, r#"{"errors":["refused"]}"#)
}

#[test]
fn a_4xx_answer_to_either_discovery_request_fails_the_build() {
    let cases = [
        ("evidence keys 403", Some(refused(403)), None, 403),
        ("properties 401", None, Some(refused(401)), 401),
        (
            "evidence keys 500 and properties 403",
            Some(FakeClient::with_status(500, "fault")),
            Some(refused(403)),
            403,
        ),
        (
            "evidence keys unreachable and properties 404",
            Some(Err("connection refused".to_owned())),
            Some(refused(404)),
            404,
        ),
    ];
    for (case, evidence_keys, properties, expected) in cases {
        let client = Arc::new(FakeClient::default());
        if let Some(answer) = evidence_keys {
            client.set_evidence_keys(answer);
        }
        if let Some(answer) = properties {
            client.set_properties(answer);
        }
        let result = CloudRequestEngine::builder()
            .resource_key("test-resource-key")
            .endpoint("https://cloud.example.test/api/v4/")
            .http_client(client)
            .build();
        match result {
            Err(Error::CloudRequest { status_code, .. }) => {
                assert_eq!(status_code, expected, "{case}")
            }
            Ok(_) => panic!("{case}: expected the build to fail"),
            Err(other) => panic!("{case}: unexpected error {other:?}"),
        }
    }
}

#[test]
fn evidence_keys_that_cannot_be_reached_are_fetched_on_first_use() {
    let client = Arc::new(FakeClient::default());
    client.set_evidence_keys(Err("connection refused".to_owned()));
    client.set_data(FakeClient::ok(r#"{"device":{"ismobile":true}}"#));
    let mut builder = CloudRequestEngine::builder()
        .resource_key("test-resource-key")
        .endpoint("https://cloud.example.test/api/v4/")
        .http_client(client.clone());
    let engine = builder
        .build()
        .expect("an unreachable cloud does not fail the build");

    assert!(!engine.has_loaded_metadata());
    assert!(engine.accepted_evidence_keys().is_err());
    assert!(
        engine.public_properties().is_ok(),
        "the other request worked"
    );
    assert!(!engine.evidence_key_filter().include("header.user-agent"));
    assert!(builder.export_state().is_none());

    let engine = Arc::new(engine);
    let pipeline = Pipeline::builder()
        .add_element(engine.clone())
        .suppress_process_exceptions(true)
        .build()
        .unwrap();

    // Still unreachable, so the request fails before any data request.
    assert_eq!(errors_processing(&pipeline).len(), 1);
    assert_eq!(requests_to(&client, "/json"), 0);

    // Reachable again, so the next request fetches the evidence keys and is
    // answered, sending the evidence they accept.
    client.set_evidence_keys(FakeClient::ok(r#"["header.user-agent"]"#));
    assert_eq!(errors_processing(&pipeline), Vec::<String>::new());
    assert!(engine.has_loaded_metadata());
    assert!(engine.evidence_key_filter().include("header.user-agent"));
    let sent = client.requests.lock().unwrap().last().cloned().unwrap();
    assert!(
        sent.form
            .iter()
            .any(|(name, value)| name == "user-agent" && value == "UA"),
        "the data request carried the user agent: {:?}",
        sent.form
    );

    // Once known, the keys are not asked for again, and the properties that
    // arrived at build time were never asked for again.
    assert_eq!(errors_processing(&pipeline), Vec::<String>::new());
    assert_eq!(requests_to(&client, "evidencekeys"), 3);
    assert_eq!(requests_to(&client, "accessibleproperties"), 1);
}

#[test]
fn properties_answered_5xx_are_fetched_on_first_use() {
    let client = Arc::new(FakeClient::default());
    client.set_properties(FakeClient::with_status(503, "Service Unavailable"));
    client.set_data(FakeClient::ok(r#"{"device":{"ismobile":true}}"#));
    let engine = Arc::new(engine_with(client.clone()));
    assert!(engine.public_properties().is_err());
    assert!(
        engine.accepted_evidence_keys().is_ok(),
        "the other request worked"
    );

    let pipeline = Pipeline::builder()
        .add_element(engine.clone())
        .suppress_process_exceptions(true)
        .build()
        .unwrap();
    assert_eq!(errors_processing(&pipeline).len(), 1);

    client.set_properties(FakeClient::ok(
        r#"{"Products":{"device":{"DataTier":"CloudV4","Properties":[]}}}"#,
    ));
    assert_eq!(errors_processing(&pipeline), Vec::<String>::new());
    assert!(engine.has_loaded_metadata());
    let products = engine.public_properties().unwrap();
    assert!(products.products.contains_key("device"));
    assert_eq!(requests_to(&client, "evidencekeys"), 1);
    assert_eq!(requests_to(&client, "accessibleproperties"), 3);
}

#[test]
fn warnings_are_stored_not_raised() {
    let client = Arc::new(FakeClient::default());
    client.set_evidence_keys(FakeClient::ok(r#"["header.user-agent"]"#));
    client.set_data(FakeClient::ok(
        r#"{"device":{"x":1},"warnings":["low entropy client-hints"]}"#,
    ));

    let engine = engine_with(client);
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(engine))
        .build()
        .unwrap();

    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("header.user-agent", "UA").build());
    data.process().unwrap();
    assert!(data.errors().is_empty(), "warnings do not become errors");
    let cloud = data.get(CloudRequestEngine::DATA_KEY).unwrap();
    assert_eq!(
        cloud.warnings(),
        vec!["low entropy client-hints".to_owned()]
    );
}

#[test]
fn accessible_properties_are_resolved_at_build() {
    let client = Arc::new(FakeClient::default());
    client.set_properties(FakeClient::ok(
        r#"{"Products":{"device":{"DataTier":"CloudV4","Properties":[{"Name":"IsMobile","Type":"Bool"}]}}}"#,
    ));

    let engine = engine_with(client.clone());
    // Discovery ran at build (evidencekeys + accessibleproperties), so the
    // accessor returns the resolved value and makes no further request.
    let after_build = client.request_count();
    assert_eq!(
        after_build, 2,
        "build fetched evidencekeys and accessibleproperties"
    );
    let products = engine.public_properties().unwrap();
    let device = products.products.get("device").unwrap();
    assert_eq!(device.properties[0].name, "IsMobile");
    let _ = engine.public_properties().unwrap();
    assert_eq!(
        client.request_count(),
        after_build,
        "the accessor performs no I/O"
    );
}

#[test]
fn retry_after_is_propagated_on_429() {
    let client = Arc::new(FakeClient::default());
    client.set_evidence_keys(FakeClient::ok(r#"["header.user-agent"]"#));
    client.set_data(Ok(CloudHttpResponse {
        status: 429,
        body: r#"{"errors":["rate limit exceeded"]}"#.to_owned(),
        retry_after: Some("42".to_owned()),
    }));

    let engine = engine_with(client);
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(engine))
        .build()
        .unwrap();

    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("header.user-agent", "UA").build());
    let err = data.process().unwrap_err();
    let Error::Aggregate(errors) = err else {
        panic!("expected aggregate");
    };
    match &errors[0].source {
        Error::CloudRequest {
            status_code,
            retry_after_seconds,
            ..
        } => {
            assert_eq!(*status_code, 429);
            assert_eq!(*retry_after_seconds, Some(42));
        }
        other => panic!("unexpected error {other:?}"),
    }
}

#[test]
fn recovery_mode_blocks_after_repeated_data_failures() {
    let client = Arc::new(FakeClient::default());
    // Discovery succeeds so the engine builds; every data POST then fails at the
    // transport level, so the recovery gate trips and later POSTs are blocked.
    client.set_evidence_keys(FakeClient::ok(r#"["header.user-agent"]"#));
    client.set_properties(FakeClient::ok(r#"{"Products":{}}"#));
    client.set_data(Err("connection refused".to_owned()));

    let engine = CloudRequestEngine::builder()
        .resource_key("rk")
        .endpoint("https://cloud.example.test/api/v4/")
        .http_client(client.clone())
        .failures_to_enter_recovery(2)
        .failures_window_seconds(100)
        .recovery_seconds(60.0)
        .build()
        .unwrap();

    // The two successful discovery fetches happened at build time.
    let after_build = client.request_count();
    assert_eq!(after_build, 2);

    let pipeline = Pipeline::builder()
        .add_element(Arc::new(engine))
        .suppress_process_exceptions(true)
        .build()
        .unwrap();

    // Drive several requests; each data POST fails and records a failure, so the
    // gate trips and later POSTs are short-circuited.
    for _ in 0..5 {
        let mut data = pipeline
            .create_flow_data_with(Evidence::builder().add("header.user-agent", "UA").build());
        data.process().unwrap();
        assert!(!data.errors().is_empty());
    }

    // Once recovery has tripped, the gate blocks before the HTTP call, so fewer
    // than five data POSTs were attempted.
    let data_posts = client.request_count() - after_build;
    assert!(
        data_posts < 5,
        "recovery gate should suppress some data POSTs, made {data_posts}"
    );
}

/// A transport answering as the cloud service did on 26 October 2026, with a
/// 404 and a plain text body for a `user-agent` of 32,000 bytes or more, and
/// JSON for anything else. With `failing` set it answers every request 503.
struct SizeLimitedCloud {
    failing: bool,
}

impl CloudHttpClient for SizeLimitedCloud {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        if self.failing {
            return FakeClient::with_status(503, "Service Unavailable");
        }
        let user_agent = request
            .form
            .iter()
            .find(|(name, _)| name == "user-agent")
            .map_or(0, |(_, value)| value.len());
        if user_agent >= 32_000 {
            FakeClient::with_status(404, "Status code: 404, 'Error' method not found")
        } else {
            FakeClient::ok(r#"{"device":{"ismobile":false}}"#)
        }
    }
}

/// A pipeline over one engine with the default recovery settings, ten
/// failures in 100 seconds opening 60 seconds of recovery.
fn recovery_pipeline(cloud: SizeLimitedCloud) -> Arc<Pipeline> {
    let state = CloudEngineState {
        evidence_keys: vec![EvidenceKeyEntry {
            key: "query.user-agent".to_owned(),
            order: 0,
        }],
        accessible_properties: LicensedProducts::default(),
    };
    let engine = CloudRequestEngine::builder()
        .resource_key("rk")
        .http_client(Arc::new(cloud))
        .set_state(state)
        .build()
        .unwrap();
    Pipeline::builder()
        .add_element(Arc::new(engine))
        .suppress_process_exceptions(true)
        .build()
        .unwrap()
}

/// The error messages a request with this user agent records.
fn errors_for(pipeline: &Arc<Pipeline>, user_agent: &str) -> Vec<String> {
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("query.user-agent", user_agent)
            .build(),
    );
    data.process().unwrap();
    data.errors()
        .iter()
        .map(|error| error.source.to_string())
        .collect()
}

#[test]
fn answers_to_one_visitors_own_content_leave_the_next_visitor_served() {
    // The gate is shared by every visitor, so a visitor sending evidence the
    // service refuses must not be able to open a recovery period for the rest.
    let pipeline = recovery_pipeline(SizeLimitedCloud { failing: false });
    let oversized = "A".repeat(32_000);
    for _ in 0..10 {
        assert_eq!(errors_for(&pipeline, &oversized).len(), 1);
    }
    let next = errors_for(&pipeline, "Mozilla/5.0");
    assert!(next.is_empty(), "the next visitor was refused: {next:?}");
}

#[test]
fn service_errors_still_open_a_recovery_period() {
    let pipeline = recovery_pipeline(SizeLimitedCloud { failing: true });
    for _ in 0..10 {
        assert_eq!(errors_for(&pipeline, "Mozilla/5.0").len(), 1);
    }
    let next = errors_for(&pipeline, "Mozilla/5.0");
    assert!(
        next.iter()
            .any(|error| error.contains("temporarily restricted")),
        "ten 503 answers did not open a recovery period: {next:?}"
    );
}

/// A consumer-supplied response cache, recording its lookups and stores so the
/// tests can assert how it was used. It stands in for any [`PutCache`], for
/// example one a `wasm32-wasip1` host would back with its own key/value store.
#[derive(Default)]
struct RecordingCache {
    store: Mutex<HashMap<String, String>>,
    gets: Mutex<usize>,
    puts: Mutex<usize>,
}

impl RecordingCache {
    fn puts(&self) -> usize {
        *self.puts.lock().unwrap()
    }
    fn gets(&self) -> usize {
        *self.gets.lock().unwrap()
    }
}

impl Cache<String, String> for RecordingCache {
    fn get(&self, key: &String) -> Option<String> {
        *self.gets.lock().unwrap() += 1;
        self.store.lock().unwrap().get(key).cloned()
    }
    fn len(&self) -> usize {
        self.store.lock().unwrap().len()
    }
}

impl PutCache<String, String> for RecordingCache {
    fn put(&self, key: String, value: String) {
        *self.puts.lock().unwrap() += 1;
        self.store.lock().unwrap().insert(key, value);
    }
}

#[test]
fn supplied_cache_serves_repeated_requests_without_a_second_cloud_call() {
    let client = Arc::new(FakeClient::default());
    client.set_evidence_keys(FakeClient::ok(r#"["header.user-agent"]"#));
    client.set_data(FakeClient::ok(r#"{"device":{"ismobile":true}}"#));

    let cache = Arc::new(RecordingCache::default());
    let engine = CloudRequestEngine::builder()
        .resource_key("test-resource-key")
        .endpoint("https://cloud.example.test/api/v4/")
        .http_client(client.clone())
        .cache(cache.clone())
        .build()
        .unwrap();
    // Build made the two discovery requests; no data POST yet.
    let after_build = client.request_count();
    assert_eq!(after_build, 2);

    let pipeline = Pipeline::builder()
        .add_element(Arc::new(engine))
        .build()
        .unwrap();
    let evidence = || Evidence::builder().add("header.user-agent", "UA").build();

    // First request: a cache miss, so the engine calls the cloud and stores the
    // response. The result is not a cache hit.
    let mut first = pipeline.create_flow_data_with(evidence());
    first.process().unwrap();
    let first_cloud = first.get(CloudRequestEngine::DATA_KEY).unwrap();
    assert_eq!(
        first_cloud.json_response(),
        Some(r#"{"device":{"ismobile":true}}"#)
    );
    assert!(!first_cloud.cache_hit(), "the first request is a miss");
    assert_eq!(
        client.request_count(),
        after_build + 1,
        "one data POST made"
    );
    assert_eq!(cache.puts(), 1, "the miss stored one entry");

    // Second identical request: served from the cache, so no further cloud call.
    let mut second = pipeline.create_flow_data_with(evidence());
    second.process().unwrap();
    let second_cloud = second.get(CloudRequestEngine::DATA_KEY).unwrap();
    assert_eq!(
        second_cloud.json_response(),
        Some(r#"{"device":{"ismobile":true}}"#),
        "the cached body is returned verbatim"
    );
    assert!(
        second_cloud.cache_hit(),
        "the second request is a cache hit"
    );
    assert_eq!(
        client.request_count(),
        after_build + 1,
        "no second data POST: the response came from the cache"
    );
    assert!(cache.gets() >= 2, "the cache was consulted on each request");
}

#[test]
fn host_provided_cache_outlives_the_engine_for_short_lived_instances() {
    // The wasm short-lived case: the host owns the cache and hands it to each
    // freshly built engine, so a cached response survives an engine being
    // discarded and rebuilt (a cold start). State is injected so neither build
    // touches the network.
    let shared: Arc<RecordingCache> = Arc::new(RecordingCache::default());
    let evidence = || Evidence::builder().add("header.user-agent", "UA").build();

    // First instance: a working transport. It misses the cache, calls the cloud
    // and populates the shared cache, then is dropped.
    {
        let client = Arc::new(FakeClient::default());
        client.set_data(FakeClient::ok(r#"{"device":{"ismobile":true}}"#));
        let engine = CloudRequestEngine::builder()
            .resource_key("rk")
            .http_client(client.clone())
            .set_state(sample_state())
            .cache(shared.clone())
            .build()
            .unwrap();
        let pipeline = Pipeline::builder()
            .add_element(Arc::new(engine))
            .build()
            .unwrap();
        let mut data = pipeline.create_flow_data_with(evidence());
        data.process().unwrap();
        assert_eq!(
            client.request_count(),
            1,
            "first instance made the data POST"
        );
    }
    assert_eq!(
        shared.puts(),
        1,
        "the first instance populated the shared cache"
    );

    // Second instance: a transport that errors on every call. Because the shared
    // cache already holds the response, the new engine serves it without ever
    // touching the transport.
    let client = Arc::new(FakeClient::default()); // no scripted data: would Err
    let engine = CloudRequestEngine::builder()
        .resource_key("rk")
        .http_client(client.clone())
        .set_state(sample_state())
        .cache(shared.clone())
        .build()
        .unwrap();
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(engine))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data_with(evidence());
    data.process().unwrap();

    let cloud = data.get(CloudRequestEngine::DATA_KEY).unwrap();
    assert!(
        cloud.cache_hit(),
        "the rebuilt instance served from the cache"
    );
    assert_eq!(
        cloud.json_response(),
        Some(r#"{"device":{"ismobile":true}}"#)
    );
    assert_eq!(
        client.request_count(),
        0,
        "the rebuilt instance made no request: state injected, response cached"
    );
}

#[test]
fn a_response_carrying_a_51did_is_never_stored() {
    // The cloud creates a 51Did for the one request that asked for it, so a body
    // carrying one is not kept for the next request with the same evidence.
    let client = Arc::new(FakeClient::default());
    client.set_evidence_keys(FakeClient::ok(r#"["header.user-agent","query.id.usage"]"#));
    client.set_data(FakeClient::ok(
        r#"{"device":{"ismobile":true},"fodid":{"idrandglobal":"created-for-one-request"}}"#,
    ));

    let cache = Arc::new(RecordingCache::default());
    let engine = CloudRequestEngine::builder()
        .resource_key("test-resource-key")
        .endpoint("https://cloud.example.test/api/v4/")
        .http_client(client.clone())
        .cache(cache.clone())
        .build()
        .unwrap();
    let after_build = client.request_count();
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(engine))
        .build()
        .unwrap();
    let evidence = || {
        Evidence::builder()
            .add("header.user-agent", "UA")
            .add("query.id.usage", "non-marketing")
            .build()
    };

    for request in 1..=2 {
        let mut data = pipeline.create_flow_data_with(evidence());
        data.process().unwrap();
        let cloud = data.get(CloudRequestEngine::DATA_KEY).unwrap();
        assert!(!cloud.cache_hit(), "request {request} is not a cache hit");
    }
    assert_eq!(cache.puts(), 0, "no body carrying a 51Did was stored");
    assert_eq!(
        client.request_count(),
        after_build + 2,
        "each request reached the cloud"
    );
}

/// A cache holding one body under every key, as a store shared with other
/// writers can hold a body for the key a request makes.
struct StoredBodyCache(String);

impl Cache<String, String> for StoredBodyCache {
    fn get(&self, _key: &String) -> Option<String> {
        Some(self.0.clone())
    }
    fn len(&self) -> usize {
        1
    }
}

impl PutCache<String, String> for StoredBodyCache {
    fn put(&self, _key: String, _value: String) {}
}

#[test]
fn a_stored_response_carrying_a_51did_is_never_served() {
    // The 51Did in a stored body was created for another request, so the engine
    // asks the cloud rather than serving it, whatever case the member is in.
    for member in ["fodid", "FODid"] {
        let client = Arc::new(FakeClient::default());
        client.set_data(FakeClient::ok(
            r#"{"device":{"ismobile":true},"fodid":{"idrandglobal":"created-for-this-request"}}"#,
        ));
        let stored = format!(
            r#"{{"device":{{"ismobile":true}},"{member}":{{"idrandglobal":"created-for-another-request"}}}}"#
        );
        let engine = CloudRequestEngine::builder()
            .resource_key("rk")
            .http_client(client.clone())
            .set_state(sample_state())
            .cache(Arc::new(StoredBodyCache(stored)))
            .build()
            .unwrap();
        let pipeline = Pipeline::builder()
            .add_element(Arc::new(engine))
            .build()
            .unwrap();

        let mut data = pipeline
            .create_flow_data_with(Evidence::builder().add("header.user-agent", "UA").build());
        data.process().unwrap();

        let cloud = data.get(CloudRequestEngine::DATA_KEY).unwrap();
        assert!(
            cloud
                .json_response()
                .is_some_and(|json| json.contains("created-for-this-request")),
            "the {member} body in the store was not served, got {:?}",
            cloud.json_response()
        );
        assert!(!cloud.cache_hit(), "the {member} body is not a cache hit");
        assert_eq!(client.request_count(), 1, "the cloud was asked instead");
    }
}

/// Build a state snapshot with one accepted evidence key and one product
/// property, the kind a consumer would persist and re-inject.
fn sample_state() -> CloudEngineState {
    CloudEngineState {
        evidence_keys: vec![EvidenceKeyEntry {
            key: "header.user-agent".to_owned(),
            order: 0,
        }],
        accessible_properties: LicensedProducts::parse(
            r#"{"Products":{"device":{"DataTier":"CloudV4","Properties":[{"Name":"IsMobile","Type":"Bool"}]}}}"#,
        )
        .unwrap(),
    }
}

#[test]
fn injected_state_skips_discovery_and_makes_no_discovery_request() {
    let client = Arc::new(FakeClient::default());
    // Only the data endpoint is scripted. Because a state is injected, the builder
    // makes no evidencekeys or accessibleproperties request at all.
    client.set_data(FakeClient::ok(r#"{"device":{"ismobile":true}}"#));

    let engine = CloudRequestEngine::builder()
        .resource_key("test-resource-key")
        .endpoint("https://cloud.example.test/api/v4/")
        .http_client(client.clone())
        .set_state(sample_state())
        .build()
        .unwrap();

    // The injected properties are available, and the build made no request.
    let products = engine.public_properties().unwrap();
    assert_eq!(
        products.products.get("device").unwrap().properties[0].name,
        "IsMobile"
    );
    assert_eq!(
        client.request_count(),
        0,
        "no discovery request when state is injected"
    );

    let pipeline = Pipeline::builder()
        .add_element(Arc::new(engine))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", "UA")
            .add("cookie.not-accepted", "should-be-dropped")
            .build(),
    );
    data.process().unwrap();

    // Exactly one request: the data POST. Discovery was skipped entirely.
    assert_eq!(
        client.request_count(),
        1,
        "only the data POST should be made when state is injected"
    );
    let requests = client.requests.lock().unwrap();
    let data_request = requests
        .iter()
        .find(|r| r.method == HttpMethod::Post)
        .expect("a POST to the data endpoint");
    // The injected evidence filter was applied: the accepted header is present
    // and stripped, the non-accepted cookie is dropped.
    assert!(data_request
        .form
        .iter()
        .any(|(k, v)| k == "user-agent" && v == "UA"));
    assert!(!data_request.form.iter().any(|(k, _)| k == "not-accepted"));
}

#[test]
fn injected_state_makes_no_discovery_request_at_build() {
    // When a state is injected the builder makes no discovery request. A client
    // that errors on every call proves the build never touches it for discovery.
    // The builder is kept so its retained state can be exported after the build.
    let client = Arc::new(FakeClient::default());
    let mut builder = CloudRequestEngine::builder()
        .resource_key("rk")
        .http_client(client.clone())
        .set_state(sample_state());
    let engine = builder.build().unwrap();

    // Both discovery results resolve from the injected state with no request.
    assert!(engine.has_loaded_metadata());
    let state = builder.export_state().expect("the builder holds the state");
    assert_eq!(state.evidence_keys.len(), 1);
    assert_eq!(client.request_count(), 0);
}

#[test]
fn exported_state_round_trips_through_serde_and_back_into_a_builder() {
    // Serialize an exported state and read it back, the way a host store would.
    let original = sample_state();
    let json = serde_json::to_string(&original).unwrap();
    let restored: CloudEngineState = serde_json::from_str(&json).unwrap();

    // Inject the restored state into a fresh builder. A client that errors on
    // every call proves the build relies entirely on the injected values. The
    // builder retains the state, so it exports the same snapshot it was given.
    let client = Arc::new(FakeClient::default());
    let mut builder = CloudRequestEngine::builder()
        .resource_key("rk")
        .http_client(client.clone())
        .set_state(restored);
    let _engine = builder.build().unwrap();

    let exported = builder.export_state().expect("the builder holds the state");
    assert_eq!(
        client.request_count(),
        0,
        "round-tripped state needs no fetch"
    );
    assert_eq!(exported.evidence_keys, original.evidence_keys);
    assert_eq!(
        exported
            .accessible_properties
            .products
            .get("device")
            .unwrap()
            .properties[0]
            .name,
        "IsMobile"
    );
}

#[test]
fn exported_state_returns_the_build_time_discovery_when_nothing_is_injected() {
    // With no injected state, the builder fetches discovery as it builds: one
    // evidencekeys fetch and one accessibleproperties fetch. The builder retains
    // the result, so exporting returns those resolved values with no further
    // request.
    let client = Arc::new(FakeClient::default());
    client.set_evidence_keys(FakeClient::ok(
        r#"["header.user-agent","query.user-agent"]"#,
    ));
    client.set_properties(FakeClient::ok(
        r#"{"Products":{"device":{"Properties":[{"Name":"IsMobile","Type":"Bool"}]}}}"#,
    ));

    let mut builder = CloudRequestEngine::builder()
        .resource_key("test-resource-key")
        .endpoint("https://cloud.example.test/api/v4/")
        .http_client(client.clone());
    let _engine = builder.build().unwrap();
    assert_eq!(
        client.request_count(),
        2,
        "the builder fetched evidencekeys and accessibleproperties"
    );

    let state = builder.export_state().expect("the builder holds the state");
    // The evidence keys are lowercased and sorted in the snapshot.
    let keys: Vec<&str> = state.evidence_keys.iter().map(|e| e.key.as_str()).collect();
    assert_eq!(keys, vec!["header.user-agent", "query.user-agent"]);
    assert_eq!(
        state
            .accessible_properties
            .products
            .get("device")
            .unwrap()
            .properties[0]
            .name,
        "IsMobile"
    );

    // Exporting performs no I/O, so the request count is unchanged.
    let _ = builder.export_state();
    assert_eq!(
        client.request_count(),
        2,
        "export performs no further request"
    );
}

/// The URL the builder asked the `accessibleproperties` endpoint with.
fn accessible_properties_url(client: &FakeClient) -> String {
    client
        .requests
        .lock()
        .unwrap()
        .iter()
        .find(|r| r.url.contains("accessibleproperties"))
        .expect("the build asked for the accessible properties")
        .url
        .clone()
}

#[test]
fn accessible_properties_are_asked_for_with_the_license_key() {
    // The cloud adds the products a license key grants to those of the
    // resource key, and the data request sends both keys, so the metadata
    // has to be asked for with both as well.
    let client = Arc::new(FakeClient::default());
    CloudRequestEngine::builder()
        .resource_key("test-resource-key")
        .license_key("LICENSE+KEY/1")
        .endpoint("https://cloud.example.test/api/v4/")
        .http_client(client.clone())
        .build()
        .unwrap();
    assert_eq!(
        accessible_properties_url(&client),
        "https://cloud.example.test/api/v4/accessibleproperties\
         ?resource=test-resource-key&license=LICENSE%2BKEY%2F1"
    );
}

#[test]
fn accessible_properties_are_asked_for_without_a_blank_license_key() {
    for license in [None, Some(""), Some("  ")] {
        let client = Arc::new(FakeClient::default());
        let mut builder = CloudRequestEngine::builder()
            .resource_key("test-resource-key")
            .endpoint("https://cloud.example.test/api/v4/")
            .http_client(client.clone());
        if let Some(license) = license {
            builder = builder.license_key(license);
        }
        builder.build().unwrap();
        assert_eq!(
            accessible_properties_url(&client),
            "https://cloud.example.test/api/v4/accessibleproperties\
             ?resource=test-resource-key",
            "license key {license:?}"
        );
    }
}

#[test]
fn accessible_properties_failure_names_neither_key() {
    // The keys travel in the query string of this request, so a failure must
    // name the endpoint without them, whether the build returns it (a 4xx) or
    // a request does after the retry on first use (a 5xx).
    for status in [403, 500] {
        let client = Arc::new(FakeClient::default());
        client.set_properties(FakeClient::with_status(status, "{}"));
        let result = CloudRequestEngine::builder()
            .resource_key("test-resource-key")
            .license_key("secret-license-key")
            .endpoint("https://cloud.example.test/api/v4/")
            .http_client(client)
            .build();
        let message = match (status, result) {
            (403, Err(error)) => error.to_string(),
            (500, Ok(engine)) => {
                let pipeline = Pipeline::builder()
                    .add_element(Arc::new(engine))
                    .suppress_process_exceptions(true)
                    .build()
                    .unwrap();
                let errors = errors_processing(&pipeline);
                assert_eq!(errors.len(), 1, "{errors:?}");
                errors[0].clone()
            }
            (status, Ok(_)) => panic!("expected the build to fail on a {status}"),
            (status, Err(error)) => {
                panic!("expected the build to work on a {status}: {error}")
            }
        };
        assert!(
            message.contains("https://cloud.example.test/api/v4/accessibleproperties"),
            "{status}, the endpoint is named: {message}"
        );
        assert!(
            !message.contains("secret-license-key"),
            "{status}, the license key is not in the message: {message}"
        );
        assert!(
            !message.contains("test-resource-key"),
            "{status}, the resource key is not in the message: {message}"
        );
    }
}

/// Build an engine that authenticates with a license key alone and asks for the
/// named properties, which is the one configuration in which the cloud service
/// honours a property list.
fn license_engine_with(client: Arc<FakeClient>, values: &[&str]) -> CloudRequestEngine {
    CloudRequestEngine::builder()
        .license_key("test-license-key")
        .values(values.to_vec())
        .endpoint("https://cloud.example.test/api/v4/")
        .http_client(client)
        .build()
        .unwrap()
}

#[test]
fn a_license_key_alone_authenticates_the_request() {
    let client = Arc::new(FakeClient::default());
    client.set_evidence_keys(FakeClient::ok(r#"["header.user-agent"]"#));
    client.set_data(FakeClient::ok(
        r#"{"device":{"ismobile":true,"iscrawler":false}}"#,
    ));

    let engine = license_engine_with(client.clone(), &["device.ismobile", "device.iscrawler"]);
    // Only the evidence keys were fetched. The accessible-properties endpoint
    // takes a resource key and refuses a request without one, so a license-key
    // engine does not call it and starts with no accessible properties.
    assert_eq!(client.request_count(), 1, "evidence keys only");
    assert!(engine.public_properties().unwrap().products.is_empty());
    assert_eq!(engine.resource_key(), None);

    let pipeline = Pipeline::builder()
        .add_element(Arc::new(engine))
        .build()
        .unwrap();
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("header.user-agent", "UA").build());
    data.process().unwrap();

    let requests = client.requests.lock().unwrap();
    let data_request = requests
        .iter()
        .find(|r| r.method == HttpMethod::Post)
        .expect("a POST to the data endpoint");

    // The license key authenticates the request on its own, and the properties
    // the caller named travel with it as a comma-separated list.
    assert!(data_request
        .form
        .iter()
        .any(|(k, v)| k == "license" && v == "test-license-key"));
    assert!(
        !data_request.form.iter().any(|(k, _)| k == "resource"),
        "no resource key is sent"
    );
    assert!(
        data_request
            .form
            .iter()
            .any(|(k, v)| k == "values" && v == "device.ismobile,device.iscrawler"),
        "the property list should reach the request, got {:?}",
        data_request.form
    );
    // The evidence still travels alongside it.
    assert!(data_request
        .form
        .iter()
        .any(|(k, v)| k == "user-agent" && v == "UA"));
}

#[test]
fn a_property_asked_for_and_not_returned_is_reported_once() {
    let client = Arc::new(FakeClient::default());
    client.set_evidence_keys(FakeClient::ok(r#"["header.user-agent"]"#));
    // The service answers 200 carrying the property the license covers, and says
    // nothing at all about the one it left out, so the difference is the only
    // sign that a property was dropped.
    client.set_data(FakeClient::ok(r#"{"device":{"ismobile":true}}"#));

    let engine = license_engine_with(client.clone(), &["device.ismobile", "device.iscrawler"]);
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(engine))
        .build()
        .unwrap();

    let mut first =
        pipeline.create_flow_data_with(Evidence::builder().add("header.user-agent", "UA").build());
    first.process().unwrap();
    assert!(
        first.errors().is_empty(),
        "a dropped property does not fail the request"
    );
    let warnings = first.get(CloudRequestEngine::DATA_KEY).unwrap().warnings();
    assert_eq!(warnings.len(), 1, "one warning, got {warnings:?}");
    assert!(
        warnings[0].contains("device.iscrawler")
            && !warnings[0].contains("device.ismobile")
            && warnings[0].contains("entitlement matter rather than a fault"),
        "the warning should name only the missing property and say what it means, \
         got {}",
        warnings[0]
    );

    // A second flow data through the same engine is not told again, because the
    // credential and the property list were both settled when it was built and
    // repeating the comparison would learn nothing.
    let mut second =
        pipeline.create_flow_data_with(Evidence::builder().add("header.user-agent", "UA2").build());
    second.process().unwrap();
    assert!(
        second
            .get(CloudRequestEngine::DATA_KEY)
            .unwrap()
            .warnings()
            .is_empty(),
        "the difference is reported once per engine"
    );
}

#[test]
fn nothing_is_reported_when_every_property_comes_back() {
    let client = Arc::new(FakeClient::default());
    client.set_evidence_keys(FakeClient::ok(r#"["header.user-agent"]"#));
    // A property that the evidence cannot populate still comes back as a key,
    // with a reason beside it, so it counts as answered rather than dropped.
    client.set_data(FakeClient::ok(
        r#"{"device":{"ismobile":true},
            "location":{"country":null,"countrynullreason":"needs JavaScript"}}"#,
    ));

    let engine = license_engine_with(client.clone(), &["device.ismobile", "location.country"]);
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(engine))
        .build()
        .unwrap();
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("header.user-agent", "UA").build());
    data.process().unwrap();
    assert!(data
        .get(CloudRequestEngine::DATA_KEY)
        .unwrap()
        .warnings()
        .is_empty());
}

#[test]
fn evidence_named_values_does_not_displace_the_engines_property_list() {
    let client = Arc::new(FakeClient::default());
    // The service advertises `query.values` as accepted evidence, so it can
    // reach the engine as evidence and would otherwise write a second `values`
    // field into the body, leaving which list applied to the request undecided.
    client.set_evidence_keys(FakeClient::ok(r#"["header.user-agent","query.values"]"#));
    client.set_data(FakeClient::ok(r#"{"device":{"ismobile":true}}"#));

    let engine = license_engine_with(client.clone(), &["device.ismobile"]);
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(engine))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", "UA")
            .add("query.values", "device.iscrawler")
            .build(),
    );
    data.process().unwrap();

    let requests = client.requests.lock().unwrap();
    let data_request = requests
        .iter()
        .find(|r| r.method == HttpMethod::Post)
        .expect("a POST to the data endpoint");
    let values: Vec<&String> = data_request
        .form
        .iter()
        .filter(|(k, _)| k == "values")
        .map(|(_, v)| v)
        .collect();
    assert_eq!(values.len(), 1, "one property list, got {values:?}");
    assert_eq!(values[0], "device.ismobile", "the engine's list wins");
}

/// A fake transport that answers each data request with the `user-agent` field
/// it was sent, so a test can tell which request a response was made for.
#[derive(Default)]
struct EchoClient {
    data_requests: Mutex<usize>,
}

impl CloudHttpClient for EchoClient {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        *self.data_requests.lock().unwrap() += 1;
        let user_agent = request
            .form
            .iter()
            .find(|(name, _)| name == "user-agent")
            .map(|(_, value)| value.clone())
            .unwrap_or_default();
        FakeClient::ok(&serde_json::json!({ "device": { "useragent": user_agent } }).to_string())
    }
}

#[test]
fn evidence_that_differs_never_shares_a_cache_entry() {
    // The web integration decodes the query string, so a query value can hold
    // a line break followed by text shaped like another evidence field. That
    // request must not be given the cache key of a visitor whose evidence
    // really holds both values, or one is served the answer made for the other.
    let client = Arc::new(EchoClient::default());
    let state = CloudEngineState {
        evidence_keys: [
            "header.user-agent",
            "query.user-agent",
            "header.x-device-user-agent",
        ]
        .into_iter()
        .map(|key| EvidenceKeyEntry {
            key: key.to_owned(),
            order: 0,
        })
        .collect(),
        accessible_properties: LicensedProducts::default(),
    };
    let engine = CloudRequestEngine::builder()
        .resource_key("rk")
        .http_client(client.clone())
        .set_state(state)
        .cache(Arc::new(RecordingCache::default()))
        .build()
        .unwrap();
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(engine))
        .build()
        .unwrap();

    let mut crafted = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("query.user-agent", "Mozilla/5.0\nx-device-user-agent=Other")
            .build(),
    );
    crafted.process().unwrap();

    let mut visitor = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", "Mozilla/5.0")
            .add("header.x-device-user-agent", "Other")
            .build(),
    );
    visitor.process().unwrap();

    let cloud = visitor.get(CloudRequestEngine::DATA_KEY).unwrap();
    assert_eq!(
        cloud.json_response(),
        Some(r#"{"device":{"useragent":"Mozilla/5.0"}}"#),
        "the visitor was served the answer made for the other request"
    );
    assert!(
        !cloud.cache_hit(),
        "the visitor's request is not a cache hit"
    );
    assert_eq!(
        *client.data_requests.lock().unwrap(),
        2,
        "each request reached the cloud"
    );
}
