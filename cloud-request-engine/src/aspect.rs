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

//! The processing every Cloud Aspect Engine shares, as the
//! [Cloud Aspect Engine specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/cloud-aspect-engine.md#processing)
//! gives it.
//!
//! [`aspect_response`] reads the Cloud Request Engine's response for one
//! request and takes the portion for the engine's Data Key, and
//! [`aspect_values`] gives each property of a portion with its value, or
//! with the reason it has none. Each engine turns those values into its own
//! Element Data.
//!
//! A property with no value is a JSON `null`. Its reason is the entry named
//! after it with the suffix [`NULL_REASON_SUFFIX`], or its entry in the
//! [`NULL_VALUE_REASONS`] object beside the properties, each found in any
//! letter case, or [`UNKNOWN_REASON`] when there is neither, as .NET's
//! `CloudAspectEngineBase` gives it. The entries that give reasons are not
//! properties.

use serde_json::{Map, Value};

use fiftyone_pipeline_core::{Error, FlowData, Result};

use crate::engine::CloudRequestEngine;
use crate::response::cloud_error;

/// The suffix of the entry giving the reason the property it is named after
/// has no value.
pub const NULL_REASON_SUFFIX: &str = "nullreason";

/// The entry, beside the properties, mapping each property name to the
/// reason it has no value.
pub const NULL_VALUE_REASONS: &str = "nullValueReasons";

/// The reason for a property with no value whose response gives none.
pub const UNKNOWN_REASON: &str = "Unknown";

/// The Cloud Request Engine's response to one request, as a Cloud Aspect
/// Engine reads it.
#[derive(Debug, Clone, PartialEq)]
pub enum AspectResponse {
    /// The Cloud Request Engine's request failed, so there is no response.
    /// That engine recorded its own error. The Cloud Aspect Engine records
    /// that it did not process with
    /// [`CloudRequestEngine::record_not_processed`], and gives data marked as
    /// from a failed cloud request.
    NoResponse,
    /// The portion of the response for the engine's Data Key, which is empty
    /// when the response has none.
    Portion(Map<String, Value>),
}

/// One property of a portion, as [`aspect_values`] gives it.
#[derive(Debug, Clone, PartialEq)]
pub enum AspectValue<'a> {
    /// The property's value.
    Value(&'a Value),
    /// The property has no value, for this reason.
    NoValue(String),
}

/// Read the Cloud Request Engine's response in `data` and take the portion
/// for `data_key`, found in any letter case.
///
/// Returns an [`Error::PipelineConfiguration`] naming `engine_name` when no
/// Cloud Request Engine processed `data` before the calling engine, and an
/// [`Error::CloudRequest`] when the response is not JSON.
pub fn aspect_response(
    data: &FlowData,
    data_key: &str,
    engine_name: &str,
) -> Result<AspectResponse> {
    let cloud = data
        .get(CloudRequestEngine::DATA_KEY)
        .filter(|cloud| cloud.process_started())
        .ok_or_else(|| {
            Error::configuration(format!(
                "No cloud request data is present. This is probably because there is no \
                 'CloudRequestEngine' before the '{engine_name}' in the pipeline. This \
                 engine cannot produce results until that is corrected."
            ))
        })?;

    match cloud.json_response() {
        Some(json) if !json.is_empty() => {
            aspect_portion(json, data_key).map(AspectResponse::Portion)
        }
        _ => Ok(AspectResponse::NoResponse),
    }
}

/// The portion of a whole cloud response for `data_key`, found in any letter
/// case, and empty when the response has none or the member is not an
/// object. Returns an [`Error::CloudRequest`] when `json` is not JSON.
pub fn aspect_portion(json: &str, data_key: &str) -> Result<Map<String, Value>> {
    let root: Value = serde_json::from_str(json).map_err(|error| {
        cloud_error(
            0,
            None,
            format!("failed to parse the cloud response for '{data_key}' as JSON: {error}"),
        )
    })?;
    Ok(root
        .as_object()
        .and_then(|root| field(root, data_key))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default())
}

/// Each property of a portion, in the portion's order, with its value or,
/// for a `null`, the reason it has none. The entries giving reasons are left
/// out.
pub fn aspect_values(portion: &Map<String, Value>) -> Vec<(&str, AspectValue<'_>)> {
    portion
        .iter()
        .filter(|(name, _)| !is_reason_entry(name))
        .map(|(name, value)| {
            let value = if value.is_null() {
                AspectValue::NoValue(null_reason(portion, name))
            } else {
                AspectValue::Value(value)
            };
            (name.as_str(), value)
        })
        .collect()
}

/// The reason the property `name` has no value: its entry with the suffix
/// [`NULL_REASON_SUFFIX`], else its entry in [`NULL_VALUE_REASONS`], each
/// found in any letter case and holding text, else [`UNKNOWN_REASON`].
pub fn null_reason(portion: &Map<String, Value>, name: &str) -> String {
    field(portion, &format!("{name}{NULL_REASON_SUFFIX}"))
        .and_then(Value::as_str)
        .or_else(|| {
            field(portion, NULL_VALUE_REASONS)
                .and_then(Value::as_object)
                .and_then(|reasons| field(reasons, name))
                .and_then(Value::as_str)
        })
        .unwrap_or(UNKNOWN_REASON)
        .to_owned()
}

/// Whether an entry of a portion gives reasons rather than being a property,
/// being one ending [`NULL_REASON_SUFFIX`] or the [`NULL_VALUE_REASONS`]
/// object, in any letter case.
pub fn is_reason_entry(name: &str) -> bool {
    let suffix = NULL_REASON_SUFFIX.len();
    (name.len() >= suffix
        && name.as_bytes()[name.len() - suffix..]
            .eq_ignore_ascii_case(NULL_REASON_SUFFIX.as_bytes()))
        || name.eq_ignore_ascii_case(NULL_VALUE_REASONS)
}

/// The member `name` of a JSON object, by its exact name and then in any
/// letter case.
fn field<'a>(object: &'a Map<String, Value>, name: &str) -> Option<&'a Value> {
    object.get(name).or_else(|| {
        object
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value)
    })
}
