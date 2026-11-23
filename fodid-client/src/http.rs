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

//! The one HTTP operation the client needs, and the built-in transport.

use core::future::Future;
use core::pin::Pin;
#[cfg(feature = "reqwest-client")]
use std::time::Duration;

/// A boxed future that borrows for `'a` and is not required to be `Send`.
///
/// Every awaitable operation in this crate resolves through this type, so
/// the crate needs no async runtime of its own and no future it returns has
/// to cross threads. That is what lets a host such as a `wasm32-wasip1`
/// edge runtime, whose request and response types cannot leave the thread
/// they were made on, implement [`DidHttpClient`] and await the client.
pub type LocalBoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// The HTTP method used for a request to the 51Did endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    /// An HTTP GET, used for the key and verify endpoints.
    Get,
    /// An HTTP POST, used for redeem, which reads its parameters from a
    /// url-encoded form body so no credential is ever written to an access
    /// log.
    Post,
}

/// A single request the client asks a [`DidHttpClient`] to perform.
#[derive(Debug, Clone)]
pub struct DidHttpRequest {
    /// The HTTP method.
    pub method: HttpMethod,
    /// The absolute URL to request. The 51Did routes carry the resource key
    /// as part of the route, so this must not be copied into an error message
    /// as it stands.
    pub url: String,
    /// The url-encoded form fields to send as the POST body, empty for a
    /// GET. The transport is responsible for url-encoding these.
    pub form: Vec<(String, String)>,
    /// The `User-Agent` to send, naming this package and its version.
    pub user_agent: String,
    /// Further headers to send, as name and value. The signing key fetch
    /// carries the licence key in one where the client was given a licence
    /// key, so a transport sends every one of them and never copies a value
    /// into an error message or a log.
    pub headers: Vec<(String, String)>,
}

/// Whatever the server answered, whatever the status.
#[derive(Debug, Clone)]
pub struct DidHttpResponse {
    /// The HTTP status code.
    pub status: u16,
    /// The response body, read as text.
    pub body: String,
}

/// The transport the client sends through.
///
/// Implemented so that a test can stand in for the network and a caller can
/// route the client's requests through an HTTP stack of its own. That second
/// case is not hypothetical: this crate has to build for `wasm32-wasip1`,
/// where there is no `reqwest`, and a host such as an edge runtime supplies
/// its own fetch.
///
/// Implementations MUST be `Send + Sync`, so one client can serve many
/// threads, which is the same rule the cloud request engine's transport
/// carries. The future `send` returns is a [`LocalBoxFuture`] and is
/// deliberately not required to be `Send`, so a host whose request or
/// response types cannot cross threads can still implement it. Write `send`
/// by hand, as in the example, boxing the body with `Box::pin`.
///
/// # Example
///
/// ```
/// use fodid_client::{
///     DidHttpClient, DidHttpRequest, DidHttpResponse, HttpMethod, LocalBoxFuture,
/// };
///
/// struct HostTransport;
///
/// impl DidHttpClient for HostTransport {
///     fn send<'a>(
///         &'a self,
///         request: &'a DidHttpRequest,
///     ) -> LocalBoxFuture<'a, Result<DidHttpResponse, String>> {
///         Box::pin(async move {
///             // Hand request.url, request.headers, request.form
///             // (url-encoded for a POST) and request.user_agent to the
///             // host's own fetch, await it, then return the status and
///             // body it answered with.
///             let _ = (request.method == HttpMethod::Post, &request.url);
///             Err("not connected in this example".to_string())
///         })
///     }
/// }
/// ```
pub trait DidHttpClient: Send + Sync {
    /// Sends the request and resolves to whatever the server answered,
    /// whatever the status. The future borrows the request and the
    /// transport for `'a`. Every one of [`DidHttpRequest::headers`] is sent
    /// beside the `User-Agent`, because the signing key fetch carries the
    /// licence key in one.
    ///
    /// Resolve to `Err` with a human readable message ONLY when the request
    /// did not complete, being a connection failure, a timeout, or an answer
    /// that could not be read. A status the caller did not want is still a
    /// completed request and comes back as `Ok`, because the client decides
    /// what each status means and says so in its own words.
    ///
    /// The message travels into an error that anything may print, and the
    /// address can carry the resource key in its route, so an implementation
    /// that quotes the address should pass the message through
    /// [`crate::redact::redact`] first. A header value is never quoted,
    /// because it can be the licence key. The client cleans the message
    /// again on the way out, with the credentials it holds, so an
    /// implementation that forgets is still covered.
    fn send<'a>(
        &'a self,
        request: &'a DidHttpRequest,
    ) -> LocalBoxFuture<'a, Result<DidHttpResponse, String>>;
}

/// The built-in [`DidHttpClient`], backed by an asynchronous [`reqwest`]
/// client with rustls.
///
/// Compiled only with the `reqwest-client` feature, which is off by default
/// so the crate builds for `wasm32-wasip1` and so a caller that supplies its
/// own transport pulls in no HTTP stack it does not want. The reqwest client
/// runs on a tokio runtime, so a call through this transport is awaited from
/// inside one.
#[cfg(feature = "reqwest-client")]
pub struct ReqwestClient {
    client: reqwest::Client,
}

#[cfg(feature = "reqwest-client")]
impl ReqwestClient {
    /// Creates a client with the given request timeout. A zero timeout means
    /// no timeout.
    pub fn new(timeout: Duration) -> Result<Self, String> {
        let mut builder = reqwest::Client::builder();
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
impl Default for ReqwestClient {
    /// A client with the default thirty second timeout.
    fn default() -> Self {
        Self::new(Duration::from_secs(30)).expect("the default HTTP client builds")
    }
}

#[cfg(feature = "reqwest-client")]
impl DidHttpClient for ReqwestClient {
    fn send<'a>(
        &'a self,
        request: &'a DidHttpRequest,
    ) -> LocalBoxFuture<'a, Result<DidHttpResponse, String>> {
        Box::pin(async move {
            let mut builder = match request.method {
                HttpMethod::Get => self.client.get(&request.url),
                HttpMethod::Post => self.client.post(&request.url).form(&request.form),
            };
            for (name, value) in &request.headers {
                builder = builder.header(name.as_str(), value.as_str());
            }
            // The address carries the resource key in its route, and reqwest
            // puts the address into its own message too, so the whole line is
            // cleaned rather than only the part this code wrote.
            let response = builder
                .header("User-Agent", &request.user_agent)
                .send()
                .await
                .map_err(|e| {
                    crate::redact::redact(&format!(
                        "failed to send request to '{}': {e}",
                        request.url
                    ))
                    .into_owned()
                })?;
            let status = response.status().as_u16();
            let body = response.text().await.map_err(|e| {
                crate::redact::redact(&format!(
                    "failed to read the answer from '{}': {e}",
                    request.url
                ))
                .into_owned()
            })?;
            Ok(DidHttpResponse { status, body })
        })
    }
}

#[cfg(all(test, feature = "reqwest-client"))]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread::JoinHandle;

    use super::*;
    use crate::client::LICENCE_KEY_HEADER;

    /// Answers one request on a local port with an empty key list, and hands
    /// back the request head as it arrived, so the test sees what the
    /// built-in transport put on the wire.
    fn serve_once() -> (String, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a local port");
        let base = format!("http://{}/", listener.local_addr().expect("the port"));
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept the request");
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).expect("read the request head");
                head.push(byte[0]);
            }
            stream
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\n[]")
                .expect("answer the request");
            String::from_utf8_lossy(&head).into_owned()
        });
        (base, server)
    }

    #[tokio::test]
    async fn the_built_in_transport_sends_every_header() {
        let (base, server) = serve_once();
        // No proxy, so a proxy set in the environment cannot take the
        // request somewhere other than the local port.
        let transport = ReqwestClient {
            client: reqwest::Client::builder()
                .no_proxy()
                .build()
                .expect("the HTTP client builds"),
        };
        let request = DidHttpRequest {
            method: HttpMethod::Get,
            url: format!("{base}id/key"),
            form: Vec::new(),
            user_agent: "fodid-client/test".to_string(),
            headers: vec![(LICENCE_KEY_HEADER.to_string(), "licence-value".to_string())],
        };
        let response = transport
            .send(&request)
            .await
            .expect("the request completes");
        assert_eq!(response.status, 200);
        assert_eq!(response.body, "[]");
        // Header names are not case sensitive, so the head is read in lower
        // case.
        let head = server
            .join()
            .expect("the local server finished")
            .to_ascii_lowercase();
        assert!(head.starts_with("get /id/key http/1.1\r\n"), "{head}");
        assert!(
            head.contains("\r\nx-51d-license-key: licence-value\r\n"),
            "{head}"
        );
        assert!(
            head.contains("\r\nuser-agent: fodid-client/test\r\n"),
            "{head}"
        );
    }
}
