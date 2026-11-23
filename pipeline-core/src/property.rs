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

//! Property metadata.
//!
//! Every flow element MUST publish metadata describing the properties it can
//! populate, per the
//! [properties specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/properties.md#property-metadata).
//! [`PropertyMetaData`] carries the fields from that specification's metadata
//! table. Aspect engines layer additional fields (description, data tiers) on
//! top; this type is built with `..Default::default()`-friendly setters so that
//! the engines crate can wrap it without this core type needing to know about
//! aspect concepts.

use crate::value::PropertyValueType;

/// Metadata describing a single property that a flow element can populate.
///
/// The element that owns the property is identified by its string data key
/// (`element_data_key`) rather
/// than by a back-reference to the element object, which keeps the metadata a
/// plain value that is cheap to clone and free of lifetime entanglements.
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyMetaData {
    /// The name of the property. Matches the string key used to store the
    /// value in the element data.
    pub name: String,

    /// The data key of the [`crate::FlowElement`] that populates this property.
    pub element_data_key: String,

    /// The category the property belongs to, used to organize elements that
    /// populate many properties. Empty if uncategorised.
    pub category: String,

    /// The type of the values this property returns.
    pub value_type: PropertyValueType,

    /// Whether the property is currently available to the caller. If `false`,
    /// the value will not be present in element data.
    pub available: bool,

    /// Only relevant for JavaScript properties. If `true`, the JavaScript is
    /// not executed automatically on the client device. See the
    /// [web-integration notes](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/web-integration.md).
    pub delay_execution: bool,

    /// The names of any JavaScript properties that, when executed, gather extra
    /// evidence that helps determine this property's value.
    pub evidence_properties: Vec<String>,

    /// Where this property's value is a collection of complex objects, the
    /// metadata for the properties of those objects. Empty otherwise. Currently
    /// only used by the hardware-profile lookup engine.
    pub item_properties: Vec<PropertyMetaData>,
}

impl PropertyMetaData {
    /// Create metadata for an available property with the given name, owning
    /// element data key and value type. The remaining fields take their
    /// defaults (no category, available, no delayed execution, no evidence or
    /// item properties) and can be set with the builder-style methods.
    pub fn new(
        name: impl Into<String>,
        element_data_key: impl Into<String>,
        value_type: PropertyValueType,
    ) -> Self {
        PropertyMetaData {
            name: name.into(),
            element_data_key: element_data_key.into(),
            category: String::new(),
            value_type,
            available: true,
            delay_execution: false,
            evidence_properties: Vec::new(),
            item_properties: Vec::new(),
        }
    }

    /// Set the property category. Returns `self` for chaining.
    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.category = category.into();
        self
    }

    /// Set the `available` flag. Returns `self` for chaining.
    pub fn with_available(mut self, available: bool) -> Self {
        self.available = available;
        self
    }

    /// Set the `delay_execution` flag. Returns `self` for chaining.
    pub fn with_delay_execution(mut self, delay_execution: bool) -> Self {
        self.delay_execution = delay_execution;
        self
    }

    /// Set the evidence property names. Returns `self` for chaining.
    pub fn with_evidence_properties<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.evidence_properties = names.into_iter().map(Into::into).collect();
        self
    }

    /// Set the item-property metadata. Returns `self` for chaining.
    pub fn with_item_properties<I>(mut self, item_properties: I) -> Self
    where
        I: IntoIterator<Item = PropertyMetaData>,
    {
        self.item_properties = item_properties.into_iter().collect();
        self
    }
}
