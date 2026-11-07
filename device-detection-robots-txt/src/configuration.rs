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

//! The robots.txt engine, found by the name .NET gives its builder, so that
//! a configuration file written for .NET makes the same engine here. See
//! [`fiftyone_pipeline_core::configuration`].
//!
//! [`add_builders`] adds `RobotsTxtEngineBuilder`, which a configuration can
//! also name `RobotsTxtEngine`. It takes no build parameters, and .NET's
//! settings:
//!
//! - `SetUserAgent`, and `UseDefaultTdlSourceResolver`, which does the same,
//!   give the User-Agent the GitHub TDL source resolver sends, as the
//!   builder's `set_user_agent` does. Blank text is refused,
//!   as .NET refuses it. Without the `reqwest-client` feature there is no
//!   GitHub resolver, so the text is read and has no effect, with a warning
//!   logged.
//! - `SetProperties` and `SetProperty`, the properties to give.
//! - `SetCacheSize` and `SetCacheHitOrMiss` are read and have no effect,
//!   with .NET's warning that the engine has no cache logged, as .NET logs
//!   it. `SetLazyLoadingTimeout` is read and has no effect, with a warning
//!   logged, as the engine has no lazy loading.
//! - `SetPerformanceProfile` is refused once the profile is read, as .NET's
//!   builder does not implement it.
//! - `SetTdlSourceResolver`, `SetCache` and `SetLazyLoading` take their
//!   values in code only, so naming one fails as it does in .NET.
//!
//! The engine needs an on-premise Device Detection engine earlier in the
//! pipeline, so a configuration names one first. The pipeline holds the
//! engine made, so it can be read back.
//!
//! ```no_run
//! use fiftyone_device_detection_robots_txt::{configuration, RobotsTxtEngine};
//! use fiftyone_pipeline_core::configuration::{BuilderRegistry, PipelineOptions};
//! use fiftyone_pipeline_core::PipelineBuilder;
//!
//! let mut registry = BuilderRegistry::new();
//! fiftyone_device_detection_onpremise::configuration::add_builders(&mut registry);
//! configuration::add_builders(&mut registry);
//! let options = PipelineOptions::from_json(
//!     r#"{
//!         "PipelineOptions": {
//!             "Elements": [
//!                 {
//!                     "BuilderName": "DeviceDetectionHashEngine",
//!                     "BuildParameters": {
//!                         "DataFile": "Enterprise-HashV41.hash",
//!                         "CreateTempDataCopy": "false",
//!                         "AutoUpdate": "false",
//!                         "DataFileSystemWatcher": "false"
//!                     }
//!                 },
//!                 {
//!                     "BuilderName": "RobotsTxtEngine",
//!                     "BuildParameters": { "UserAgent": "Example/1.0" }
//!                 }
//!             ]
//!         }
//!     }"#,
//! )?;
//! let pipeline = PipelineBuilder::new().build_from_configuration(&registry, &options)?;
//! let engine = pipeline
//!     .resource::<RobotsTxtEngine>()
//!     .expect("the robots.txt engine");
//! assert!(engine.model().is_some());
//! # Ok::<(), fiftyone_pipeline_core::Error>(())
//! ```

use std::sync::Arc;

use fiftyone_pipeline_core::configuration::{
    parse_enum, BuildContext, BuilderDefinition, BuilderRegistry, ConfigType, ParseFailure,
};
use fiftyone_pipeline_core::{Error, FlowElement, Result};

use crate::RobotsTxtEngineBuilder;

/// The name of the engine in the warnings logged.
const ENGINE_NAME: &str = "RobotsTxtEngine";

/// .NET's warning for a cache setting, which a property keyed engine reads
/// and ignores.
const NO_CACHE: &str = "Caching is not supported by property keyed engines.";

/// .NET's message for a blank User-Agent.
const NO_USER_AGENT: &str = "User-Agent must be supplied. (Parameter 'userAgent')";

/// .NET's message for a method its builder does not implement.
const NOT_IMPLEMENTED: &str = "The method or operation is not implemented.";

/// .NET's message for a profile number that does not fit in 32 bits.
const TOO_LARGE: &str = "Value was either too large or too small for an Int32.";

/// Add the robots.txt engine's builder to `registry`, as
/// `RobotsTxtEngineBuilder`. Returns `registry` for chaining.
pub fn add_builders(registry: &mut BuilderRegistry) -> &mut BuilderRegistry {
    registry.add(robots_txt_engine_builder())
}

/// A profile read as .NET's `Enum.Parse` reads a `PerformanceProfiles`,
/// before `SetPerformanceProfile` refuses it. That gives a profile's name
/// or any whole number that fits in 32 bits, and refuses a larger one
/// outright.
enum PerformanceProfiles {
    Read,
    TooLarge,
}

impl ConfigType for PerformanceProfiles {
    const TYPE_NAME: &'static str = "PerformanceProfiles";

    fn parse_text(text: &str) -> std::result::Result<Self, ParseFailure> {
        let names = [
            ("LowMemory", 0, ()),
            ("MaxPerformance", 1, ()),
            ("HighPerformance", 2, ()),
            ("Balanced", 3, ()),
            ("BalancedTemp", 4, ()),
        ];
        parse_enum(text, &names)
            .map(|()| PerformanceProfiles::Read)
            .or_else(|refused| {
                let trimmed = text.trim_matches(|c| matches!(c, '\u{9}'..='\u{d}' | ' '));
                let digits = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
                if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(refused);
                }
                Ok(trimmed
                    .parse::<i32>()
                    .map_or(PerformanceProfiles::TooLarge, |_| PerformanceProfiles::Read))
            })
    }
}

/// Log that the setting `setting` has no effect, and why.
fn ignored(setting: &str, why: &str) {
    log::warn!("The {ENGINE_NAME} setting '{setting}' has no effect here and is ignored, {why}.");
}

/// Give the GitHub TDL source resolver the User-Agent `text`, refusing
/// blank text as .NET does.
fn user_agent(
    builder: RobotsTxtEngineBuilder,
    text: String,
    setting: &str,
) -> Result<RobotsTxtEngineBuilder> {
    if text.trim().is_empty() {
        return Err(Error::configuration(NO_USER_AGENT));
    }
    #[cfg(feature = "reqwest-client")]
    {
        let _ = setting;
        Ok(builder.set_user_agent(text))
    }
    #[cfg(not(feature = "reqwest-client"))]
    {
        ignored(
            setting,
            "as this crate is compiled without the `reqwest-client` feature, so it has no \
             GitHub TDL source resolver",
        );
        Ok(builder)
    }
}

/// The builder, with its settings in the order .NET's
/// `RobotsTxtEngineBuilder` and its base classes declare them.
fn robots_txt_engine_builder() -> BuilderDefinition<RobotsTxtEngineBuilder> {
    BuilderDefinition::new(
        "RobotsTxtEngineBuilder",
        |builder: RobotsTxtEngineBuilder, context: &mut BuildContext| {
            let engine = builder.build()?;
            context.add_resource(Arc::clone(&engine) as _);
            Ok(engine as Arc<dyn FlowElement>)
        },
    )
    .try_setting(
        "SetPerformanceProfile",
        |_: RobotsTxtEngineBuilder, profile: PerformanceProfiles| {
            Err(Error::configuration(match profile {
                PerformanceProfiles::Read => NOT_IMPLEMENTED,
                PerformanceProfiles::TooLarge => TOO_LARGE,
            }))
        },
    )
    .code_only_setting("SetTdlSourceResolver", "ITdlSourceResolver")
    .try_setting(
        "UseDefaultTdlSourceResolver",
        |builder: RobotsTxtEngineBuilder, text: String| {
            user_agent(builder, text, "UseDefaultTdlSourceResolver")
        },
    )
    .try_setting(
        "SetUserAgent",
        |builder: RobotsTxtEngineBuilder, text: String| user_agent(builder, text, "SetUserAgent"),
    )
    .setting("SetCacheSize", |builder: RobotsTxtEngineBuilder, _: i32| {
        log::warn!("{NO_CACHE}");
        builder
    })
    .setting(
        "SetCacheHitOrMiss",
        |builder: RobotsTxtEngineBuilder, _: bool| {
            log::warn!("{NO_CACHE}");
            builder
        },
    )
    .code_only_setting("SetCache", "CacheConfiguration")
    .code_only_setting("SetLazyLoading", "LazyLoadingConfiguration")
    .setting(
        "SetLazyLoadingTimeout",
        |builder: RobotsTxtEngineBuilder, _: i32| {
            ignored("SetLazyLoadingTimeout", "as the engine has no lazy loading");
            builder
        },
    )
    .setting(
        "SetProperties",
        |builder: RobotsTxtEngineBuilder, properties: Vec<String>| builder.properties(properties),
    )
    .setting(
        "SetProperty",
        |builder: RobotsTxtEngineBuilder, property: String| builder.property(property),
    )
    .build_method(Vec::new(), |_, _| Ok(RobotsTxtEngineBuilder::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> Option<bool> {
        match PerformanceProfiles::parse_text(text) {
            Ok(PerformanceProfiles::Read) => Some(true),
            Ok(PerformanceProfiles::TooLarge) => Some(false),
            Err(_) => None,
        }
    }

    /// .NET's `Enum.Parse`, ignoring case, reads the five names and any
    /// whole number, and refuses other text.
    #[test]
    fn a_performance_profile_is_read_before_it_is_refused() {
        for text in [
            "LowMemory",
            "maxperformance",
            " Balanced ",
            "4",
            "+2",
            "5",
            "-1",
        ] {
            assert_eq!(read(text), Some(true), "{text}");
        }
        assert_eq!(read("99999999999"), Some(false));
        for text in ["", "Bogus", "InMemory", "1.5"] {
            assert_eq!(read(text), None, "{text}");
        }
    }
}
