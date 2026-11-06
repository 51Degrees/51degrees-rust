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

//! The on-premise Device Detection engine, found by the name .NET gives its
//! Hash engine's builder, so that a configuration file written for .NET
//! makes the same engine here. See [`fiftyone_pipeline_core::configuration`].
//!
//! [`add_builders`] adds `DeviceDetectionHashEngineBuilder`, which a
//! configuration can also name `DeviceDetectionHashEngine`. Its build
//! parameters are .NET's `DataFile`, a path the current folder resolves when
//! it is relative, and `CreateTempDataCopy`, which reads a copy of the data
//! file made in the temporary folder. It takes .NET's settings:
//!
//! - `SetPerformanceProfile`, read as .NET reads it, by the exact name or
//!   number of `LowMemory`, `MaxPerformance`, `HighPerformance`, `Balanced`
//!   or `BalancedTemp`. `MaxPerformance` is
//!   [`PerformanceProfile::InMemory`] here, and `BalancedTemp` is
//!   [`PerformanceProfile::Balanced`], as .NET's `BalancedTemp` loads the
//!   data file as `Balanced` does and makes no copy.
//! - `SetConcurrency`, `SetProperties` and `SetProperty`.
//! - `SetCacheSize`, refused with .NET's message, as the engine keeps no
//!   results.
//! - `SetDataDownloadType`, `SetLazyLoadingTimeout` and `SetCacheHitOrMiss`
//!   set to true are read and have no effect, with a warning logged. The
//!   download type is the one the data file names, as in .NET, and the
//!   engine has no lazy loading and no cache.
//! - The data file settings of [`fiftyone_pipeline_engines::configuration`].
//!   Automatic updates and the file system watcher are on unless the
//!   configuration turns them off, as in .NET, and any update needs a
//!   [`DataUpdateService`] given to the
//!   [`PipelineBuilder`](fiftyone_pipeline_core::PipelineBuilder) as a
//!   resource, and a license key for automatic updates or update on startup.
//! - `SetLazyLoading` and `SetCache` take their values in code only, so
//!   naming one fails as it does in .NET.
//!
//! The Hash matching settings, such as `SetDifference` and
//! `SetUsePredictiveGraph`, are not taken, because the engine's builder has
//! none, so naming one fails as an unknown setting does.
//!
//! The pipeline holds the engine made, so it can be read back.
//!
//! ```no_run
//! use fiftyone_device_detection_onpremise::{configuration, DeviceDetectionOnPremiseEngine};
//! use fiftyone_native::PerformanceProfile;
//! use fiftyone_pipeline_core::configuration::{BuilderRegistry, PipelineOptions};
//! use fiftyone_pipeline_core::PipelineBuilder;
//!
//! let mut registry = BuilderRegistry::new();
//! configuration::add_builders(&mut registry);
//! let options = PipelineOptions::from_json(
//!     r#"{
//!         "PipelineOptions": {
//!             "Elements": [
//!                 {
//!                     "BuilderName": "DeviceDetectionHashEngine",
//!                     "BuildParameters": {
//!                         "DataFile": "51Degrees-LiteV4.1.hash",
//!                         "CreateTempDataCopy": "false",
//!                         "AutoUpdate": "false",
//!                         "DataFileSystemWatcher": "false",
//!                         "PerformanceProfile": "MaxPerformance"
//!                     }
//!                 }
//!             ]
//!         }
//!     }"#,
//! )?;
//! let pipeline = PipelineBuilder::new().build_from_configuration(&registry, &options)?;
//! let engine = pipeline
//!     .resource::<DeviceDetectionOnPremiseEngine>()
//!     .expect("the Device Detection engine");
//! assert_eq!(engine.performance_profile(), PerformanceProfile::InMemory);
//! # Ok::<(), fiftyone_pipeline_core::Error>(())
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::configuration::{
    BuildContext, BuilderDefinition, BuilderRegistry, Parameter,
};
use fiftyone_pipeline_core::{Error, FlowElement, Result};
use fiftyone_pipeline_engines::configuration::{
    add_data_file_settings, performance_profile_number, DataFileSettings,
};
use fiftyone_pipeline_engines::DataUpdateService;

use crate::DeviceDetectionOnPremiseEngineBuilder;

/// The name of the engine in the messages .NET gives.
const ENGINE_NAME: &str = "DeviceDetectionHashEngine";

/// Add the Hash engine's builder to `registry`, as
/// `DeviceDetectionHashEngineBuilder`. Returns `registry` for chaining.
pub fn add_builders(registry: &mut BuilderRegistry) -> &mut BuilderRegistry {
    registry.add(device_detection_hash_engine_builder())
}

/// The settings read from a configuration, held as .NET's
/// `DeviceDetectionHashEngineBuilder` holds them until the engine is made.
struct DeviceDetectionHashEngineSettings {
    /// The data file and whether to read a copy of it, or `None` when the
    /// configuration gave neither.
    data_file: Option<(String, bool)>,
    profile: Option<PerformanceProfile>,
    concurrency: Option<u16>,
    properties: Vec<String>,
    data_file_settings: DataFileSettings,
}

impl DeviceDetectionHashEngineSettings {
    fn new(data_file: Option<(String, bool)>) -> Self {
        DeviceDetectionHashEngineSettings {
            data_file,
            profile: None,
            concurrency: None,
            properties: Vec::new(),
            data_file_settings: DataFileSettings::new(),
        }
    }

    /// Make the engine, refusing first what .NET refuses before it loads
    /// the data file.
    fn engine(self, context: &mut BuildContext) -> Result<Arc<dyn FlowElement>> {
        let Some((data_file, create_temp_data_copy)) = self.data_file else {
            return Err(Error::configuration(
                "This builder requires one and only one data file to be configured but 0 data \
                 file configurations that have been supplied.",
            ));
        };
        let service = context.resource::<DataUpdateService>();
        let settings = &self.data_file_settings;
        settings.check(service.is_some())?;

        // .NET reads a relative path from the current folder.
        let path = PathBuf::from(&data_file);
        let path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .map_err(|e| Error::configuration(e.to_string()))?
                .join(path)
        };
        let mut builder = DeviceDetectionOnPremiseEngineBuilder::new(&path)
            .properties(self.properties)
            .auto_update(
                settings.configuration(&path),
                service.filter(|_| settings.updates_requested()),
            );
        if let Some(profile) = self.profile {
            builder = builder.performance_profile(profile);
        }
        if let Some(concurrency) = self.concurrency {
            builder = builder.concurrency(concurrency);
        }
        // .NET makes no copy when the temporary folder is empty text.
        match settings.temp_dir_path() {
            Some("") => {}
            Some(folder) => {
                builder = builder
                    .temp_dir_path(folder)
                    .create_temp_data_copy(create_temp_data_copy);
            }
            None => builder = builder.create_temp_data_copy(create_temp_data_copy),
        }
        let engine = builder.build()?;
        context.add_resource(Arc::clone(&engine) as _);
        Ok(engine as Arc<dyn FlowElement>)
    }
}

/// The profile `SetPerformanceProfile` names, refused with .NET's messages.
fn performance_profile(text: &str) -> Result<PerformanceProfile> {
    Ok(match performance_profile_number(text, ENGINE_NAME)? {
        0 => PerformanceProfile::LowMemory,
        1 => PerformanceProfile::InMemory,
        2 => PerformanceProfile::HighPerformance,
        _ => PerformanceProfile::Balanced,
    })
}

/// Log that the setting `setting` has no effect, and why.
fn ignored(setting: &str, why: &str) {
    log::warn!("The {ENGINE_NAME} setting '{setting}' has no effect here and is ignored, {why}.");
}

/// The builder, with its settings in the order .NET's
/// `DeviceDetectionHashEngineBuilder` and its base classes declare them.
fn device_detection_hash_engine_builder() -> BuilderDefinition<DeviceDetectionHashEngineSettings> {
    let definition = BuilderDefinition::new(
        "DeviceDetectionHashEngineBuilder",
        |settings: DeviceDetectionHashEngineSettings, context: &mut BuildContext| {
            settings.engine(context)
        },
    )
    .try_setting(
        "SetPerformanceProfile",
        |mut settings: DeviceDetectionHashEngineSettings, name: String| {
            settings.profile = Some(performance_profile(&name)?);
            Ok(settings)
        },
    )
    .code_only_setting("SetPerformanceProfile", "PerformanceProfiles")
    .setting(
        "SetConcurrency",
        |mut settings: DeviceDetectionHashEngineSettings, concurrency: u16| {
            settings.concurrency = Some(concurrency);
            settings
        },
    )
    .code_only_setting("SetCache", "CacheConfiguration")
    .try_setting(
        "SetCacheSize",
        |_: DeviceDetectionHashEngineSettings, _: i32| {
            Err(Error::configuration(
                "A results cache cannot be configured in the Device-Detection Hash engine. The \
                 overhead of having to manage native object lifetimes when a cache is enabled \
                 outweighs the benefit of the cache.",
            ))
        },
    )
    .setting(
        "SetDataDownloadType",
        |settings: DeviceDetectionHashEngineSettings, _: String| {
            ignored("SetDataDownloadType", "as the data file names its own type");
            settings
        },
    );
    add_data_file_settings(
        definition,
        |settings: &mut DeviceDetectionHashEngineSettings| &mut settings.data_file_settings,
    )
    .code_only_setting("SetLazyLoading", "LazyLoadingConfiguration")
    .setting(
        "SetLazyLoadingTimeout",
        |settings: DeviceDetectionHashEngineSettings, _: i32| {
            ignored("SetLazyLoadingTimeout", "as the engine has no lazy loading");
            settings
        },
    )
    .setting(
        "SetCacheHitOrMiss",
        |settings: DeviceDetectionHashEngineSettings, record: bool| {
            if record {
                ignored("SetCacheHitOrMiss", "as the engine has no cache");
            }
            settings
        },
    )
    .setting(
        "SetProperties",
        |mut settings: DeviceDetectionHashEngineSettings, properties: Vec<String>| {
            settings.properties.extend(properties);
            settings
        },
    )
    .setting(
        "SetProperty",
        |mut settings: DeviceDetectionHashEngineSettings, property: String| {
            settings.properties.push(property);
            settings
        },
    )
    .build_method(
        vec![
            Parameter::new::<String>("datafile"),
            Parameter::new::<bool>("createTempDataCopy"),
        ],
        |args, _| {
            Ok(DeviceDetectionHashEngineSettings::new(Some((
                args.get::<String>("datafile").clone(),
                *args.get::<bool>("createTempDataCopy"),
            ))))
        },
    )
    .build_method(vec![Parameter::code_only("data", "Stream")], |_, _| {
        unreachable!("no configuration can give a stream")
    })
    .build_method(Vec::new(), |_, _| {
        Ok(DeviceDetectionHashEngineSettings::new(None))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The profile each text gave in .NET 4.5.180, or the message it
    /// refused the text with.
    #[test]
    fn a_performance_profile_is_read_as_dotnet_reads_it() {
        for (text, profile) in [
            ("LowMemory", PerformanceProfile::LowMemory),
            ("MaxPerformance", PerformanceProfile::InMemory),
            ("HighPerformance", PerformanceProfile::HighPerformance),
            ("Balanced", PerformanceProfile::Balanced),
            ("BalancedTemp", PerformanceProfile::Balanced),
            (" HighPerformance ", PerformanceProfile::HighPerformance),
            ("0", PerformanceProfile::LowMemory),
            ("4", PerformanceProfile::Balanced),
            ("+2", PerformanceProfile::HighPerformance),
            ("LowMemory,Balanced", PerformanceProfile::Balanced),
            ("LowMemory, Balanced", PerformanceProfile::Balanced),
        ] {
            assert_eq!(performance_profile(text).unwrap(), profile, "{text}");
        }
        for text in [
            "maxperformance",
            "Bogus",
            "",
            "Default",
            "InMemory",
            "1,Balanced",
            "LowMemory,",
            "99999999999",
        ] {
            assert_eq!(
                performance_profile(text).unwrap_err().message(),
                format!(
                    "'{text}' is not a valid performance profile. Available profiles are \
                     'LowMemory', 'MaxPerformance', 'HighPerformance', 'Balanced', \
                     'BalancedTemp'."
                ),
                "{text}"
            );
        }
        for (text, number) in [("5", 5), ("-1", -1), ("Balanced,BalancedTemp", 7)] {
            assert_eq!(
                performance_profile(text).unwrap_err().message(),
                format!(
                    "The performance profile '{number}' is not valid for a \
                     DeviceDetectionHashEngine. (Parameter 'profile')"
                ),
                "{text}"
            );
        }
    }
}
