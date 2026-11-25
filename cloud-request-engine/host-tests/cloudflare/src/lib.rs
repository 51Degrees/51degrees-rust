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

//! The routes test.mjs calls. Each answers `ok` or says what went wrong.
//!
//! - `/process` builds a cloud request engine over a stub client and
//!   processes one flow data, which reads the clock before the request.
//! - `/failure` does the same with a stub whose data request fails, so the
//!   engine records the failure against the clock as well.
//!
//! wasm32-unknown-unknown has no clock in its standard library, so a read
//! of `std::time::Instant` there panics. The worker crate's panic hook
//! writes the message to the log test.mjs prints.

use std::sync::Arc;

use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine,
};
use fiftyone_pipeline_core::{Evidence, Pipeline};
use worker::{event, Context, Env, Request, Response, Result};

// Nothing here calls getrandom. It is named so its JavaScript backend, which
// ahash draws its seed from, is linked into the Worker.
use getrandom as _;

/// A made-up resource key. The stub client answers every request, so it is
/// never checked.
const RESOURCE_KEY: &str = "example-resource-key";

/// An endpoint no request reaches, since the stub client answers in place.
const ENDPOINT: &str = "https://cloud.example.test/api/v4/";

/// The evidence keys the stub reports the cloud accepts.
const EVIDENCE_KEYS: &str = r#"["header.user-agent","query.user-agent"]"#;

/// The accessible properties the stub reports, which list no product.
const PROPERTIES: &str = r#"{"Products":{}}"#;

/// The JSON the stub answers the data request with.
const DATA: &str = r#"{"device":{"ismobile":true}}"#;

/// The User-Agent sent as evidence.
const USER_AGENT: &str = "Mozilla/5.0 (example)";

/// The HTTP status of a successful response.
const STATUS_OK: u16 = 200;

/// The message the stub gives when its data request fails.
const TRANSPORT_FAILURE: &str = "the stub refused the request";

/// A client that answers the engine's requests in the Worker, so the test
/// needs no network and no key.
struct Stub {
    /// Whether the data request fails as a transport failure would.
    fail_data: bool,
}

impl Stub {
    fn ok(body: &str) -> std::result::Result<CloudHttpResponse, String> {
        Ok(CloudHttpResponse {
            status: STATUS_OK,
            body: body.to_owned(),
            retry_after: None,
        })
    }
}

impl CloudHttpClient for Stub {
    fn send(&self, request: &CloudHttpRequest) -> std::result::Result<CloudHttpResponse, String> {
        if request.url.contains("evidencekeys") {
            Stub::ok(EVIDENCE_KEYS)
        } else if request.url.contains("accessibleproperties") {
            Stub::ok(PROPERTIES)
        } else if self.fail_data {
            Err(TRANSPORT_FAILURE.to_owned())
        } else {
            Stub::ok(DATA)
        }
    }
}

/// Builds an engine over the stub and processes one flow data, answering
/// `ok` when the outcome is the one expected or what went wrong.
fn process(fail_data: bool) -> String {
    let engine = CloudRequestEngine::builder()
        .resource_key(RESOURCE_KEY)
        .endpoint(ENDPOINT)
        .http_client(Arc::new(Stub { fail_data }))
        .build();
    let engine = match engine {
        Ok(engine) => engine,
        Err(error) => return format!("the engine was not built: {error}"),
    };
    let pipeline = match Pipeline::builder().add_element(Arc::new(engine)).build() {
        Ok(pipeline) => pipeline,
        Err(error) => return format!("the pipeline was not built: {error}"),
    };
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", USER_AGENT)
            .build(),
    );
    let processed = data.process();
    match (fail_data, processed) {
        (false, Ok(_)) => match data.get(CloudRequestEngine::DATA_KEY) {
            Some(cloud) if cloud.json_response() == Some(DATA) => "ok".to_owned(),
            Some(cloud) => format!("the response read {:?}", cloud.json_response()),
            None => "the flow data holds no cloud response".to_owned(),
        },
        (false, Err(error)) => format!("the request failed: {error}"),
        (true, Ok(_)) => "the failed request was not reported".to_owned(),
        (true, Err(error)) if error.to_string().contains(TRANSPORT_FAILURE) => "ok".to_owned(),
        (true, Err(error)) => format!("the failure read {error}"),
    }
}

#[event(fetch)]
async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    match req.url()?.path() {
        "/process" => Response::ok(process(false)),
        "/failure" => Response::ok(process(true)),
        _ => Response::error("not found", 404),
    }
}
