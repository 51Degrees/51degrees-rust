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

//! Default endpoints, header names and recovery tunables for the cloud request
//! engine.
//!
//! These follow the defaults listed in the
//! [cloud-request-engine specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/cloud-request-engine.md#configuration-options).

/// The environment variable that, when set, overrides the base cloud URL.
///
/// The name starts with an underscore because a POSIX shell cannot set a name
/// that starts with a digit, so `export _51DEGREES_CLOUD_ENDPOINT=...` works.
pub const CLOUD_ENDPOINT_VARIABLE: &str = "_51DEGREES_CLOUD_ENDPOINT";

/// The old name of the environment variable that overrides the base cloud
/// URL, which [`CLOUD_ENDPOINT_VARIABLE`] replaces.
///
/// The old name is read only when [`CLOUD_ENDPOINT_VARIABLE`] is unset or
/// empty, and a warning is logged the first time its value is used.
#[deprecated(note = "use CLOUD_ENDPOINT_VARIABLE, which names _51DEGREES_CLOUD_ENDPOINT")]
pub const CLOUD_ENDPOINT_ENV_VAR: &str = "51DEGREES_CLOUD_ENDPOINT";

/// The default base URL for the cloud service, including the API version path.
/// The endpoint filenames are appended to this to form the full URLs.
pub const CLOUD_URI_DEFAULT: &str = "https://cloud.51degrees.com/api/v4/";

/// The filename appended to the base URL for the data (JSON) endpoint, which is
/// the one each flow data is POSTed to.
pub const DATA_FILENAME: &str = "json";

/// The filename appended to the base URL for the accessible-properties endpoint.
pub const PROPERTIES_FILENAME: &str = "accessibleproperties";

/// The filename appended to the base URL for the evidence-keys endpoint.
pub const EVIDENCE_KEYS_FILENAME: &str = "evidencekeys";

/// The name of the HTTP header set to the configured cloud-request origin.
pub const ORIGIN_HEADER_NAME: &str = "Origin";

/// The form field carrying the resource key in the POST body and in the query
/// string of the discovery requests.
pub const RESOURCE_PARAMETER: &str = "resource";

/// The form field carrying the license key, which either stands in for a
/// resource key or travels alongside one, granting the listed properties it
/// covers on top of those the resource key carries.
pub const LICENSE_PARAMETER: &str = "license";

/// The form field naming the properties the caller wants returned, as a
/// comma-separated list of `product.property` names.
///
/// A license key carries no property list of its own, so the cloud service
/// requires this field on a request that authenticates with one and answers
/// `400` without it. With a resource key, the listed properties are added to
/// those the resource key carries.
pub const VALUES_PARAMETER: &str = "values";

/// The default request timeout in seconds.
pub const TIMEOUT_DEFAULT_SECONDS: u64 = 2;

/// The default recovery-period duration in seconds. A zero or negative value
/// disables the recovery period.
pub const RECOVERY_SECONDS_DEFAULT: f64 = 60.0;

/// The default number of failures within the window that triggers recovery.
pub const FAILURES_TO_ENTER_RECOVERY_DEFAULT: u32 = 10;

/// The minimum permitted value for the failures-to-enter-recovery threshold.
pub const FAILURES_TO_ENTER_RECOVERY_MIN: u32 = 1;

/// The maximum permitted value for the failures-to-enter-recovery threshold.
pub const FAILURES_TO_ENTER_RECOVERY_MAX: u32 = 100;

/// The default window, in seconds, within which the failure threshold must be
/// reached for the engine to enter recovery.
pub const FAILURES_WINDOW_SECONDS_DEFAULT: u64 = 100;

/// The data key under which this engine stores its element data in a flow data.
/// Downstream cloud aspect engines read the raw JSON from here. The key is
/// `cloud-response`, the key the other 51Degrees implementations use.
pub const ELEMENT_DATA_KEY: &str = "cloud-response";

/// The element-data field that holds the raw JSON response body.
pub const JSON_RESPONSE_KEY: &str = "json-response";

/// The element-data field flag confirming the engine started processing.
pub const PROCESS_STARTED_KEY: &str = "process-started";

/// The element-data field holding the advisory warning messages, if any, that
/// the cloud service returned in the response's `warnings` array.
pub const WARNINGS_KEY: &str = "warnings";

/// The top-level member of a data response that carries 51Did values.
///
/// The cloud creates each 51Did afresh for the request that asked for it, so a
/// response carrying this member answers that one request and is never shared
/// with another through the response cache.
pub const IDENTIFIER_MEMBER: &str = "fodid";
