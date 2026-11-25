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

//! [`FetchClient`], an [`AsyncCloudHttpClient`] over the Workers `fetch`,
//! for `wasm32-unknown-unknown` on Cloudflare Workers, turned on by the
//! `cloudflare` feature.
//!
//! A Worker cannot block on a request, so it builds the engine with
//! [`CloudRequestEngineBuilder::build_async`](crate::CloudRequestEngineBuilder::build_async)
//! and awaits [`CloudRequestEngine::fetch`](crate::CloudRequestEngine::fetch)
//! with each flow data before processing it.
//!
//! ```no_run
//! use std::sync::Arc;
//! use std::time::Duration;
//!
//! use fiftyone_cloud_request_engine::cloudflare::FetchClient;
//! use fiftyone_cloud_request_engine::CloudRequestEngine;
//! use fiftyone_pipeline_core::{Evidence, Pipeline};
//!
//! # async fn run(resource_key: &str, user_agent: &str) -> fiftyone_pipeline_core::Result<()> {
//! let engine = Arc::new(
//!     CloudRequestEngine::builder()
//!         .resource_key(resource_key)
//!         .async_http_client(Arc::new(FetchClient::new(Duration::from_secs(2))))
//!         .build_async()
//!         .await?,
//! );
//! let pipeline = Pipeline::builder().add_element(engine.clone()).build()?;
//! let mut data = pipeline.create_flow_data_with(
//!     Evidence::builder().add("header.user-agent", user_agent).build(),
//! );
//! engine.fetch(&mut data).await;
//! data.process()?;
//! # Ok(())
//! # }
//! ```

use std::future::{poll_fn, Future};
use std::pin::pin;
use std::task::Poll;
use std::time::Duration;

use fiftyone_pipeline_core::redact::redact;
use worker::wasm_bindgen::JsValue;
use worker::{AbortController, Delay, Fetch, Headers, Method, Request, RequestInit};

use crate::constants;
use crate::engine::{encode_query_value, url_without_query};
use crate::http::{
    AsyncCloudHttpClient, CloudHttpRequest, CloudHttpResponse, HttpMethod, LocalBoxFuture,
};

/// The header a request names its body's type with.
const CONTENT_TYPE_HEADER: &str = "Content-Type";

/// The content type of a form body.
const FORM_CONTENT_TYPE: &str = "application/x-www-form-urlencoded";

/// The header a server names a retry hint with.
const RETRY_AFTER_HEADER: &str = "Retry-After";

/// An [`AsyncCloudHttpClient`] that sends each request with the Workers
/// `fetch`, giving up on a request that has not answered within its
/// timeout.
#[derive(Debug, Clone)]
pub struct FetchClient {
    timeout: Duration,
}

impl FetchClient {
    /// A client that gives up on a request after `timeout`, where zero
    /// means no limit.
    pub fn new(timeout: Duration) -> Self {
        FetchClient { timeout }
    }
}

impl Default for FetchClient {
    /// A client with the engine's default timeout.
    fn default() -> Self {
        FetchClient::new(Duration::from_secs(constants::TIMEOUT_DEFAULT_SECONDS))
    }
}

impl AsyncCloudHttpClient for FetchClient {
    fn send<'a>(
        &'a self,
        request: &'a CloudHttpRequest,
    ) -> LocalBoxFuture<'a, Result<CloudHttpResponse, String>> {
        Box::pin(send(request, self.timeout))
    }
}

/// Send `request`, and read the whole answer, within `timeout`.
async fn send(request: &CloudHttpRequest, timeout: Duration) -> Result<CloudHttpResponse, String> {
    // A discovery request carries the keys in its query string, so messages
    // name the endpoint without it, and the whole message is cleaned too.
    let endpoint = url_without_query(&request.url);
    let failed = |what: &str, error: worker::Error| {
        redact(&format!("failed to {what} '{endpoint}': {error}")).into_owned()
    };

    let headers = Headers::new();
    let mut init = RequestInit::new();
    match request.method {
        HttpMethod::Get => {
            init.with_method(Method::Get);
        }
        HttpMethod::Post => {
            headers
                .set(CONTENT_TYPE_HEADER, FORM_CONTENT_TYPE)
                .map_err(|e| failed("build the request to", e))?;
            init.with_method(Method::Post)
                .with_body(Some(JsValue::from_str(&encode_form(&request.form))));
        }
    }
    if let Some(origin) = &request.origin {
        headers
            .set(constants::ORIGIN_HEADER_NAME, origin)
            .map_err(|e| failed("build the request to", e))?;
    }
    init.with_headers(headers);
    let fetch = Fetch::Request(
        Request::new_with_init(&request.url, &init)
            .map_err(|e| failed("build the request to", e))?,
    );

    let controller = AbortController::default();
    let signal = controller.signal();
    let answer = async {
        let mut response = fetch
            .send_with_signal(&signal)
            .await
            .map_err(|e| failed("send request to", e))?;
        let status = response.status_code();
        let retry_after = response.headers().get(RETRY_AFTER_HEADER).ok().flatten();
        let body = response
            .text()
            .await
            .map_err(|e| failed("read response body from", e))?;
        Ok(CloudHttpResponse {
            status,
            body,
            retry_after,
        })
    };
    match within(answer, timeout).await {
        Some(answer) => answer,
        None => {
            // Stop the request the runtime is still making.
            controller.abort();
            Err(format!(
                "the request to '{endpoint}' timed out after {} ms",
                timeout.as_millis()
            ))
        }
    }
}

/// The output of `future`, or [`None`] when `timeout` passes first. A zero
/// timeout means no limit.
async fn within<F: Future>(future: F, timeout: Duration) -> Option<F::Output> {
    if timeout.is_zero() {
        return Some(future.await);
    }
    let mut future = pin!(future);
    let mut delay = pin!(Delay::from(timeout));
    poll_fn(|cx| {
        if let Poll::Ready(output) = future.as_mut().poll(cx) {
            Poll::Ready(Some(output))
        } else if delay.as_mut().poll(cx).is_ready() {
            Poll::Ready(None)
        } else {
            Poll::Pending
        }
    })
    .await
}

/// `form` as a url-encoded body.
fn encode_form(form: &[(String, String)]) -> String {
    form.iter()
        .map(|(name, value)| format!("{}={}", encode_query_value(name), encode_query_value(value)))
        .collect::<Vec<_>>()
        .join("&")
}
