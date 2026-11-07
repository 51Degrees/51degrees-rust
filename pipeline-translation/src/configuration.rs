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

//! The translation engine, found by the name .NET gives its builder, so
//! that a configuration file written for .NET makes the same engine here.
//! See [`fiftyone_pipeline_core::configuration`].
//!
//! [`add_builders`] adds `TranslationEngineBuilder`, which a configuration
//! can also name `TranslationEngine`, with the settings of .NET's builder
//! that take one value.
//!
//! - `SourceElementDataKey`, `FixedLanguage` and
//!   `MissingTranslationBehavior`, which is `Original`, `EmptyString` or
//!   `FlowError`, ignoring case, or the number of one of them.
//! - `AddSource`, the path of a source file, read as
//!   [`TranslationEngineBuilder::add_source`] reads one.
//!
//! Each translation is a pair of property names, so translations are given
//! in code only, and `AddTranslation` fails with .NET's message for a method
//! taking two values. A translation engine made from a configuration alone
//! therefore always fails to build, as in .NET. A crate that translates
//! particular properties offers a builder of its own that adds its sources
//! and translations, as `fiftyone-ip-intelligence-translation` does for
//! country names, and a configuration names that builder.
//!
//! ```
//! use fiftyone_pipeline_core::configuration::{BuilderRegistry, PipelineOptions};
//! use fiftyone_pipeline_core::PipelineBuilder;
//! use fiftyone_pipeline_translation::configuration;
//!
//! let mut registry = BuilderRegistry::new();
//! configuration::add_builders(&mut registry);
//! let options = PipelineOptions::from_json(
//!     r#"{
//!         "PipelineOptions": {
//!             "Elements": [
//!                 {
//!                     "BuilderName": "TranslationEngine",
//!                     "BuildParameters": {
//!                         "SourceElementDataKey": "animals",
//!                         "FixedLanguage": "fr_FR",
//!                         "MissingTranslationBehavior": "EmptyString"
//!                     }
//!                 }
//!             ]
//!         }
//!     }"#,
//! )?;
//! let error = PipelineBuilder::new()
//!     .build_from_configuration(&registry, &options)
//!     .err()
//!     .expect("an engine with no translation is not built");
//! assert!(error
//!     .message()
//!     .contains("At least one property translation must be configured."));
//! # Ok::<(), fiftyone_pipeline_core::Error>(())
//! ```

use std::sync::Arc;

use fiftyone_pipeline_core::configuration::{
    parse_enum, BuildContext, BuilderDefinition, BuilderRegistry, ConfigType, ParseFailure,
};
use fiftyone_pipeline_core::FlowElement;

use crate::{MissingTranslationBehavior, TranslationEngine, TranslationEngineBuilder};

/// Add the translation engine's builder to `registry`, as
/// `TranslationEngineBuilder`. Returns `registry` for chaining.
pub fn add_builders(registry: &mut BuilderRegistry) -> &mut BuilderRegistry {
    registry.add(translation_engine_builder())
}

/// The settings in the order .NET's `GetMethods` gives the builder's
/// methods, which is the order the two `AddSource` methods are tried in.
fn translation_engine_builder() -> BuilderDefinition<TranslationEngineBuilder> {
    BuilderDefinition::new(
        "TranslationEngineBuilder",
        |builder: TranslationEngineBuilder, context: &mut BuildContext| {
            let engine = Arc::new(builder.build()?);
            context.add_resource(Arc::clone(&engine) as _);
            Ok(engine as Arc<dyn FlowElement>)
        },
    )
    .setting(
        "SetSourceElementDataKey",
        |builder: TranslationEngineBuilder, key: String| builder.set_source_element_data_key(key),
    )
    .setting(
        "SetFixedLanguage",
        |builder: TranslationEngineBuilder, language: String| builder.set_fixed_language(language),
    )
    .setting(
        "SetMissingTranslationBehavior",
        |builder: TranslationEngineBuilder, behavior: MissingTranslationBehavior| {
            builder.set_missing_translation_behavior(behavior)
        },
    )
    .method_with_parameters("AddTranslation", 2)
    .setting(
        "AddSource",
        |builder: TranslationEngineBuilder, path: String| builder.add_source(path),
    )
    .method_with_parameters("AddSource", 2)
    .build_method(Vec::new(), |_, _| Ok(TranslationEngine::builder()))
}

impl ConfigType for MissingTranslationBehavior {
    const TYPE_NAME: &'static str = "MissingTranslationBehavior";

    fn parse_text(text: &str) -> Result<Self, ParseFailure> {
        parse_enum(
            text,
            &[
                ("Original", 0, MissingTranslationBehavior::Original),
                ("EmptyString", 1, MissingTranslationBehavior::EmptyString),
                ("FlowError", 2, MissingTranslationBehavior::FlowError),
            ],
        )
    }
}
