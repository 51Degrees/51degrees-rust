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

//! The translation engine in a pipeline, following the cases of the .NET
//! translation engine's tests.

use std::any::Any;
use std::collections::BTreeMap;
use std::sync::Arc;

use fiftyone_pipeline_core::{
    ElementData, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    MapElementData, NoValueError, Pipeline, PropertyMetaData, PropertyValue, PropertyValueType,
    Result, TypedKey, WeightedValue,
};
use fiftyone_pipeline_engines::{AspectDataBase, AspectPropertyValue};
use fiftyone_pipeline_translation::{
    Languages, MissingTranslationBehavior, TranslationEngine, TranslationValue, BASE_LANGUAGE,
    LANGUAGE_EVIDENCE_KEYS, TRANSLATION_KEY,
};

const SOURCE_KEY: &str = "evidencecopy";

const FRENCH_ANIMALS: &str = "cat: chat\ndog: chien\n";

const BEHAVIORS: [MissingTranslationBehavior; 3] = [
    MissingTranslationBehavior::Original,
    MissingTranslationBehavior::EmptyString,
    MissingTranslationBehavior::FlowError,
];

/// Adds a copy of the data it is given under its key.
struct Source<T> {
    key: &'static str,
    data: T,
    filter: EvidenceKeyFilterWhitelist,
}

impl<T: ElementData + Clone> Source<T> {
    fn new(key: &'static str, data: T) -> Self {
        Source {
            key,
            data,
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        }
    }
}

impl<T: ElementData + Clone> FlowElement for Source<T> {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        let copy = self.data.clone();
        data.get_or_add(TypedKey::<T>::new(self.key), move || copy)?;
        Ok(())
    }

    fn data_key(&self) -> &str {
        self.key
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

/// Data that answers every property the same way, whether or not it lists
/// it.
#[derive(Clone)]
struct Answering {
    holds: bool,
    answer: std::result::Result<PropertyValue, String>,
}

impl ElementData for Answering {
    fn get(&self, _name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        self.answer.clone().map_err(NoValueError::new)
    }

    fn has_property(&self, _name: &str) -> bool {
        self.holds
    }

    fn keys(&self) -> Vec<String> {
        Vec::new()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

fn animal(value: impl Into<PropertyValue>) -> MapElementData {
    MapElementData::new().set("Animal", value)
}

fn french_animals(behavior: MissingTranslationBehavior, extra: &str) -> TranslationEngine {
    TranslationEngine::builder()
        .set_source_element_data_key(SOURCE_KEY)
        .set_missing_translation_behavior(behavior)
        .add_source_text("animals.fr_FR.yml", format!("{FRENCH_ANIMALS}{extra}"))
        .add_translation("Animal", "AnimalTranslated")
        .build()
        .unwrap()
}

fn evidence(entries: &[(&str, &str)]) -> Evidence {
    entries
        .iter()
        .fold(Evidence::builder(), |builder, (key, value)| {
            builder.add(key, *value)
        })
        .build()
}

fn element(element: impl FlowElement + 'static) -> Arc<dyn FlowElement> {
    Arc::new(element)
}

fn process_elements(elements: Vec<Arc<dyn FlowElement>>, entries: &[(&str, &str)]) -> FlowData {
    let pipeline = elements
        .into_iter()
        .fold(Pipeline::builder(), |builder, element| {
            builder.add_element(element)
        })
        .suppress_process_exceptions(true)
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data_with(evidence(entries));
    data.process().unwrap();
    data
}

fn process<T: ElementData + Clone>(
    engine: TranslationEngine,
    source: T,
    entries: &[(&str, &str)],
) -> FlowData {
    process_elements(
        vec![element(Source::new(SOURCE_KEY, source)), element(engine)],
        entries,
    )
}

fn translated(data: &FlowData, name: &str) -> AspectPropertyValue<TranslationValue> {
    data.get(TRANSLATION_KEY)
        .expect("the engine adds its data")
        .value(name)
        .cloned()
        .unwrap_or_else(|| panic!("'{name}' is set"))
}

fn string(value: &str) -> AspectPropertyValue<TranslationValue> {
    AspectPropertyValue::new(value.into())
}

fn error_messages(data: &FlowData) -> Vec<String> {
    data.errors()
        .iter()
        .map(|error| error.source.message().into_owned())
        .collect()
}

#[test]
fn a_string_is_translated() {
    let engine = french_animals(MissingTranslationBehavior::Original, "");
    let data = process(
        engine,
        animal("dog"),
        &[("header.accept-language", "fr_FR")],
    );
    assert_eq!(translated(&data, "AnimalTranslated"), string("chien"));
    assert_eq!(
        data.get_str("translation")
            .unwrap()
            .get("animaltranslated")
            .unwrap(),
        PropertyValue::from("chien")
    );
    assert!(data.errors().is_empty());
}

#[test]
fn each_form_of_accept_language_finds_the_language() {
    for header in [
        "fr-FR, fr;q=0.9, en;q=0.8, de;q=0.7, *;q=0.5",
        "fr,sr;q=0.9,uk;q=0.8,en-US;q=0.7,en;q=0.6,ru;q=0.5,hr;q=0.4",
        "fr",
        "fr-FR",
        "fr_FR",
    ] {
        let engine = french_animals(MissingTranslationBehavior::Original, "");
        let data = process(engine, animal("dog"), &[("header.accept-language", header)]);
        assert_eq!(
            translated(&data, "AnimalTranslated"),
            string("chien"),
            "{header}"
        );
    }
}

#[test]
fn a_value_is_matched_ignoring_case() {
    let engine = french_animals(MissingTranslationBehavior::Original, "");
    let data = process(
        engine,
        animal("Dog"),
        &[("header.accept-language", "fr_FR")],
    );
    assert_eq!(translated(&data, "AnimalTranslated"), string("chien"));
}

#[test]
fn building_needs_sources_translations_and_a_source_key() {
    let no_sources = TranslationEngine::builder()
        .add_translation("Animal", "AnimalTranslated")
        .set_source_element_data_key("somekey")
        .build();
    let no_translations = TranslationEngine::builder()
        .add_source_text("animals.fr_FR.yml", FRENCH_ANIMALS)
        .set_source_element_data_key("somekey")
        .build();
    let no_source_key = TranslationEngine::builder()
        .add_source_text("animals.fr_FR.yml", FRENCH_ANIMALS)
        .add_translation("Animal", "AnimalTranslated")
        .build();
    let blank_source_key = TranslationEngine::builder()
        .add_source_text("animals.fr_FR.yml", FRENCH_ANIMALS)
        .add_translation("Animal", "AnimalTranslated")
        .set_source_element_data_key(" ")
        .build();
    for (result, message) in [
        (no_sources, "At least one source file must be configured."),
        (
            no_translations,
            "At least one property translation must be configured.",
        ),
        (no_source_key, "The source element key must be configured."),
        (
            blank_source_key,
            "The source element key must be configured.",
        ),
    ] {
        let error = result.err().expect("the build fails");
        assert_eq!(error.message(), message);
    }
}

#[test]
fn a_source_name_with_no_locale_fails_the_build() {
    let error = TranslationEngine::builder()
        .set_source_element_data_key(SOURCE_KEY)
        .add_source_text("animals.mmm_kp.yml", FRENCH_ANIMALS)
        .add_translation("Animal", "AnimalTranslated")
        .build()
        .err()
        .expect("the build fails");
    assert_eq!(
        error.message(),
        "Source name 'animals.mmm_kp.yml' does not contain a valid locale code."
    );
}

#[test]
fn a_source_named_twice_fails_the_build() {
    let error = TranslationEngine::builder()
        .set_source_element_data_key(SOURCE_KEY)
        .add_source_text("animals.fr_FR.yml", FRENCH_ANIMALS)
        .add_source_text("animals.fr_FR.yml", FRENCH_ANIMALS)
        .add_translation("Animal", "AnimalTranslated")
        .build()
        .err()
        .expect("the build fails");
    assert_eq!(
        error.message(),
        "A source named 'animals.fr_FR.yml' was added twice."
    );
}

#[test]
fn a_list_is_translated() {
    let engine = french_animals(MissingTranslationBehavior::Original, "");
    let source = animal(vec!["cat".to_owned(), "dog".to_owned()]);
    let data = process(engine, source, &[("header.accept-language", "fr_FR")]);
    assert_eq!(
        translated(&data, "AnimalTranslated"),
        AspectPropertyValue::new(vec!["chat".to_owned(), "chien".to_owned()].into())
    );
}

#[test]
fn a_value_from_aspect_data_is_translated() {
    let engine = french_animals(MissingTranslationBehavior::Original, "");
    let source = AspectDataBase::new(SOURCE_KEY).set("Animal", "cat");
    let data = process(engine, source, &[("header.accept-language", "fr_FR")]);
    assert_eq!(translated(&data, "AnimalTranslated"), string("chat"));
}

#[test]
fn a_weighted_list_keeps_its_weightings() {
    let engine = french_animals(MissingTranslationBehavior::Original, "");
    let record = |raw: i64, value: &str| {
        BTreeMap::from([
            ("rawweighting".to_owned(), PropertyValue::Integer(raw)),
            ("value".to_owned(), PropertyValue::from(value)),
        ])
    };
    let source = animal(PropertyValue::KeyValueList(vec![
        record(1, "cat"),
        record(2, "dog"),
    ]));
    let data = process(engine, source, &[("header.accept-language", "fr_FR")]);
    assert_eq!(
        translated(&data, "AnimalTranslated"),
        AspectPropertyValue::new(
            vec![
                WeightedValue::new(1, "chat".to_owned()),
                WeightedValue::new(2, "chien".to_owned()),
            ]
            .into()
        )
    );
    assert_eq!(
        data.get_str("translation")
            .unwrap()
            .get("AnimalTranslated")
            .unwrap(),
        PropertyValue::KeyValueList(vec![record(1, "chat"), record(2, "chien")])
    );
}

/// The IP Intelligence data gives each weighting as a weight from 0 to 1,
/// which is turned back into the exact raw weighting.
#[test]
fn a_weight_from_zero_to_one_gives_the_raw_weighting() {
    let engine = french_animals(MissingTranslationBehavior::Original, "");
    let record = |raw: u16, value: &str| {
        BTreeMap::from([
            (
                "weight".to_owned(),
                PropertyValue::Double(f64::from(WeightedValue::new(raw, ()).weighting())),
            ),
            ("value".to_owned(), PropertyValue::from(value)),
        ])
    };
    let source = animal(PropertyValue::KeyValueList(vec![
        record(35535, "cat"),
        record(30000, "dog"),
    ]));
    let data = process(engine, source, &[("header.accept-language", "fr_FR")]);
    assert_eq!(
        translated(&data, "AnimalTranslated"),
        AspectPropertyValue::new(
            vec![
                WeightedValue::new(35535, "chat".to_owned()),
                WeightedValue::new(30000, "chien".to_owned()),
            ]
            .into()
        )
    );
}

#[test]
fn a_language_with_no_translator_follows_the_behavior() {
    for language in ["de_DE", "mm_llk"] {
        for behavior in BEHAVIORS {
            let engine = french_animals(behavior, "");
            let data = process(
                engine,
                animal("cat"),
                &[("header.accept-language", language)],
            );
            match behavior {
                MissingTranslationBehavior::Original => {
                    assert_eq!(translated(&data, "AnimalTranslated"), string("cat"));
                    assert!(data.errors().is_empty());
                }
                MissingTranslationBehavior::EmptyString => {
                    assert_eq!(translated(&data, "AnimalTranslated"), string(""));
                    assert!(data.errors().is_empty());
                }
                MissingTranslationBehavior::FlowError => {
                    assert_eq!(
                        error_messages(&data),
                        [format!(
                            "There was no translator configured for the language '{language}'."
                        )]
                    );
                    assert_eq!(data.errors()[0].element_data_key, "translation");
                    let output = data.get(TRANSLATION_KEY).unwrap();
                    assert_eq!(output.names().count(), 0);
                }
            }
        }
    }
}

#[test]
fn a_value_with_no_translation_follows_the_behavior() {
    // A blank translation in the source counts as none.
    for (extra, value) in [("", "buffalo"), ("buffalo: \n", "buffalo")] {
        for behavior in BEHAVIORS {
            let engine = french_animals(behavior, extra);
            let data = process(
                engine,
                animal(value),
                &[("header.accept-language", "fr_FR")],
            );
            let output = translated(&data, "AnimalTranslated");
            match behavior {
                MissingTranslationBehavior::Original => {
                    assert_eq!(output, string("buffalo"));
                    assert!(data.errors().is_empty());
                }
                MissingTranslationBehavior::EmptyString => {
                    assert_eq!(output, string(""));
                    assert!(data.errors().is_empty());
                }
                MissingTranslationBehavior::FlowError => {
                    assert_eq!(
                        output,
                        AspectPropertyValue::new(TranslationValue::String(None))
                    );
                    assert_eq!(
                        error_messages(&data),
                        ["There was no translation found for the value 'buffalo'."]
                    );
                    // Nothing is written for the value in the property bag.
                    let bag = data.get_str("translation").unwrap();
                    assert!(bag.keys().is_empty());
                    assert!(bag.get("AnimalTranslated").is_err());
                }
            }
        }
    }
}

#[test]
fn a_source_value_with_no_value_keeps_its_reason() {
    let engine = french_animals(MissingTranslationBehavior::Original, "");
    let source = Answering {
        holds: true,
        answer: Err("Test no value message".to_owned()),
    };
    let data = process(engine, source, &[("header.accept-language", "fr_FR")]);
    assert_eq!(
        translated(&data, "AnimalTranslated"),
        AspectPropertyValue::no_value("Test no value message")
    );
}

#[test]
fn a_fixed_language_needs_no_evidence() {
    let engine = TranslationEngine::builder()
        .set_source_element_data_key(SOURCE_KEY)
        .add_source_text("animals.fr_FR.yml", FRENCH_ANIMALS)
        .set_fixed_language("fr_FR")
        .add_translation("Animal", "AnimalTranslated")
        .build()
        .unwrap();
    assert_eq!(engine.fixed_language(), Some("fr_FR"));
    let data = process(engine, animal("cat"), &[("header.accept-language", "de")]);
    assert_eq!(translated(&data, "AnimalTranslated"), string("chat"));
}

#[test]
fn a_fixed_language_with_no_locale_leaves_the_language_to_the_evidence() {
    let engine = TranslationEngine::builder()
        .set_source_element_data_key(SOURCE_KEY)
        .add_source_text("animals.fr_FR.yml", FRENCH_ANIMALS)
        .set_fixed_language("French")
        .add_translation("Animal", "AnimalTranslated")
        .build()
        .unwrap();
    assert_eq!(engine.fixed_language(), None);
    let data = process(engine, animal("cat"), &[("header.accept-language", "fr")]);
    assert_eq!(translated(&data, "AnimalTranslated"), string("chat"));
}

/// Two engines sharing the data key write to the same data, so the second
/// can translate what the first wrote.
#[test]
fn two_engines_can_be_chained() {
    let codes = TranslationEngine::builder()
        .set_source_element_data_key(SOURCE_KEY)
        .add_source_text("codes.en_GB.yml", "GB: United Kingdom\n")
        .set_fixed_language("en_GB")
        .add_translation("CountryCode", "Country")
        .build()
        .unwrap();
    let countries = TranslationEngine::builder()
        .set_source_element_data_key(codes.data_key())
        .add_source_text("countries.fr_FR.yml", "United Kingdom: Royaume-Uni\n")
        .add_translation("Country", "CountryTranslated")
        .build()
        .unwrap();
    let source = MapElementData::new().set("CountryCode", "GB");
    let data = process_elements(
        vec![
            element(Source::new(SOURCE_KEY, source)),
            element(codes),
            element(countries),
        ],
        &[("header.accept-language", "fr_FR")],
    );
    assert_eq!(translated(&data, "Country"), string("United Kingdom"));
    assert_eq!(
        translated(&data, "CountryTranslated"),
        string("Royaume-Uni")
    );
    assert_eq!(data.data_keys(), [SOURCE_KEY, "translation"]);
    assert!(data.errors().is_empty());
}

/// Each element of a parallel group writes data of its own, so engines that
/// share a data key are refused there and run one after another instead.
#[test]
fn engines_sharing_a_data_key_are_refused_in_one_parallel_group() {
    let outcome = Pipeline::builder()
        .add_element(element(Source::new(SOURCE_KEY, animal("Dog"))))
        .add_parallel_elements([
            element(french_animals(MissingTranslationBehavior::Original, "")),
            element(french_animals(MissingTranslationBehavior::Original, "")),
        ])
        .build();
    assert_eq!(
        outcome.err().expect("the build fails").message(),
        "Two elements of a parallel group have the data key 'translation', so \
         only one of them could keep its data. Add one of them on its own."
    );
}

#[test]
fn english_preferred_gives_no_translation() {
    let engine = french_animals(MissingTranslationBehavior::Original, "");
    let data = process(
        engine,
        animal("dog"),
        &[("header.accept-language", "en-US,en;q=0.9,fr;q=0.5")],
    );
    assert_eq!(translated(&data, "AnimalTranslated"), string("dog"));
}

#[test]
fn a_locale_is_resolved_against_those_available() {
    let locales = ["fr_FR", "de_DE"];
    let resolve = |value: &str| {
        Languages::try_resolve_locale(value, locales.iter().copied(), Some(BASE_LANGUAGE)).unwrap()
    };
    assert_eq!(resolve("en-US,en;q=0.9,fr;q=0.5"), None);
    assert_eq!(resolve("fr,en;q=0.5"), Some("fr_FR"));
}

#[test]
fn the_translator_found_gives_its_locale() {
    let engine = TranslationEngine::builder()
        .set_source_element_data_key(SOURCE_KEY)
        .add_source_text("animals.fr_FR.yml", "cat: chat\n")
        .add_source_text("animals.de_DE.yml", "cat: Katze\n")
        .add_translation("Animal", "AnimalTranslated")
        .build()
        .unwrap();
    let languages = engine.languages();
    for (language, expected) in [
        ("fr_FR", Some("fr_FR")),
        ("fr", Some("fr_FR")),
        ("fr,de-DE;q=0.5", Some("fr_FR")),
        ("ja_JP", None),
    ] {
        let found = languages.try_get_translator(language).unwrap();
        assert_eq!(found.map(|(locale, _)| locale), expected, "{language}");
    }
    let (_, german) = languages.try_get_translator("de").unwrap().unwrap();
    assert_eq!(
        german.translate("cat", &mut Vec::new()).as_deref(),
        Some("Katze")
    );
}

#[test]
fn the_language_comes_from_the_first_evidence_with_a_value() {
    let engine = || {
        TranslationEngine::builder()
            .set_source_element_data_key(SOURCE_KEY)
            .add_source_text("animals.fr_FR.yml", "cat: chat\n")
            .add_source_text("animals.de_DE.yml", "cat: Katze\n")
            .add_source_text("animals.es_ES.yml", "cat: gato\n")
            .add_translation("Animal", "AnimalTranslated")
            .build()
            .unwrap()
    };
    let cases: [(&[(&str, &str)], &str); 5] = [
        (
            &[
                ("query.translation", "de_DE"),
                ("query.accept-language", "es"),
                ("header.accept-language", "fr"),
            ],
            "Katze",
        ),
        (
            &[
                ("query.accept-language", "es"),
                ("header.accept-language", "fr"),
            ],
            "gato",
        ),
        (
            &[("query.translation", " "), ("header.accept-language", "fr")],
            "chat",
        ),
        // A value that names no language held is not passed over.
        (
            &[
                ("query.translation", "ja_JP"),
                ("header.accept-language", "fr"),
            ],
            "cat",
        ),
        (&[], "cat"),
    ];
    for (entries, expected) in cases {
        let data = process(engine(), animal("cat"), entries);
        assert_eq!(
            translated(&data, "AnimalTranslated"),
            string(expected),
            "{entries:?}"
        );
    }
}

#[test]
fn no_language_with_flow_error_adds_an_error() {
    let engine = french_animals(MissingTranslationBehavior::FlowError, "");
    let data = process(engine, animal("cat"), &[]);
    assert_eq!(
        error_messages(&data),
        ["The evidence did not contain a language to translate to."]
    );
    assert_eq!(data.get(TRANSLATION_KEY).unwrap().names().count(), 0);
}

#[test]
fn an_accept_language_that_cannot_be_read_is_an_error() {
    let engine = french_animals(MissingTranslationBehavior::Original, "");
    let data = process(
        engine,
        animal("cat"),
        &[("header.accept-language", "fr,de;q=2")],
    );
    assert_eq!(
        error_messages(&data),
        ["The format of value 'de;q=2' is invalid."]
    );
    assert_eq!(data.get(TRANSLATION_KEY).unwrap().names().count(), 0);
}

#[test]
fn missing_source_data_is_an_error() {
    let engine = french_animals(MissingTranslationBehavior::Original, "");
    let data = process_elements(vec![element(engine)], &[("header.accept-language", "fr")]);
    assert_eq!(
        error_messages(&data),
        ["The source data 'evidencecopy' could not be found in the FlowData."]
    );
    assert_eq!(data.get(TRANSLATION_KEY).unwrap().names().count(), 0);
}

#[test]
fn a_value_that_cannot_be_translated_stops_the_rest() {
    let engine = TranslationEngine::builder()
        .set_source_element_data_key(SOURCE_KEY)
        .add_source_text("animals.fr_FR.yml", FRENCH_ANIMALS)
        .add_translation("Animal", "AnimalTranslated")
        .add_translation("Legs", "LegsTranslated")
        .add_translation("Other", "OtherTranslated")
        .build()
        .unwrap();
    let source = animal("cat").set("Legs", 4_i64).set("Other", "dog");
    let data = process(engine, source, &[("header.accept-language", "fr")]);
    assert_eq!(
        error_messages(&data),
        [
            "The value of the source property 'Legs' is of the type 'Integer', which is not \
          supported for translation."
        ]
    );
    let output = data.get(TRANSLATION_KEY).unwrap();
    assert_eq!(output.names().collect::<Vec<_>>(), ["AnimalTranslated"]);
}

#[test]
fn the_engine_lists_each_destination_once() {
    let engine = TranslationEngine::builder()
        .set_source_element_data_key(SOURCE_KEY)
        .add_source_text("animals.fr_FR.yml", FRENCH_ANIMALS)
        .add_translation("Animal", "AnimalTranslated")
        .add_translation("Other", "animaltranslated")
        .add_translation_of_type("Names", "NamesTranslated", PropertyValueType::KeyValueList)
        .set_element_data_key("animals")
        .build()
        .unwrap();
    let properties: Vec<_> = engine
        .properties()
        .iter()
        .map(|property| {
            (
                property.name.as_str(),
                property.element_data_key.as_str(),
                property.value_type,
                property.available,
            )
        })
        .collect();
    assert_eq!(
        properties,
        [
            (
                "AnimalTranslated",
                "animals",
                PropertyValueType::String,
                true
            ),
            (
                "NamesTranslated",
                "animals",
                PropertyValueType::KeyValueList,
                true
            ),
        ]
    );
    assert_eq!(engine.data_key(), "animals");
    let filter = engine.evidence_key_filter();
    for key in [
        "query.translation",
        "query.accept-language",
        "header.accept-language",
    ] {
        assert!(filter.include(key), "{key}");
    }
    assert!(!filter.include("header.user-agent"));
}

#[cfg(not(target_family = "wasm"))]
mod files {
    use super::*;
    use std::path::PathBuf;

    /// A folder of its own for one test, removed when the test ends.
    struct Folder(PathBuf);

    impl Folder {
        fn new(test: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "fiftyone-pipeline-translation-{}-{test}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Folder(path)
        }

        fn write(&self, name: &str, text: &str) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, text).unwrap();
            path
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_source_file_is_read() {
        let folder = Folder::new("file");
        let path = folder.write("animals.fr_FR.yml", FRENCH_ANIMALS);
        let engine = TranslationEngine::builder()
            .set_source_element_data_key(SOURCE_KEY)
            .add_source(path)
            .add_translation("Animal", "AnimalTranslated")
            .build()
            .unwrap();
        let data = process(engine, animal("dog"), &[("header.accept-language", "fr")]);
        assert_eq!(translated(&data, "AnimalTranslated"), string("chien"));
    }

    #[test]
    fn a_wildcard_adds_every_matching_file() {
        let folder = Folder::new("wildcard");
        folder.write("animals.fr_FR.yml", FRENCH_ANIMALS);
        folder.write("animals.de_DE.yml", "cat: Katze\ndog: Hund\n");
        folder.write("plants.es_ES.yml", "tree: arbol\n");
        let engine = TranslationEngine::builder()
            .set_source_element_data_key(SOURCE_KEY)
            .add_source(folder.0.join("animals.*.yml"))
            .add_translation("Animal", "AnimalTranslated")
            .build()
            .unwrap();
        // In order of name.
        assert_eq!(
            engine.languages().locales().collect::<Vec<_>>(),
            ["de_DE", "fr_FR"]
        );
    }

    #[test]
    fn a_missing_file_fails_the_build() {
        let folder = Folder::new("missing");
        let error = TranslationEngine::builder()
            .set_source_element_data_key(SOURCE_KEY)
            .add_source(folder.0.join("animals.fr_FR.yml"))
            .add_translation("Animal", "AnimalTranslated")
            .build()
            .err()
            .expect("the build fails");
        assert!(error.message().starts_with("The source file '"), "{error}");
    }

    #[test]
    fn an_empty_source_file_fails_the_build() {
        let folder = Folder::new("empty");
        let path = folder.write("animals.fr_FR.yml", "");
        let error = TranslationEngine::builder()
            .set_source_element_data_key(SOURCE_KEY)
            .add_source(path)
            .add_translation("Animal", "AnimalTranslated")
            .build()
            .err()
            .expect("the build fails");
        assert_eq!(
            error.message(),
            "The source for animals.fr_FR.yml could not be parsed into a valid translation \
             lookup."
        );
    }
}

#[test]
fn the_language_evidence_keys_are_a_slice_in_order() {
    // A slice's type does not hold its length, so a key added later leaves
    // the constant's type unchanged.
    let keys: &'static [&str] = LANGUAGE_EVIDENCE_KEYS;
    assert_eq!(
        keys,
        [
            "query.translation",
            "query.accept-language",
            "header.accept-language"
        ]
    );
}
