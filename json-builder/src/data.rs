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

//! The element data produced by the JSON builder.

use std::any::Any;

use fiftyone_pipeline_core::{ElementData, NoValueError, PropertyValue, TypedKey};

use crate::constants::JSON_PROPERTY_KEY;

/// The typed key for retrieving [`JsonBuilderData`] from a flow data.
///
/// Its name is the JSON builder's element data key, so a caller can do
/// `flow_data.get(JSON_BUILDER_DATA_KEY)` to recover the strongly-typed data.
pub const JSON_BUILDER_DATA_KEY: TypedKey<JsonBuilderData> =
    TypedKey::new(crate::constants::JSON_BUILDER_ELEMENT_DATA_KEY);

/// The element data the JSON builder writes into the flow data.
///
/// It carries exactly one value, the serialized JSON document, accessible
/// through [`JsonBuilderData::json`] or by the property name
/// [`crate::JSON_PROPERTY_KEY`].
#[derive(Debug, Clone, Default)]
pub struct JsonBuilderData {
    json: String,
}

impl JsonBuilderData {
    /// Create empty JSON builder data. The element fills it in during
    /// processing.
    pub fn new() -> Self {
        JsonBuilderData {
            json: String::new(),
        }
    }

    /// The serialized JSON document.
    pub fn json(&self) -> &str {
        &self.json
    }

    /// Replace the serialized JSON document. Used by the element once it has
    /// built the document.
    pub fn set_json(&mut self, json: impl Into<String>) {
        self.json = json.into();
    }
}

impl ElementData for JsonBuilderData {
    fn get(&self, name: &str) -> Result<PropertyValue, NoValueError> {
        // Only the `json` property is owned by this data. Per the coordination
        // rules, any other name must report no value so that
        // FlowData::get_evidence_or_property stays unambiguous.
        if name.eq_ignore_ascii_case(JSON_PROPERTY_KEY) {
            Ok(PropertyValue::String(self.json.clone()))
        } else {
            Err(NoValueError::new(format!(
                "No value for property '{name}'."
            )))
        }
    }

    fn keys(&self) -> Vec<String> {
        vec![JSON_PROPERTY_KEY.to_owned()]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
