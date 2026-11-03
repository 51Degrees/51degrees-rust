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

//! The builder for the on-premise Hash device-detection engine.
//!
//! [`DeviceDetectionOnPremiseEngineBuilder`] collects the data file path, the
//! [`PerformanceProfile`], an optional restricted property set and optional
//! automatic-update settings, then opens the data file and assembles a
//! [`DeviceDetectionOnPremiseEngine`].

use std::path::PathBuf;
use std::sync::Arc;

use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{Error, Result};
use fiftyone_pipeline_engines::{
    DataFileConfiguration, DataUpdateService, FiftyOneUrlFormatter, OnPremiseAspectEngine,
    DISTRIBUTOR_URL,
};

use crate::engine::{open_manager, DeviceDetectionOnPremiseEngine};
use crate::operational_copy::OperationalCopy;

/// The download type the Distributor serves Hash data files as, sent in the
/// update URL's `Type` parameter.
pub const DATA_DOWNLOAD_TYPE: &str = "HashV41";

/// A fluent builder for [`DeviceDetectionOnPremiseEngine`].
///
/// At a minimum supply the data file path through [`Self::new`]. The remaining
/// settings have working defaults: the [`PerformanceProfile::Default`] profile,
/// all available properties, and no automatic updates.
///
/// # Example
///
/// ```no_run
/// use fiftyone_device_detection_onpremise::DeviceDetectionOnPremiseEngineBuilder;
/// use fiftyone_native::PerformanceProfile;
///
/// # fn main() -> fiftyone_pipeline_core::Result<()> {
/// let engine = DeviceDetectionOnPremiseEngineBuilder::new("51Degrees-LiteV4.1.hash")
///     .performance_profile(PerformanceProfile::HighPerformance)
///     .property("IsMobile")
///     .property("PlatformName")
///     .build()?;
/// # let _ = engine;
/// # Ok(())
/// # }
/// ```
pub struct DeviceDetectionOnPremiseEngineBuilder {
    data_file_path: PathBuf,
    profile: PerformanceProfile,
    requested_properties: Vec<String>,
    data_source_tier: Option<String>,
    auto_update: Option<AutoUpdate>,
    concurrency: Option<u16>,
    temp_dir: Option<PathBuf>,
}

/// The optional automatic-update settings captured by the builder.
struct AutoUpdate {
    /// The configuration the engine's data file is registered with.
    config: DataFileConfiguration,
    /// The service the data file is registered with on build, if the caller
    /// supplied one. When absent the engine is built without registering it,
    /// and the configuration is still carried on the data file so a caller can
    /// register it later.
    service: Option<Arc<DataUpdateService>>,
}

impl DeviceDetectionOnPremiseEngineBuilder {
    /// Start building an engine that loads the Hash data file at `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        DeviceDetectionOnPremiseEngineBuilder {
            data_file_path: path.into(),
            profile: PerformanceProfile::Default,
            requested_properties: Vec::new(),
            data_source_tier: None,
            auto_update: None,
            concurrency: None,
            temp_dir: None,
        }
    }

    /// Set the performance profile used to load the data file. Defaults to
    /// [`PerformanceProfile::Default`].
    pub fn performance_profile(mut self, profile: PerformanceProfile) -> Self {
        self.profile = profile;
        self
    }

    /// Set the expected concurrency: the number of threads that will process
    /// through the built engine at once.
    ///
    /// This sizes the file-handle pool the file-backed collections use under the
    /// [`PerformanceProfile::LowMemory`] and [`PerformanceProfile::Balanced`]
    /// profiles. The pool defaults to the number of cores; if more threads than
    /// that will share the engine, set this to at least that many or threads
    /// will contend on the pool. It has no effect on the in-memory profiles,
    /// where the data set is fully resident.
    pub fn concurrency(mut self, concurrency: u16) -> Self {
        self.concurrency = Some(concurrency);
        self
    }

    /// Set the folder for the engine's temporary files. With automatic updates
    /// on, every profile but [`PerformanceProfile::InMemory`] reads a copy of
    /// the data file made there, so an update can replace the data file itself.
    /// Defaults to the system's temporary folder, [`std::env::temp_dir`].
    pub fn temp_dir_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.temp_dir = Some(path.into());
        self
    }

    /// Restrict the engine to a single named property, adding to any already
    /// requested. With no restriction the engine loads every property the data
    /// file supports.
    ///
    /// A component with no named property reads as `0` in the device id. Naming
    /// `DeviceId` resolves every component, so the device id is complete.
    pub fn property(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        if !self
            .requested_properties
            .iter()
            .any(|p| p.eq_ignore_ascii_case(&name))
        {
            self.requested_properties.push(name);
        }
        self
    }

    /// Restrict the engine to the named properties, adding to any already
    /// requested.
    pub fn properties<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for name in names {
            self = self.property(name);
        }
        self
    }

    /// Override the data-source tier the engine reports, for example `Premium`.
    ///
    /// By default the engine reads the tier embedded in the data file header
    /// (`Lite`, `Enterprise`, `TAC` and so on). Set this only to override that,
    /// for example to report a more specific tier than the file records, or when
    /// the header name is absent. When neither the override nor the header name
    /// is available the engine falls back to `Lite`, the free tier.
    pub fn data_source_tier(mut self, tier: impl Into<String>) -> Self {
        self.data_source_tier = Some(tier.into());
        self
    }

    /// Enable automatic updates with an explicit data-file configuration.
    ///
    /// The configuration's `data_file_path` is overridden with the builder's
    /// path so the engine and the update service agree on the file. A
    /// configuration with no update URL polls the 51Degrees Distributor, with
    /// the Distributor's URL formatter and the Hash download type filled in,
    /// as the other 51Degrees implementations do. When `service` is supplied
    /// the data file is registered with it on [`Self::build`]; otherwise the
    /// configuration is still attached to the engine's data file so a caller
    /// can register it later. A configuration with automatic updates or update
    /// on startup on needs a licence key, or [`Self::build`] refuses.
    pub fn auto_update(
        mut self,
        config: DataFileConfiguration,
        service: Option<Arc<DataUpdateService>>,
    ) -> Self {
        let mut config = config;
        config.data_file_path = Some(self.data_file_path.clone());
        self.auto_update = Some(AutoUpdate { config, service });
        self
    }

    /// Enable automatic updates from a remote URL, polling on the default
    /// interval, and register the data file with `service` on build.
    ///
    /// A convenience over [`Self::auto_update`] that builds a default
    /// [`DataFileConfiguration`] for the data file path and sets the update URL.
    pub fn data_update_url(self, url: impl Into<String>, service: Arc<DataUpdateService>) -> Self {
        let path = self.data_file_path.clone();
        let config = DataFileConfiguration::builder(path)
            .data_update_url(url)
            .build();
        self.auto_update(config, Some(service))
    }

    /// Open the data file and build the engine.
    ///
    /// Refuses, before opening the file, when the configuration given to
    /// [`Self::auto_update`] has automatic updates or update on startup on and
    /// no licence key, as .NET does. When automatic updates
    /// were configured with a service, the engine's data file is registered
    /// with that service before the engine is returned, so a startup poll (if
    /// requested) and the polling schedule are in effect. A startup poll that
    /// fails leaves the engine on the data file it loaded, as the
    /// specification says, and the service logs the failure.
    pub fn build(self) -> Result<Arc<DeviceDetectionOnPremiseEngine>> {
        // The data-file configuration the engine carries. When auto-update was
        // requested, use that configuration; otherwise build a static one with
        // updates disabled so the engine still has a registered file path for
        // a manual `refresh` or `check_for_update`.
        let (data_file_config, service) = match self.auto_update {
            Some(AutoUpdate { config, service }) => (with_distributor_defaults(config), service),
            None => (
                DataFileConfiguration::builder(self.data_file_path.clone())
                    .automatic_updates_enabled(false)
                    .file_system_watcher_enabled(false)
                    .build(),
                None,
            ),
        };
        data_file_config.check_license_keys()?;

        // Open the data set with the requested property restriction, from a
        // copy of the data file when updates can replace the file while it is
        // open.
        let copy = OperationalCopy::when_needed(
            &self.data_file_path,
            self.temp_dir.as_deref(),
            self.profile,
            &data_file_config,
        );
        let manager = open_manager(
            copy.as_ref()
                .map_or(self.data_file_path.as_path(), OperationalCopy::path),
            self.profile,
            &self.requested_properties,
            self.concurrency,
        )?;

        let engine = Arc::new(
            DeviceDetectionOnPremiseEngine::from_manager(
                manager,
                copy,
                self.profile,
                self.requested_properties,
                data_file_config,
                self.data_source_tier,
                self.concurrency,
            )
            .with_temp_dir(self.temp_dir),
        );

        // Register the data file with the update service if one was supplied.
        if let Some(service) = service {
            let data_file = engine
                .data_files()
                .first()
                .cloned()
                .ok_or_else(|| Error::configuration("engine has no data file to register"))?;
            let as_engine: Arc<dyn OnPremiseAspectEngine> = engine.clone();
            // A failed update on start-up leaves the engine on the data file it
            // loaded, as the specification says, and the service logs it.
            let _ = service.register(as_engine, data_file);
        }

        Ok(engine)
    }
}

/// Fill in the Distributor as the update source where the configuration names
/// none: its URL, its URL formatter and the Hash download type.
fn with_distributor_defaults(mut config: DataFileConfiguration) -> DataFileConfiguration {
    if config.data_update_url.is_none() {
        config.data_update_url = Some(DISTRIBUTOR_URL.to_owned());
    }
    if config.url_formatter.is_none() {
        config.url_formatter = Some(Arc::new(FiftyOneUrlFormatter));
    }
    if config.data_download_type.is_none() {
        config.data_download_type = Some(DATA_DOWNLOAD_TYPE.to_owned());
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;
    use fiftyone_pipeline_engines::AspectEngineDataFile;

    /// A configuration naming no update source gets the Distributor, its URL
    /// formatter and the Hash download type, and the formatted URL is the one
    /// the other implementations request. A configuration naming its own
    /// source and type keeps them.
    #[test]
    fn distributor_defaults_fill_only_what_is_missing() {
        let defaulted = with_distributor_defaults(
            DataFileConfiguration::builder("d.hash")
                .data_update_license_keys(["KEY1"])
                .build(),
        );
        assert_eq!(defaulted.data_update_url.as_deref(), Some(DISTRIBUTOR_URL));
        assert!(defaulted.url_formatter.is_some());
        assert_eq!(
            defaulted.data_download_type.as_deref(),
            Some(DATA_DOWNLOAD_TYPE)
        );
        let file = AspectEngineDataFile::new(defaulted);
        assert_eq!(
            file.formatted_update_url().as_deref(),
            Some(
                "https://distributor.51degrees.com/api/v2/download\
                 ?LicenseKeys=KEY1&Download=True&Type=HashV41"
            )
        );

        let kept = with_distributor_defaults(
            DataFileConfiguration::builder("d.hash")
                .data_update_url("https://example.invalid/d.hash")
                .data_download_type("Other")
                .build(),
        );
        assert_eq!(
            kept.data_update_url.as_deref(),
            Some("https://example.invalid/d.hash")
        );
        assert_eq!(kept.data_download_type.as_deref(), Some("Other"));
    }

    /// With automatic updates on and no licence key the build refuses before
    /// it opens the file, so the error is about the key and not the file. With
    /// a key the file is opened, and the missing file is the error.
    #[test]
    fn a_missing_licence_key_refuses_before_the_file_is_opened() {
        let error = DeviceDetectionOnPremiseEngineBuilder::new("does-not-exist.hash")
            .auto_update(
                DataFileConfiguration::builder("does-not-exist.hash").build(),
                None,
            )
            .build()
            .err()
            .expect("the build refuses");
        assert!(error.to_string().contains("licence key"), "{error}");

        let error = DeviceDetectionOnPremiseEngineBuilder::new("does-not-exist.hash")
            .auto_update(
                DataFileConfiguration::builder("does-not-exist.hash")
                    .data_update_license_keys(["KEY1"])
                    .build(),
                None,
            )
            .build()
            .err()
            .expect("the file does not exist");
        assert!(!error.to_string().contains("licence key"), "{error}");
    }
}
