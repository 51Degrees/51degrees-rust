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

//! Mapping the cloud `device` JSON object into a [`DeviceDataBase`].
//!
//! The cloud service returns a single JSON document whose `device` member holds
//! the device-detection result. Each property appears as a flat entry, for
//! example `"ismobile": true` or `"hardwarename": ["iPhone"]`. When a property
//! has no value the cloud sends JSON `null` and pairs it with a sibling entry
//! named `<property>nullreason` that explains why, for example
//! `"hardwarevendor": null, "hardwarevendornullreason": "No matching profiles"`.
//!
//! [`map_device_object`] turns that object into a [`DeviceDataBase`], the same
//! concrete type the on-premise engine populates, so the two engines are
//! interface-compatible (see the [crate documentation](crate)).
//!
//! ## How a value becomes a typed [`PropertyValue`]
//!
//! The cloud JSON is dynamically typed, so each value is mapped to the closest
//! [`PropertyValue`] variant by inspecting the JSON node:
//!
//! - a JSON boolean becomes [`PropertyValue::Bool`],
//! - an integer-valued number becomes [`PropertyValue::Integer`], a fractional
//!   number becomes [`PropertyValue::Double`],
//! - a string becomes [`PropertyValue::String`],
//! - an array of scalars becomes [`PropertyValue::StringList`] (each element
//!   stringified).
//!
//! The shared [`DeviceDataBase`] typed accessors read these back and coerce them
//! as needed (for example a one-element list is accepted for a scalar list
//! accessor), so a caller sees the same shape whichever engine ran.
//!
//! ## No-value handling
//!
//! A `null` property is held with no value through
//! [`DeviceDataBase::insert_no_value`], with the reason the shared Cloud
//! Aspect Engine processing in [`fiftyone_cloud_request_engine::aspect`]
//! gives it, as step 4 of the
//! [cloud aspect engine specification's processing](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/cloud-aspect-engine.md#processing)
//! says. A typed accessor or `get` for the property gives that message, and
//! the JSON builder writes it back as `<property>nullreason`, as the
//! [null values feature](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/properties.md#null-values)
//! describes.

use fiftyone_cloud_request_engine::aspect::{aspect_values, AspectValue};
use fiftyone_device_detection_shared::DeviceDataBase;
use fiftyone_pipeline_core::PropertyValue;
use serde_json::{Map, Value};

/// Build a [`DeviceDataBase`] from the cloud `device` JSON object, or one
/// profile of the `hardware` object, through the shared Cloud Aspect Engine
/// processing in [`fiftyone_cloud_request_engine::aspect`].
///
/// Every non-null property is written into the data's dynamic bag as the
/// nearest [`PropertyValue`]. A null property is held with no value, with
/// the reason [`aspect_values`] gives it. The entries giving reasons are left
/// out. The keys are stored under their cloud names (lower case as the cloud
/// sends them); the bag folds case, so the shared canonical accessors such as
/// `IsMobile` still resolve them.
///
/// Returns an empty [`DeviceDataBase`] if `device` is not a JSON object, which
/// keeps a malformed or unexpected payload from panicking.
pub(crate) fn map_device_object(device: &Value) -> DeviceDataBase {
    match device.as_object() {
        Some(object) => map_device_portion(object),
        None => DeviceDataBase::new(),
    }
}

/// As [`map_device_object`], for an object already taken from the response.
pub(crate) fn map_device_portion(object: &Map<String, Value>) -> DeviceDataBase {
    let mut data = DeviceDataBase::new();
    for (name, value) in aspect_values(object) {
        match value {
            AspectValue::NoValue(reason) => data.insert_no_value(name, reason),
            AspectValue::Value(value) => {
                if let Some(property) = json_to_property_value(value) {
                    data.insert(name, property);
                }
            }
        }
    }
    data
}

/// Convert a single cloud JSON value into the nearest [`PropertyValue`].
///
/// Returns `None` for a JSON `null` (handled separately) or an empty object,
/// neither of which maps onto a scalar device property.
fn json_to_property_value(value: &Value) -> Option<PropertyValue> {
    match value {
        Value::Bool(b) => Some(PropertyValue::Bool(*b)),
        Value::Number(number) => {
            // The cloud sends device metrics as integers and a few properties
            // (for example screen sizes in millimetres) as fractional numbers.
            // Prefer an integer when the number has no fractional part, narrowing
            // integral values to an integer.
            if let Some(i) = number.as_i64() {
                Some(PropertyValue::Integer(i))
            } else {
                number.as_f64().map(PropertyValue::Double)
            }
        }
        Value::String(s) => Some(PropertyValue::String(s.clone())),
        Value::Array(items) => Some(PropertyValue::StringList(
            items.iter().map(json_scalar_to_string).collect(),
        )),
        // A bare object with no list context is not a device scalar property, so
        // it is skipped rather than guessed at.
        Value::Object(_) | Value::Null => None,
    }
}

/// Stringify a scalar JSON array element, turning each non-object array item
/// into its string form.
fn json_scalar_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        // An array element that is itself a structure is rendered as compact
        // JSON, so no information is lost even though it is not a plain scalar.
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fiftyone_cloud_request_engine::aspect::{is_reason_entry, UNKNOWN_REASON};
    use fiftyone_device_detection_shared::DeviceData;
    use fiftyone_pipeline_core::ElementData;

    #[test]
    fn maps_scalar_values_to_typed_properties() {
        let device = serde_json::json!({
            "ismobile": true,
            "hardwarevendor": "Apple",
            "screenpixelswidth": 1170,
            "screenmmwidth": 71.5
        });
        let data = map_device_object(&device);

        assert!(*data.is_mobile().value().unwrap());
        assert_eq!(data.hardware_vendor().value().unwrap(), "Apple");
        assert_eq!(*data.screen_pixels_width().value().unwrap(), 1170);
        // A property without a typed accessor is reachable through the bag and
        // keeps its fractional type.
        assert_eq!(data.get("screenmmwidth").unwrap().as_double(), Some(71.5));
    }

    #[test]
    fn maps_array_to_string_list() {
        let device = serde_json::json!({ "hardwarename": ["iPhone", "iPhone 15"] });
        let data = map_device_object(&device);
        assert_eq!(
            data.hardware_name().value().unwrap(),
            &["iPhone".to_owned(), "iPhone 15".to_owned()]
        );
    }

    #[test]
    fn null_property_holds_its_reason() {
        let device = serde_json::json!({
            "hardwarevendor": null,
            "hardwarevendornullreason": "No matching profiles were found."
        });
        let data = map_device_object(&device);

        let vendor = data.hardware_vendor();
        assert!(!vendor.has_value());
        assert_eq!(
            vendor.no_value_message(),
            Some("No matching profiles were found.")
        );
        assert_eq!(
            data.get("HardwareVendor").unwrap_err().message,
            "No matching profiles were found."
        );
        assert!(data.has_property("hardwarevendor"));
    }

    #[test]
    fn null_reason_entries_are_left_out() {
        let device = serde_json::json!({
            "ismobile": true,
            "hardwarevendor": null,
            "hardwarevendornullreason": "No matching profiles were found.",
            "orphannullreason": "No property of this name.",
            "nullreason": "No name at all."
        });
        let data = map_device_object(&device);

        assert_eq!(data.keys(), vec!["ismobile", "hardwarevendor"]);
        assert!(data.get("hardwarevendornullreason").is_err());
        assert!(data.get("orphannullreason").is_err());
        assert!(data.get("nullreason").is_err());
    }

    #[test]
    fn null_property_with_no_reason_is_unknown() {
        let device = serde_json::json!({
            "hardwarevendor": null,
            "platformname": null,
            "platformnamenullreason": 7
        });
        let data = map_device_object(&device);

        assert_eq!(
            data.hardware_vendor().no_value_message(),
            Some(UNKNOWN_REASON)
        );
        assert_eq!(
            data.platform_name().no_value_message(),
            Some(UNKNOWN_REASON)
        );
    }

    #[test]
    fn null_reason_is_found_ignoring_case() {
        let device = serde_json::json!({
            "HardwareVendor": null,
            "hardwarevendorNullReason": "No matching profiles were found."
        });
        let data = map_device_object(&device);

        assert_eq!(
            data.hardware_vendor().no_value_message(),
            Some("No matching profiles were found.")
        );
        assert_eq!(data.keys(), vec!["hardwarevendor"]);
    }

    #[test]
    fn non_object_payload_yields_empty_data() {
        let data = map_device_object(&serde_json::json!("not an object"));
        assert!(data.keys().is_empty());
    }

    #[test]
    fn null_reason_key_detection_is_case_insensitive() {
        assert!(is_null_reason_key("hardwarevendornullreason"));
        assert!(is_null_reason_key("HardwareVendorNullReason"));
        assert!(is_null_reason_key("nullreason"));
        assert!(!is_null_reason_key("reason"));
        assert!(!is_null_reason_key("ismobile"));
        assert!(!is_null_reason_key("nullreasonx"));
    }
}
