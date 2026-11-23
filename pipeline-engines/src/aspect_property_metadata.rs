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

//! Aspect-specific property metadata.
//!
//! [`AspectPropertyMetaData`] layers the two extra fields that aspect engines
//! publish on top of the core [`PropertyMetaData`]: a free-text `description`
//! and the list of `data_tiers_where_present`. It realises the
//! [aspect-property-metadata section](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/properties.md#aspect-property-metadata)
//! of the properties specification.
//!
//! The core [`PropertyMetaData`] deliberately knows nothing about aspect
//! concepts, so this type wraps it rather than the core type carrying these
//! fields. The wrapped core metadata is still available through
//! [`AspectPropertyMetaData::core`] so the value can be handed to any API that
//! works with the plain core type, including
//! [`fiftyone_pipeline_core::FlowElement::properties`].

use fiftyone_pipeline_core::{PropertyMetaData, PropertyValueType};

/// Metadata describing a property populated by an aspect engine.
///
/// Wraps a core [`PropertyMetaData`] and adds the aspect-only `description` and
/// `data_tiers_where_present`. The data tiers drive missing-property reasoning
/// for on-premise engines: when a property is in the engine metadata but the
/// engine's current data tier is not in this list, the value is missing because
/// a data-file upgrade is required (see
/// [`crate::MissingPropertyService`]).
#[derive(Debug, Clone, PartialEq)]
pub struct AspectPropertyMetaData {
    /// The wrapped core metadata, carrying the name, owning element data key,
    /// value type and the rest of the specification metadata fields.
    core: PropertyMetaData,

    /// A free-text description of the property, as published by the engine.
    /// Empty when the engine supplies no description.
    description: String,

    /// The data tiers (for example `Lite`, `Premium`, `Enterprise`) whose data
    /// files contain a value for this property. Empty for cloud engines, which
    /// do not expose tiers.
    data_tiers_where_present: Vec<String>,
}

impl AspectPropertyMetaData {
    /// Create aspect metadata for an available property with the given name,
    /// owning element data key and value type.
    ///
    /// The description and data tiers start empty and can be set with the
    /// builder-style methods. The wrapped core metadata can be further refined
    /// through [`AspectPropertyMetaData::map_core`].
    pub fn new(
        name: impl Into<String>,
        element_data_key: impl Into<String>,
        value_type: PropertyValueType,
    ) -> Self {
        AspectPropertyMetaData {
            core: PropertyMetaData::new(name, element_data_key, value_type),
            description: String::new(),
            data_tiers_where_present: Vec::new(),
        }
    }

    /// Wrap an existing core [`PropertyMetaData`] as aspect metadata, with an
    /// empty description and no data tiers.
    pub fn from_core(core: PropertyMetaData) -> Self {
        AspectPropertyMetaData {
            core,
            description: String::new(),
            data_tiers_where_present: Vec::new(),
        }
    }

    /// Set the description. Returns `self` for chaining.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set the data tiers where the property is present. Returns `self` for
    /// chaining.
    pub fn with_data_tiers<I, S>(mut self, tiers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.data_tiers_where_present = tiers.into_iter().map(Into::into).collect();
        self
    }

    /// Apply a transformation to the wrapped core metadata, for example to set
    /// the category or availability through the core builder methods. Returns
    /// `self` for chaining.
    pub fn map_core<F>(mut self, f: F) -> Self
    where
        F: FnOnce(PropertyMetaData) -> PropertyMetaData,
    {
        self.core = f(self.core);
        self
    }

    /// The property name. Convenience accessor that reads through to the core
    /// metadata.
    pub fn name(&self) -> &str {
        &self.core.name
    }

    /// The data key of the element that populates this property.
    pub fn element_data_key(&self) -> &str {
        &self.core.element_data_key
    }

    /// Whether the property is currently available to the caller.
    pub fn available(&self) -> bool {
        self.core.available
    }

    /// The property's free-text description.
    pub fn description(&self) -> &str {
        &self.description
    }

    /// The data tiers in whose data files this property is present.
    pub fn data_tiers_where_present(&self) -> &[String] {
        &self.data_tiers_where_present
    }

    /// Borrow the wrapped core [`PropertyMetaData`].
    pub fn core(&self) -> &PropertyMetaData {
        &self.core
    }

    /// Consume this instance, returning the wrapped core [`PropertyMetaData`].
    pub fn into_core(self) -> PropertyMetaData {
        self.core
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_with_aspect_fields() {
        let meta = AspectPropertyMetaData::new("IsMobile", "device", PropertyValueType::Bool)
            .with_description("True if the device is a mobile phone.")
            .with_data_tiers(["Lite", "Premium", "Enterprise"])
            .map_core(|c| c.with_category("Hardware"));

        assert_eq!(meta.name(), "IsMobile");
        assert_eq!(meta.element_data_key(), "device");
        assert!(meta.available());
        assert_eq!(meta.description(), "True if the device is a mobile phone.");
        assert_eq!(
            meta.data_tiers_where_present(),
            ["Lite", "Premium", "Enterprise"]
        );
        assert_eq!(meta.core().category, "Hardware");
    }

    #[test]
    fn from_core_preserves_core_fields() {
        let core =
            PropertyMetaData::new("x", "elem", PropertyValueType::String).with_available(false);
        let meta = AspectPropertyMetaData::from_core(core);
        assert!(!meta.available());
        assert_eq!(meta.description(), "");
        assert!(meta.data_tiers_where_present().is_empty());
    }
}
