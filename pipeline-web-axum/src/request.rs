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

//! An [`fiftyone_pipeline_web::RequestData`] implementation backed by an axum
//! request.
//!
//! [`AxumRequestData`] is the small shim the framework-neutral web crate asks an
//! adapter to provide. It reads the parts of an HTTP request the pipeline turns
//! into evidence, headers, cookies, the query string, the form body, the client
//! IP address and the protocol, from the pieces axum hands a handler or
//! middleware.
//!
//! # How each source is read
//!
//! - **Headers** come straight from the request [`HeaderMap`]. A header whose
//!   value is not valid UTF-8 is skipped, because evidence values are strings.
//! - **Cookies** are parsed from every `Cookie` header, split on `;` then on the
//!   first `=`, with the name and value trimmed. This is the standard
//!   `name=value; name2=value2` cookie grammar and needs no external crate.
//! - **Query parameters** are parsed from the request URI's query string with a
//!   minimal `application/x-www-form-urlencoded` decoder (see
//!   [`crate::form`]).
//! - **Form parameters** are decoded from a captured body with the same decoder.
//!   The middleware and the JSON handler buffer the body once and pass the bytes
//!   in, so the body is read a single time.
//! - **Client IP** prefers the left-most address in an `X-Forwarded-For` header
//!   (the original client when behind one or more proxies), then falls back to
//!   the peer address axum recorded through
//!   [`axum::extract::ConnectInfo`]. Supply that peer address with
//!   [`AxumRequestData::with_peer_addr`].
//! - **Protocol** is `https` when an `X-Forwarded-Proto` header says so, when a
//!   `Forwarded` header carries `proto=https`, or when the request URI carries
//!   an `https` scheme. Otherwise it is `http`.
//! - **Method** is the one attached with [`AxumRequestData::with_method`], as
//!   the endpoint handlers do, and absent otherwise.
//!
//! The struct borrows the header map and URI from the request and owns only the
//! small extras (the buffered form bytes, the optional peer address and the
//! optional method), so building one per request is cheap.

use std::net::SocketAddr;

use axum::http::{HeaderMap, Method, Uri};

use crate::form::parse_form_urlencoded;

/// The `X-Forwarded-For` header, carrying the client and proxy chain.
const HEADER_X_FORWARDED_FOR: &str = "x-forwarded-for";
/// The `X-Forwarded-Proto` header, carrying the original request scheme.
const HEADER_X_FORWARDED_PROTO: &str = "x-forwarded-proto";
/// The standardised `Forwarded` header (RFC 7239).
const HEADER_FORWARDED: &str = "forwarded";
/// The `Cookie` request header.
const HEADER_COOKIE: &str = "cookie";

/// A view over an axum request that implements
/// [`fiftyone_pipeline_web::RequestData`].
///
/// Construct one from the request's [`HeaderMap`] and [`Uri`] with
/// [`AxumRequestData::new`], then optionally attach the captured form body and
/// the peer socket address. The struct holds borrowed references to the header
/// map and URI, so it lives no longer than the request parts it reads.
///
/// # Example
///
/// ```
/// use axum::http::{HeaderMap, HeaderValue, Uri};
/// use fiftyone_pipeline_web::RequestData;
/// use fiftyone_pipeline_web_axum::AxumRequestData;
///
/// let mut headers = HeaderMap::new();
/// headers.insert("user-agent", HeaderValue::from_static("test-agent"));
/// let uri: Uri = "/page?fod-js-object-name=fod".parse().unwrap();
///
/// let request = AxumRequestData::new(&headers, &uri);
/// assert_eq!(
///     request.headers().iter().find(|(n, _)| n == "user-agent").map(|(_, v)| v.as_str()),
///     Some("test-agent"),
/// );
/// assert_eq!(
///     request.query_params(),
///     vec![("fod-js-object-name".to_owned(), "fod".to_owned())],
/// );
/// ```
pub struct AxumRequestData<'a> {
    headers: &'a HeaderMap,
    uri: &'a Uri,
    form_body: Vec<u8>,
    peer_addr: Option<SocketAddr>,
    method: Option<Method>,
}

impl<'a> AxumRequestData<'a> {
    /// Build a request view from the header map and URI of an axum request.
    ///
    /// The form body is empty and no peer address or method is attached. Use
    /// [`AxumRequestData::with_form_body`] for a POST body,
    /// [`AxumRequestData::with_peer_addr`] to supply the connection's peer
    /// address (normally from [`axum::extract::ConnectInfo`]) and
    /// [`AxumRequestData::with_method`] for the request method.
    pub fn new(headers: &'a HeaderMap, uri: &'a Uri) -> Self {
        AxumRequestData {
            headers,
            uri,
            form_body: Vec::new(),
            peer_addr: None,
            method: None,
        }
    }

    /// Attach the buffered request body so its
    /// `application/x-www-form-urlencoded` fields are exposed as form
    /// parameters. Returns `self` for chaining.
    ///
    /// The caller buffers the body once (the middleware and the JSON handler do
    /// this) and passes the bytes here, so the body is consumed a single time.
    pub fn with_form_body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.form_body = body.into();
        self
    }

    /// Attach the connection's peer socket address, used as the client IP when
    /// no `X-Forwarded-For` header is present. Returns `self` for chaining.
    pub fn with_peer_addr(mut self, peer_addr: SocketAddr) -> Self {
        self.peer_addr = Some(peer_addr);
        self
    }

    /// Attach the request method, which decides whether a matching
    /// `If-None-Match` gets a `304` or a `412` (see
    /// [`fiftyone_pipeline_web::RequestData::method`]). Returns `self` for
    /// chaining.
    pub fn with_method(mut self, method: Method) -> Self {
        self.method = Some(method);
        self
    }

    /// Find the first header value with the given (lowercase) name as a string,
    /// skipping a value that is not valid UTF-8.
    fn header_str(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }
}

impl fiftyone_pipeline_web::RequestData for AxumRequestData<'_> {
    fn headers(&self) -> Vec<(String, String)> {
        self.headers
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_owned(), value.to_owned()))
            })
            .collect()
    }

    fn cookies(&self) -> Vec<(String, String)> {
        let mut cookies = Vec::new();
        // A request may carry more than one Cookie header; read every one.
        for value in self.headers.get_all(HEADER_COOKIE) {
            let Ok(text) = value.to_str() else { continue };
            for pair in text.split(';') {
                let pair = pair.trim();
                if pair.is_empty() {
                    continue;
                }
                if let Some((name, value)) = pair.split_once('=') {
                    cookies.push((name.trim().to_owned(), value.trim().to_owned()));
                }
            }
        }
        cookies
    }

    fn query_params(&self) -> Vec<(String, String)> {
        match self.uri.query() {
            Some(query) => parse_form_urlencoded(query.as_bytes()),
            None => Vec::new(),
        }
    }

    fn form_params(&self) -> Vec<(String, String)> {
        parse_form_urlencoded(&self.form_body)
    }

    fn client_ip(&self) -> Option<String> {
        // A proxy chain records the original client first in X-Forwarded-For, so
        // take the left-most, non-empty entry.
        if let Some(forwarded) = self.header_str(HEADER_X_FORWARDED_FOR) {
            if let Some(first) = forwarded
                .split(',')
                .map(str::trim)
                .find(|entry| !entry.is_empty())
            {
                return Some(first.to_owned());
            }
        }

        // Otherwise the connection peer is the closest thing to a client IP. The
        // port is dropped so the value is a bare address, matching the
        // server.client-ip evidence the pipeline expects.
        self.peer_addr.map(|addr| addr.ip().to_string())
    }

    fn is_https(&self) -> bool {
        // A terminating proxy reports the original scheme through one of the
        // forwarded headers; honour those first, then the URI scheme.
        if let Some(proto) = self.header_str(HEADER_X_FORWARDED_PROTO) {
            // The header may list a chain (rarely); the first entry is the one
            // closest to the client.
            if let Some(first) = proto.split(',').next() {
                return first.trim().eq_ignore_ascii_case("https");
            }
        }

        if let Some(forwarded) = self.header_str(HEADER_FORWARDED) {
            if forwarded_proto_is_https(forwarded) {
                return true;
            }
        }

        self.uri
            .scheme_str()
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https"))
    }

    fn method(&self) -> Option<String> {
        self.method
            .as_ref()
            .map(|method| method.as_str().to_owned())
    }
}

/// True if an RFC 7239 `Forwarded` header carries `proto=https` in its first
/// forwarded element.
///
/// Only the first element (closest to the client) is consulted, and only its
/// `proto` directive. A quoted value (`proto="https"`) is unwrapped. Anything
/// else is treated as not-https.
fn forwarded_proto_is_https(forwarded: &str) -> bool {
    // Elements are comma separated; directives within an element are semicolon
    // separated. The first element is the one closest to the client.
    let first_element = forwarded.split(',').next().unwrap_or("");
    for directive in first_element.split(';') {
        let directive = directive.trim();
        if let Some((key, value)) = directive.split_once('=') {
            if key.trim().eq_ignore_ascii_case("proto") {
                let value = value.trim().trim_matches('"');
                return value.eq_ignore_ascii_case("https");
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue};
    use fiftyone_pipeline_web::RequestData;

    fn headers_with(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.append(
                axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        headers
    }

    #[test]
    fn headers_are_read_verbatim() {
        let headers = headers_with(&[("user-agent", "agent"), ("accept", "text/html")]);
        let uri: Uri = "/".parse().unwrap();
        let request = AxumRequestData::new(&headers, &uri);
        let collected = request.headers();
        assert!(collected.contains(&("user-agent".to_owned(), "agent".to_owned())));
        assert!(collected.contains(&("accept".to_owned(), "text/html".to_owned())));
    }

    #[test]
    fn cookies_split_on_semicolons() {
        let headers = headers_with(&[("cookie", "a=1; b=2;  c=3")]);
        let uri: Uri = "/".parse().unwrap();
        let request = AxumRequestData::new(&headers, &uri);
        assert_eq!(
            request.cookies(),
            vec![
                ("a".to_owned(), "1".to_owned()),
                ("b".to_owned(), "2".to_owned()),
                ("c".to_owned(), "3".to_owned()),
            ]
        );
    }

    #[test]
    fn query_is_url_decoded() {
        let headers = HeaderMap::new();
        let uri: Uri = "/p?name=John%20Doe&flag=on".parse().unwrap();
        let request = AxumRequestData::new(&headers, &uri);
        assert_eq!(
            request.query_params(),
            vec![
                ("name".to_owned(), "John Doe".to_owned()),
                ("flag".to_owned(), "on".to_owned()),
            ]
        );
    }

    #[test]
    fn form_body_is_decoded() {
        let headers = HeaderMap::new();
        let uri: Uri = "/".parse().unwrap();
        let request = AxumRequestData::new(&headers, &uri).with_form_body("x=1&y=two+words");
        assert_eq!(
            request.form_params(),
            vec![
                ("x".to_owned(), "1".to_owned()),
                ("y".to_owned(), "two words".to_owned()),
            ]
        );
    }

    #[test]
    fn client_ip_prefers_forwarded_for() {
        let headers = headers_with(&[("x-forwarded-for", "1.2.3.4, 5.6.7.8")]);
        let uri: Uri = "/".parse().unwrap();
        let request =
            AxumRequestData::new(&headers, &uri).with_peer_addr("9.9.9.9:443".parse().unwrap());
        assert_eq!(request.client_ip(), Some("1.2.3.4".to_owned()));
    }

    #[test]
    fn client_ip_falls_back_to_peer() {
        let headers = HeaderMap::new();
        let uri: Uri = "/".parse().unwrap();
        let request =
            AxumRequestData::new(&headers, &uri).with_peer_addr("9.9.9.9:443".parse().unwrap());
        assert_eq!(request.client_ip(), Some("9.9.9.9".to_owned()));
    }

    #[test]
    fn https_from_forwarded_proto() {
        let headers = headers_with(&[("x-forwarded-proto", "https")]);
        let uri: Uri = "/".parse().unwrap();
        assert!(AxumRequestData::new(&headers, &uri).is_https());
    }

    #[test]
    fn https_from_forwarded_header() {
        let headers = headers_with(&[("forwarded", "for=1.2.3.4;proto=https;by=proxy")]);
        let uri: Uri = "/".parse().unwrap();
        assert!(AxumRequestData::new(&headers, &uri).is_https());
    }

    #[test]
    fn http_by_default() {
        let headers = HeaderMap::new();
        let uri: Uri = "/".parse().unwrap();
        assert!(!AxumRequestData::new(&headers, &uri).is_https());
    }

    #[test]
    fn method_is_the_one_attached() {
        let headers = HeaderMap::new();
        let uri: Uri = "/".parse().unwrap();
        assert_eq!(AxumRequestData::new(&headers, &uri).method(), None);
        let request = AxumRequestData::new(&headers, &uri).with_method(Method::POST);
        assert_eq!(request.method().as_deref(), Some("POST"));
    }
}
