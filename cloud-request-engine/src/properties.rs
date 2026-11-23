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

//! The accessible-properties metadata returned by the cloud `accessibleproperties`
//! endpoint.
//!
//! These types deserialize the response body so downstream cloud aspect engines
//! can discover which products and properties the configured resource key
//! grants.
//!
//! The cloud uses PascalCase field names (for example `Products`, `Properties`,
//! `Name`), so [`serde`] aliases accept both PascalCase and camelCase to be
//! robust to either casing.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The top-level accessible-properties response.
///
/// `products` is keyed on the product name (for example `device`, `location`),
/// each value listing the properties the resource key grants for that product.
///
/// The deserialize aliases accept the cloud's PascalCase or camelCase field
/// names. Serialization writes the snake_case field names, which deserialize
/// again unchanged, and the products are held in a [`BTreeMap`] so the serialized
/// form is deterministic (sorted by product name). The type therefore round-trips
/// through [`CloudEngineState`](crate::CloudEngineState) byte-for-byte, so a
/// discovered snapshot can be persisted, content-addressed and re-injected.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LicensedProducts {
    /// The accessible products, keyed by product name.
    #[serde(alias = "Products", alias = "products", default)]
    pub products: BTreeMap<String, ProductMetaData>,

    /// Any error messages returned alongside the property metadata.
    #[serde(alias = "Errors", alias = "errors", default)]
    pub errors: Vec<String>,
}

/// The metadata for a single product the resource key grants access to.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProductMetaData {
    /// The accessible data tier for this product, when supplied.
    #[serde(alias = "DataTier", alias = "dataTier", default)]
    pub data_tier: Option<String>,

    /// The properties accessible for this product.
    #[serde(alias = "Properties", alias = "properties", default)]
    pub properties: Vec<CloudPropertyMetaData>,
}

/// The metadata for a single property within a product.
///
/// The `value_type` is the JSON type name the cloud reports (for example
/// `String`, `Bool`, `Array`, `WeightedString`), not a native Rust type, so a
/// downstream cloud aspect engine can reconstruct the original shape.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CloudPropertyMetaData {
    /// The property name.
    #[serde(alias = "Name", alias = "name", default)]
    pub name: String,

    /// The JSON type name reported for the property's values.
    #[serde(alias = "Type", alias = "type", default)]
    pub value_type: String,

    /// The property category, when supplied.
    #[serde(alias = "Category", alias = "category", default)]
    pub category: Option<String>,

    /// Whether execution of a JavaScript property is delayed on the client.
    #[serde(alias = "DelayExecution", alias = "delayExecution", default)]
    pub delay_execution: bool,

    /// The names of evidence (JavaScript) properties that gather extra evidence
    /// for this property.
    #[serde(alias = "EvidenceProperties", alias = "evidenceProperties", default)]
    pub evidence_properties: Vec<String>,

    /// The metadata for sub-item properties, where the value is a collection of
    /// complex objects.
    #[serde(alias = "ItemProperties", alias = "itemProperties", default)]
    pub item_properties: Vec<CloudPropertyMetaData>,
}

impl LicensedProducts {
    /// Parse the accessible-properties JSON body.
    pub fn parse(json: &str) -> Result<LicensedProducts, serde_json::Error> {
        serde_json::from_str(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pascal_case_response() {
        let json = r#"{
            "Products": {
                "device": {
                    "DataTier": "CloudV4",
                    "Properties": [
                        { "Name": "IsMobile", "Type": "Bool", "Category": "Device" },
                        { "Name": "HardwareName", "Type": "Array" }
                    ]
                }
            }
        }"#;
        let products = LicensedProducts::parse(json).unwrap();
        let device = products.products.get("device").unwrap();
        assert_eq!(device.data_tier.as_deref(), Some("CloudV4"));
        assert_eq!(device.properties.len(), 2);
        assert_eq!(device.properties[0].name, "IsMobile");
        assert_eq!(device.properties[0].value_type, "Bool");
        assert_eq!(device.properties[0].category.as_deref(), Some("Device"));
    }

    #[test]
    fn parses_camel_case_response() {
        let json =
            r#"{"products":{"location":{"properties":[{"name":"Country","type":"String"}]}}}"#;
        let products = LicensedProducts::parse(json).unwrap();
        let location = products.products.get("location").unwrap();
        assert_eq!(location.properties[0].name, "Country");
    }

    #[test]
    fn missing_optional_fields_default() {
        let json = r#"{"Products":{"device":{"Properties":[{"Name":"X","Type":"String"}]}}}"#;
        let products = LicensedProducts::parse(json).unwrap();
        let device = products.products.get("device").unwrap();
        assert!(device.data_tier.is_none());
        assert!(device.properties[0].evidence_properties.is_empty());
        assert!(!device.properties[0].delay_execution);
    }
}
