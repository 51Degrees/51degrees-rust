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

//! Tests of the asynchronous path, being `async_http_client`, `build_async`
//! and `fetch`, driven against in-process clients so nothing reaches the
//! network. The asynchronous client waits once before it answers, so each
//! test proves the engine awaits it rather than finishing in one poll.

use std::future::Future;
use std::pin::pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use fiftyone_cloud_request_engine::{
    AsyncCloudHttpClient, CloudEngineState, CloudHttpClient, CloudHttpRequest, CloudHttpResponse,
    CloudRequestEngine, EvidenceKeyEntry, LicensedProducts, LocalBoxFuture,
};
use fiftyone_pipeline_core::{Error, Evidence, FlowData, Pipeline};

/// The base endpoint the engines are given. Nothing is sent to it.
const ENDPOINT: &str = "https://cloud.example.test/api/v4/";

/// The evidence keys the clients report the cloud accepts.
const EVIDENCE_KEYS: &str = r#"["header.user-agent","query.user-agent"]"#;

/// The accessible properties the clients report.
const PROPERTIES: &str = r#"{"Products":{}}"#;

/// The JSON the clients answer the data request with.
const DATA: &str = r#"{"device":{"ismobile":true}}"#;

/// The User-Agent sent as evidence.
const USER_AGENT: &str = "Mozilla/5.0 (async)";

/// Drive `future` to its end on this thread. The futures here wake
/// themselves before they wait, so polling again is enough.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
    }
}

/// A future that is pending on its first poll, so whoever awaits it must
/// come back.
struct WaitOnce(bool);

impl Future for WaitOnce {
    type Output = ();

    fn poll(mut self: std::pin::Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            Poll::Ready(())
        } else {
            self.0 = true;
            context.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

/// The scripted answers and the requests received, shared by both clients.
#[derive(Default)]
struct Script {
    requests: Mutex<Vec<CloudHttpRequest>>,
    /// The data request's answer, a success with [`DATA`] when unset.
    data: Mutex<Option<Result<CloudHttpResponse, String>>>,
    /// The discovery answers' status, 200 when unset.
    discovery_status: Mutex<Option<u16>>,
}

impl Script {
    fn answer(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        self.requests.lock().unwrap().push(request.clone());
        let ok = |status: u16, body: &str| {
            Ok(CloudHttpResponse {
                status,
                body: body.to_owned(),
                retry_after: None,
            })
        };
        let discovery = self.discovery_status.lock().unwrap().unwrap_or(200);
        if request.url.contains("evidencekeys") {
            ok(discovery, EVIDENCE_KEYS)
        } else if request.url.contains("accessibleproperties") {
            ok(discovery, PROPERTIES)
        } else {
            self.data
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| ok(200, DATA))
        }
    }

    fn urls(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.url.clone())
            .collect()
    }
}

/// An asynchronous client that waits once and then answers from its script.
#[derive(Default)]
struct AsyncClient(Script);

impl AsyncCloudHttpClient for AsyncClient {
    fn send<'a>(
        &'a self,
        request: &'a CloudHttpRequest,
    ) -> LocalBoxFuture<'a, Result<CloudHttpResponse, String>> {
        Box::pin(async move {
            WaitOnce(false).await;
            self.0.answer(request)
        })
    }
}

/// A blocking client that answers from its script.
#[derive(Default)]
struct BlockingClient(Script);

impl CloudHttpClient for BlockingClient {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        self.0.answer(request)
    }
}

fn builder() -> fiftyone_cloud_request_engine::CloudRequestEngineBuilder {
    CloudRequestEngine::builder()
        .resource_key("test-resource-key")
        .endpoint(ENDPOINT)
}

fn async_engine(client: &Arc<AsyncClient>) -> Arc<CloudRequestEngine> {
    Arc::new(
        block_on(builder().async_http_client(client.clone()).build_async())
            .expect("the engine builds"),
    )
}

fn flow_data(engine: &Arc<CloudRequestEngine>, suppress: bool) -> FlowData {
    let pipeline = Pipeline::builder()
        .add_element(engine.clone())
        .suppress_process_exceptions(suppress)
        .build()
        .unwrap();
    pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", USER_AGENT)
            .build(),
    )
}

fn saved_state() -> CloudEngineState {
    CloudEngineState {
        evidence_keys: vec![EvidenceKeyEntry {
            key: "header.user-agent".to_owned(),
            order: 0,
        }],
        accessible_properties: LicensedProducts::default(),
    }
}

#[test]
fn build_async_discovers_and_fetch_answers_processing() {
    let client = Arc::new(AsyncClient::default());
    let engine = async_engine(&client);
    assert!(engine.has_loaded_metadata());

    let mut data = flow_data(&engine, false);
    block_on(engine.fetch(&mut data));
    data.process().unwrap();

    let cloud = data.get(CloudRequestEngine::DATA_KEY).unwrap();
    assert_eq!(cloud.json_response(), Some(DATA));
    assert!(cloud.process_started());
    assert_eq!(
        client.0.urls(),
        vec![
            format!("{ENDPOINT}evidencekeys"),
            format!("{ENDPOINT}accessibleproperties?resource=test-resource-key"),
            format!("{ENDPOINT}json"),
        ]
    );
}

#[test]
fn fetch_sends_the_same_request_as_processing_with_a_blocking_client() {
    let blocking = Arc::new(BlockingClient::default());
    let engine = Arc::new(builder().http_client(blocking.clone()).build().unwrap());
    let mut data = flow_data(&engine, false);
    data.process().unwrap();

    let client = Arc::new(AsyncClient::default());
    let async_engine = async_engine(&client);
    let mut async_data = flow_data(&async_engine, false);
    block_on(async_engine.fetch(&mut async_data));
    async_data.process().unwrap();

    let blocking_requests = blocking.0.requests.lock().unwrap().clone();
    let async_requests = client.0.requests.lock().unwrap().clone();
    assert_eq!(blocking_requests.len(), async_requests.len());
    for (blocking, sent) in blocking_requests.iter().zip(&async_requests) {
        assert_eq!(blocking.method, sent.method);
        assert_eq!(blocking.url, sent.url);
        assert_eq!(blocking.form, sent.form);
        assert_eq!(blocking.origin, sent.origin);
    }
}

#[test]
fn processing_without_fetch_names_fetch_and_sends_nothing() {
    let client = Arc::new(AsyncClient::default());
    let engine = async_engine(&client);
    let sent = client.0.requests.lock().unwrap().len();

    let mut data = flow_data(&engine, false);
    let error = data.process().unwrap_err().to_string();

    assert!(error.contains("CloudRequestEngine::fetch"), "{error}");
    assert_eq!(client.0.requests.lock().unwrap().len(), sent);
}

#[test]
fn build_with_only_an_async_client_names_build_async_unless_a_state_is_saved() {
    let client = Arc::new(AsyncClient::default());
    let error = match builder().async_http_client(client.clone()).build() {
        Ok(_) => panic!("the build needs build_async"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("build_async"), "{error}");
    assert!(client.0.requests.lock().unwrap().is_empty());

    let engine = builder()
        .async_http_client(client.clone())
        .set_state(saved_state())
        .build()
        .unwrap();
    assert!(engine.has_loaded_metadata());
    assert!(client.0.requests.lock().unwrap().is_empty());
}

#[test]
fn a_failed_fetch_is_returned_by_processing_and_recorded_when_suppressed() {
    let client = Arc::new(AsyncClient::default());
    *client.0.data.lock().unwrap() = Some(Err("the connection was refused".to_owned()));
    let engine = async_engine(&client);

    let mut data = flow_data(&engine, false);
    block_on(engine.fetch(&mut data));
    let error = data.process().unwrap_err().to_string();
    assert!(error.contains("the connection was refused"), "{error}");

    let mut data = flow_data(&engine, true);
    block_on(engine.fetch(&mut data));
    data.process().unwrap();
    assert_eq!(data.errors().len(), 1);
    match &data.errors()[0].source {
        Error::CloudRequest { status_code, .. } => assert_eq!(*status_code, 0),
        other => panic!("unexpected error {other}"),
    }
}

#[test]
fn fetch_retries_discovery_that_failed_at_build_time() {
    let client = Arc::new(AsyncClient::default());
    *client.0.discovery_status.lock().unwrap() = Some(503);
    let engine = async_engine(&client);
    assert!(!engine.has_loaded_metadata());

    *client.0.discovery_status.lock().unwrap() = None;
    let mut data = flow_data(&engine, false);
    block_on(engine.fetch(&mut data));
    data.process().unwrap();

    assert!(engine.has_loaded_metadata());
    let cloud = data.get(CloudRequestEngine::DATA_KEY).unwrap();
    assert_eq!(cloud.json_response(), Some(DATA));
}

#[test]
fn build_async_fails_on_a_4xx_discovery_answer() {
    let client = Arc::new(AsyncClient::default());
    *client.0.discovery_status.lock().unwrap() = Some(403);
    let result = block_on(builder().async_http_client(client.clone()).build_async());
    match result {
        Err(Error::CloudRequest { status_code, .. }) => assert_eq!(status_code, 403),
        Err(other) => panic!("unexpected error {other}"),
        Ok(_) => panic!("a 4xx discovery answer must fail the build"),
    }
}

#[test]
fn a_second_fetch_and_a_fetch_after_processing_send_nothing() {
    let client = Arc::new(AsyncClient::default());
    let engine = async_engine(&client);
    let mut data = flow_data(&engine, false);
    block_on(engine.fetch(&mut data));
    let sent = client.0.requests.lock().unwrap().len();

    block_on(engine.fetch(&mut data));
    assert_eq!(client.0.requests.lock().unwrap().len(), sent);

    data.process().unwrap();
    block_on(engine.fetch(&mut data));
    assert_eq!(client.0.requests.lock().unwrap().len(), sent);
}

#[test]
fn fetch_uses_the_blocking_client_when_there_is_no_async_one() {
    let blocking = Arc::new(BlockingClient::default());
    let engine = Arc::new(builder().http_client(blocking.clone()).build().unwrap());
    let mut data = flow_data(&engine, false);
    block_on(engine.fetch(&mut data));
    let sent = blocking.0.requests.lock().unwrap().len();
    data.process().unwrap();

    assert_eq!(blocking.0.requests.lock().unwrap().len(), sent);
    let cloud = data.get(CloudRequestEngine::DATA_KEY).unwrap();
    assert_eq!(cloud.json_response(), Some(DATA));
}
