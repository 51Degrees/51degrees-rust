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

//! The 51Degrees metadata model.
//!
//! 51Degrees data files expose richer metadata than the generic pipeline
//! metadata. Properties belong to a component (for example Hardware, Browser),
//! carry display and presentation hints, and enumerate the values they can
//! return. This module follows the
//! [data-model specification](https://github.com/51Degrees/specifications/blob/main/data-model-specification/README.md).
//!
//! The types are plain owned value objects rather than the lazily-resolved,
//! data-file-backed objects the on-premise engines use, which keeps them usable
//! for both cloud and on-premise engines and free of any native dependency. The
//! 51Degrees property metadata composes the engines crate's
//! [`AspectPropertyMetaData`] so the same value can still be handed to any API
//! that takes the core or aspect metadata.

use fiftyone_pipeline_core::{PropertyMetaData, PropertyValueType};
use fiftyone_pipeline_engines::AspectPropertyMetaData;

/// Metadata for a single value a property can return, for example `"True"` or
/// `"Samsung"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueMetaData {
    /// The name of the value, for example `"True"`.
    pub name: String,
    /// A full description of the value's meaning. Empty when none is supplied.
    pub description: String,
    /// A URL with more information about the value. Empty when none is supplied.
    pub url: String,
}

impl ValueMetaData {
    /// Create value metadata with the given name and no description or URL.
    pub fn new(name: impl Into<String>) -> Self {
        ValueMetaData {
            name: name.into(),
            description: String::new(),
            url: String::new(),
        }
    }

    /// Set the description. Returns `self` for chaining.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set the URL. Returns `self` for chaining.
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = url.into();
        self
    }
}

/// 51Degrees-specific property metadata.
///
/// Wraps an [`AspectPropertyMetaData`] (which itself wraps the core
/// [`PropertyMetaData`]) and adds the presentation and classification fields
/// that 51Degrees data files publish: the owning component name, a URL, a
/// display order, and the mandatory/list/obsolete/show/show-values flags, plus
/// the list of [`ValueMetaData`] the property can return.
#[derive(Debug, Clone, PartialEq)]
pub struct FiftyOneAspectPropertyMetaData {
    aspect: AspectPropertyMetaData,
    /// The name of the component this property belongs to. Empty if none.
    component_name: String,
    /// A URL with more information about the property.
    url: String,
    /// The order in which to display the property. Lower is shown first.
    display_order: u8,
    /// True if a value for the property is always present.
    mandatory: bool,
    /// True if the property returns a list of values.
    list: bool,
    /// True if the property is obsolete.
    obsolete: bool,
    /// True if the property should be shown in user interfaces.
    show: bool,
    /// True if the property's values should be shown in user interfaces.
    show_values: bool,
    /// The values the property can return.
    values: Vec<ValueMetaData>,
}

impl FiftyOneAspectPropertyMetaData {
    /// Create 51Degrees property metadata for an available property.
    ///
    /// The presentation flags default to a sensible visible-but-optional
    /// property (`show` and `show_values` true, the rest false), with display
    /// order `0` and no values. Use the builder methods to refine them.
    pub fn new(
        name: impl Into<String>,
        element_data_key: impl Into<String>,
        value_type: PropertyValueType,
    ) -> Self {
        Self::from_aspect(AspectPropertyMetaData::new(
            name,
            element_data_key,
            value_type,
        ))
    }

    /// Wrap an existing [`AspectPropertyMetaData`] with default 51Degrees
    /// presentation fields.
    pub fn from_aspect(aspect: AspectPropertyMetaData) -> Self {
        FiftyOneAspectPropertyMetaData {
            aspect,
            component_name: String::new(),
            url: String::new(),
            display_order: 0,
            mandatory: false,
            list: false,
            obsolete: false,
            show: true,
            show_values: true,
            values: Vec::new(),
        }
    }

    /// Set the owning component name. Returns `self` for chaining.
    pub fn with_component_name(mut self, name: impl Into<String>) -> Self {
        self.component_name = name.into();
        self
    }

    /// Set the property URL. Returns `self` for chaining.
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = url.into();
        self
    }

    /// Set the display order. Returns `self` for chaining.
    pub fn with_display_order(mut self, display_order: u8) -> Self {
        self.display_order = display_order;
        self
    }

    /// Set the `mandatory` flag. Returns `self` for chaining.
    pub fn with_mandatory(mut self, mandatory: bool) -> Self {
        self.mandatory = mandatory;
        self
    }

    /// Set the `list` flag. Returns `self` for chaining.
    pub fn with_list(mut self, list: bool) -> Self {
        self.list = list;
        self
    }

    /// Set the `obsolete` flag. Returns `self` for chaining.
    pub fn with_obsolete(mut self, obsolete: bool) -> Self {
        self.obsolete = obsolete;
        self
    }

    /// Set the `show` flag. Returns `self` for chaining.
    pub fn with_show(mut self, show: bool) -> Self {
        self.show = show;
        self
    }

    /// Set the `show_values` flag. Returns `self` for chaining.
    pub fn with_show_values(mut self, show_values: bool) -> Self {
        self.show_values = show_values;
        self
    }

    /// Set the values this property can return. Returns `self` for chaining.
    pub fn with_values<I>(mut self, values: I) -> Self
    where
        I: IntoIterator<Item = ValueMetaData>,
    {
        self.values = values.into_iter().collect();
        self
    }

    /// The property name.
    pub fn name(&self) -> &str {
        self.aspect.name()
    }

    /// The name of the component this property belongs to.
    pub fn component_name(&self) -> &str {
        &self.component_name
    }

    /// A URL with more information about the property.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The display order. Lower is shown first.
    pub fn display_order(&self) -> u8 {
        self.display_order
    }

    /// True if a value for the property is always present.
    pub fn mandatory(&self) -> bool {
        self.mandatory
    }

    /// True if the property returns a list of values.
    pub fn list(&self) -> bool {
        self.list
    }

    /// True if the property is obsolete.
    pub fn obsolete(&self) -> bool {
        self.obsolete
    }

    /// True if the property should be shown.
    pub fn show(&self) -> bool {
        self.show
    }

    /// True if the property's values should be shown.
    pub fn show_values(&self) -> bool {
        self.show_values
    }

    /// The values this property can return.
    pub fn values(&self) -> &[ValueMetaData] {
        &self.values
    }

    /// Find the value with the given name, matched case-insensitively.
    pub fn value(&self, name: &str) -> Option<&ValueMetaData> {
        self.values
            .iter()
            .find(|v| v.name.eq_ignore_ascii_case(name))
    }

    /// Borrow the wrapped [`AspectPropertyMetaData`].
    pub fn aspect(&self) -> &AspectPropertyMetaData {
        &self.aspect
    }

    /// Borrow the wrapped core [`PropertyMetaData`].
    pub fn core(&self) -> &PropertyMetaData {
        self.aspect.core()
    }
}

/// Metadata for a component of an engine's results, for example Hardware.
///
/// A component groups a set of properties and has a unique id.
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentMetaData {
    /// The unique id of the component within the data set.
    pub component_id: u8,
    /// The name of the component, for example `"HardwareProfile"`.
    pub name: String,
    /// The properties that belong to this component.
    properties: Vec<FiftyOneAspectPropertyMetaData>,
}

impl ComponentMetaData {
    /// Create a component with the given id and name and no properties.
    pub fn new(component_id: u8, name: impl Into<String>) -> Self {
        ComponentMetaData {
            component_id,
            name: name.into(),
            properties: Vec::new(),
        }
    }

    /// Set the component's properties. Returns `self` for chaining.
    pub fn with_properties<I>(mut self, properties: I) -> Self
    where
        I: IntoIterator<Item = FiftyOneAspectPropertyMetaData>,
    {
        self.properties = properties.into_iter().collect();
        self
    }

    /// The properties that belong to this component.
    pub fn properties(&self) -> &[FiftyOneAspectPropertyMetaData] {
        &self.properties
    }

    /// Find the property with the given name, matched case-insensitively.
    pub fn property(&self, name: &str) -> Option<&FiftyOneAspectPropertyMetaData> {
        self.properties
            .iter()
            .find(|p| p.name().eq_ignore_ascii_case(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_builds_with_fiftyone_fields() {
        let property =
            FiftyOneAspectPropertyMetaData::new("IsMobile", "device", PropertyValueType::Bool)
                .with_component_name("Hardware")
                .with_url("https://example.com/IsMobile")
                .with_display_order(3)
                .with_mandatory(true)
                .with_list(false)
                .with_obsolete(false)
                .with_show(true)
                .with_show_values(true)
                .with_values([
                    ValueMetaData::new("True").with_description("It is mobile."),
                    ValueMetaData::new("False"),
                ]);

        assert_eq!(property.name(), "IsMobile");
        assert_eq!(property.component_name(), "Hardware");
        assert_eq!(property.display_order(), 3);
        assert!(property.mandatory());
        assert_eq!(property.values().len(), 2);
        assert_eq!(property.value("true").unwrap().description, "It is mobile.");
        assert_eq!(property.core().value_type, PropertyValueType::Bool);
    }

    #[test]
    fn component_finds_property_case_insensitively() {
        let component = ComponentMetaData::new(1, "Hardware").with_properties([
            FiftyOneAspectPropertyMetaData::new("IsMobile", "device", PropertyValueType::Bool),
        ]);

        assert_eq!(component.component_id, 1);
        assert!(component.property("ismobile").is_some());
        assert!(component.property("missing").is_none());
    }
}
