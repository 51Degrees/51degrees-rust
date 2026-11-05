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

//! The builder for [`JsonBuilderElement`].

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

use crate::constants::{DEFAULT_ELEMENT_EXCLUSION_LIST, DEFAULT_PROPERTY_EXCLUSION_LIST};
use crate::converter::JsonValueConverter;
use crate::element::JsonBuilderElement;
use crate::steps::{DefaultJsonBuilderSteps, JsonBuilderSteps};

/// Configures and constructs a [`JsonBuilderElement`].
///
/// The element and property exclusion lists start at the defaults
/// ([`crate::DEFAULT_ELEMENT_EXCLUSION_LIST`] and
/// [`crate::DEFAULT_PROPERTY_EXCLUSION_LIST`]). The methods below add to or
/// replace them. [`JsonBuilderElementBuilder::set_properties`] limits the
/// output to the listed properties. All matching is case-insensitive, so
/// names are stored lowercased. [`JsonBuilderElementBuilder::set_steps`] and
/// [`JsonBuilderElementBuilder::add_json_converter`] change how the output is
/// written.
///
/// # Example
///
/// ```
/// use fiftyone_json_builder::JsonBuilderElement;
///
/// // Keep the defaults but also hide a property and a whole element.
/// let element = JsonBuilderElement::builder()
///     .exclude_property("setheaderbrowseraccept-ch")
///     .exclude_element("debug")
///     .build();
/// ```
#[derive(Clone)]
pub struct JsonBuilderElementBuilder {
    element_exclusion: HashSet<String>,
    property_exclusion: HashSet<String>,
    selected_properties: HashSet<String>,
    steps: Arc<dyn JsonBuilderSteps>,
    converters: Vec<Arc<dyn JsonValueConverter>>,
}

impl JsonBuilderElementBuilder {
    /// Start a builder pre-populated with the default exclusion lists.
    pub fn new() -> Self {
        JsonBuilderElementBuilder {
            element_exclusion: lowercased(DEFAULT_ELEMENT_EXCLUSION_LIST),
            property_exclusion: lowercased(DEFAULT_PROPERTY_EXCLUSION_LIST),
            selected_properties: HashSet::new(),
            steps: Arc::new(DefaultJsonBuilderSteps),
            converters: Vec::new(),
        }
    }

    /// Use these steps to write the JSON in place of the builder's own. See
    /// [`JsonBuilderSteps`].
    pub fn set_steps(mut self, steps: impl JsonBuilderSteps + 'static) -> Self {
        self.steps = Arc::new(steps);
        self
    }

    /// Add a converter, asked after any added before it. See
    /// [`JsonValueConverter`].
    pub fn add_json_converter(mut self, converter: impl JsonValueConverter + 'static) -> Self {
        self.converters.push(Arc::new(converter));
        self
    }

    /// Replace the converters with these, in this order.
    pub fn set_json_converters<I>(mut self, converters: I) -> Self
    where
        I: IntoIterator<Item = Arc<dyn JsonValueConverter>>,
    {
        self.converters = converters.into_iter().collect();
        self
    }

    /// Add a single property name to the exclusion list. The name is matched
    /// case-insensitively.
    pub fn exclude_property(mut self, name: impl AsRef<str>) -> Self {
        self.property_exclusion.insert(name.as_ref().to_lowercase());
        self
    }

    /// Add several property names to the exclusion list.
    pub fn exclude_properties<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for name in names {
            self.property_exclusion.insert(name.as_ref().to_lowercase());
        }
        self
    }

    /// Replace the entire property exclusion list with the supplied names,
    /// discarding the defaults.
    pub fn set_property_exclusion_list<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.property_exclusion = names
            .into_iter()
            .map(|n| n.as_ref().to_lowercase())
            .collect();
        self
    }

    /// Write only these properties, replacing any list set before. An empty
    /// list, the default, writes every property. This is the `SetProperties`
    /// option of the
    /// [specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/json-builder.md#configuration-options).
    ///
    /// Each name is a complete name, being the element data key and the
    /// property name joined by a dot, as in `device.ismobile`. A property in
    /// a list of records adds the list's name, as in
    /// `hardware.profiles.hardwarename`, which keeps the `profiles` list with
    /// only that property in each record. Listing `hardware.profiles` keeps
    /// every property of each record. Names are matched without regard to
    /// case.
    ///
    /// JavaScript properties are always written, and every element keeps its
    /// entry, which is empty when none of its properties is selected. A
    /// property's `nullreason`, `delayexecution` and `evidenceproperties`
    /// keys are written with the property and left out with it. See
    /// [`JsonBuilderElement::is_property_selected`].
    ///
    /// # Example
    ///
    /// ```
    /// use fiftyone_json_builder::JsonBuilderElement;
    ///
    /// let element = JsonBuilderElement::builder()
    ///     .set_properties(["device.ismobile", "hardware.profiles.hardwarename"])
    ///     .build();
    /// assert!(element.is_property_selected("device.IsMobile"));
    /// assert!(element.is_property_selected("hardware.profiles"));
    /// assert!(!element.is_property_selected("device.browsername"));
    /// ```
    pub fn set_properties<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.selected_properties = names
            .into_iter()
            .map(|n| n.as_ref().to_lowercase())
            .collect();
        self
    }

    /// Add a single element data key to the exclusion list. The key is matched
    /// case-insensitively.
    pub fn exclude_element(mut self, key: impl AsRef<str>) -> Self {
        self.element_exclusion.insert(key.as_ref().to_lowercase());
        self
    }

    /// Add several element data keys to the exclusion list.
    pub fn exclude_elements<I, S>(mut self, keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for key in keys {
            self.element_exclusion.insert(key.as_ref().to_lowercase());
        }
        self
    }

    /// Replace the entire element exclusion list with the supplied keys,
    /// discarding the defaults.
    ///
    /// Use with care: the JSON and JavaScript builders normally exclude
    /// themselves through the defaults, so a replacement list should keep
    /// `json-builder` and `javascript` to avoid the builders serializing their
    /// own output.
    pub fn set_element_exclusion_list<I, S>(mut self, keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.element_exclusion = keys
            .into_iter()
            .map(|k| k.as_ref().to_lowercase())
            .collect();
        self
    }

    /// Build the configured [`JsonBuilderElement`].
    pub fn build(self) -> JsonBuilderElement {
        JsonBuilderElement::from_parts(
            self.element_exclusion,
            self.property_exclusion,
            self.selected_properties,
            self.steps,
            self.converters,
        )
    }
}

impl fmt::Debug for JsonBuilderElementBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JsonBuilderElementBuilder")
            .field("element_exclusion", &self.element_exclusion)
            .field("property_exclusion", &self.property_exclusion)
            .field("selected_properties", &self.selected_properties)
            .field("converters", &self.converters.len())
            .finish_non_exhaustive()
    }
}

impl Default for JsonBuilderElementBuilder {
    fn default() -> Self {
        JsonBuilderElementBuilder::new()
    }
}

/// Collect a slice of string literals into a lowercased set.
fn lowercased(items: &[&str]) -> HashSet<String> {
    items.iter().map(|s| s.to_lowercase()).collect()
}
