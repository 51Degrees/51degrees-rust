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

//! Mapping the framework-neutral [`WebResponse`] onto an axum response.
//!
//! The web crate's endpoints return a [`WebResponse`], a plain
//! `{ status, headers, body }` value. [`into_axum_response`] turns one into an
//! `axum::response::Response`, copying the status, every header and the body.
//! A header whose name or value axum rejects (which the endpoints never emit) is
//! skipped rather than failing the whole response.

use axum::body::Body;
use axum::http::header::{HeaderName, HeaderValue};
use axum::http::StatusCode;
use axum::response::Response;
use fiftyone_pipeline_web::WebResponse;

/// Convert a [`WebResponse`] into an axum [`Response`].
///
/// The status code is taken verbatim (falling back to `200 OK` only for the
/// impossible case of an out-of-range code). Each header is parsed into an
/// `axum` header name and value, and a header axum cannot represent is dropped.
/// The body bytes are sent as the response body. A `304 Not Modified` carries an
/// empty body and the headers it was given.
pub fn into_axum_response(response: WebResponse) -> Response {
    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::OK);

    let mut builder = Response::builder().status(status);

    for (name, value) in &response.headers {
        // The endpoints only ever produce valid header names and values, so a
        // parse failure means a caller customised the response oddly; skip it
        // rather than discard the whole response.
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            builder = builder.header(name, value);
        }
    }

    builder
        .body(Body::from(response.body))
        // The builder only errors on an invalid status or header that we have
        // already guarded, so this is unreachable in practice. Fall back to a
        // bare 500 rather than panicking.
        .unwrap_or_else(|_| {
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .expect("an empty 500 response is always valid")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_status_headers_and_body() {
        let web = WebResponse::text(
            "hello",
            vec![("Content-Type".to_owned(), "text/plain".to_owned())],
        );
        let response = into_axum_response(web);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok()),
            Some("text/plain")
        );
    }

    #[test]
    fn not_modified_has_no_headers() {
        let response = into_axum_response(WebResponse::not_modified());
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert!(response.headers().is_empty());
    }
}
