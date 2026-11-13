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

//! End-to-end tests over the real blocking [`reqwest`] transport, against a
//! local [`tiny_http`] server. These confirm the url-encoded form body, the
//! prefix stripping seen on the wire, and the `Origin` header.
//!
//! Gated on the `reqwest-client` feature: these build a `CloudRequestEngine`
//! without supplying a client, so they exercise the built-in reqwest transport,
//! which is only compiled with that feature.
#![cfg(feature = "reqwest-client")]

#[allow(dead_code)]
#[path = "../../deprecated-names/environment.rs"]
mod deprecated_names;

use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

use fiftyone_cloud_request_engine::{CloudEngineState, CloudRequestEngine};
use fiftyone_pipeline_core::{Evidence, Pipeline};

/// What the test server captured from the data POST.
struct Captured {
    body: String,
    origin: Option<String>,
    user_agent: Option<String>,
}

/// Start a local server that answers the three cloud endpoints and reports back
/// what it received on the data POST. The builder fetches `evidencekeys` and
/// `accessibleproperties` as it builds the engine, so both are answered here;
/// only the data POST is captured. Returns the base URL and a receiver for the
/// captured request.
fn start_server(
    evidence_keys_json: &'static str,
    accessible_properties_json: &'static str,
    data_json: &'static str,
) -> (String, mpsc::Receiver<Captured>) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        for mut request in server.incoming_requests() {
            let url = request.url().to_owned();
            if url.contains("evidencekeys") {
                let response = tiny_http::Response::from_string(evidence_keys_json);
                let _ = request.respond(response);
            } else if url.contains("accessibleproperties") {
                let response = tiny_http::Response::from_string(accessible_properties_json);
                let _ = request.respond(response);
            } else {
                // The data endpoint: capture the body and the Origin header.
                let origin = request
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv("Origin"))
                    .map(|h| h.value.as_str().to_owned());
                let user_agent = request
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv("User-Agent"))
                    .map(|h| h.value.as_str().to_owned());
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);
                let _ = tx.send(Captured {
                    body,
                    origin,
                    user_agent,
                });
                let response = tiny_http::Response::from_string(data_json);
                let _ = request.respond(response);
            }
        }
    });

    (format!("http://127.0.0.1:{port}/"), rx)
}

#[test]
fn posts_url_encoded_form_with_stripped_prefixes_and_origin() {
    let (base, rx) = start_server(
        r#"["header.user-agent","query.user-agent"]"#,
        r#"{"Products":{}}"#,
        r#"{"device":{"ismobile":true}}"#,
    );

    let engine = CloudRequestEngine::builder()
        .resource_key("live-resource-key")
        .endpoint(base)
        .cloud_request_origin("https://example.com")
        .timeout_seconds(5)
        .build()
        .unwrap();

    let pipeline = Pipeline::builder()
        .add_element(Arc::new(engine))
        .build()
        .unwrap();

    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", "header-ua")
            .add("query.user-agent", "query-ua")
            .build(),
    );
    data.process().unwrap();

    // The raw JSON came back and is stored under the cloud key.
    let cloud = data.get(CloudRequestEngine::DATA_KEY).unwrap();
    assert_eq!(
        cloud.json_response(),
        Some(r#"{"device":{"ismobile":true}}"#)
    );

    let captured = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
    // url-encoded form: resource key present.
    assert!(
        captured.body.contains("resource=live-resource-key"),
        "body was: {}",
        captured.body
    );
    // The prefix is stripped on the wire (no `header.` / `query.`), and the
    // query value wins the precedence conflict over the header value.
    assert!(
        captured.body.contains("user-agent=query-ua"),
        "body was: {}",
        captured.body
    );
    assert!(
        !captured.body.contains("header.user-agent"),
        "prefix should be stripped, body was: {}",
        captured.body
    );
    assert!(
        !captured.body.contains("header-ua"),
        "query value should win"
    );
    // The Origin header was sent.
    assert_eq!(captured.origin.as_deref(), Some("https://example.com"));
    // The end user's User-Agent travels only as evidence in the body, never in
    // the request's own User-Agent header. The cloud service treats a request
    // whose own header names a browser as a browser page, and such a page gets
    // no 51Did until it has returned the results of every snippet.
    assert!(
        captured
            .user_agent
            .as_deref()
            .is_none_or(|ua| !ua.contains("query-ua") && !ua.contains("header-ua")),
        "the evidence User-Agent must not be sent as the HTTP User-Agent, got {:?}",
        captured.user_agent
    );
}

#[test]
fn transport_failure_to_dead_endpoint_builds_and_leaves_discovery_to_first_use() {
    // Point at a port nothing is listening on. The builder's discovery fetch
    // fails at the transport level, which is transient, so the build completes
    // and the engine has no discovery results until a process call fetches
    // them.
    let engine = CloudRequestEngine::builder()
        .resource_key("rk")
        .endpoint("http://127.0.0.1:1/") // port 1: refused
        .timeout_seconds(2)
        .build()
        .expect("a transport failure during discovery does not fail the build");
    assert!(!engine.has_loaded_metadata());
    assert!(
        matches!(
            engine.accepted_evidence_keys(),
            Err(fiftyone_pipeline_core::Error::CloudRequest { status_code: 0, .. })
        ),
        "the evidence keys are not known"
    );
}

#[test]
fn state_round_trips_over_the_real_transport() {
    // A representative accessible-properties body so the exported state carries a
    // product and properties, the way a real resource key would.
    const ACCESSIBLE_PROPERTIES: &str = r#"{"Products":{"device":{"DataTier":"CloudV4","Properties":[{"Name":"IsMobile","Type":"Bool"},{"Name":"PlatformName","Type":"String"}]}}}"#;
    const EVIDENCE_KEYS: &str = r#"["header.user-agent","query.user-agent"]"#;

    let (base, _rx) = start_server(
        EVIDENCE_KEYS,
        ACCESSIBLE_PROPERTIES,
        r#"{"device":{"ismobile":true}}"#,
    );

    // Builder 1: it fetches discovery from the (local) cloud at build time over
    // the real reqwest transport, and retains the state for export.
    let mut builder1 = CloudRequestEngine::builder()
        .resource_key("live-resource-key")
        .endpoint(base)
        .timeout_seconds(5);
    let _engine1 = builder1
        .build()
        .expect("the first engine builds, fetching discovery from the cloud");
    let state1 = builder1
        .export_state()
        .expect("the builder holds the fetched state");
    // The state genuinely came from the cloud: it carries the advertised product.
    assert!(
        state1.accessible_properties.products.contains_key("device"),
        "the builder should have fetched accessible properties from the cloud"
    );
    assert!(!state1.evidence_keys.is_empty());

    // Persist and restore the state, as a host store would.
    let json = serde_json::to_string(&state1).unwrap();
    let restored: CloudEngineState = serde_json::from_str(&json).unwrap();

    // Builder 2: the state is injected and the endpoint points at a dead port. If
    // the builder tried to fetch discovery it would fail; that the build succeeds
    // proves it used the injected state and did not call the cloud.
    let mut builder2 = CloudRequestEngine::builder()
        .resource_key("live-resource-key")
        .endpoint("http://127.0.0.1:1/")
        .timeout_seconds(2)
        .set_state(restored);
    let _engine2 = builder2
        .build()
        .expect("the second engine builds from the injected state without any network call");
    let state2 = builder2
        .export_state()
        .expect("the builder holds the state");

    // The two snapshots are identical.
    assert_eq!(state1.evidence_keys, state2.evidence_keys);
    assert_eq!(
        serde_json::to_string(&state1.accessible_properties).unwrap(),
        serde_json::to_string(&state2.accessible_properties).unwrap(),
        "the re-injected engine exports the same accessible properties"
    );
}

/// Resolve a cloud resource key from the environment for the live round-trip
/// test, checking the aligned name first and then the CI-exported tiered names.
fn live_resource_key() -> Option<String> {
    [
        "_51DEGREES_RESOURCE_KEY",
        "_51DEGREES_RESOURCE_KEY_BESPOKE",
        "_51DEGREES_RESOURCE_KEY_BASIC",
    ]
    .into_iter()
    .find_map(|name| match crate::deprecated_names::var(name) {
        Ok(value) if !value.trim().is_empty() => Some(value.trim().to_owned()),
        _ => None,
    })
}

/// The end-to-end feature against the real 51Degrees cloud: build with a resource
/// key so the builder fetches the state from the cloud, persist it, build a
/// second engine with that state injected, and confirm the two states match and
/// the second build makes no cloud call. Ignored by default; runs only when a
/// resource key is present in the environment.
#[test]
#[ignore = "requires a network and a real resource key (_51DEGREES_RESOURCE_KEY or the _51DEGREES_RESOURCE_KEY_BESPOKE/_BASIC tiered names)"]
fn live_cloud_state_round_trips() {
    let Some(resource_key) = live_resource_key() else {
        eprintln!("no resource key in the environment; skipping live cloud state round-trip");
        return;
    };

    // First build hits the real cloud and the builder retains the resolved state.
    let mut builder1 = CloudRequestEngine::builder()
        .resource_key(resource_key.clone())
        .timeout_seconds(10);
    let _engine1 = builder1
        .build()
        .expect("the first engine builds, fetching discovery from the real cloud");
    let state1 = builder1
        .export_state()
        .expect("the builder holds the fetched state");
    assert!(
        !state1.evidence_keys.is_empty(),
        "the cloud should advertise at least one accepted evidence key"
    );
    assert!(
        !state1.accessible_properties.products.is_empty(),
        "the cloud should advertise at least one accessible product"
    );

    // Persist and restore, then build a second engine from the cached state with
    // the endpoint pointed at a dead port, so a build that succeeds proves no
    // cloud call was made.
    let json = serde_json::to_string(&state1).unwrap();
    let restored: CloudEngineState = serde_json::from_str(&json).unwrap();
    let mut builder2 = CloudRequestEngine::builder()
        .resource_key(resource_key)
        .endpoint("http://127.0.0.1:1/")
        .timeout_seconds(2)
        .set_state(restored);
    let _engine2 = builder2
        .build()
        .expect("the second engine builds from the cached state without any cloud call");
    let state2 = builder2
        .export_state()
        .expect("the builder holds the state");

    assert_eq!(state1.evidence_keys, state2.evidence_keys);
    assert_eq!(
        serde_json::to_string(&state1.accessible_properties).unwrap(),
        serde_json::to_string(&state2.accessible_properties).unwrap(),
    );
}

/// Resolve a cloud license key from the environment for the live license-key
/// test. A license key is the server-side credential, so it is read from the
/// same variable the data-file update example uses.
fn live_license_key() -> Option<String> {
    match crate::deprecated_names::var("_51DEGREES_LICENSE_KEY") {
        Ok(value) if !value.trim().is_empty() => Some(value.trim().to_owned()),
        _ => None,
    }
}

/// The license-key path against the real 51Degrees cloud: authenticate on a
/// license key alone, name the properties wanted, and confirm the answer carries
/// them. Also confirms the silent drop is caught, by asking for a property no
/// subscription covers alongside one that is covered, which the service answers
/// `200` without naming what it left out.
///
/// Ignored by default. It runs only when a license key covering
/// `device.ismobile` is present in the environment.
#[test]
#[ignore = "requires a network and a real license key (_51DEGREES_LICENSE_KEY)"]
fn live_license_key_names_the_properties_it_wants() {
    let Some(license_key) = live_license_key() else {
        eprintln!("no license key in the environment; skipping the live license-key test");
        return;
    };

    // `no-such-property` is not a property of any product, so the service drops
    // it whatever the subscription covers, which is the case being exercised.
    let engine = CloudRequestEngine::builder()
        .license_key(license_key)
        .values(["device.ismobile", "device.no-such-property"])
        .timeout_seconds(10)
        .build()
        .expect("an engine authenticating on a license key alone builds");

    let pipeline = Pipeline::builder()
        .add_element(Arc::new(engine))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add(
                "header.user-agent",
                "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
                 AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 \
                 Mobile/15E148 Safari/604.1",
            )
            .build(),
    );
    data.process().expect("the live request succeeds");

    let cloud = data.get(CloudRequestEngine::DATA_KEY).unwrap();
    let json = cloud.json_response().expect("a JSON response");
    let body: serde_json::Value = serde_json::from_str(json).unwrap();
    assert_eq!(
        body["device"]["ismobile"],
        serde_json::Value::Bool(true),
        "the asked-for property should be in the answer, got {json}"
    );

    // The dropped property is named once, and the request itself stands.
    let warnings = cloud.warnings();
    assert_eq!(warnings.len(), 1, "one warning, got {warnings:?}");
    assert!(
        warnings[0].contains("device.no-such-property"),
        "the warning should name the property that did not come back, got {}",
        warnings[0]
    );
}
