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

//! The builder for a translation engine, and how its sources are read.

use std::collections::HashMap;
use std::path::Path;

use fiftyone_pipeline_core::{Error, PropertyValueType, Result};

use crate::behavior::MissingTranslationBehavior;
use crate::engine::{TranslationEngine, TranslationProperty, TRANSLATION_DATA_KEY};
use crate::languages::Languages;
use crate::translator::Translator;

/// Builds a [`TranslationEngine`].
///
/// An engine needs a source data key, at least one translation and at least
/// one source. A source is a YAML file of `value: translation` lines for one
/// language, whose name is `<anything>.<locale>.yml`, such as
/// `countries.fr_FR.yml`, where the locale is two lowercase letters, `_` and
/// two capitals. A problem found while adding a source is reported by
/// [`TranslationEngineBuilder::build`].
#[derive(Debug, Default)]
pub struct TranslationEngineBuilder {
    data_key: Option<&'static str>,
    source_data_key: Option<String>,
    translations: Vec<(TranslationProperty, PropertyValueType)>,
    sources: Vec<(String, String)>,
    fixed_language: Option<String>,
    behavior: MissingTranslationBehavior,
    error: Option<Error>,
}

impl TranslationEngineBuilder {
    /// A builder with no sources or translations, translating as
    /// [`MissingTranslationBehavior::Original`] says.
    pub fn new() -> Self {
        TranslationEngineBuilder::default()
    }

    /// Set the data key of the element whose values are translated, such as
    /// `ip`.
    pub fn set_source_element_data_key(mut self, key: impl Into<String>) -> Self {
        self.source_data_key = Some(key.into());
        self
    }

    /// Set the data key the engine stores its data under. Engines that share
    /// a key write to the same data. The default is
    /// [`TRANSLATION_DATA_KEY`](crate::TRANSLATION_DATA_KEY).
    pub fn set_element_data_key(mut self, key: &'static str) -> Self {
        self.data_key = Some(key);
        self
    }

    /// Translate every value to this locale, such as `en_GB`, whatever the
    /// request asks for. The first locale found in the text is used, and
    /// text holding none leaves the language to the evidence.
    pub fn set_fixed_language(mut self, language: impl Into<String>) -> Self {
        self.fixed_language = Some(language.into());
        self
    }

    /// Set what the engine gives for a value it has no translation for.
    pub fn set_missing_translation_behavior(
        mut self,
        behavior: MissingTranslationBehavior,
    ) -> Self {
        self.behavior = behavior;
        self
    }

    /// Translate the source property `source` to the property `destination`
    /// of the engine's data, whose metadata gives its type as a string.
    pub fn add_translation(
        self,
        source: impl Into<String>,
        destination: impl Into<String>,
    ) -> Self {
        self.add_translation_of_type(source, destination, PropertyValueType::String)
    }

    /// Translate the source property `source` to the property `destination`
    /// of the engine's data, whose metadata gives its type as `value_type`.
    pub fn add_translation_of_type(
        mut self,
        source: impl Into<String>,
        destination: impl Into<String>,
        value_type: PropertyValueType,
    ) -> Self {
        self.translations.push((
            TranslationProperty {
                source: source.into(),
                destination: destination.into(),
            },
            value_type,
        ));
        self
    }

    /// Add a source file. A `*` in the file's name adds every file in its
    /// folder whose name matches, in order of name, so `countries.*.yml`
    /// adds every language of `countries`.
    pub fn add_source(mut self, path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !name.contains('*') {
            return match std::fs::read_to_string(path) {
                Ok(text) => self.add_source_text(name, text),
                Err(error) => self.fail(format!(
                    "The source file '{}' could not be read: {error}",
                    path.display()
                )),
            };
        }
        let folder = match path.parent() {
            Some(folder) if !folder.as_os_str().is_empty() => folder,
            _ => Path::new("."),
        };
        let entries = match std::fs::read_dir(folder) {
            Ok(entries) => entries,
            Err(error) => {
                return self.fail(format!(
                    "The folder '{}' could not be read: {error}",
                    folder.display()
                ))
            }
        };
        let mut matches: Vec<(String, std::path::PathBuf)> = entries
            .flatten()
            .filter(|entry| entry.path().is_file())
            .filter_map(|entry| {
                let file = entry.file_name().to_string_lossy().into_owned();
                matches_wildcard(&name, &file).then(|| (file, entry.path()))
            })
            .collect();
        matches.sort();
        for (file, path) in matches {
            self = match std::fs::read_to_string(&path) {
                Ok(text) => self.add_source_text(file, text),
                Err(error) => self.fail(format!(
                    "The source file '{}' could not be read: {error}",
                    path.display()
                )),
            };
        }
        self
    }

    /// Add a source from its text, under the file name that gives its
    /// locale. This is how a source compiled into a program is added.
    pub fn add_source_text(mut self, name: impl Into<String>, text: impl Into<String>) -> Self {
        let name = name.into();
        if self.sources.iter().any(|(held, _)| *held == name) {
            return self.fail(format!("A source named '{name}' was added twice."));
        }
        self.sources.push((name, text.into()));
        self
    }

    /// Keep the first problem found, for [`TranslationEngineBuilder::build`].
    fn fail(mut self, message: String) -> Self {
        self.error.get_or_insert(Error::configuration(message));
        self
    }

    /// Build the engine.
    ///
    /// Fails if adding a source failed, if there is no translation, no source
    /// or no source data key, if a source's name gives no locale, or if a
    /// source is not a YAML mapping of values to their translations.
    pub fn build(self) -> Result<TranslationEngine> {
        if let Some(error) = self.error {
            return Err(error);
        }
        if self.translations.is_empty() {
            return Err(Error::configuration(
                "At least one property translation must be configured.",
            ));
        }
        if self.sources.is_empty() {
            return Err(Error::configuration(
                "At least one source file must be configured.",
            ));
        }
        let source_data_key = match self.source_data_key {
            Some(key) if !key.trim().is_empty() => key.trim().to_owned(),
            _ => {
                return Err(Error::configuration(
                    "The source element key must be configured.",
                ))
            }
        };
        let fixed_language = self
            .fixed_language
            .as_deref()
            .and_then(find_locale)
            .map(str::to_owned);
        let mut languages = Languages::new();
        for (name, text) in &self.sources {
            let locale = locale_of_source(name)?;
            let translations = parse_source(name, text)?;
            let translations = translations
                .into_iter()
                .map(|(source, translation)| (source, translation.unwrap_or_default()));
            languages.add_language(
                locale,
                Translator::with_translations(translations, self.behavior)?,
            );
        }
        Ok(TranslationEngine::new(
            self.data_key.unwrap_or(TRANSLATION_DATA_KEY),
            source_data_key,
            self.translations,
            fixed_language,
            self.behavior,
            languages,
        ))
    }
}

/// True if `name` matches `pattern`, where each `*` stands for any run of
/// characters, including none.
fn matches_wildcard(pattern: &str, name: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else {
        return rest.is_empty();
    };
    for part in middle {
        match rest.find(part) {
            Some(index) => rest = &rest[index + part.len()..],
            None => return false,
        }
    }
    rest.len() >= last.len() && rest.ends_with(last)
}

/// The first locale in `text`: two lowercase letters, `_` and two capitals,
/// such as `fr_FR`.
fn find_locale(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    (0..bytes.len().saturating_sub(4))
        .find(|&start| {
            let candidate = &bytes[start..start + 5];
            candidate[0].is_ascii_lowercase()
                && candidate[1].is_ascii_lowercase()
                && candidate[2] == b'_'
                && candidate[3].is_ascii_uppercase()
                && candidate[4].is_ascii_uppercase()
        })
        .map(|start| &text[start..start + 5])
}

/// The locale a source's name gives, from its second last part between dots.
fn locale_of_source(name: &str) -> Result<&str> {
    if name.trim().is_empty() {
        return Err(Error::configuration("Source name cannot be blank."));
    }
    let parts: Vec<&str> = name.split('.').collect();
    if parts.len() < 3 {
        return Err(Error::configuration(format!(
            "Source name '{name}' does not have the correct format name. It \
             should be 'somename.locale.yml' e.g. 'countries.en_GB.yml'."
        )));
    }
    find_locale(parts[parts.len() - 2]).ok_or_else(|| {
        Error::configuration(format!(
            "Source name '{name}' does not contain a valid locale code."
        ))
    })
}

/// Read a source's text as a YAML mapping of values to their translations.
/// A translation that is empty or `null` is `None`.
fn parse_source(name: &str, text: &str) -> Result<HashMap<String, Option<String>>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    match serde_norway::from_str::<Option<HashMap<String, Option<String>>>>(text) {
        Ok(Some(translations)) => Ok(translations),
        Ok(None) => Err(Error::configuration(format!(
            "The source for {name} could not be parsed into a valid translation lookup."
        ))),
        Err(error) => Err(Error::configuration(format!(
            "The source for {name} could not be parsed into a valid translation \
             lookup: {error}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wildcard_stands_for_any_run_of_characters() {
        assert!(matches_wildcard("animals.*.yml", "animals.fr_FR.yml"));
        assert!(matches_wildcard("animals.*.yml", "animals..yml"));
        assert!(matches_wildcard("*", "anything"));
        assert!(matches_wildcard("a*b*c", "aXbYc"));
        assert!(matches_wildcard("a*b*c", "abc"));
        assert!(!matches_wildcard("animals.*.yml", "plants.fr_FR.yml"));
        assert!(!matches_wildcard("animals.*.yml", "animals.fr_FR.yaml"));
        assert!(!matches_wildcard("a*aa", "aa"));
    }

    #[test]
    fn the_first_locale_in_the_text_is_found() {
        assert_eq!(find_locale("fr_FR"), Some("fr_FR"));
        assert_eq!(find_locale("xfr_FRx"), Some("fr_FR"));
        assert_eq!(find_locale("mmm_kp"), None);
        assert_eq!(find_locale("fr-FR"), None);
        assert_eq!(find_locale("fr_F"), None);
        assert_eq!(find_locale(""), None);
    }

    #[test]
    fn a_source_name_gives_its_locale() {
        assert_eq!(locale_of_source("countries.fr_FR.yml").unwrap(), "fr_FR");
        assert_eq!(locale_of_source("a.b.de_DE.yaml").unwrap(), "de_DE");
        for name in ["", " ", "fr_FR.yml", "animals.mmm_kp.yml"] {
            assert!(locale_of_source(name).is_err(), "{name}");
        }
    }

    #[test]
    fn a_source_is_read_as_text_whatever_it_looks_like() {
        let text = "\u{feff}NO: Norway\nON: 'On'\n1: one\nbuffalo:\nbison: ~\nyak: ''\n";
        let translations = parse_source("codes.en_GB.yml", text).unwrap();
        assert_eq!(translations["NO"].as_deref(), Some("Norway"));
        assert_eq!(translations["ON"].as_deref(), Some("On"));
        assert_eq!(translations["1"].as_deref(), Some("one"));
        assert_eq!(translations["buffalo"], None);
        assert_eq!(translations["bison"], None);
        assert_eq!(translations["yak"].as_deref(), Some(""));
    }

    #[test]
    fn a_source_that_is_not_a_mapping_is_refused() {
        for text in ["", "# Only a comment.\n", "- cat\n- dog\n", "cat: [chat]\n"] {
            let error = parse_source("animals.fr_FR.yml", text).unwrap_err();
            assert!(
                error
                    .message()
                    .starts_with("The source for animals.fr_FR.yml could not be parsed"),
                "{text}: {error}"
            );
        }
    }
}
