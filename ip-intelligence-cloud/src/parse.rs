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

//! Turns the cloud JSON for the `ip` aspect into an [`IpIntelligenceDataBase`].
//!
//! The cloud response is one JSON object per aspect under a top-level key, so
//! the engine hands this module the value of the `ip` member. That value is an
//! object of property name to encoded value. This module decides, for each
//! property, which weighted value type it carries and inserts it into the shared
//! data through the matching `set_weighted_*` builder, recording a no-value with
//! its reason where the cloud determined nothing.

use std::collections::HashMap;

use fiftyone_ip_intelligence_shared::IpIntelligenceDataBase;
use fiftyone_pipeline_core::{round_angle, WeightedValue};
use serde_json::Value;

/// The JSON field carrying the integer weight factor of a weighted value.
///
/// The cloud emits this in lower case (`rawweighting`), so the lookup below is
/// case-insensitive to be robust either way.
const RAW_WEIGHTING_FIELD: &str = "rawweighting";

/// The JSON field carrying the candidate value of a weighted value.
const VALUE_FIELD: &str = "value";

/// The suffix on a sibling key that carries the no-value reason for a property
/// (`<propertyname>nullreason`).
const NULL_REASON_SUFFIX: &str = "nullreason";

/// The top-level object, a sibling of the property object, that maps property
/// name to no-value reason in the newer cloud response shape.
const NULL_VALUE_REASONS_FIELD: &str = "nullValueReasons";

/// The no-value message used when the cloud sends a `null` value but supplies no
/// reason.
const UNKNOWN_REASON: &str = "Unknown";

/// The value type a property carries, used to pick which shared setter receives
/// it.
///
/// Most IP Intelligence properties are plain (a single value). A small number
/// are weighted: the cloud reports those with a `Weighted<Type>` type name (for
/// example `WeightedString`) and encodes them as an array of
/// `{ rawweighting, value }` objects. The plain kinds map the bare cloud type to
/// the scalar the data model stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValueKind {
    /// A plain string property (string, IP range, WKB geometry, JavaScript,
    /// boolean rendering, and the default for any unrecognised type).
    String,
    /// A plain integer property, for example a time-zone offset.
    Integer,
    /// A plain single-precision float property, for example a latitude.
    Double,
    /// A plain boolean property, for example an `IsVPN` network flag.
    Bool,
    /// A weighted list of string values, the only weighted form IP Intelligence
    /// uses (`CountryCodesGeographical`, `CountryCodesPopulation`, `Mcc`).
    WeightedString,
}

impl ValueKind {
    /// Classify a cloud metadata type name, matched case-insensitively.
    ///
    /// A `Weighted<...>` type is a weighted string. A bare type maps to its plain
    /// scalar, defaulting to [`ValueKind::String`] for any type not specifically
    /// handled (string, IP address, WKT string, JavaScript, boolean, ...).
    pub(crate) fn from_cloud_type(type_name: &str) -> ValueKind {
        let lower = type_name.to_ascii_lowercase();
        if lower.starts_with("weighted") {
            // Every weighted IP Intelligence property is a weighted string list.
            return ValueKind::WeightedString;
        }
        match lower.as_str() {
            // Integer-like cloud types.
            "int" | "integer" | "long" | "int32" | "int64" | "byte" => ValueKind::Integer,
            // Floating-point cloud types.
            "float" | "double" | "single" => ValueKind::Double,
            // Boolean cloud types.
            "bool" | "boolean" => ValueKind::Bool,
            // Everything else (String, IpAddress, WktString, JavaScript, ...) is
            // a plain string.
            _ => ValueKind::String,
        }
    }

    /// Infer the kind from a property's encoded JSON value, used when no cloud
    /// metadata is available for the property.
    ///
    /// An array whose entries are `{ rawweighting, value }` objects is a weighted
    /// string. Otherwise the kind is inferred from the (single) value: a
    /// whole-number is an integer, a fractional number a double, and anything
    /// else a string.
    pub(crate) fn from_json_value(value: &Value) -> ValueKind {
        if let Value::Array(items) = value {
            let weighted = items.iter().any(|item| {
                matches!(item, Value::Object(map) if field_ci(map, RAW_WEIGHTING_FIELD).is_some())
            });
            if weighted {
                return ValueKind::WeightedString;
            }
        }
        let sample = match value {
            Value::Array(items) => items.first().unwrap_or(value),
            other => other,
        };
        match sample {
            Value::Number(n) => {
                if n.is_i64() || n.is_u64() {
                    ValueKind::Integer
                } else {
                    ValueKind::Double
                }
            }
            Value::Bool(_) => ValueKind::Bool,
            _ => ValueKind::String,
        }
    }
}

/// Populate `data` from the `ip` aspect object of the cloud response.
///
/// `aspect` is the JSON object found under the response's `ip` member. `kinds`
/// maps each property name (lower case) to the value kind derived from the cloud
/// metadata; a property absent from the map has its kind inferred from its JSON
/// value. Properties whose value is `null` (or an empty list) are recorded as
/// no-values with the reason found in either a `<name>nullreason` sibling or the
/// top-level `nullValueReasons` object.
pub(crate) fn populate_from_aspect(
    data: &mut IpIntelligenceDataBase,
    aspect: &serde_json::Map<String, Value>,
    kinds: &HashMap<String, ValueKind>,
) {
    // Collect the reasons from the dedicated top-level object first, so a
    // null value can find its message regardless of the encoding in use.
    let reasons = collect_null_reasons(aspect);

    for (name, value) in aspect {
        // The dedicated reasons object and the per-property reason siblings are
        // metadata about other properties, not properties in their own right.
        if name == NULL_VALUE_REASONS_FIELD || is_null_reason_key(name) {
            continue;
        }

        let kind = kinds
            .get(&name.to_ascii_lowercase())
            .copied()
            .unwrap_or_else(|| ValueKind::from_json_value(value));

        match kind {
            // A weighted property is an array of weighted candidates. A non-empty
            // list carries real values; an empty list (the property was `null`,
            // an empty array, or every entry was null/malformed) is a no-value.
            ValueKind::WeightedString => match parse_weighted_list(value) {
                ParsedProperty::Values(list) if !list.is_empty() => {
                    let weighted = list
                        .into_iter()
                        .map(|(weight, value)| WeightedValue::new(weight, value_to_string(&value)))
                        .collect();
                    data.set_weighted_string(name, weighted);
                }
                ParsedProperty::Values(_) | ParsedProperty::NoValue => {
                    let reason = lookup_reason(name, aspect, &reasons);
                    data.set_weighted_string_no_value(name, reason);
                }
            },
            // A plain property carries a single value. A `null` (or an array with
            // no usable entry) is a no-value with its reason.
            _ => match plain_scalar(value) {
                Some(scalar) => insert_plain(data, name, kind, &scalar),
                None => {
                    let reason = lookup_reason(name, aspect, &reasons);
                    insert_no_value(data, name, kind, reason);
                }
            },
        }
    }
}

/// The outcome of parsing a single property's encoded value.
enum ParsedProperty {
    /// The parsed weighted candidates. The list may be empty when the array was
    /// empty or every entry was null or malformed, in which case the caller
    /// treats it the same as [`ParsedProperty::NoValue`].
    Values(Vec<(u16, Value)>),
    /// The value was `null`, signalling the property has no value.
    NoValue,
}

/// Parse a property's JSON value into a list of `(raw_weighting, candidate)`
/// pairs, or signal a no-value.
///
/// The cloud encodes a weighted property as an array of objects, each with a
/// `rawweighting` and a `value`. A `null` value is a no-value. For robustness a
/// bare scalar (a non-array, non-null value) is treated as a single candidate
/// with full weighting, so a response that omits the weighting wrapper still
/// yields a usable value.
fn parse_weighted_list(value: &Value) -> ParsedProperty {
    match value {
        Value::Null => ParsedProperty::NoValue,
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                if let Some((weight, candidate)) = parse_weighted_object(item) {
                    out.push((weight, candidate));
                }
            }
            ParsedProperty::Values(out)
        }
        // A bare scalar with no weighting wrapper: treat it as a single
        // candidate at full weighting.
        other => ParsedProperty::Values(vec![(u16::MAX, other.clone())]),
    }
}

/// Parse one `{ "rawweighting": .., "value": .. }` object into its parts.
///
/// Returns `None` when the object is missing the value or the value is `null`,
/// so a malformed or empty entry is skipped rather than producing a spurious
/// candidate. A missing weighting defaults to full weighting.
fn parse_weighted_object(item: &Value) -> Option<(u16, Value)> {
    match item {
        Value::Object(map) => {
            let candidate = field_ci(map, VALUE_FIELD)?;
            if candidate.is_null() {
                return None;
            }
            let weight = field_ci(map, RAW_WEIGHTING_FIELD)
                .and_then(Value::as_u64)
                .map(|w| w.min(u64::from(u16::MAX)) as u16)
                .unwrap_or(u16::MAX);
            Some((weight, candidate.clone()))
        }
        // A bare scalar inside the array (no weighting wrapper) is a single
        // full-weight candidate.
        Value::Null => None,
        other => Some((u16::MAX, other.clone())),
    }
}

/// Extract the single plain value a plain property carries.
///
/// A bare scalar is taken directly. An array (some cloud encodings wrap even a
/// single plain value) yields its first usable entry, unwrapping a
/// `{ value: .. }` object. A `null`, an empty array, or an array of only null or
/// malformed entries yields [`None`], signalling a no-value.
fn plain_scalar(value: &Value) -> Option<Value> {
    match value {
        Value::Null => None,
        Value::Array(items) => items.iter().find_map(|item| match item {
            Value::Null => None,
            Value::Object(map) => field_ci(map, VALUE_FIELD)
                .filter(|inner| !inner.is_null())
                .cloned(),
            other => Some(other.clone()),
        }),
        other => Some(other.clone()),
    }
}

/// Insert a single plain value into `data`, converting it to the kind's value
/// type. A numeric value that fails to parse falls back to its string form so
/// the value is still surfaced through the dynamic bag. A float is given to 3
/// decimal places by [`round_angle`], because the IP Intelligence floats are
/// the latitude and the longitude.
fn insert_plain(data: &mut IpIntelligenceDataBase, name: &str, kind: ValueKind, value: &Value) {
    match kind {
        ValueKind::String => data.set_string(name, value_to_string(value)),
        ValueKind::Integer => match value_to_i64(value) {
            Some(int) => data.set_integer(name, int as i32),
            None => data.set_string(name, value_to_string(value)),
        },
        ValueKind::Double => match value_to_f64(value) {
            Some(float) => data.set_float(name, round_angle(float)),
            None => data.set_string(name, value_to_string(value)),
        },
        ValueKind::Bool => match value_to_bool(value) {
            Some(flag) => data.set_boolean(name, flag),
            None => data.set_string(name, value_to_string(value)),
        },
        // Weighted properties are handled by the caller, not here.
        ValueKind::WeightedString => {
            data.set_weighted_string(
                name,
                vec![WeightedValue::new(u16::MAX, value_to_string(value))],
            );
        }
    }
}

/// Record a no-value for `name` of the given kind, with `reason` as its message.
fn insert_no_value(data: &mut IpIntelligenceDataBase, name: &str, kind: ValueKind, reason: String) {
    match kind {
        ValueKind::String => data.set_string_no_value(name, reason),
        ValueKind::Integer => data.set_integer_no_value(name, reason),
        ValueKind::Double => data.set_float_no_value(name, reason),
        ValueKind::Bool => data.set_boolean_no_value(name, reason),
        ValueKind::WeightedString => data.set_weighted_string_no_value(name, reason),
    }
}

/// Find the no-value reason for `name`, checking the `<name>nullreason` sibling
/// first and then the top-level `nullValueReasons` object, falling back to
/// `"Unknown"`.
fn lookup_reason(
    name: &str,
    aspect: &serde_json::Map<String, Value>,
    reasons: &HashMap<String, String>,
) -> String {
    let sibling = format!("{name}{NULL_REASON_SUFFIX}");
    if let Some(Value::String(message)) = field_ci(aspect, &sibling) {
        return message.clone();
    }
    if let Some(message) = reasons.get(&name.to_ascii_lowercase()) {
        return message.clone();
    }
    UNKNOWN_REASON.to_owned()
}

/// Build the lower-cased name to reason map from the top-level
/// `nullValueReasons` object, if present.
fn collect_null_reasons(aspect: &serde_json::Map<String, Value>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(Value::Object(reasons)) = field_ci(aspect, NULL_VALUE_REASONS_FIELD) {
        for (name, value) in reasons {
            if let Value::String(message) = value {
                map.insert(name.to_ascii_lowercase(), message.clone());
            }
        }
    }
    map
}

/// True if `name` is a `<propertyname>nullreason` sibling key.
fn is_null_reason_key(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(NULL_REASON_SUFFIX)
}

/// Look up a field in a JSON object case-insensitively, since the cloud mixes
/// PascalCase property names with lower-case weighted-value field names.
fn field_ci<'a>(map: &'a serde_json::Map<String, Value>, name: &str) -> Option<&'a Value> {
    if let Some(value) = map.get(name) {
        return Some(value);
    }
    let lower = name.to_ascii_lowercase();
    map.iter()
        .find(|(key, _)| key.to_ascii_lowercase() == lower)
        .map(|(_, value)| value)
}

/// Render a JSON candidate to the string form a weighted-string property stores.
/// Strings are taken verbatim; other scalars use their JSON text so a value the
/// cloud sent as a number or bool still reads back sensibly.
fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Convert a JSON candidate to an `i64`, accepting an integer number or a string
/// that parses as one.
fn value_to_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

/// Convert a JSON candidate to an `f64`, accepting any number or a string that
/// parses as one.
fn value_to_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// Convert a JSON candidate to a `bool`, accepting a JSON boolean, the textual
/// forms (`true`/`false`, `yes`/`no`) or the numeric forms (`1`/`0`).
fn value_to_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => n.as_i64().map(|i| i != 0),
        Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Some(true),
            "false" | "0" | "no" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fiftyone_ip_intelligence_shared::IP_DATA_KEY_NAME;
    use fiftyone_pipeline_core::{ElementData, PropertyValue};

    /// The data key the test data is attributed to.
    const ENGINE_KEY: &str = IP_DATA_KEY_NAME;

    fn aspect(json: &str) -> serde_json::Map<String, Value> {
        match serde_json::from_str::<Value>(json).unwrap() {
            Value::Object(map) => map,
            other => panic!("expected an object, got {other:?}"),
        }
    }

    #[test]
    fn value_kind_classifies_cloud_types() {
        // Any Weighted* type is a weighted string.
        assert_eq!(
            ValueKind::from_cloud_type("WeightedString"),
            ValueKind::WeightedString
        );
        assert_eq!(
            ValueKind::from_cloud_type("weightedinteger"),
            ValueKind::WeightedString
        );
        // Plain scalar types map to their kind.
        assert_eq!(ValueKind::from_cloud_type("Int32"), ValueKind::Integer);
        assert_eq!(ValueKind::from_cloud_type("Single"), ValueKind::Double);
        assert_eq!(ValueKind::from_cloud_type("Float"), ValueKind::Double);
        // String, IP address and WKT string are all plain strings.
        assert_eq!(ValueKind::from_cloud_type("String"), ValueKind::String);
        assert_eq!(ValueKind::from_cloud_type("IPAddress"), ValueKind::String);
        assert_eq!(ValueKind::from_cloud_type("WktString"), ValueKind::String);
    }

    #[test]
    fn parses_weighted_string_array() {
        // A weighted property is an array of { rawweighting, value } objects; the
        // array shape alone classifies it as weighted when no metadata is given.
        let map = aspect(
            r#"{ "CountryCodesGeographical": [
                { "rawweighting": 20000, "value": "GB" },
                { "rawweighting": 60000, "value": "FR" }
            ] }"#,
        );
        let mut data = IpIntelligenceDataBase::new(ENGINE_KEY);
        populate_from_aspect(&mut data, &map, &HashMap::new());

        let list = data
            .weighted_string("CountryCodesGeographical")
            .into_value()
            .unwrap();
        // The shared setter sorts high weighting first.
        assert_eq!(list[0].value, "FR");
        assert_eq!(list[0].raw_weighting, 60000);
        assert_eq!(list[1].value, "GB");
    }

    #[test]
    fn parses_plain_string() {
        let map = aspect(r#"{ "Country": "United Kingdom" }"#);
        let mut data = IpIntelligenceDataBase::new(ENGINE_KEY);
        populate_from_aspect(&mut data, &map, &HashMap::new());
        assert_eq!(
            data.string("Country").into_value().unwrap(),
            "United Kingdom"
        );
    }

    #[test]
    fn parses_plain_integer_and_float_by_metadata() {
        // The cloud may still wrap a single plain value in an array; the plain
        // kinds unwrap it to one scalar.
        let map = aspect(
            r#"{
                "TimeZoneOffset": [ { "rawweighting": 65535, "value": 60 } ],
                "Latitude": [ { "rawweighting": 65535, "value": 51.45 } ]
            }"#,
        );
        let mut kinds = HashMap::new();
        kinds.insert("timezoneoffset".to_owned(), ValueKind::Integer);
        kinds.insert("latitude".to_owned(), ValueKind::Double);

        let mut data = IpIntelligenceDataBase::new(ENGINE_KEY);
        populate_from_aspect(&mut data, &map, &kinds);

        assert_eq!(*data.integer("TimeZoneOffset").value().unwrap(), 60_i32);
        assert!((data.float("Latitude").value().unwrap() - 51.45_f32).abs() < 1e-4);
    }

    /// A latitude or longitude the response gives with more places is given
    /// to 3, as the on-premise engine gives it, and kept as a 32-bit float.
    #[test]
    fn a_float_is_given_to_three_decimal_places() {
        let map = aspect(
            r#"{
                "Latitude": 18.542741172521133,
                "Longitude": [ { "rawweighting": 65535, "value": -0.0625 } ]
            }"#,
        );
        let mut kinds = HashMap::new();
        kinds.insert("latitude".to_owned(), ValueKind::Double);
        kinds.insert("longitude".to_owned(), ValueKind::Double);

        let mut data = IpIntelligenceDataBase::new(ENGINE_KEY);
        populate_from_aspect(&mut data, &map, &kinds);

        assert_eq!(*data.float("Latitude").value().unwrap(), 18.543_f32);
        assert_eq!(*data.float("Longitude").value().unwrap(), -0.063_f32);
        assert_eq!(data.get("Latitude").unwrap(), PropertyValue::Float(18.543));
    }

    #[test]
    fn plain_null_with_sibling_reason() {
        let map = aspect(
            r#"{
                "RegisteredName": null,
                "RegisteredNamenullreason": "no data for this IP"
            }"#,
        );
        let mut kinds = HashMap::new();
        kinds.insert("registeredname".to_owned(), ValueKind::String);

        let mut data = IpIntelligenceDataBase::new(ENGINE_KEY);
        populate_from_aspect(&mut data, &map, &kinds);

        let value = data.string("RegisteredName");
        assert!(!value.has_value());
        assert_eq!(value.no_value_message(), Some("no data for this IP"));
    }

    #[test]
    fn plain_null_with_top_level_reasons() {
        let map = aspect(
            r#"{
                "CountryCode": null,
                "nullValueReasons": { "CountryCode": "empty result" }
            }"#,
        );
        let mut data = IpIntelligenceDataBase::new(ENGINE_KEY);
        populate_from_aspect(&mut data, &map, &HashMap::new());

        let value = data.string("CountryCode");
        assert!(!value.has_value());
        assert_eq!(value.no_value_message(), Some("empty result"));
    }

    #[test]
    fn plain_null_without_reason_is_unknown() {
        let map = aspect(r#"{ "Town": null }"#);
        let mut data = IpIntelligenceDataBase::new(ENGINE_KEY);
        populate_from_aspect(&mut data, &map, &HashMap::new());
        assert_eq!(data.string("Town").no_value_message(), Some(UNKNOWN_REASON));
    }

    #[test]
    fn plain_empty_array_surfaces_sibling_reason() {
        let map = aspect(
            r#"{
                "CountryCode": [],
                "CountryCodenullreason": "no data for this IP"
            }"#,
        );
        let mut data = IpIntelligenceDataBase::new(ENGINE_KEY);
        populate_from_aspect(&mut data, &map, &HashMap::new());

        let value = data.string("CountryCode");
        assert!(!value.has_value());
        assert_eq!(value.no_value_message(), Some("no data for this IP"));
    }

    #[test]
    fn weighted_all_null_array_is_a_no_value() {
        let map = aspect(
            r#"{ "CountryCodesGeographical": [
                { "rawweighting": 20000, "value": null },
                { "rawweighting": 60000 }
            ] }"#,
        );
        let mut data = IpIntelligenceDataBase::new(ENGINE_KEY);
        populate_from_aspect(&mut data, &map, &HashMap::new());

        // Every entry was null or malformed, so the property has no value.
        assert!(!data.weighted_string("CountryCodesGeographical").has_value());
    }

    #[test]
    fn weighting_defaults_to_full_when_absent() {
        let map = aspect(r#"{ "CountryCodesGeographical": [ { "value": "GB" } ] }"#);
        let mut kinds = HashMap::new();
        kinds.insert(
            "countrycodesgeographical".to_owned(),
            ValueKind::WeightedString,
        );
        let mut data = IpIntelligenceDataBase::new(ENGINE_KEY);
        populate_from_aspect(&mut data, &map, &kinds);
        let list = data
            .weighted_string("CountryCodesGeographical")
            .into_value()
            .unwrap();
        assert_eq!(list[0].raw_weighting, u16::MAX);
    }

    #[test]
    fn plain_bare_scalar_is_a_value() {
        let map = aspect(r#"{ "Country": "GB" }"#);
        let mut data = IpIntelligenceDataBase::new(ENGINE_KEY);
        populate_from_aspect(&mut data, &map, &HashMap::new());
        assert_eq!(data.string("Country").into_value().unwrap(), "GB");
    }
}
