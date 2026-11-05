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

//! The framework-neutral HTTP response value.
//!
//! [`WebResponse`] is a plain data structure describing what the client-side
//! endpoints want sent back: a status code, a set of headers and a body. It
//! carries no dependency on any web framework, so an adapter crate (for example
//! the axum adapter) maps it onto that framework's own response type.
//!
//! The body is held as a `Vec<u8>` because the JavaScript and JSON endpoints
//! both produce text that is sent verbatim, and bytes are the lowest common
//! denominator every framework can accept. Convenience constructors
//! ([`WebResponse::text`], [`WebResponse::not_modified`]) cover the two shapes
//! the endpoints return.

/// A framework-neutral HTTP response.
///
/// An adapter turns this into its framework's response. The header list
/// preserves insertion order and may contain repeated names, although the
/// endpoints in this crate never emit a duplicate name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebResponse {
    /// The HTTP status code, for example `200` or `304`.
    pub status: u16,
    /// The response headers as ordered `(name, value)` pairs. Names are written
    /// in their canonical HTTP casing (for example `Content-Type`).
    pub headers: Vec<(String, String)>,
    /// The response body. Empty for a `304 Not Modified`.
    pub body: Vec<u8>,
}

impl WebResponse {
    /// The HTTP status code for a normal, successful response.
    pub const STATUS_OK: u16 = 200;

    /// The HTTP status code returned when the client's cached copy is still
    /// current (its `If-None-Match` matched the computed `ETag`).
    pub const STATUS_NOT_MODIFIED: u16 = 304;

    /// Create a `200 OK` text response with the given body and headers.
    ///
    /// The body is taken as a string and stored as its UTF-8 bytes. The headers
    /// are taken verbatim in the order supplied.
    pub fn text(body: impl Into<String>, headers: Vec<(String, String)>) -> Self {
        WebResponse {
            status: Self::STATUS_OK,
            headers,
            body: body.into().into_bytes(),
        }
    }

    /// Create a `304 Not Modified` response with no body and no headers.
    ///
    /// The client-side endpoints add to it the caching and CORS headers the
    /// `200` would have carried (see [`crate::serve_json`]).
    pub fn not_modified() -> Self {
        WebResponse {
            status: Self::STATUS_NOT_MODIFIED,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    /// True if this is a `304 Not Modified` response.
    pub fn is_not_modified(&self) -> bool {
        self.status == Self::STATUS_NOT_MODIFIED
    }

    /// The body interpreted as a UTF-8 string slice, if it is valid UTF-8.
    ///
    /// The endpoints always produce UTF-8, so this succeeds for any response
    /// this crate builds. It is provided as a convenience for tests and adapters
    /// that want the text without copying.
    pub fn body_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.body).ok()
    }

    /// Look up the first header value with the given name, matched
    /// case-insensitively. Returns `None` if the header is absent.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}
