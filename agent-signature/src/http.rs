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

//! The HTTP client key directories, agent cards and registries are fetched
//! with.
//!
//! The element only ever sends a GET, from a thread of its own, so the
//! client is a small blocking trait. With the `reqwest-client` feature the
//! crate provides `ReqwestClient`, which follows no redirects. A caller can
//! supply any other client through
//! [`crate::AgentSignatureElementBuilder::set_http_client`], and decides for
//! itself whether that client follows redirects.

use std::time::Duration;

/// One GET the element asks the client to make.
#[derive(Debug, Clone)]
pub struct HttpRequest<'a> {
    /// The absolute address to fetch.
    pub url: &'a str,
    /// The value of the `Accept` header.
    pub accept: &'a str,
    /// The value of the `User-Agent` header, naming the product and its
    /// version.
    pub user_agent: &'a str,
    /// The time left for the request, after which it should be abandoned.
    pub timeout: Duration,
    /// The most of the body the element reads. The element refuses a body
    /// longer than this, so a client need read no more than one byte beyond
    /// it, and should not, because the address is chosen by whoever sent the
    /// request being checked.
    pub max_response_bytes: usize,
}

/// A response the client received, whatever its status.
#[derive(Debug, Clone, Default)]
pub struct HttpResponse {
    /// The HTTP status code.
    pub status: u16,
    /// The address that answered, where the client knows it. A response
    /// from any address other than the one asked for is refused, because
    /// the key directory and agent card drafts forbid redirects.
    pub url: Option<String>,
    /// The response headers as `(name, value)` pairs.
    pub headers: Vec<(String, String)>,
    /// The body, or as much of it as the client read.
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// The values of the named header joined with `, `, or `None` where the
    /// response does not carry it. Names are matched without regard to case.
    pub fn header(&self, name: &str) -> Option<String> {
        let values: Vec<&str> = self
            .headers
            .iter()
            .filter(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
            .collect();
        if values.is_empty() {
            None
        } else {
            Some(values.join(", "))
        }
    }
}

/// Sends the GET requests the element makes.
///
/// Implementations must be `Send + Sync`, because one element serves many
/// requests at once and fetches on threads of its own.
pub trait HttpClient: Send + Sync {
    /// Send `request` and return the response for any response received,
    /// whatever its status. Return `Err` with a readable reason only where
    /// no response was received, such as a connection failure or a timeout.
    fn get(&self, request: &HttpRequest<'_>) -> Result<HttpResponse, String>;
}

#[cfg(feature = "reqwest-client")]
pub use reqwest_client::ReqwestClient;

#[cfg(feature = "reqwest-client")]
mod reqwest_client {
    use std::io::Read;

    use super::{HttpClient, HttpRequest, HttpResponse};

    /// The built-in [`HttpClient`], backed by a blocking reqwest client that
    /// follows no redirects, because the address fetched is chosen by
    /// whoever sent the request and a redirect would move the fetch
    /// somewhere the checks made before it never saw.
    ///
    /// Compiled only with the `reqwest-client` feature.
    #[derive(Debug, Clone)]
    pub struct ReqwestClient {
        client: reqwest::blocking::Client,
    }

    impl ReqwestClient {
        /// Create a client.
        pub fn new() -> Result<Self, String> {
            let client = reqwest::blocking::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|error| format!("the HTTP client could not be built: {error}"))?;
            Ok(ReqwestClient { client })
        }
    }

    impl HttpClient for ReqwestClient {
        fn get(&self, request: &HttpRequest<'_>) -> Result<HttpResponse, String> {
            let response = self
                .client
                .get(request.url)
                .header(reqwest::header::ACCEPT, request.accept)
                .header(reqwest::header::USER_AGENT, request.user_agent)
                .timeout(request.timeout)
                .send()
                .map_err(|error| error.to_string())?;
            let status = response.status().as_u16();
            let url = Some(response.url().to_string());
            let headers = response
                .headers()
                .iter()
                .filter_map(|(name, value)| {
                    value
                        .to_str()
                        .ok()
                        .map(|value| (name.as_str().to_owned(), value.to_owned()))
                })
                .collect();
            // One byte beyond the limit is enough to tell a body that is too
            // long from one that is not.
            let limit = u64::try_from(request.max_response_bytes)
                .unwrap_or(u64::MAX)
                .saturating_add(1);
            let mut body = Vec::new();
            response
                .take(limit)
                .read_to_end(&mut body)
                .map_err(|error| error.to_string())?;
            Ok(HttpResponse {
                status,
                url,
                headers,
                body,
            })
        }
    }
}
