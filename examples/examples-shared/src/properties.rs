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

//! Render a property value to a display string.
//!
//! Examples print many properties without caring about their underlying type,
//! so this helper reads a property by name from any
//! [`fiftyone_pipeline_core::ElementData`] (the dynamic property bag) and turns
//! whatever it finds into a string. It handles the absent and no-value cases by
//! returning a clear marker rather than failing.

use fiftyone_pipeline_core::{ElementData, PropertyValue};

/// The marker returned when a property is absent from the element data, or
/// present but has no value (the
/// [null-values rule](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/properties.md#null-values)).
pub const NO_VALUE_MARKER: &str = "Unknown";

/// Read the property `name` from `data` and render it as a display string.
///
/// Looks the property up in the dynamic bag ([`ElementData::get`], which matches
/// names case-insensitively) and formats the result:
///
/// - a string or JavaScript value is returned as-is,
/// - a boolean, integer or double is formatted with its natural `Display`,
/// - a list of strings is joined with `", "`,
/// - a list of key-value collections is rendered compactly as
///   `[{k=v, ...}, ...]`.
///
/// When the property is missing, or present but holds no value, the
/// [`NO_VALUE_MARKER`] is returned so the caller always gets a printable string.
pub fn get_property_as_string(data: &dyn ElementData, name: &str) -> String {
    match data.get(name) {
        Ok(value) => property_value_to_string(&value),
        // `get` returns an error both for an unknown property name and for a
        // present-but-no-value property. Examples treat both the same way: there
        // is nothing to display, so the marker is returned.
        Err(_) => NO_VALUE_MARKER.to_owned(),
    }
}

/// Render a single [`PropertyValue`] to a string using the rules described on
/// [`get_property_as_string`].
pub fn property_value_to_string(value: &PropertyValue) -> String {
    match value {
        PropertyValue::String(s) | PropertyValue::JavaScript(s) => s.clone(),
        PropertyValue::Bool(b) => b.to_string(),
        PropertyValue::Integer(i) => i.to_string(),
        PropertyValue::Double(d) => d.to_string(),
        PropertyValue::StringList(list) => list.join(", "),
        PropertyValue::KeyValueList(records) => {
            let rendered: Vec<String> = records
                .iter()
                .map(|record| {
                    let pairs: Vec<String> = record
                        .iter()
                        .map(|(k, v)| format!("{k}={}", property_value_to_string(v)))
                        .collect();
                    format!("{{{}}}", pairs.join(", "))
                })
                .collect();
            format!("[{}]", rendered.join(", "))
        }
        // `PropertyValue` is non-exhaustive, so a future value type renders with
        // the no-value marker rather than failing to compile here.
        _ => NO_VALUE_MARKER.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use fiftyone_pipeline_core::MapElementData;

    fn bag() -> MapElementData {
        MapElementData::new()
            .set("IsMobile", true)
            .set("PlatformName", "Windows")
            .set("Weight", 42i64)
            .set("Ratio", 1.5f64)
            .set(
                "HardwareVendorList",
                vec!["Samsung".to_owned(), "Apple".to_owned()],
            )
    }

    #[test]
    fn renders_scalar_values() {
        let data = bag();
        assert_eq!(get_property_as_string(&data, "IsMobile"), "true");
        assert_eq!(get_property_as_string(&data, "PlatformName"), "Windows");
        assert_eq!(get_property_as_string(&data, "Weight"), "42");
        assert_eq!(get_property_as_string(&data, "Ratio"), "1.5");
    }

    #[test]
    fn is_case_insensitive() {
        let data = bag();
        assert_eq!(get_property_as_string(&data, "ismobile"), "true");
        assert_eq!(get_property_as_string(&data, "PLATFORMNAME"), "Windows");
    }

    #[test]
    fn joins_list_values() {
        let data = bag();
        assert_eq!(
            get_property_as_string(&data, "HardwareVendorList"),
            "Samsung, Apple"
        );
    }

    #[test]
    fn missing_property_returns_marker() {
        let data = bag();
        assert_eq!(
            get_property_as_string(&data, "Nonexistent"),
            NO_VALUE_MARKER
        );
    }

    #[test]
    fn renders_key_value_list() {
        let mut record = BTreeMap::new();
        record.insert("Model".to_owned(), PropertyValue::String("X".to_owned()));
        record.insert("Year".to_owned(), PropertyValue::Integer(2026));
        let value = PropertyValue::KeyValueList(vec![record]);
        assert_eq!(property_value_to_string(&value), "[{Model=X, Year=2026}]");
    }
}
