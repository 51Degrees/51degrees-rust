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

//! The HTTP transport the engine uses to talk to the cloud service.
//!
//! The transport is abstracted behind the [`CloudHttpClient`] trait so the
//! engine logic (evidence filtering, response parsing, recovery) can be unit
//! tested without reaching the network. When the `reqwest-client` feature is
//! enabled, the crate provides `ReqwestClient`, a built-in transport wrapping a
//! blocking reqwest client. Without the feature (for example on a target where
//! reqwest does not build, such as `wasm32-wasip1`) a consumer supplies its own
//! [`CloudHttpClient`].
//!
//! A transport returns a [`CloudHttpResponse`] for any request that completed,
//! whatever the status code, so the engine can apply the cloud error rules to
//! non-success responses. A transport returns `Err` only when the request did
//! not complete at all (a connection failure, a timeout, a DNS error and so
//! on), which the engine maps to an [`fiftyone_pipeline_core::Error::CloudRequest`]
//! with a status code of zero.

#[cfg(feature = "reqwest-client")]
use std::time::Duration;

#[cfg(feature = "reqwest-client")]
use fiftyone_pipeline_core::redact::redact;

/// The outcome of a completed HTTP request to the cloud service.
///
/// "Completed" means a response was received, regardless of its status code.
/// The engine inspects [`CloudHttpResponse::status`] and the body to decide
/// whether the response represents success or a cloud error.
#[derive(Debug, Clone)]
pub struct CloudHttpResponse {
    /// The HTTP status code.
    pub status: u16,
    /// The response body, read in full as a string. Cloud responses are JSON or,
    /// for transport-level failures surfaced by the service, plain text.
    pub body: String,
    /// The value of the `Retry-After` header, if the service supplied one. This
    /// is parsed by the engine into a retry hint, chiefly for `429` responses.
    pub retry_after: Option<String>,
}

impl CloudHttpResponse {
    /// True if the status code is in the 2xx success range.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// The HTTP method used for a cloud request.
///
/// The data endpoint is always reached with [`HttpMethod::Post`] (the evidence
/// travels as a url-encoded form body). The discovery endpoints
/// (`evidencekeys`, `accessibleproperties`) are reached with
/// [`HttpMethod::Get`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    /// An HTTP GET, used for the discovery endpoints.
    Get,
    /// An HTTP POST, used for the data endpoint with url-encoded form data.
    Post,
}

/// A single request the engine asks a [`CloudHttpClient`] to perform.
#[derive(Debug, Clone)]
pub struct CloudHttpRequest {
    /// The HTTP method.
    pub method: HttpMethod,
    /// The absolute URL to request. On a discovery request this carries the
    /// resource key as a query parameter, so it must not be copied into an
    /// error message as it stands.
    pub url: String,
    /// The url-encoded form fields to send as the POST body. Empty for GET
    /// requests. The transport is responsible for url-encoding these. The
    /// `resource` and `license` fields are credentials.
    pub form: Vec<(String, String)>,
    /// The value to set the `Origin` header to, if a cloud-request origin is
    /// configured. `None` leaves the header unset.
    pub origin: Option<String>,
}

/// The transport used to send requests to the cloud service.
///
/// Implementations MUST be `Send + Sync` so a single engine instance can serve
/// many concurrent flow data, matching the
/// [thread-safety specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/thread-safety.md#flow-elements).
pub trait CloudHttpClient: Send + Sync {
    /// Send `request` and return the [`CloudHttpResponse`] for any response that
    /// was received. Return `Err` with a human-readable message only when the
    /// request did not complete (for example a connection failure or timeout).
    ///
    /// The message travels into an error that anything may print, and
    /// [`CloudHttpRequest::url`] carries the resource key on a discovery
    /// request, so an implementation that quotes the address should pass the
    /// message through [`fiftyone_pipeline_core::redact::redact`] first. The
    /// engine cleans the message again on the way out, with the credentials it
    /// holds, so an implementation that forgets is still covered.
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String>;
}

/// The built-in [`CloudHttpClient`], backed by a blocking [`reqwest`] client.
///
/// Compiled only with the `reqwest-client` feature. The client is configured
/// with the engine's request timeout. It sets the `Origin` header when the
/// request carries one and posts the evidence as url-encoded form data, exactly
/// as the cloud HTTP API expects.
#[cfg(feature = "reqwest-client")]
pub struct ReqwestClient {
    client: reqwest::blocking::Client,
}

#[cfg(feature = "reqwest-client")]
impl ReqwestClient {
    /// Create a client with the given request timeout. A zero (non-positive)
    /// timeout means no timeout, mapping to an infinite one.
    pub fn new(timeout: Duration) -> Result<Self, String> {
        let mut builder = reqwest::blocking::Client::builder();
        if !timeout.is_zero() {
            builder = builder.timeout(timeout);
        }
        let client = builder
            .build()
            .map_err(|e| format!("failed to build HTTP client: {e}"))?;
        Ok(ReqwestClient { client })
    }
}

#[cfg(feature = "reqwest-client")]
impl CloudHttpClient for ReqwestClient {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        let mut builder = match request.method {
            HttpMethod::Get => self.client.get(&request.url),
            HttpMethod::Post => self.client.post(&request.url).form(&request.form),
        };
        if let Some(origin) = &request.origin {
            builder = builder.header(super::constants::ORIGIN_HEADER_NAME, origin);
        }

        // The discovery requests carry the keys in the query string, so the
        // messages name the endpoint without it, and the reqwest error has its
        // own copy of the URL removed. The whole line is then cleaned as
        // well, so a credential reaching the message by any other route is
        // still taken out.
        let endpoint = crate::engine::url_without_query(&request.url);
        let response = builder.send().map_err(|e| {
            redact(&format!(
                "failed to send request to '{endpoint}': {}",
                e.without_url()
            ))
            .into_owned()
        })?;

        let status = response.status().as_u16();
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_owned());
        // Read the whole body. A body that cannot be read as text is treated as
        // a transport failure rather than an empty response.
        let body = response.text().map_err(|e| {
            redact(&format!(
                "failed to read response body from '{endpoint}': {}",
                e.without_url()
            ))
            .into_owned()
        })?;

        Ok(CloudHttpResponse {
            status,
            body,
            retry_after,
        })
    }
}
