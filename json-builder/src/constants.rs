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

//! String and numeric constants shared across the JSON builder.
//!
//! These define the JSON shape produced by the builder.

/// The data key under which the JSON builder stores its element data.
///
/// This is the element's own key, returned from
/// [`fiftyone_pipeline_core::FlowElement::data_key`]. The serialized JSON string
/// itself is held under the [`JSON_PROPERTY_KEY`] property of that data.
pub const JSON_BUILDER_ELEMENT_DATA_KEY: &str = "json-builder";

/// The name of the single property the JSON builder populates: the serialized
/// JSON document, as a string.
///
/// The property name is `"json"`. Read it from the element data with
/// [`crate::JsonBuilderData::json`] or by string key through
/// [`fiftyone_pipeline_core::ElementData::get`].
pub const JSON_PROPERTY_KEY: &str = "json";

/// The evidence key carrying the request sequence number.
///
/// The web integration increments this each time the client posts collected
/// evidence back. The JSON builder reads it to decide whether to keep emitting
/// the `javascriptProperties` list (see [`MAX_JAVASCRIPT_ITERATIONS`]). The key
/// is `"query.sequence"`.
pub const SEQUENCE_EVIDENCE_KEY: &str = "query.sequence";

/// The maximum sequence number at which the JavaScript-properties list is still
/// emitted.
///
/// Once the client has executed the JavaScript this many times the extra
/// round-trips stop being worthwhile, so the list is suppressed to break the
/// loop.
pub const MAX_JAVASCRIPT_ITERATIONS: i64 = 10;

/// The top-level key under which the list of JavaScript property paths is
/// emitted. This name is preserved with its original casing (it is the one key
/// that is not lowercased).
pub const JAVASCRIPT_PROPERTIES_KEY: &str = "javascriptProperties";

/// The top-level key under which collected flow errors are emitted. The key is
/// `"errors"`.
pub const ERRORS_KEY: &str = "errors";

/// The top-level key under which the flow data's warnings are emitted. The
/// key is `"warnings"`.
pub const WARNINGS_KEY: &str = "warnings";

/// The suffix appended to a property name to emit the reason it has no value.
/// The suffix is `"nullreason"`.
pub const NULL_REASON_SUFFIX: &str = "nullreason";

/// The suffix appended to a property name to flag that its JavaScript should not
/// be executed automatically on the client. The suffix is `"delayexecution"`.
pub const DELAY_EXECUTION_SUFFIX: &str = "delayexecution";

/// The suffix appended to a property name to list the JavaScript properties
/// whose execution would gather evidence for it. The suffix is
/// `"evidenceproperties"`.
pub const EVIDENCE_PROPERTIES_SUFFIX: &str = "evidenceproperties";

/// The element data keys excluded from the JSON output by default.
///
/// These elements either serialize the result themselves (the JSON and
/// JavaScript builders), exist only as a transport detail (the cloud request
/// engine, whose data holds the raw cloud response), or perform a side effect
/// rather than producing data the client needs (set-headers and usage
/// sharing). Each is named by its own crate's data key. Matching is
/// case-insensitive.
pub const DEFAULT_ELEMENT_EXCLUSION_LIST: &[&str] = &[
    fiftyone_cloud_request_engine::ELEMENT_DATA_KEY,
    // The JavaScript builder depends on this crate, so its key is written out
    // here, and the JavaScript builder's tests check that it matches.
    "javascriptbuilderelement",
    JSON_BUILDER_ELEMENT_DATA_KEY,
    fiftyone_pipeline_engines_fiftyone::constants::SET_HEADERS_DEFAULT_ELEMENT_DATA_KEY,
    fiftyone_pipeline_engines_fiftyone::constants::SHARE_USAGE_DEFAULT_ELEMENT_DATA_KEY,
];

/// The property names excluded from the JSON output by default, which is
/// none, so every property an element data lists is written, as .NET writes
/// it. A caller excludes names with
/// [`JsonBuilderElementBuilder::exclude_property`](crate::JsonBuilderElementBuilder::exclude_property).
pub const DEFAULT_PROPERTY_EXCLUSION_LIST: &[&str] = &[];
