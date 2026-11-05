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

//! Shared string constants used throughout the pipeline.
//!
//! These fix the evidence key spellings and the default web endpoint paths used
//! across the crates. The engines and web crates re-export the subset they need
//! rather than redefining them.

/// The string used to separate the prefix and field parts of an evidence key,
/// for example the `.` in `header.user-agent`.
///
/// See the [evidence specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/evidence.md).
pub const EVIDENCE_SEPARATOR: &str = ".";

/// Prefix for evidence obtained from an HTTP request's query string or passed
/// into the pipeline directly by the application for off-line processing.
pub const EVIDENCE_QUERY_PREFIX: &str = "query";

/// Prefix for evidence obtained from HTTP headers.
pub const EVIDENCE_HTTP_HEADER_PREFIX: &str = "header";

/// Prefix for evidence obtained from cookies.
pub const EVIDENCE_COOKIE_PREFIX: &str = "cookie";

/// Prefix for evidence obtained from the server that the pipeline is running
/// on, for example the server's own IP address.
pub const EVIDENCE_SERVER_PREFIX: &str = "server";

/// Prefix used internally by 51Degrees.
pub const EVIDENCE_FIFTYONE_PREFIX: &str = "fiftyone";

/// Prefix used to supply geo-location information.
pub const EVIDENCE_LOCATION_PREFIX: &str = "location";

/// Prefix for evidence relating to the user's session.
pub const EVIDENCE_SESSION_PREFIX: &str = "session";

/// The suffix used when the User-Agent is passed as evidence.
pub const EVIDENCE_USER_AGENT: &str = "user-agent";

/// The complete key to use when the client IP address is passed as evidence.
pub const EVIDENCE_CLIENT_IP_KEY: &str = "server.client-ip";

/// The complete key to use when the User-Agent is passed as evidence in the
/// query string (or set directly for off-line processing).
pub const EVIDENCE_QUERY_USER_AGENT_KEY: &str = "query.user-agent";

/// The complete key to use when the User-Agent is passed as evidence in the
/// HTTP headers.
pub const EVIDENCE_HEADER_USER_AGENT_KEY: &str = "header.user-agent";

/// The complete key to use when the `Protocol` HTTP header is passed as
/// evidence.
pub const EVIDENCE_PROTOCOL_KEY: &str = "header.protocol";

/// The complete key to use when the method of the request is passed as
/// evidence, exactly as the request line carried it.
///
/// The three request line values are supplied byte for byte, with no
/// decoding and no normalisation, because the agent signature element
/// rebuilds the text an HTTP message signature was made over from them and
/// one changed byte makes a valid signature read as invalid. An integration
/// that supplies them supplies all three on every request. See the
/// [web integration specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/web-integration.md#populating-evidence).
pub const EVIDENCE_REQUEST_METHOD_KEY: &str = "server.request-method";

/// The complete key to use when the path of the request is passed as
/// evidence, exactly as the request line carried it.
pub const EVIDENCE_REQUEST_PATH_KEY: &str = "server.request-path";

/// The complete key to use when the query string of the request is passed as
/// evidence, whole and undecoded, without its leading `?`. It is empty where
/// the request carried no query string, so that a missing key always means
/// the integration does not supply the request line.
pub const EVIDENCE_REQUEST_QUERY_KEY: &str = "server.request-query";

/// The prefix added to all cookies set by 51Degrees client-side code that can
/// be used as evidence.
pub const FIFTYONE_COOKIE_PREFIX: &str = "51d_";

/// The default endpoint at which the client JavaScript posts collected
/// evidence and receives the JSON result.
///
/// The web crate matches request paths against this case-insensitively by
/// suffix.
pub const DEFAULT_JSON_ENDPOINT: &str = "/51dpipeline/json";

/// The default path at which the rendered client-side JavaScript is served.
///
/// The web crate matches request paths against this case-insensitively by
/// suffix.
pub const DEFAULT_CORE_JS_ENDPOINT: &str = "/51Degrees.core.js";

/// The default value of the pipeline `suppress_process_exceptions` flag.
///
/// The default is `false` so that errors are highly visible during
/// development, per the
/// [exception-handling specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/exception-handling.md).
pub const DEFAULT_SUPPRESS_PROCESS_EXCEPTIONS: bool = false;
