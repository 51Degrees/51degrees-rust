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

//! The translation engine for the 51Degrees pipeline, as the
//! [translation engine specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/translation-engine.md)
//! describes.
//!
//! A [`TranslationEngine`] reads properties from the data of one other
//! element, translates their values into the language the request asks for,
//! and writes them to its own [`TranslationData`] under new names. Each
//! language's translations come from a YAML source of `value: translation`
//! lines, named for its locale, such as `animals.fr_FR.yml`.
//!
//! The language is read from the `query.translation`,
//! `query.accept-language` or `header.accept-language` evidence, in that
//! order, unless the engine is given a fixed language. A request that
//! prefers English gets no translation, because the source values are
//! already in English.
//!
//! ```
//! use fiftyone_pipeline_translation::{MissingTranslationBehavior, TranslationEngine};
//!
//! let engine = TranslationEngine::builder()
//!     .set_source_element_data_key("animals")
//!     .add_source_text("animals.fr_FR.yml", "cat: chat\ndog: chien\n")
//!     .add_source_text("animals.de_DE.yml", "cat: Katze\ndog: Hund\n")
//!     .add_translation("Animal", "AnimalTranslated")
//!     .build()?;
//!
//! // The engine reads the language from the request's evidence. Here the
//! // same resolution is asked for directly.
//! let (locale, translator) = engine
//!     .languages()
//!     .try_get_translator("de-CH,fr;q=0.8,en;q=0.5")?
//!     .expect("French is held");
//! assert_eq!(locale, "fr_FR");
//! assert_eq!(translator.translate("Dog", &mut Vec::new()).as_deref(), Some("chien"));
//! assert_eq!(translator.behavior(), MissingTranslationBehavior::Original);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![warn(missing_docs)]

mod behavior;
mod builder;
pub mod configuration;
mod data;
mod engine;
mod languages;
mod translator;

pub use behavior::MissingTranslationBehavior;
pub use builder::TranslationEngineBuilder;
pub use data::{
    TranslationData, TranslationValue, NO_TRANSLATION_MESSAGE, RAW_WEIGHTING_KEY, VALUE_KEY,
};
pub use engine::{
    TranslationEngine, TranslationProperty, LANGUAGE_EVIDENCE_KEYS, TRANSLATION_DATA_KEY,
    TRANSLATION_KEY,
};
pub use languages::{InvalidAcceptLanguage, Languages, BASE_LANGUAGE};
pub use translator::Translator;
