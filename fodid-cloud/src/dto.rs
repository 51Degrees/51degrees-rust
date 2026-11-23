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

//! Mapping the cloud `fodid` JSON object into a [`FodIdDataBase`].
//!
//! The cloud service returns the identifier values under its `fodid` member,
//! each a base64-encoded OWID envelope string, for example
//! `"idprobglobal": "AzUxZC5lcwBzGTMA..."`. When a value cannot be issued the
//! cloud sends JSON `null` paired with a sibling `<property>nullreason` entry
//! explaining why, exactly as the device and IP blocks do.
//!
//! [`map_fodid_object`] writes each non-null identifier string into the bag
//! under its cloud name (lower case, which the case-folding bag still resolves
//! to the canonical `IdProbGlobal` / `IdProbLic` accessors) and preserves any
//! `nullreason` sibling so a no-value keeps the cloud's explanation.

use fiftyone_fodid_core::FodIdDataBase;
use fiftyone_pipeline_core::PropertyValue;
use serde_json::Value;

/// The case-insensitive suffix the cloud uses for the sibling entry that
/// explains why a property has no value.
pub(crate) const NULL_REASON_SUFFIX: &str = "nullreason";

/// Build a [`FodIdDataBase`] from the cloud `fodid` JSON object.
///
/// `fodid` is the `fodid` member of the cloud response, already isolated from
/// the surrounding document. Each non-null identifier string is written into the
/// data's dynamic bag; null values are skipped but their `<property>nullreason`
/// sibling is preserved so the no-value explanation is not lost.
///
/// Returns an empty [`FodIdDataBase`] if `fodid` is not a JSON object, which
/// keeps a malformed or unexpected payload from panicking.
pub(crate) fn map_fodid_object(fodid: &Value) -> FodIdDataBase {
    let mut data = FodIdDataBase::new();

    let object = match fodid.as_object() {
        Some(object) => object,
        None => return data,
    };

    for (name, value) in object {
        // The nullreason siblings are stored verbatim so a no-value identifier
        // keeps its cloud message; they are not identifiers in their own right.
        if is_null_reason_key(name) {
            if let Value::String(message) = value {
                data.insert(name, message.clone());
            }
            continue;
        }

        // A null value carries no identifier: leave it absent so the typed
        // accessor reports a no-value. The paired nullreason (handled above)
        // keeps the reason.
        if value.is_null() {
            continue;
        }

        // The identifiers are base64 strings. A string maps straight through;
        // any other scalar is stringified defensively so nothing is silently
        // lost, while a structural value (array or object) is not an identifier
        // and is skipped.
        match value {
            Value::String(s) => data.insert(name, PropertyValue::String(s.clone())),
            Value::Bool(b) => data.insert(name, PropertyValue::String(b.to_string())),
            Value::Number(n) => data.insert(name, PropertyValue::String(n.to_string())),
            Value::Array(_) | Value::Object(_) | Value::Null => {}
        }
    }

    data
}

/// True if `name` is a `<property>nullreason` sibling key, matched
/// case-insensitively as the cloud may use any casing.
fn is_null_reason_key(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.len() > NULL_REASON_SUFFIX.len() && name.ends_with(NULL_REASON_SUFFIX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fiftyone_fodid_core::FodIdData;
    use fiftyone_pipeline_core::ElementData;

    #[test]
    fn maps_both_identifier_strings() {
        let fodid = serde_json::json!({
            "idprobglobal": "AzUxZC5lcwBzGTMA",
            "idproblic": "bGljZW5jZQ"
        });
        let data = map_fodid_object(&fodid);
        assert_eq!(data.id_prob_global().value().unwrap(), "AzUxZC5lcwBzGTMA");
        assert_eq!(data.id_prob_lic().value().unwrap(), "bGljZW5jZQ");
    }

    #[test]
    fn null_identifier_is_absent_and_reason_preserved() {
        let fodid = serde_json::json!({
            "idprobglobal": null,
            "idprobglobalnullreason": "The usage policy does not permit a global identifier."
        });
        let data = map_fodid_object(&fodid);

        assert!(!data.id_prob_global().has_value());
        assert_eq!(
            data.get("idprobglobalnullreason").unwrap().as_str(),
            Some("The usage policy does not permit a global identifier.")
        );
    }

    #[test]
    fn non_object_payload_yields_empty_data() {
        let data = map_fodid_object(&serde_json::json!("not an object"));
        assert!(data.keys().is_empty());
    }

    #[test]
    fn structural_values_are_skipped() {
        let fodid = serde_json::json!({ "idprobglobal": ["unexpected", "array"] });
        let data = map_fodid_object(&fodid);
        assert!(!data.id_prob_global().has_value());
    }

    #[test]
    fn null_reason_key_detection_is_case_insensitive() {
        assert!(is_null_reason_key("idprobglobalnullreason"));
        assert!(is_null_reason_key("IdProbLicNullReason"));
        assert!(!is_null_reason_key("nullreason"));
        assert!(!is_null_reason_key("idprobglobal"));
    }
}
