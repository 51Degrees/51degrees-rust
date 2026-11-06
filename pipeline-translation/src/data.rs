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

//! The values a translation engine writes, and the element data that holds
//! them.

use std::any::Any;
use std::collections::BTreeMap;

use fiftyone_pipeline_core::{ElementData, NoValueError, PropertyValue, WeightedValue};
use fiftyone_pipeline_engines::AspectPropertyValue;

/// The key of the raw weighting in each record of a weighted list, as the
/// property bag gives it.
pub const RAW_WEIGHTING_KEY: &str = "rawweighting";

/// The key of the value in each record of a weighted list, as the property
/// bag gives it.
pub const VALUE_KEY: &str = "value";

/// The reason the property bag gives for a single value that has no
/// translation.
pub const NO_TRANSLATION_MESSAGE: &str =
    "There was no translation for the value, and the flow data's errors say which value.";

/// A value to translate, or one translated, keeping the form of the value it
/// came from.
///
/// A value, or an item, is `None` only when it had no translation and the
/// engine is set to
/// [`MissingTranslationBehavior::FlowError`](crate::MissingTranslationBehavior::FlowError).
#[derive(Debug, Clone, PartialEq)]
pub enum TranslationValue {
    /// A single string.
    String(Option<String>),
    /// A list of strings.
    StringList(Vec<Option<String>>),
    /// A list of strings, each with its weighting, which translation keeps.
    WeightedList(Vec<WeightedValue<Option<String>>>),
}

impl From<&str> for TranslationValue {
    fn from(value: &str) -> Self {
        TranslationValue::String(Some(value.to_owned()))
    }
}

impl From<String> for TranslationValue {
    fn from(value: String) -> Self {
        TranslationValue::String(Some(value))
    }
}

impl From<Vec<String>> for TranslationValue {
    fn from(values: Vec<String>) -> Self {
        TranslationValue::StringList(values.into_iter().map(Some).collect())
    }
}

impl From<Vec<WeightedValue<String>>> for TranslationValue {
    fn from(values: Vec<WeightedValue<String>>) -> Self {
        TranslationValue::WeightedList(
            values
                .into_iter()
                .map(|item| WeightedValue::new(item.raw_weighting, Some(item.value)))
                .collect(),
        )
    }
}

impl TranslationValue {
    /// The value as the property bag gives it.
    ///
    /// A weighted list is a list of records, each with a `rawweighting` and a
    /// `value`. A single value with no translation has no value, and an item
    /// with no translation is left out of its list.
    pub fn to_property_value(&self) -> Result<PropertyValue, NoValueError> {
        match self {
            TranslationValue::String(Some(value)) => Ok(PropertyValue::String(value.clone())),
            TranslationValue::String(None) => Err(NoValueError::new(NO_TRANSLATION_MESSAGE)),
            TranslationValue::StringList(values) => Ok(PropertyValue::StringList(
                values.iter().flatten().cloned().collect(),
            )),
            TranslationValue::WeightedList(values) => Ok(PropertyValue::KeyValueList(
                values
                    .iter()
                    .filter_map(|item| {
                        item.value.as_ref().map(|value| {
                            BTreeMap::from([
                                (
                                    RAW_WEIGHTING_KEY.to_owned(),
                                    PropertyValue::Integer(i64::from(item.raw_weighting)),
                                ),
                                (VALUE_KEY.to_owned(), PropertyValue::String(value.clone())),
                            ])
                        })
                    })
                    .collect(),
            )),
        }
    }
}

/// The element data a translation engine writes, holding each translated
/// property under its destination name.
///
/// Several translation engines may write to the same data, as engines that
/// share a data key do. Names are matched ignoring case, and
/// [`ElementData::keys`] lists them in the order they were first set,
/// leaving out a single value that has no translation.
#[derive(Debug, Clone, Default)]
pub struct TranslationData {
    /// The name as first set, its lowercase form, and its value.
    values: Vec<(String, String, AspectPropertyValue<TranslationValue>)>,
}

impl TranslationData {
    /// Empty data.
    pub fn new() -> Self {
        TranslationData::default()
    }

    /// Set a property's value. A property already set, ignoring case, keeps
    /// its name and place and takes the new value.
    pub fn set(&mut self, name: impl Into<String>, value: AspectPropertyValue<TranslationValue>) {
        let name = name.into();
        let lowercase = name.to_lowercase();
        match self
            .values
            .iter_mut()
            .find(|(_, held, _)| *held == lowercase)
        {
            Some((_, _, held)) => *held = value,
            None => self.values.push((name, lowercase, value)),
        }
    }

    /// A property's value, or `None` when the property was never set.
    pub fn value(&self, name: &str) -> Option<&AspectPropertyValue<TranslationValue>> {
        let lowercase = name.to_lowercase();
        self.values
            .iter()
            .find(|(_, held, _)| *held == lowercase)
            .map(|(_, _, value)| value)
    }

    /// The names of the properties set, in the order they were first set.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.values.iter().map(|(name, _, _)| name.as_str())
    }
}

impl ElementData for TranslationData {
    fn get(&self, name: &str) -> Result<PropertyValue, NoValueError> {
        match self.value(name) {
            Some(AspectPropertyValue::Value(value)) => value.to_property_value(),
            Some(AspectPropertyValue::NoValue { message }) => Err(NoValueError::new(message)),
            None => Err(NoValueError::new(format!(
                "No value for property '{name}'."
            ))),
        }
    }

    fn has_property(&self, name: &str) -> bool {
        self.value(name).is_some()
    }

    fn keys(&self) -> Vec<String> {
        self.values
            .iter()
            .filter(|(_, _, value)| {
                !matches!(
                    value,
                    AspectPropertyValue::Value(TranslationValue::String(None))
                )
            })
            .map(|(name, _, _)| name.clone())
            .collect()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_set_again_keeps_its_place_and_first_spelling() {
        let mut data = TranslationData::new();
        data.set("Animal", AspectPropertyValue::new("chat".into()));
        data.set("Colour", AspectPropertyValue::new("rouge".into()));
        data.set("ANIMAL", AspectPropertyValue::new("chien".into()));
        assert_eq!(data.names().collect::<Vec<_>>(), ["Animal", "Colour"]);
        assert_eq!(data.keys(), ["Animal", "Colour"]);
        assert_eq!(data.get("animal").unwrap(), PropertyValue::from("chien"));
    }

    #[test]
    fn the_bag_gives_each_form_of_value() {
        let mut data = TranslationData::new();
        data.set(
            "List",
            AspectPropertyValue::new(TranslationValue::StringList(vec![
                Some("chat".to_owned()),
                None,
                Some("chien".to_owned()),
            ])),
        );
        data.set(
            "Weighted",
            AspectPropertyValue::new(TranslationValue::WeightedList(vec![
                WeightedValue::new(40000, Some("chat".to_owned())),
                WeightedValue::new(25535, None),
            ])),
        );
        data.set("Missing", AspectPropertyValue::no_value("Not known."));
        data.set(
            "Untranslated",
            AspectPropertyValue::new(TranslationValue::String(None)),
        );

        assert_eq!(
            data.get("List").unwrap(),
            PropertyValue::StringList(vec!["chat".to_owned(), "chien".to_owned()])
        );
        assert_eq!(
            data.get("Weighted").unwrap(),
            PropertyValue::KeyValueList(vec![BTreeMap::from([
                ("rawweighting".to_owned(), PropertyValue::Integer(40000)),
                ("value".to_owned(), PropertyValue::from("chat")),
            ])])
        );
        assert_eq!(data.get("Missing").unwrap_err().message, "Not known.");
        assert_eq!(
            data.get("Untranslated").unwrap_err().message,
            NO_TRANSLATION_MESSAGE
        );
        assert!(data.has_property("Untranslated"));
        assert!(!data.has_property("Other"));
        // The untranslated single value is left out, as nothing is written
        // for it.
        assert_eq!(data.keys(), ["List", "Weighted", "Missing"]);
    }
}
