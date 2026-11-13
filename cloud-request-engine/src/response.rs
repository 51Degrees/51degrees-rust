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

//! Parsing of cloud responses into either the raw JSON body or a
//! [`fiftyone_pipeline_core::Error::CloudRequest`].
//!
//! The rules implemented here come from the
//! [HTTP-requests section](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/cloud-request-engine.md#http-requests)
//! of the specification:
//!
//! - If the response body contains a top-level `errors` array with entries,
//!   raise an error using those messages.
//! - If the body is empty, raise `No data in response from cloud service at
//!   '[url]'`.
//! - Otherwise, if the status code indicates failure, raise `Cloud service at
//!   '[url]' returned status code '[code]' with content [body]`.
//! - Any entries in a top-level `warnings` array are returned so the caller can
//!   log them.

use fiftyone_pipeline_core::redact::redact;
use fiftyone_pipeline_core::Error;

use crate::http::CloudHttpResponse;

/// The result of validating a cloud response that did not raise an error.
///
/// Carries the raw JSON body (stored verbatim in the engine's element data) and
/// any warning strings the service returned, which the caller logs.
#[derive(Debug, Clone)]
pub struct ParsedResponse {
    /// The raw JSON response body, exactly as received.
    pub json: String,
    /// The warning messages from the response's `warnings` array, if any.
    pub warnings: Vec<String>,
}

/// Parse a `Retry-After` header value into a number of seconds.
///
/// The cloud service sends `Retry-After` as a delta in seconds (the only form
/// the 51Degrees service uses), so an HTTP-date form is not handled here and
/// yields `None`.
pub fn parse_retry_after(value: Option<&str>) -> Option<u64> {
    value.and_then(|v| v.trim().parse::<u64>().ok())
}

/// Validate a completed cloud response.
///
/// `url` is the endpoint that was called, used to build the standard error
/// messages. `check_for_error_messages` is `false` for the `evidencekeys`
/// endpoint, whose body is a flat JSON array that never carries an `errors`
/// object.
///
/// Anything that reaches the error message, or the warnings, goes through
/// [`fiftyone_pipeline_core::redact`] first, because the address carries the
/// resource key as a query parameter and the service repeats the key back
/// inside its own message when it cannot read it. The parsed JSON returned on
/// success is untouched, since that is data a downstream engine reads rather
/// than text anyone prints.
///
/// Returns `Ok(ParsedResponse)` when the response is usable, or
/// `Err(Error::CloudRequest)` describing the failure.
pub fn validate_response(
    response: &CloudHttpResponse,
    url: &str,
    check_for_error_messages: bool,
) -> Result<ParsedResponse, Error> {
    let (outcome, warnings) = validate_keeping_warnings(response, url, check_for_error_messages);
    outcome.map(|json| ParsedResponse { json, warnings })
}

/// As [`validate_response`], with the response's warnings returned beside
/// the outcome rather than inside it, so a caller can log them when the
/// response also carries errors.
pub(crate) fn validate_keeping_warnings(
    response: &CloudHttpResponse,
    url: &str,
    check_for_error_messages: bool,
) -> (Result<String, Error>, Vec<String>) {
    let retry_after = parse_retry_after(response.retry_after.as_deref());
    let body = response.body.as_str();
    let mut messages: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    // Whether the response carries usable data beyond any errors object.
    let mut has_data = !body.is_empty();

    if has_data && check_for_error_messages {
        match serde_json::from_str::<serde_json::Value>(body) {
            Ok(serde_json::Value::Object(map)) => {
                let has_errors = map.contains_key("errors");
                // Data is present if there is at least one field besides the
                // errors object.
                has_data = if has_errors {
                    map.len() > 1
                } else {
                    !map.is_empty()
                };

                if let Some(errors) = map.get("errors").and_then(|v| v.as_array()) {
                    messages.extend(errors.iter().filter_map(json_value_to_message));
                }
                if let Some(warns) = map.get("warnings").and_then(|v| v.as_array()) {
                    warnings.extend(
                        warns
                            .iter()
                            .filter_map(json_value_to_message)
                            .map(|warning| redact(&warning).into_owned()),
                    );
                }
            }
            Ok(_) => {
                // A non-object JSON value (for example a bare array) carries no
                // errors object, so it is treated as data.
            }
            Err(e) => {
                let error = cloud_error(
                    response.status,
                    retry_after,
                    format!("failed to parse the cloud service response as JSON: {e}"),
                );
                return (Err(error), warnings);
            }
        }
    }

    // The address carries the resource key on the discovery request, so it is
    // cleaned once here and the cleaned form is what every message below uses.
    let url = redact(url);
    // No explicit error, but also no data: report the empty-response message.
    if messages.is_empty() && !has_data {
        messages.push(format!("No data in response from cloud service at '{url}'"));
    }
    // No explicit error, data present, but a non-success status code: report the
    // status-code message.
    else if messages.is_empty() && !response.is_success() {
        // The body is cleaned before it is cut down, so that cutting it can
        // never leave the front half of a credential behind.
        messages.push(format!(
            "Cloud service at '{url}' returned status code '{}' with content {}",
            response.status,
            truncate(&redact(body), 1000)
        ));
    }

    if messages.is_empty() {
        (Ok(response.body.clone()), warnings)
    } else {
        // The service repeats the resource key back inside its own error text,
        // so the joined message is cleaned whatever it was built from.
        let mut message = redact(&messages.join("; ")).into_owned();
        // An invalid or missing resource key is the most common cause of a cloud
        // error, so point the reader at the configurator to create a valid one.
        if message.to_lowercase().contains("resource key") {
            message.push_str(
                ". Create a resource key at \
                 https://configure.51degrees.com?utm_source=code&utm_medium=comment&utm_campaign=51degrees-rust&utm_content=cloud-request-engine-src-response.rs&utm_term=resource-key-invalid",
            );
        }
        (
            Err(cloud_error(response.status, retry_after, message)),
            warnings,
        )
    }
}

/// Build an [`Error::CloudRequest`] from the status, retry hint and message.
pub fn cloud_error(status: u16, retry_after_seconds: Option<u64>, message: String) -> Error {
    Error::CloudRequest {
        status_code: status,
        retry_after_seconds,
        message,
    }
}

/// Convert a JSON value from an `errors`/`warnings` array into a message string.
/// Strings are taken verbatim; other scalar values are stringified.
fn json_value_to_message(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Null => None,
        other => Some(other.to_string()),
    }
}

/// Truncate `text` to at most `max` bytes on a char boundary, appending an
/// ellipsis marker when truncation happens, so error messages stay bounded.
fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(status: u16, body: &str) -> CloudHttpResponse {
        CloudHttpResponse {
            status,
            body: body.to_owned(),
            retry_after: None,
        }
    }

    #[test]
    fn success_returns_raw_json() {
        let resp = response(200, r#"{"device":{"ismobile":true}}"#);
        let parsed = validate_response(&resp, "https://x/json", true).unwrap();
        assert_eq!(parsed.json, r#"{"device":{"ismobile":true}}"#);
        assert!(parsed.warnings.is_empty());
    }

    #[test]
    fn errors_array_raises_cloud_error() {
        let resp = response(400, r#"{"errors":["bad resource key"]}"#);
        let err = validate_response(&resp, "https://x/json", true).unwrap_err();
        match err {
            Error::CloudRequest {
                status_code,
                message,
                ..
            } => {
                assert_eq!(status_code, 400);
                assert!(message.contains("bad resource key"));
            }
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    fn multiple_errors_are_joined() {
        let resp = response(400, r#"{"errors":["first","second"]}"#);
        let err = validate_response(&resp, "https://x/json", true).unwrap_err();
        let Error::CloudRequest { message, .. } = err else {
            panic!("expected cloud error");
        };
        assert!(message.contains("first") && message.contains("second"));
    }

    #[test]
    fn empty_body_raises_no_data() {
        let resp = response(200, "");
        let err = validate_response(&resp, "https://cloud/json", true).unwrap_err();
        let Error::CloudRequest { message, .. } = err else {
            panic!("expected cloud error");
        };
        assert!(message.contains("No data in response"));
        assert!(message.contains("https://cloud/json"));
    }

    #[test]
    fn non_success_without_errors_reports_status() {
        let resp = response(503, r#"{"something":"else"}"#);
        let err = validate_response(&resp, "https://cloud/json", true).unwrap_err();
        let Error::CloudRequest {
            status_code,
            message,
            ..
        } = err
        else {
            panic!("expected cloud error");
        };
        assert_eq!(status_code, 503);
        assert!(message.contains("status code '503'"));
    }

    #[test]
    fn warnings_are_returned_not_raised() {
        let resp = response(
            200,
            r#"{"device":{"x":1},"warnings":["low entropy hints"]}"#,
        );
        let parsed = validate_response(&resp, "https://x/json", true).unwrap();
        assert_eq!(parsed.warnings, vec!["low entropy hints".to_owned()]);
    }

    #[test]
    fn evidence_keys_array_is_not_error_checked() {
        // A flat array body with check disabled is returned as-is.
        let resp = response(200, r#"["header.user-agent","query.user-agent"]"#);
        let parsed = validate_response(&resp, "https://x/evidencekeys", false).unwrap();
        assert_eq!(parsed.json, r#"["header.user-agent","query.user-agent"]"#);
    }

    #[test]
    fn retry_after_is_parsed_from_header() {
        let resp = CloudHttpResponse {
            status: 429,
            body: r#"{"errors":["rate limited"]}"#.to_owned(),
            retry_after: Some("30".to_owned()),
        };
        let err = validate_response(&resp, "https://x/json", true).unwrap_err();
        let Error::CloudRequest {
            retry_after_seconds,
            status_code,
            ..
        } = err
        else {
            panic!("expected cloud error");
        };
        assert_eq!(status_code, 429);
        assert_eq!(retry_after_seconds, Some(30));
    }

    #[test]
    fn invalid_json_with_check_raises() {
        let resp = response(200, "this is not json");
        let err = validate_response(&resp, "https://x/json", true).unwrap_err();
        let Error::CloudRequest { message, .. } = err else {
            panic!("expected cloud error");
        };
        assert!(message.contains("failed to parse"));
    }
}
