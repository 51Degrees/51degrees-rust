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

//! The translation engine.

use std::collections::BTreeMap;

use fiftyone_pipeline_core::{
    ElementData, Error, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData,
    FlowElement, FlowError, PropertyMetaData, PropertyValue, PropertyValueType, Result, TypedKey,
    WeightedValue,
};
use fiftyone_pipeline_engines::AspectPropertyValue;

use crate::behavior::MissingTranslationBehavior;
use crate::builder::TranslationEngineBuilder;
use crate::data::{TranslationData, TranslationValue, RAW_WEIGHTING_KEY, VALUE_KEY};
use crate::languages::Languages;
use crate::translator::Translator;

/// The data key a translation engine stores its data under unless it is
/// given another. Engines that share it write to the same data, so they run
/// one after another rather than in one parallel group.
pub const TRANSLATION_DATA_KEY: &str = "translation";

/// The typed key for the data of engines under [`TRANSLATION_DATA_KEY`].
pub const TRANSLATION_KEY: TypedKey<TranslationData> = TypedKey::new(TRANSLATION_DATA_KEY);

/// The evidence keys the language is read from, most preferred first. The
/// first with a value that is not blank is used.
///
/// A slice rather than an array, so a key added later does not change its
/// type.
pub const LANGUAGE_EVIDENCE_KEYS: &[&str] = &[
    "query.translation",
    "query.accept-language",
    "header.accept-language",
];

/// The key of the weighting, from 0 to 1, in each record of a weighted list
/// that gives one rather than a raw weighting.
const WEIGHT_KEY: &str = "weight";

/// One property to translate, from its name in the source data to its name
/// in the engine's data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationProperty {
    /// The property's name in the source data.
    pub source: String,
    /// The name the translated value is stored under.
    pub destination: String,
}

/// Translates values another element produced into the language the request
/// asks for, as the
/// [translation engine specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/translation-engine.md)
/// describes.
///
/// The engine reads each configured source property from the data under its
/// source data key, translates it, and writes it to its own data under the
/// destination name. A string, a list of strings and a weighted list of
/// strings can be translated, and each keeps its form and weightings.
///
/// The language is the fixed language if one is set. Otherwise it is the
/// value of the first of [`LANGUAGE_EVIDENCE_KEYS`] that is not blank, read
/// as an `Accept-Language` value, as
/// [`Languages::try_resolve_locale`] describes. That value is the only one
/// tried, even when it names no language the engine has.
///
/// Build one with [`TranslationEngine::builder`].
pub struct TranslationEngine {
    data_key: &'static str,
    source_data_key: String,
    translations: Vec<TranslationProperty>,
    fixed_language: Option<String>,
    behavior: MissingTranslationBehavior,
    languages: Languages,
    empty_translator: Translator,
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

impl TranslationEngine {
    /// A builder for a translation engine.
    pub fn builder() -> TranslationEngineBuilder {
        TranslationEngineBuilder::new()
    }

    pub(crate) fn new(
        data_key: &'static str,
        source_data_key: String,
        translations: Vec<(TranslationProperty, PropertyValueType)>,
        fixed_language: Option<String>,
        behavior: MissingTranslationBehavior,
        languages: Languages,
    ) -> Self {
        let mut properties: Vec<PropertyMetaData> = Vec::new();
        for (translation, value_type) in &translations {
            let lowercase = translation.destination.to_lowercase();
            if !properties
                .iter()
                .any(|property| property.name.to_lowercase() == lowercase)
            {
                properties.push(PropertyMetaData::new(
                    translation.destination.clone(),
                    data_key,
                    *value_type,
                ));
            }
        }
        TranslationEngine {
            data_key,
            source_data_key,
            translations: translations
                .into_iter()
                .map(|(translation, _)| translation)
                .collect(),
            fixed_language,
            behavior,
            languages,
            empty_translator: Translator::new(behavior),
            filter: EvidenceKeyFilterWhitelist::new(LANGUAGE_EVIDENCE_KEYS),
            properties,
        }
    }

    /// The typed key of the data this engine writes.
    pub fn typed_key(&self) -> TypedKey<TranslationData> {
        TypedKey::new(self.data_key)
    }

    /// The data key of the element whose values are translated.
    pub fn source_data_key(&self) -> &str {
        &self.source_data_key
    }

    /// The properties translated, in the order they were added.
    pub fn translations(&self) -> &[TranslationProperty] {
        &self.translations
    }

    /// The locale every value is translated to, when one is fixed.
    pub fn fixed_language(&self) -> Option<&str> {
        self.fixed_language.as_deref()
    }

    /// What the engine gives for a value it has no translation for.
    pub fn behavior(&self) -> MissingTranslationBehavior {
        self.behavior
    }

    /// The translators the engine holds, one for each locale.
    pub fn languages(&self) -> &Languages {
        &self.languages
    }

    /// The language asked for, being the fixed language or the first
    /// language evidence that is not blank.
    fn target_language(&self, evidence: &Evidence) -> Option<String> {
        if let Some(language) = &self.fixed_language {
            return Some(language.clone());
        }
        LANGUAGE_EVIDENCE_KEYS.iter().find_map(|key| {
            evidence
                .get(key)
                .filter(|value| !value.trim().is_empty())
                .map(str::to_owned)
        })
    }

    fn error(&self, message: String) -> FlowError {
        FlowError::new(self.data_key, Error::Pipeline { message })
    }

    /// Translate every property with `translator` and write the results.
    ///
    /// A value of a form that cannot be translated stops the work there, and
    /// the values translated before it are still written.
    fn populate(&self, data: &mut FlowData, translator: &Translator) -> Result<()> {
        let mut values = Vec::new();
        let mut messages = Vec::new();
        let mut failure = None;
        if let Some(source) = data.get_str(&self.source_data_key) {
            for translation in &self.translations {
                let value = match read_source(source, &translation.source) {
                    Ok(SourceValue::Absent) => AspectPropertyValue::no_value(format!(
                        "The source property '{}' could not be found in the source data.",
                        translation.source
                    )),
                    Ok(SourceValue::NoValue(message)) => AspectPropertyValue::no_value(message),
                    Ok(SourceValue::Value(value)) => {
                        AspectPropertyValue::new(translator.translate_value(&value, &mut messages))
                    }
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                };
                values.push((translation.destination.clone(), value));
            }
        }
        let output = data.get_or_add(self.typed_key(), TranslationData::new)?;
        for (name, value) in values {
            output.set(name, value);
        }
        for message in messages {
            data.add_error(self.error(message), true);
        }
        failure.map_or(Ok(()), Err)
    }
}

impl FlowElement for TranslationEngine {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        data.get_or_add(self.typed_key(), TranslationData::new)?;

        if data.get_str(&self.source_data_key).is_none() {
            let message = format!(
                "The source data '{}' could not be found in the FlowData.",
                self.source_data_key
            );
            data.add_error(self.error(message), true);
            return Ok(());
        }

        let Some(language) = self.target_language(data.evidence()) else {
            if self.behavior == MissingTranslationBehavior::FlowError {
                let message = "The evidence did not contain a language to translate to.";
                data.add_error(self.error(message.to_owned()), true);
                return Ok(());
            }
            return self.populate(data, &self.empty_translator);
        };

        let found = self
            .languages
            .try_get_translator(&language)
            .map_err(|error| Error::Pipeline {
                message: error.to_string(),
            })?;
        match found {
            Some((_, translator)) => self.populate(data, translator),
            None if self.behavior == MissingTranslationBehavior::FlowError => {
                let message =
                    format!("There was no translator configured for the language '{language}'.");
                data.add_error(self.error(message), true);
                Ok(())
            }
            None => self.populate(data, &self.empty_translator),
        }
    }

    fn data_key(&self) -> &str {
        self.data_key
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

/// A source property as read for translation.
enum SourceValue {
    /// The source data does not hold the property.
    Absent,
    /// The source data holds the property with no value, for this reason.
    NoValue(String),
    /// A value to translate.
    Value(TranslationValue),
}

/// Read a source property.
///
/// Data another translation engine wrote is read as it was written. Other
/// data is read through its property bag, where a weighted list is a list of
/// records, each with a `value` and either a `rawweighting` or a `weight`
/// from 0 to 1. A `weight` is turned back into the raw weighting it was made
/// from, which is exact because the weight is the raw weighting divided by
/// 65,535 as a 32-bit float.
fn read_source(source: &dyn ElementData, name: &str) -> Result<SourceValue> {
    if name.trim().is_empty() || !source.has_property(name) {
        return Ok(SourceValue::Absent);
    }
    if let Some(translations) = source.as_any().downcast_ref::<TranslationData>() {
        return Ok(match translations.value(name) {
            None | Some(AspectPropertyValue::Value(TranslationValue::String(None))) => {
                SourceValue::Absent
            }
            Some(AspectPropertyValue::NoValue { message }) => SourceValue::NoValue(message.clone()),
            Some(AspectPropertyValue::Value(value)) => SourceValue::Value(value.clone()),
        });
    }
    let value = match source.get(name) {
        Ok(value) => value,
        Err(error) => return Ok(SourceValue::NoValue(error.message)),
    };
    let unsupported = |kind: &str| Error::Pipeline {
        message: format!(
            "The value of the source property '{name}' is of the type '{kind}', \
             which is not supported for translation."
        ),
    };
    Ok(SourceValue::Value(match value {
        PropertyValue::String(value) => TranslationValue::String(Some(value)),
        PropertyValue::StringList(values) => {
            TranslationValue::StringList(values.into_iter().map(Some).collect())
        }
        PropertyValue::KeyValueList(records) => TranslationValue::WeightedList(
            records
                .iter()
                .map(weighted_item)
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| unsupported("KeyValueList"))?,
        ),
        PropertyValue::Bool(_) => return Err(unsupported("Bool")),
        PropertyValue::Integer(_) => return Err(unsupported("Integer")),
        PropertyValue::Double(_) => return Err(unsupported("Double")),
        PropertyValue::Float(_) => return Err(unsupported("Float")),
        PropertyValue::JavaScript(_) => return Err(unsupported("JavaScript")),
        _ => return Err(unsupported("unknown")),
    }))
}

/// Read one record of a weighted list, or `None` when it is not one.
fn weighted_item(
    record: &BTreeMap<String, PropertyValue>,
) -> Option<WeightedValue<Option<String>>> {
    if record.len() != 2 {
        return None;
    }
    let value = match record.get(VALUE_KEY)? {
        PropertyValue::String(value) => value.clone(),
        _ => return None,
    };
    let raw_weighting = match (record.get(RAW_WEIGHTING_KEY), record.get(WEIGHT_KEY)) {
        (Some(PropertyValue::Integer(raw)), None) => u16::try_from(*raw).ok()?,
        (None, Some(weight)) => raw_weighting_of(weight.as_double()?)?,
        _ => return None,
    };
    Some(WeightedValue::new(raw_weighting, Some(value)))
}

/// The raw weighting a weight from 0 to 1 was made from.
fn raw_weighting_of(weight: f64) -> Option<u16> {
    if !(0.0..=1.0).contains(&weight) {
        return None;
    }
    Some((weight * f64::from(u16::MAX)).round() as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every raw weighting comes back from the weight a weighted value gives.
    #[test]
    fn every_raw_weighting_comes_back_from_its_weight() {
        for raw in 0..=u16::MAX {
            let weight = f64::from(WeightedValue::new(raw, ()).weighting());
            assert_eq!(raw_weighting_of(weight), Some(raw));
        }
    }

    #[test]
    fn a_record_that_is_not_a_weighted_item_is_refused() {
        let record = |entries: &[(&str, PropertyValue)]| {
            entries
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect::<BTreeMap<_, _>>()
        };
        assert_eq!(
            weighted_item(&record(&[
                ("rawweighting", PropertyValue::Integer(20000)),
                ("value", PropertyValue::from("GB")),
            ])),
            Some(WeightedValue::new(20000, Some("GB".to_owned())))
        );
        assert_eq!(
            weighted_item(&record(&[
                ("weight", PropertyValue::Double(1.0)),
                ("value", PropertyValue::from("GB")),
            ])),
            Some(WeightedValue::new(u16::MAX, Some("GB".to_owned())))
        );
        for entries in [
            vec![("value", PropertyValue::from("GB"))],
            vec![
                ("rawweighting", PropertyValue::Integer(70000)),
                ("value", PropertyValue::from("GB")),
            ],
            vec![
                ("rawweighting", PropertyValue::Integer(1)),
                ("value", PropertyValue::Integer(1)),
            ],
            vec![
                ("weight", PropertyValue::Double(1.5)),
                ("value", PropertyValue::from("GB")),
            ],
            vec![
                ("rawweighting", PropertyValue::Integer(1)),
                ("weight", PropertyValue::Double(1.0)),
                ("value", PropertyValue::from("GB")),
            ],
        ] {
            assert_eq!(weighted_item(&record(&entries)), None, "{entries:?}");
        }
    }
}
