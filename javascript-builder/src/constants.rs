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

//! String and boolean constants shared across the JavaScript builder.
//!
//! These define the generated JavaScript, the evidence keys it reads and the
//! defaults it applies.

/// The element data key under which the JavaScript builder stores its element
/// data.
///
/// This is the element's own key, returned from
/// [`fiftyone_pipeline_core::FlowElement::data_key`]. The generated JavaScript
/// string itself is held under the [`JAVASCRIPT_PROPERTY_KEY`] property of that
/// data.
pub const JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY: &str = "javascriptbuilderelement";

/// The name of the single property the JavaScript builder populates: the
/// generated JavaScript, as a string.
///
/// The property name is `"javascript"`, which the
/// [`crate::JavaScriptBuilderElementData`] exposes. Read it
/// from the element data with [`crate::JavaScriptBuilderElementData::javascript`]
/// or by string key through [`fiftyone_pipeline_core::ElementData::get`].
pub const JAVASCRIPT_PROPERTY_KEY: &str = "javascript";

/// The complete key to use when the `Host` HTTP header is passed as evidence
/// (`header.host`).
pub const EVIDENCE_HOST_KEY: &str = "header.host";

/// The suffix used when the JavaScript builder 'object name' parameter is
/// supplied as evidence.
pub const EVIDENCE_OBJECT_NAME_SUFFIX: &str = "fod-js-object-name";

/// The suffix used when the JavaScript builder 'enable cookies' parameter is
/// supplied as evidence.
pub const EVIDENCE_ENABLE_COOKIES_SUFFIX: &str = "fod-js-enable-cookies";

/// The complete `query.fod-js-object-name` evidence key.
pub const EVIDENCE_OBJECT_NAME: &str = "query.fod-js-object-name";

/// The complete `query.fod-js-enable-cookies` evidence key.
pub const EVIDENCE_ENABLE_COOKIES: &str = "query.fod-js-enable-cookies";

/// The protocol used when creating a callback URL if no other protocol value
/// was found or specified.
pub const FALLBACK_PROTOCOL: &str = "https";

/// The default value for the JavaScript 'object name'.
pub const BUILDER_DEFAULT_OBJECT_NAME: &str = "fod";

/// The default protocol for the builder. An empty string means the protocol from
/// the evidence collection (the protocol the original request used) is applied.
pub const BUILDER_DEFAULT_PROTOCOL: &str = "";

/// The default host for the builder. An empty string means the host from the
/// evidence collection (the host the original request used) is applied.
pub const BUILDER_DEFAULT_HOST: &str = "";

/// The default value of the flag that controls minification.
pub const BUILDER_DEFAULT_MINIFY: bool = true;

/// The default value of the flag that controls whether client-side processing
/// stores results in cookies.
pub const BUILDER_DEFAULT_ENABLE_COOKIES: bool = true;

/// The placeholder JSON used when no JSON payload is available, applied as a
/// fallback when the JSON is missing.
pub const MISSING_JSON_OBJECT: &str = "{\"errors\":[\"Json data missing.\"]}";

/// The marker substring that, when present in the JSON payload, indicates the
/// payload contains at least one delayed-execution JavaScript property. The
/// payload is checked for the substring `delayexecution`.
pub const DELAY_EXECUTION_MARKER: &str = "delayexecution";

/// The device-detection property consulted to decide whether the client browser
/// supports JavaScript promises. The builder treats a value of
/// [`PROMISE_FULL_VALUE`] as full support.
pub const PROMISE_PROPERTY: &str = "Promise";

/// The device-detection property value that indicates full promise support.
pub const PROMISE_FULL_VALUE: &str = "Full";

/// The name of the 51Did product in a cloud request engine's licensed
/// products, and the data key of the element that gives a 51Did, both
/// compared ignoring case.
///
/// The cloud service reports the product as `FODid`, whilst the 51Did cloud
/// engine stores its element data under `fodid`. The two differ only in case,
/// so the comparison ignores case. The template's user prompt section is
/// rendered for a pipeline with an element under this key offering an
/// available property, or for a key whose licensed products include this
/// product with at least one property. The value is the 51Did cloud engine's
/// element data key, copied rather than imported so this crate does not
/// depend on the 51Did crates, and a test checks the two are still equal.
pub const FODID_PRODUCT_NAME: &str = "fodid";

/// The device-detection property consulted to decide whether the client browser
/// supports the fetch API.
pub const FETCH_PROPERTY: &str = "Fetch";
