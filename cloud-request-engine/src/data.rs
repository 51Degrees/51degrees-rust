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

//! The element data produced by the cloud request engine.
//!
//! [`CloudRequestData`] holds the raw JSON response from the 51Degrees cloud
//! service. Downstream cloud aspect engines (device detection, IP intelligence
//! and so on) read this JSON and deserialize the parts they own into their own
//! aspect data, so this engine makes exactly one HTTP request per flow data
//! regardless of how many aspects are involved.
//!
//! The raw JSON is stored under two field names so every consumer can find it:
//!
//! - `cloud-response`, the data key, so the JSON can be read by the same name
//!   the data is stored under.
//! - `json-response`, the field name the other 51Degrees implementations use.
//!
//! A `process-started` boolean records that the engine ran, which lets a
//! consumer distinguish "the engine produced an empty result" from "the engine
//! never ran" (for example because a prior element failed).

use std::any::Any;

use fiftyone_pipeline_core::{ElementData, NoValueError, PropertyValue};
use fiftyone_pipeline_engines::{AspectData, AspectDataBase};

use crate::constants;

/// The element data written into a flow data by the cloud request engine.
///
/// It embeds an [`AspectDataBase`] so it is a full aspect data (a property bag
/// that also records the engine key and cache-hit flag) while exposing typed
/// accessors for the cloud JSON and the process-started flag. It is `Clone` so
/// it can be cached by the aspect engine base.
#[derive(Debug, Clone)]
pub struct CloudRequestData {
    base: AspectDataBase,
}

impl CloudRequestData {
    /// Create cloud request data attributed to the engine identified by
    /// `engine_key`, with the process-started flag set to `false`.
    pub fn new(engine_key: impl Into<String>) -> Self {
        let base = AspectDataBase::new(engine_key).set(constants::PROCESS_STARTED_KEY, false);
        CloudRequestData { base }
    }

    /// Store the raw JSON response body, under both the `cloud-response` and
    /// `json-response` field names. Returns `self` for chaining.
    pub fn with_json_response(mut self, json: impl Into<String>) -> Self {
        self.set_json_response(json);
        self
    }

    /// Set the process-started flag. Returns `self` for chaining.
    pub fn with_process_started(mut self, started: bool) -> Self {
        self.base.insert(constants::PROCESS_STARTED_KEY, started);
        self
    }

    /// Store the raw JSON response body in place, under both field names.
    pub fn set_json_response(&mut self, json: impl Into<String>) {
        let json = json.into();
        self.base.insert(constants::ELEMENT_DATA_KEY, json.clone());
        self.base.insert(constants::JSON_RESPONSE_KEY, json);
    }

    /// Set the process-started flag in place.
    pub fn set_process_started(&mut self, started: bool) {
        self.base.insert(constants::PROCESS_STARTED_KEY, started);
    }

    /// Mark this result as having been served from the engine's response cache,
    /// so a consumer reading [`AspectData::cache_hit`] sees the hit.
    pub fn set_cache_hit(&mut self) {
        self.base.set_cache_hit();
    }

    /// Store the warning messages the cloud service returned, under the
    /// `warnings` field. Warnings are advisory and do not represent a failure.
    pub fn set_warnings(&mut self, warnings: Vec<String>) {
        self.base.insert(constants::WARNINGS_KEY, warnings);
    }

    /// The warning messages the cloud service returned, if any.
    pub fn warnings(&self) -> Vec<String> {
        match self.base.values().get_value(constants::WARNINGS_KEY) {
            Some(PropertyValue::StringList(list)) => list.clone(),
            _ => Vec::new(),
        }
    }

    /// The raw JSON response body, if it has been set. Reads through the
    /// `cloud-response` field.
    pub fn json_response(&self) -> Option<&str> {
        match self.base.values().get_value(constants::ELEMENT_DATA_KEY) {
            Some(PropertyValue::String(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    /// True if the engine has started processing this flow data.
    pub fn process_started(&self) -> bool {
        matches!(
            self.base.values().get_value(constants::PROCESS_STARTED_KEY),
            Some(PropertyValue::Bool(true))
        )
    }

    /// Borrow the underlying aspect-data base.
    pub fn base(&self) -> &AspectDataBase {
        &self.base
    }
}

impl ElementData for CloudRequestData {
    fn get(&self, name: &str) -> Result<PropertyValue, NoValueError> {
        self.base.get(name)
    }

    fn keys(&self) -> Vec<String> {
        self.base.keys()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl AspectData for CloudRequestData {
    fn engine_keys(&self) -> &[String] {
        self.base.engine_keys()
    }

    fn cache_hit(&self) -> bool {
        self.base.cache_hit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_json_under_both_keys() {
        let data = CloudRequestData::new(constants::ELEMENT_DATA_KEY)
            .with_json_response(r#"{"device":{}}"#)
            .with_process_started(true);
        assert_eq!(data.json_response(), Some(r#"{"device":{}}"#));
        // Both field names resolve to the same JSON.
        assert_eq!(
            data.get(constants::ELEMENT_DATA_KEY).unwrap().as_str(),
            Some(r#"{"device":{}}"#)
        );
        assert_eq!(
            data.get(constants::JSON_RESPONSE_KEY).unwrap().as_str(),
            Some(r#"{"device":{}}"#)
        );
        assert!(data.process_started());
    }

    #[test]
    fn process_started_defaults_false() {
        let data = CloudRequestData::new(constants::ELEMENT_DATA_KEY);
        assert!(!data.process_started());
        assert_eq!(data.json_response(), None);
    }

    #[test]
    fn unknown_property_is_no_value() {
        let data = CloudRequestData::new(constants::ELEMENT_DATA_KEY);
        assert!(data.get("not-a-field").is_err());
    }
}
