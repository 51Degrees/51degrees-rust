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

//! The on-premise IP Intelligence engine, found by the name .NET gives its
//! builder, so that a configuration file written for .NET makes the same
//! engine here. See [`fiftyone_pipeline_core::configuration`].
//!
//! [`add_builders`] adds `IpiOnPremiseEngineBuilder`, which a configuration
//! can also name `IpiOnPremiseEngine`. Its build parameters are .NET's
//! `DataFile`, a path the current folder resolves when it is relative, and
//! `CreateTempDataCopy`, which reads a copy of the data file made in the
//! temporary folder. It takes .NET's settings:
//!
//! - `SetPerformanceProfile`, read as .NET reads it, by the exact name or
//!   number of `LowMemory`, `MaxPerformance`, `HighPerformance`, `Balanced`
//!   or `BalancedTemp`. `MaxPerformance` is
//!   [`PerformanceProfile::InMemory`] here, and `BalancedTemp` is
//!   [`PerformanceProfile::Balanced`], as .NET's `BalancedTemp` loads the
//!   data file as `Balanced` does and makes no copy.
//! - `SetConcurrency`, which holds whatever profile is set with it,
//!   `SetProperties` and `SetProperty`.
//! - `SetCacheSize`, refused with .NET's message, as the engine keeps no
//!   results.
//! - `SetReuseTempFile`, `SetDataDownloadType`, `SetLazyLoadingTimeout` and
//!   `SetCacheHitOrMiss` set to true are read and have no effect, with a
//!   warning logged. The engine makes a new copy of the data file each time
//!   it needs one, the download type is the IP Intelligence one, as in .NET,
//!   and the engine has no lazy loading and no cache.
//! - The data file settings of [`fiftyone_pipeline_engines::configuration`].
//!   Automatic updates and the file system watcher are on unless the
//!   configuration turns them off, as in .NET, and any update needs a
//!   [`DataUpdateService`] given to the
//!   [`PipelineBuilder`](fiftyone_pipeline_core::PipelineBuilder) as a
//!   resource, and a license key for automatic updates or update on startup
//!   unless `SetLicenseKeyRequired` is false, for an update source that
//!   serves the file without one.
//! - `SetLazyLoading` and `SetCache` take their values in code only, so
//!   naming one fails as it does in .NET.
//!
//! The pipeline holds the engine made, so it can be read back.
//!
//! ```no_run
//! use fiftyone_ip_intelligence_onpremise::{configuration, IpIntelligenceOnPremiseEngine};
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
//!                     "BuilderName": "IpiOnPremiseEngine",
//!                     "BuildParameters": {
//!                         "DataFile": "51Degrees-IPIV4AsnIpiV41.ipi",
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
//!     .resource::<IpIntelligenceOnPremiseEngine>()
//!     .expect("the IP Intelligence engine");
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

use crate::IpIntelligenceOnPremiseEngineBuilder;

/// The name of the engine in the messages .NET gives.
const ENGINE_NAME: &str = "IpiOnPremiseEngine";

/// Add the IP Intelligence engine's builder to `registry`, as
/// `IpiOnPremiseEngineBuilder`. Returns `registry` for chaining.
pub fn add_builders(registry: &mut BuilderRegistry) -> &mut BuilderRegistry {
    registry.add(ipi_on_premise_engine_builder())
}

/// The settings read from a configuration, held as .NET's
/// `IpiOnPremiseEngineBuilder` holds them until the engine is made.
struct IpiOnPremiseEngineSettings {
    /// The data file and whether to read a copy of it, or `None` when the
    /// configuration gave neither.
    data_file: Option<(String, bool)>,
    profile: Option<PerformanceProfile>,
    concurrency: Option<u16>,
    properties: Vec<String>,
    data_file_settings: DataFileSettings,
}

impl IpiOnPremiseEngineSettings {
    fn new(data_file: Option<(String, bool)>) -> Self {
        IpiOnPremiseEngineSettings {
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
        let service = context.latest_resource::<DataUpdateService>();
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
        let mut builder = IpIntelligenceOnPremiseEngineBuilder::new(&path)
            .properties(self.properties)
            .data_file_settings(settings.configuration(&path));
        if let Some(service) = service.filter(|_| settings.updates_requested()) {
            builder = builder.data_update_service(service);
        }
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
        let engine = builder.build_shared()?;
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
/// `IpiOnPremiseEngineBuilder` and its base classes declare them.
fn ipi_on_premise_engine_builder() -> BuilderDefinition<IpiOnPremiseEngineSettings> {
    let definition = BuilderDefinition::new(
        "IpiOnPremiseEngineBuilder",
        |settings: IpiOnPremiseEngineSettings, context: &mut BuildContext| settings.engine(context),
    )
    .setting(
        "SetReuseTempFile",
        |settings: IpiOnPremiseEngineSettings, reuse: bool| {
            if reuse {
                ignored(
                    "SetReuseTempFile",
                    "as the engine makes a new copy of the data file each time it needs one",
                );
            }
            settings
        },
    )
    .try_setting(
        "SetPerformanceProfile",
        |mut settings: IpiOnPremiseEngineSettings, name: String| {
            settings.profile = Some(performance_profile(&name)?);
            Ok(settings)
        },
    )
    .code_only_setting("SetPerformanceProfile", "PerformanceProfiles")
    .setting(
        "SetConcurrency",
        |mut settings: IpiOnPremiseEngineSettings, concurrency: u16| {
            settings.concurrency = Some(concurrency);
            settings
        },
    )
    .code_only_setting("SetCache", "CacheConfiguration")
    .try_setting("SetCacheSize", |_: IpiOnPremiseEngineSettings, _: i32| {
        Err(Error::configuration(
            "A results cache cannot be configured in the IP Intelligence engine. Its results \
             refer to native memory, which is not safe to share between requests.",
        ))
    })
    .setting(
        "SetDataDownloadType",
        |settings: IpiOnPremiseEngineSettings, _: String| {
            ignored(
                "SetDataDownloadType",
                "as the download type is the IP Intelligence one",
            );
            settings
        },
    );
    add_data_file_settings(definition, |settings: &mut IpiOnPremiseEngineSettings| {
        &mut settings.data_file_settings
    })
    .code_only_setting("SetLazyLoading", "LazyLoadingConfiguration")
    .setting(
        "SetLazyLoadingTimeout",
        |settings: IpiOnPremiseEngineSettings, _: i32| {
            ignored("SetLazyLoadingTimeout", "as the engine has no lazy loading");
            settings
        },
    )
    .setting(
        "SetCacheHitOrMiss",
        |settings: IpiOnPremiseEngineSettings, record: bool| {
            if record {
                ignored("SetCacheHitOrMiss", "as the engine has no cache");
            }
            settings
        },
    )
    .setting(
        "SetProperties",
        |mut settings: IpiOnPremiseEngineSettings, properties: Vec<String>| {
            settings.properties.extend(properties);
            settings
        },
    )
    .setting(
        "SetProperty",
        |mut settings: IpiOnPremiseEngineSettings, property: String| {
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
            Ok(IpiOnPremiseEngineSettings::new(Some((
                args.get::<String>("datafile").clone(),
                *args.get::<bool>("createTempDataCopy"),
            ))))
        },
    )
    .build_method(vec![Parameter::code_only("data", "Stream")], |_, _| {
        unreachable!("no configuration can give a stream")
    })
    .build_method(Vec::new(), |_, _| Ok(IpiOnPremiseEngineSettings::new(None)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// .NET 4.5.155's numbers map to these profiles, and a number that is
    /// none of them is refused naming this engine.
    #[test]
    fn a_performance_profile_is_read_as_dotnet_reads_it() {
        for (text, profile) in [
            ("LowMemory", PerformanceProfile::LowMemory),
            ("MaxPerformance", PerformanceProfile::InMemory),
            ("HighPerformance", PerformanceProfile::HighPerformance),
            ("Balanced", PerformanceProfile::Balanced),
            ("BalancedTemp", PerformanceProfile::Balanced),
            ("0", PerformanceProfile::LowMemory),
            ("4", PerformanceProfile::Balanced),
        ] {
            assert_eq!(performance_profile(text).unwrap(), profile, "{text}");
        }
        assert_eq!(
            performance_profile("5").unwrap_err().message(),
            "The performance profile '5' is not valid for a IpiOnPremiseEngine. (Parameter \
             'profile')"
        );
    }
}
