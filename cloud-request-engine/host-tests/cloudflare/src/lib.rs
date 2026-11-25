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

//! The Worker test.mjs calls. Each request builds a cloud request engine
//! over `FetchClient` with `build_async`, which fetches the evidence keys
//! and accessible properties from the stub cloud server, then awaits
//! `fetch` with one flow data, processes it through the request engine and
//! the device detection cloud engine, and answers with a JSON report of the
//! device element data or the error.
//!
//! The query string names the stub server:
//!
//! - `cloud`, the base endpoint the engine asks.
//! - `data`, a data endpoint in place of the one under `cloud`, optional.
//! - `timeout_ms`, the client's timeout in milliseconds, optional.
//!
//! wasm32-unknown-unknown has no clock in its standard library, so a read
//! of `std::time::Instant` there panics. The worker crate's panic hook
//! writes the message to the log test.mjs prints.

use std::sync::Arc;
use std::time::Duration;

use fiftyone_cloud_request_engine::cloudflare::FetchClient;
use fiftyone_cloud_request_engine::CloudRequestEngine;
use fiftyone_device_detection_cloud::DeviceDetectionCloudEngine;
use fiftyone_device_detection_shared::{DeviceData, DEVICE_DATA_KEY};
use fiftyone_pipeline_core::{Evidence, Pipeline};
use serde_json::{json, Value};
use worker::{event, Context, Env, Request, Response, Result};

// Nothing here calls getrandom. It is named so its JavaScript backend, which
// ahash draws its seed from, is linked into the Worker.
use getrandom as _;

/// A made-up resource key, which the stub server accepts.
const RESOURCE_KEY: &str = "example-resource-key";

/// The User-Agent sent as evidence.
const USER_AGENT: &str = "Mozilla/5.0 (example)";

/// The client's timeout when the query string names none, in milliseconds.
const TIMEOUT_DEFAULT_MS: u64 = 5000;

/// What the Worker was asked to do, from the query string.
struct Case {
    cloud: String,
    data: Option<String>,
    timeout: Duration,
}

impl Case {
    fn from(req: &Request) -> Result<Case> {
        let url = req.url()?;
        let param = |name: &str| {
            url.query_pairs()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.into_owned())
        };
        let timeout_ms = param("timeout_ms")
            .and_then(|ms| ms.parse().ok())
            .unwrap_or(TIMEOUT_DEFAULT_MS);
        Ok(Case {
            cloud: param("cloud").ok_or("the query string names no cloud")?,
            data: param("data"),
            timeout: Duration::from_millis(timeout_ms),
        })
    }
}

/// Builds the engines, makes the request and processes one flow data,
/// reporting the device element data or the first error.
async fn run(case: Case) -> Value {
    let mut builder = CloudRequestEngine::builder()
        .resource_key(RESOURCE_KEY)
        .endpoint(case.cloud)
        .async_http_client(Arc::new(FetchClient::new(case.timeout)));
    if let Some(data) = case.data {
        builder = builder.data_endpoint(data);
    }
    let engine = match builder.build_async().await {
        Ok(engine) => Arc::new(engine),
        Err(error) => return json!({ "stage": "build", "error": error.to_string() }),
    };
    let device = DeviceDetectionCloudEngine::builder()
        .cloud_request_engine(Arc::clone(&engine))
        .build();
    let pipeline = match Pipeline::builder()
        .add_element(engine.clone())
        .add_element(Arc::new(device))
        .build()
    {
        Ok(pipeline) => pipeline,
        Err(error) => return json!({ "stage": "pipeline", "error": error.to_string() }),
    };
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", USER_AGENT)
            .build(),
    );
    engine.fetch(&mut data).await;
    if let Err(error) = data.process() {
        return json!({ "stage": "process", "error": error.to_string() });
    }
    match data.get(DEVICE_DATA_KEY) {
        Some(device) => json!({
            "stage": "done",
            "ismobile": device.is_mobile().as_option(),
            "hardwarevendor": device.hardware_vendor().as_option(),
        }),
        None => json!({ "stage": "done", "error": "the flow data holds no device data" }),
    }
}

#[event(fetch)]
async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    let case = Case::from(&req)?;
    Response::from_json(&run(case).await)
}
