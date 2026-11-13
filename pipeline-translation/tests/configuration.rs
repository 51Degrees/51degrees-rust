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

//! The translation engine made from a configuration, compared with what
//! .NET's FiftyOne.Pipeline.Translation 4.5.129 did with the same
//! configuration, its builder given by the service provider. Each message
//! is the one .NET gave, apart from the differences the tests name.
//!
//! .NET gives a cause its builder or engine refuses as the inner message of
//! "Exception has been thrown by the target of an invocation.", and the
//! crates give the cause itself, as `fiftyone_pipeline_core`'s
//! configuration tests record. .NET lists each setting's type as
//! `RuntimeParameterInfo`, where the crates give the type.

use std::sync::Arc;

use fiftyone_pipeline_core::configuration::{
    BuilderRegistry, ConfigValue, ElementOptions, PipelineOptions,
};
use fiftyone_pipeline_core::{Error, Pipeline, PipelineBuilder, Result};
use fiftyone_pipeline_translation::configuration;

const BUILDER: &str = "TranslationEngineBuilder";
const BUILDER_TYPE: &str = "fiftyone_pipeline_translation::builder::TranslationEngineBuilder";
const NO_SUB_ELEMENTS: &str = "No SubElements";
const NO_INNER: &str = "No Inner Exception.";
const NO_TRANSLATIONS: &str =
    "At least one property translation must be configured. (Parameter 'translations')";

fn registry() -> BuilderRegistry {
    let mut registry = BuilderRegistry::new();
    configuration::add_builders(&mut registry);
    registry
}

/// The element `builder` with `parameters`, alone in a pipeline.
fn with(builder: &str, parameters: &[(&str, &str)]) -> Result<Arc<Pipeline>> {
    let element = parameters
        .iter()
        .fold(ElementOptions::new(builder), |element, (name, value)| {
            element.parameter(*name, ConfigValue::from(*value))
        });
    PipelineBuilder::new()
        .build_from_configuration(&registry(), &PipelineOptions::new().element(element))
}

fn message(result: Result<Arc<Pipeline>>) -> String {
    match result {
        Err(Error::PipelineConfiguration { message }) => message,
        Err(other) => panic!("not a configuration error: {other}"),
        Ok(_) => panic!("the pipeline was built"),
    }
}

/// The message for the element named `builder` that cannot be made.
fn failed(builder: &str, message: &str) -> String {
    format!(
        "Configuration failed to build.\nBuilderName: {builder}\nSubElement Builder Names: \
         {NO_SUB_ELEMENTS}\n{message}\n{NO_INNER}\n"
    )
}

/// .NET's message for `parameter`, which names no setting and no build
/// parameter, listing the four settings that take one value in the order
/// .NET gave them.
fn unknown(parameter: &str) -> String {
    failed(
        BUILDER,
        &format!(
            "Builder '{BUILDER_TYPE}' for element 0 has no 'Set' methods or 'Build' methods \
             that match (case-insensitively) the following parameters: {}. The available \
             configuration methods on this builder are: \nSetSourceElementDataKey (String)\n\
             SetFixedLanguage (String)\nSetMissingTranslationBehavior \
             (MissingTranslationBehavior)\nAddSource (String)\nThe available 'Build' methods \
             have the following signatures: \n\n",
            parameter.to_uppercase()
        ),
    )
}

/// .NET found the builder by each name, and the engine it made had no
/// translation, because a configuration cannot give one.
#[test]
fn the_builder_is_found_by_dotnets_names_and_has_no_translation() {
    for name in [
        "TranslationEngineBuilder",
        "TranslationEngine",
        "translationengine",
    ] {
        assert_eq!(
            message(with(name, &[])),
            failed(name, NO_TRANSLATIONS),
            "{name}"
        );
    }
}

/// .NET read all three settings, by either name, and still had no
/// translation.
#[test]
fn the_three_settings_are_read_and_the_engine_still_has_no_translation() {
    for settings in [
        [
            ("SourceElementDataKey", "ip"),
            ("FixedLanguage", "fr_FR"),
            ("MissingTranslationBehavior", "emptystring"),
        ],
        [
            ("SetSourceElementDataKey", "ip"),
            ("SetFixedLanguage", "fr_FR"),
            ("SetMissingTranslationBehavior", "FlowError"),
        ],
    ] {
        assert_eq!(
            message(with(BUILDER, &settings)),
            failed(BUILDER, NO_TRANSLATIONS),
            "{settings:?}"
        );
    }
}

/// .NET read each name ignoring case, and each number a value has.
#[test]
fn the_missing_translation_behavior_is_read_by_name_or_number() {
    for value in [
        "Original",
        "ORIGINAL",
        "emptystring",
        "FlowError",
        " FlowError ",
        "0",
        "1",
        "2",
    ] {
        assert_eq!(
            message(with(BUILDER, &[("MissingTranslationBehavior", value)])),
            failed(BUILDER, NO_TRANSLATIONS),
            "{value:?}"
        );
    }
}

/// .NET refused a name no value has with this message. It accepted `7`
/// and `Original, FlowError`, as `Enum.Parse` does, and failed only for
/// having no translation, where the crates refuse both, as
/// `fiftyone_pipeline_core::configuration::parse_enum` says.
#[test]
fn a_missing_translation_behavior_no_value_has_is_refused() {
    for value in ["Nonsense", "7", "Original, FlowError"] {
        assert_eq!(
            message(with(BUILDER, &[("MissingTranslationBehavior", value)])),
            format!(
                "Configuration failed to build.\nBuilderName: {BUILDER}\nSubElement Builder \
                 Names: {NO_SUB_ELEMENTS}\nMethod 'SetMissingTranslationBehavior' on builder \
                 '{BUILDER_TYPE}' for element 0 expects a parameter of type \
                 'MissingTranslationBehavior'. Failed to parse enum value '{value}'. Valid \
                 values are: Original, EmptyString, FlowError\nRequested value '{value}' was \
                 not found.\n"
            ),
            "{value:?}"
        );
    }
}

/// .NET's `AddTranslation` takes a source and a destination, so .NET
/// refused it.
#[test]
fn add_translation_takes_two_values_and_is_refused() {
    assert_eq!(
        message(with(BUILDER, &[("AddTranslation", "Animal")])),
        failed(
            BUILDER,
            &format!(
                "Method 'AddTranslation' on builder '{BUILDER_TYPE}' for element 0 takes more \
                 than one parameter. This is not supported."
            )
        )
    );
}

/// None of these is a method of .NET's builder, so .NET took each for a
/// build parameter and found no build method taking it.
#[test]
fn sources_and_translations_are_not_settings() {
    for name in [
        "Sources",
        "Source",
        "Translations",
        "Translation",
        "MissingTranslationBehaviour",
    ] {
        assert_eq!(
            message(with(BUILDER, &[(name, "x")])),
            unknown(name),
            "{name}"
        );
    }
}

/// The tests that read source files, compiled only off WebAssembly, where a
/// test has no file system.
#[cfg(not(target_family = "wasm"))]
mod from_files {
    use std::path::PathBuf;

    use super::*;

    /// A folder holding French and German sources, removed when the test
    /// ends.
    struct Sources(PathBuf);

    impl Sources {
        fn new(test: &str) -> Self {
            let folder = std::env::temp_dir().join(format!(
                "fiftyone-translation-configuration-{test}-{}",
                std::process::id()
            ));
            std::fs::create_dir_all(&folder).expect("the folder is made");
            std::fs::write(folder.join("animals.fr_FR.yml"), "cat: chat\ndog: chien\n")
                .expect("the French source is written");
            std::fs::write(folder.join("animals.de_DE.yml"), "cat: Katze\ndog: Hund\n")
                .expect("the German source is written");
            Sources(folder)
        }

        fn path(&self, name: &str) -> String {
            self.0.join(name).display().to_string()
        }
    }

    impl Drop for Sources {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// .NET read the file, or every file the wildcard matched, and then had
    /// no translation.
    #[test]
    fn add_source_reads_a_file() {
        let sources = Sources::new("reads");
        for name in ["animals.fr_FR.yml", "animals.*.yml"] {
            let path = sources.path(name);
            assert_eq!(
                message(with(
                    BUILDER,
                    &[("AddSource", &path), ("SourceElementDataKey", "animals")]
                )),
                failed(BUILDER, NO_TRANSLATIONS),
                "{name}"
            );
        }
    }

    /// .NET failed as soon as `AddSource` could not read the file, with
    /// "Could not find file '<full path>'." as the inner message. The
    /// crates give their own message for the file, which is checked before
    /// the translations as .NET's is.
    #[test]
    fn add_source_fails_for_a_file_that_cannot_be_read() {
        let sources = Sources::new("missing");
        let path = sources.path("missing.fr_FR.yml");
        let message = message(with(BUILDER, &[("AddSource", &path)]));
        let start = format!(
            "Configuration failed to build.\nBuilderName: {BUILDER}\nSubElement Builder \
             Names: {NO_SUB_ELEMENTS}\nThe source file '{path}' could not be read: "
        );
        assert!(message.starts_with(&start), "{message}");
        assert!(message.ends_with(&format!("\n{NO_INNER}\n")), "{message}");
    }
}
