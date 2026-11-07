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

//! Translates values into one language.

use std::collections::HashMap;

use fiftyone_pipeline_core::{Error, Result, WeightedValue};

use crate::behavior::MissingTranslationBehavior;
use crate::data::TranslationValue;

/// Translates values into one language, matching each value ignoring case.
#[derive(Debug, Clone)]
pub struct Translator {
    /// Each translation that is not blank, keyed by the lowercase source
    /// value.
    translations: HashMap<String, String>,
    behavior: MissingTranslationBehavior,
}

impl Translator {
    /// A translator with no translations, which gives every value as
    /// `behavior` says.
    pub fn new(behavior: MissingTranslationBehavior) -> Self {
        Translator {
            translations: HashMap::new(),
            behavior,
        }
    }

    /// A translator from pairs of a source value and its translation.
    ///
    /// A blank translation counts as none. Two source values that differ only
    /// in case are refused, because a value is matched ignoring case.
    pub fn with_translations<I, K, V>(
        translations: I,
        behavior: MissingTranslationBehavior,
    ) -> Result<Self>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let mut held: HashMap<String, (String, String)> = HashMap::new();
        for (source, translation) in translations {
            let source: String = source.into();
            if let Some((first, _)) = held.get(&source.to_lowercase()) {
                return Err(Error::configuration(format!(
                    "The translations hold both '{first}' and '{source}', which \
                     differ only in case."
                )));
            }
            held.insert(source.to_lowercase(), (source, translation.into()));
        }
        Ok(Translator {
            translations: held
                .into_iter()
                .filter(|(_, (_, translation))| !translation.trim().is_empty())
                .map(|(key, (_, translation))| (key, translation))
                .collect(),
            behavior,
        })
    }

    /// What this translator gives for a value it has no translation for.
    pub fn behavior(&self) -> MissingTranslationBehavior {
        self.behavior
    }

    /// Translate one value.
    ///
    /// A value with no translation is given as [`Translator::behavior`] says.
    /// With [`MissingTranslationBehavior::FlowError`] it is `None`, and the
    /// reason is added to `errors`.
    pub fn translate(&self, value: &str, errors: &mut Vec<String>) -> Option<String> {
        if let Some(translation) = self.translations.get(&value.to_lowercase()) {
            return Some(translation.clone());
        }
        match self.behavior {
            MissingTranslationBehavior::Original => Some(value.to_owned()),
            MissingTranslationBehavior::EmptyString => Some(String::new()),
            MissingTranslationBehavior::FlowError => {
                errors.push(format!(
                    "There was no translation found for the value '{value}'."
                ));
                None
            }
        }
    }

    /// Translate a value of any form, keeping its form and each weighting. An
    /// item that is already `None` stays `None`.
    pub fn translate_value(
        &self,
        value: &TranslationValue,
        errors: &mut Vec<String>,
    ) -> TranslationValue {
        let mut one = |item: &Option<String>| {
            item.as_deref()
                .and_then(|item| self.translate(item, errors))
        };
        match value {
            TranslationValue::String(item) => TranslationValue::String(one(item)),
            TranslationValue::StringList(items) => {
                TranslationValue::StringList(items.iter().map(&mut one).collect())
            }
            TranslationValue::WeightedList(items) => TranslationValue::WeightedList(
                items
                    .iter()
                    .map(|item| WeightedValue::new(item.raw_weighting, one(&item.value)))
                    .collect(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn animals(behavior: MissingTranslationBehavior) -> Translator {
        Translator::with_translations(
            [("cat", "chat"), ("Dog", "chien"), ("buffalo", " ")],
            behavior,
        )
        .unwrap()
    }

    #[test]
    fn a_value_is_matched_ignoring_case() {
        let translator = animals(MissingTranslationBehavior::Original);
        let mut errors = Vec::new();
        assert_eq!(
            translator.translate("CAT", &mut errors),
            Some("chat".to_owned())
        );
        assert_eq!(
            translator.translate("dog", &mut errors),
            Some("chien".to_owned())
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn a_missing_or_blank_translation_follows_the_behavior() {
        for value in ["horse", "buffalo"] {
            let mut errors = Vec::new();
            assert_eq!(
                animals(MissingTranslationBehavior::Original).translate(value, &mut errors),
                Some(value.to_owned())
            );
            assert_eq!(
                animals(MissingTranslationBehavior::EmptyString).translate(value, &mut errors),
                Some(String::new())
            );
            assert!(errors.is_empty());
            assert_eq!(
                animals(MissingTranslationBehavior::FlowError).translate(value, &mut errors),
                None
            );
            assert_eq!(
                errors,
                [format!(
                    "There was no translation found for the value '{value}'."
                )]
            );
        }
    }

    #[test]
    fn a_weighted_list_keeps_each_weighting() {
        let translator = animals(MissingTranslationBehavior::FlowError);
        let mut errors = Vec::new();
        let value = TranslationValue::WeightedList(vec![
            WeightedValue::new(1, Some("cat".to_owned())),
            WeightedValue::new(2, Some("horse".to_owned())),
            WeightedValue::new(3, None),
        ]);
        assert_eq!(
            translator.translate_value(&value, &mut errors),
            TranslationValue::WeightedList(vec![
                WeightedValue::new(1, Some("chat".to_owned())),
                WeightedValue::new(2, None),
                WeightedValue::new(3, None),
            ])
        );
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn source_values_that_differ_only_in_case_are_refused() {
        let error = Translator::with_translations(
            [("Congo", "Congo"), ("congo", "Congo")],
            MissingTranslationBehavior::Original,
        )
        .unwrap_err();
        assert!(error.message().contains("differ only in case"), "{error}");
    }
}
