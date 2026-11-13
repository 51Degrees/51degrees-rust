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

//! The element data: the robots.txt text with and without annotations.

use std::any::Any;

use fiftyone_pipeline_core::{ElementData, NoValueError, PropertyValue, TypedKey};
use fiftyone_pipeline_engines::{AspectData, AspectDataBase, AspectPropertyValue};

use crate::constants::{ELEMENT_DATA_KEY, PROPERTY_ANNOTATED_TEXT, PROPERTY_PLAIN_TEXT};

/// The key to read [`RobotsTxtData`] from flow data with
/// [`FlowData::get`](fiftyone_pipeline_core::FlowData::get).
pub const ROBOTS_TXT_DATA_KEY: TypedKey<RobotsTxtData> = TypedKey::new(ELEMENT_DATA_KEY);

/// The robots.txt text the engine wrote for one request.
///
/// The engine writes this data only when the request carries one of its
/// evidence keys, and writes only the properties it was built to give.
#[derive(Debug, Clone)]
pub struct RobotsTxtData {
    base: AspectDataBase,
}

impl RobotsTxtData {
    pub(crate) fn new() -> Self {
        RobotsTxtData {
            base: AspectDataBase::new(ELEMENT_DATA_KEY),
        }
    }

    pub(crate) fn insert(&mut self, name: &str, text: String) {
        self.base.insert(name, PropertyValue::String(text));
    }

    /// The robots.txt text with only the copyright comment, ready to serve
    /// as the site's `/robots.txt`.
    pub fn plain_text(&self) -> AspectPropertyValue<String> {
        self.text(PROPERTY_PLAIN_TEXT)
    }

    /// The robots.txt text with the licence, a legend and comments saying
    /// how each crawler is treated and where to find out more. Not intended
    /// to be served.
    pub fn annotated_text(&self) -> AspectPropertyValue<String> {
        self.text(PROPERTY_ANNOTATED_TEXT)
    }

    fn text(&self, name: &str) -> AspectPropertyValue<String> {
        match self.base.get(name) {
            Ok(PropertyValue::String(text)) => AspectPropertyValue::new(text),
            Ok(_) => AspectPropertyValue::no_value(format!("'{name}' is not text.")),
            Err(error) => AspectPropertyValue::no_value(error.message),
        }
    }
}

impl ElementData for RobotsTxtData {
    fn get(&self, name: &str) -> Result<PropertyValue, NoValueError> {
        self.base.get(name)
    }

    fn has_property(&self, name: &str) -> bool {
        self.base.has_property(name)
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

impl AspectData for RobotsTxtData {
    fn engine_keys(&self) -> &[String] {
        self.base.engine_keys()
    }

    fn cache_hit(&self) -> bool {
        self.base.cache_hit()
    }
}
