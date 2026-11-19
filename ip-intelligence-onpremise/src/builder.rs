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

//! The fluent builder for [`IpIntelligenceOnPremiseEngine`].
//!
//! The builder opens the `.ipi` data file with the chosen
//! [`PerformanceProfile`] and (optional) property restriction, wraps the loaded
//! native manager in the engine, and configures the data file for automatic
//! updates when requested.

use std::path::PathBuf;
use std::sync::Arc;

use fiftyone_native::ipi::Manager;
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{Error, Result};
use fiftyone_pipeline_engines::{
    AspectEngineDataFile, DataFileConfiguration, DataUpdateService, FiftyOneUrlFormatter,
    OnPremiseAspectEngine, DISTRIBUTOR_URL,
};

use crate::engine::{record_dates, IpIntelligenceOnPremiseEngine};
use crate::operational_copy::OperationalCopy;

/// The download type the Distributor serves IP Intelligence data files as,
/// sent in the update URL's `Type` parameter.
pub const DATA_DOWNLOAD_TYPE: &str = "IPIV41";

/// A fluent builder that opens an `.ipi` data file and produces an
/// [`IpIntelligenceOnPremiseEngine`].
///
/// Create one with [`IpIntelligenceOnPremiseEngine::builder`] or
/// [`IpIntelligenceOnPremiseEngineBuilder::new`], chain the configuration
/// methods, then call [`IpIntelligenceOnPremiseEngineBuilder::build`].
///
/// # Example
///
/// ```no_run
/// use fiftyone_ip_intelligence_onpremise::IpIntelligenceOnPremiseEngine;
/// use fiftyone_native::PerformanceProfile;
///
/// # fn main() -> fiftyone_pipeline_core::Result<()> {
/// let engine = IpIntelligenceOnPremiseEngine::builder("51Degrees-IPIV4AsnIpiV41.ipi")
///     .performance_profile(PerformanceProfile::LowMemory)
///     .property("RegisteredCountry")
///     .property("Country")
///     // Off because the free ASN file has no licence key for updates.
///     .auto_update(false)
///     .build()?;
/// # let _ = engine;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct IpIntelligenceOnPremiseEngineBuilder {
    data_file_path: PathBuf,
    profile: PerformanceProfile,
    requested_properties: Vec<String>,
    /// The tier to report, or [`None`] to read it from the data file header.
    data_source_tier: Option<String>,
    auto_update_enabled: bool,
    file_system_watcher_enabled: bool,
    update_on_startup: bool,
    data_update_url: Option<String>,
    data_update_license_keys: Vec<String>,
    polling_interval_seconds: Option<u64>,
    max_randomisation_seconds: Option<u64>,
    data_update_service: Option<Arc<DataUpdateService>>,
    temp_dir: Option<PathBuf>,
    temp_data_copy: bool,
    reuse_temp_file: bool,
    /// The expected concurrency, or [`None`] to keep the profile's own.
    concurrency: Option<u16>,
    /// The whole data file configuration a pipeline configuration gave, used
    /// in place of the update options above when set.
    data_file_settings: Option<DataFileConfiguration>,
}

impl IpIntelligenceOnPremiseEngineBuilder {
    /// Start building an engine for the `.ipi` data file at `data_file_path`.
    ///
    /// The defaults match .NET and Java: the engine-default performance
    /// profile, every property populated, and automatic updates
    /// and the file-system watcher on, so a licence key is needed before the
    /// engine builds unless [`Self::auto_update`] turns the updates off.
    /// Updates take effect once the engine is registered with a
    /// [`DataUpdateService`], which [`Self::data_update_service`] and
    /// [`Self::build_shared`] do.
    pub fn new(data_file_path: impl Into<PathBuf>) -> Self {
        IpIntelligenceOnPremiseEngineBuilder {
            data_file_path: data_file_path.into(),
            profile: PerformanceProfile::Default,
            requested_properties: Vec::new(),
            data_source_tier: None,
            auto_update_enabled: true,
            file_system_watcher_enabled: true,
            update_on_startup: false,
            data_update_url: None,
            data_update_license_keys: Vec::new(),
            polling_interval_seconds: None,
            max_randomisation_seconds: None,
            data_update_service: None,
            temp_dir: None,
            temp_data_copy: false,
            reuse_temp_file: false,
            concurrency: None,
            data_file_settings: None,
        }
    }

    /// Set the performance profile the data file is opened with.
    pub fn performance_profile(mut self, profile: PerformanceProfile) -> Self {
        self.profile = profile;
        self
    }

    /// Set the expected concurrency, which is the number of threads that will
    /// look up through the engine at once.
    ///
    /// Under [`PerformanceProfile::LowMemory`] and
    /// [`PerformanceProfile::Balanced`] it sizes the pool of file handles the
    /// collections left on file read through, so it is how many calls can read
    /// the data file at once, to at most 64, and a call beyond that waits for a
    /// handle. Under [`PerformanceProfile::Balanced`] it is also the number of
    /// separately locked parts of each cache, or fewer for a cache with fewer
    /// entries than the concurrency squared, and a cache with fewer entries
    /// than the concurrency is grown to it, so any value opens. The in-memory
    /// profiles read nothing from the data file once it is loaded, but for the
    /// values of properties not named when `HighPerformance` names some, so it
    /// does not change how their lookups run. Unset, the profile's own is kept,
    /// one for each processor. A refresh opens the new data file with the same
    /// value.
    pub fn concurrency(mut self, concurrency: u16) -> Self {
        self.concurrency = Some(concurrency);
        self
    }

    /// Set the folder for the engine's temporary files. With automatic updates
    /// on, every profile but [`PerformanceProfile::InMemory`] reads a copy of
    /// the data file made there, so an update can replace the data file itself,
    /// and with [`Self::create_temp_data_copy`] every profile does. Defaults to
    /// the system's temporary folder, [`std::env::temp_dir`].
    ///
    /// The engine removes each copy it reads once it no longer reads it, and
    /// leaves any other copy of the data file in the folder, such as one a
    /// process that stopped left behind. .NET, through the C library, removes
    /// those after each load from a copy other than on Apple systems, and on
    /// Linux skips any that a process has open.
    pub fn temp_dir_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.temp_dir = Some(path.into());
        self
    }

    /// Read a copy of the data file made in the temporary folder under every
    /// profile, as .NET's `createTempDataCopy` does, rather than only where
    /// automatic updates need one. The copy is removed when the engine no
    /// longer reads it. Defaults to false.
    pub fn create_temp_data_copy(mut self, create: bool) -> Self {
        self.temp_data_copy = create;
        self
    }

    /// Read a copy of the data file already in the temporary folder, when the
    /// engine reads a copy, rather than making a new one, as .NET's
    /// `SetReuseTempFile` does. A file there is taken as the C library takes
    /// one, when its name starts with the data file's name without its
    /// extension and is not the data file's name, and it has the data file's
    /// length and header. The engine removes the copy once it no longer reads
    /// it, whether it made the copy or found it, as the C library does.
    /// Defaults to false.
    ///
    /// A file in the folder that passes those checks is read as the data, and
    /// the default folder can be one every local user writes to, as `/tmp` is
    /// on Linux when `TMPDIR` is not set. With reuse on, name with
    /// [`Self::temp_dir_path`] a folder only the service writes to.
    pub fn reuse_temp_file(mut self, reuse: bool) -> Self {
        self.reuse_temp_file = reuse;
        self
    }

    /// Use `configuration` for the data file in place of the update options
    /// set on this builder, filling in the Distributor as the update source
    /// where it names none. The data file path stays this builder's.
    pub(crate) fn data_file_settings(mut self, configuration: DataFileConfiguration) -> Self {
        self.data_file_settings = Some(configuration);
        self
    }

    /// Restrict the engine to a single named property, adding to any already
    /// requested. With no properties requested the engine populates every
    /// property the data file defines. A name is matched ignoring case, and
    /// one the data file lacks is neither populated nor listed.
    pub fn property(mut self, property: impl Into<String>) -> Self {
        self.requested_properties.push(property.into());
        self
    }

    /// Restrict the engine to the named properties, adding to any already
    /// requested.
    pub fn properties<I, S>(mut self, properties: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.requested_properties
            .extend(properties.into_iter().map(Into::into));
        self
    }

    /// Override the data-source tier the engine reports.
    ///
    /// By default the engine reads the tier from the data file header (for
    /// example `Lite`, `Enterprise` or `TAC`), falling back to
    /// [`crate::DEFAULT_DATA_SOURCE_TIER`] only when the header cannot be read. This
    /// override forces a specific tier, which an application can use when it
    /// knows better than the file metadata.
    pub fn data_source_tier(mut self, tier: impl Into<String>) -> Self {
        self.data_source_tier = Some(tier.into());
        self
    }

    /// Enable or disable automatic data-file updates. On by default.
    ///
    /// When enabled, the [`DataUpdateService`] the engine is registered with
    /// polls the update URL on the polling interval and applies a downloaded
    /// update by calling the engine's `refresh`. This is the master switch for
    /// polling, and the file-system watcher runs on its own setting whether
    /// this is on or off.
    pub fn auto_update(mut self, enabled: bool) -> Self {
        self.auto_update_enabled = enabled;
        self
    }

    /// Enable or disable the file-system watcher for the data file. On by
    /// default. When enabled, whether [`Self::auto_update`] is on or off, the
    /// update service refreshes the engine when the file changes on disk, and
    /// needs no licence key to do so.
    pub fn file_system_watcher(mut self, enabled: bool) -> Self {
        self.file_system_watcher_enabled = enabled;
        self
    }

    /// Set whether the update service polls once, before the engine is
    /// returned, when the engine is registered. Off by default.
    pub fn update_on_startup(mut self, enabled: bool) -> Self {
        self.update_on_startup = enabled;
        self
    }

    /// Set the remote update URL for the data file. The 51Degrees Distributor
    /// by default.
    pub fn data_update_url(mut self, url: impl Into<String>) -> Self {
        self.data_update_url = Some(url.into());
        self
    }

    /// Set the licence keys the update URL carries. The Distributor serves
    /// nothing without one, so the engine refuses to build with automatic
    /// updates or update on startup on and none set, as .NET does.
    pub fn data_update_license_keys<I, S>(mut self, keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.data_update_license_keys = keys.into_iter().map(Into::into).collect();
        self
    }

    /// Set the interval between polls of the update URL, in seconds. Thirty
    /// minutes by default.
    pub fn polling_interval_seconds(mut self, seconds: u64) -> Self {
        self.polling_interval_seconds = Some(seconds);
        self
    }

    /// Set the most that is added at random to each polling interval, in
    /// seconds, so many instances do not poll together. Ten minutes by default.
    pub fn max_randomisation_seconds(mut self, seconds: u64) -> Self {
        self.max_randomisation_seconds = Some(seconds);
        self
    }

    /// Set the service the engine's data file is registered with on
    /// [`Self::build_shared`], which starts the polling and the watcher the
    /// other options configure.
    pub fn data_update_service(mut self, service: Arc<DataUpdateService>) -> Self {
        self.data_update_service = Some(service);
        self
    }

    /// Open the data file and build the engine.
    ///
    /// Opens the native IP-intelligence manager with the configured performance
    /// profile and property restriction, then assembles the engine and its
    /// data-file run-time state. Returns the native error if the data file
    /// fails to load. Refuses, before opening the file, when automatic updates
    /// or update on startup are on and no licence key is set. Registering with
    /// a data update service needs the shared engine [`Self::build_shared`]
    /// returns, so this also refuses to build when
    /// [`Self::data_update_service`] was set.
    pub fn build(self) -> Result<IpIntelligenceOnPremiseEngine> {
        if self.data_update_service.is_some() {
            return Err(Error::configuration(
                "a data update service was set, so build the engine with build_shared, \
                 which registers it",
            ));
        }
        self.build_engine()
    }

    /// Open the data file, build the engine, and register its data file with
    /// the service set by [`Self::data_update_service`], if any.
    ///
    /// Registration starts the file-system watcher and schedules the polls the
    /// options configure, and polls once first when update on startup is on.
    /// A startup poll that fails leaves the engine on the data file it loaded,
    /// as the specification says, and the service logs the failure. Refuses,
    /// before opening the file, when automatic updates or update on
    /// startup are on and no licence key is set.
    pub fn build_shared(mut self) -> Result<Arc<IpIntelligenceOnPremiseEngine>> {
        let service = self.data_update_service.take();
        let engine = Arc::new(self.build_engine()?);
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

    fn build_engine(self) -> Result<IpIntelligenceOnPremiseEngine> {
        let configuration = self.data_file_configuration();
        configuration.check_license_keys()?;

        let requested = self.requested_properties;

        // Open from a copy of the data file when updates can replace the file
        // while it is open.
        let copy = OperationalCopy::when_needed(
            &self.data_file_path,
            self.temp_dir.as_deref(),
            self.profile,
            &configuration,
            self.temp_data_copy,
            self.reuse_temp_file,
        );
        let manager = {
            let property_refs: Option<Vec<&str>> = if requested.is_empty() {
                None
            } else {
                Some(requested.iter().map(String::as_str).collect())
            };
            Manager::open_with_options(
                copy.as_ref()
                    .map_or(self.data_file_path.as_path(), OperationalCopy::path),
                self.profile,
                property_refs.as_deref(),
                self.concurrency,
            )?
        };

        let data_file = Arc::new(AspectEngineDataFile::new(configuration));
        record_dates(&manager, &data_file);

        Ok(IpIntelligenceOnPremiseEngine::from_parts(
            copy,
            manager,
            self.profile,
            self.data_source_tier,
            data_file,
        )?
        .with_temp_dir(self.temp_dir, self.temp_data_copy, self.reuse_temp_file)
        .with_concurrency(self.concurrency))
    }

    /// Build the data-file configuration that captures the update options. The
    /// update source is the Distributor unless a URL was given, with the
    /// Distributor's URL formatter and the IP Intelligence download type, as the
    /// other 51Degrees implementations have it. A configuration given to
    /// [`Self::data_file_settings`] replaces the update options, with the same
    /// Distributor defaults where it names none.
    fn data_file_configuration(&self) -> DataFileConfiguration {
        if let Some(settings) = &self.data_file_settings {
            let mut config = settings.clone();
            config.data_file_path = Some(self.data_file_path.clone());
            if config.data_update_url.is_none() {
                config.data_update_url = Some(DISTRIBUTOR_URL.to_owned());
            }
            if config.url_formatter.is_none() {
                config.url_formatter = Some(Arc::new(FiftyOneUrlFormatter));
            }
            if config.data_download_type.is_none() {
                config.data_download_type = Some(DATA_DOWNLOAD_TYPE.to_owned());
            }
            return config;
        }
        let mut config = DataFileConfiguration::builder(self.data_file_path.clone())
            .automatic_updates_enabled(self.auto_update_enabled)
            .file_system_watcher_enabled(self.file_system_watcher_enabled)
            .update_on_startup(self.update_on_startup)
            .data_update_url(
                self.data_update_url
                    .clone()
                    .unwrap_or_else(|| DISTRIBUTOR_URL.to_owned()),
            )
            .data_update_license_keys(self.data_update_license_keys.clone())
            .url_formatter(Some(Arc::new(FiftyOneUrlFormatter)))
            .data_download_type(DATA_DOWNLOAD_TYPE);
        if let Some(seconds) = self.polling_interval_seconds {
            config = config.polling_interval_seconds(seconds);
        }
        if let Some(seconds) = self.max_randomisation_seconds {
            config = config.max_randomisation_seconds(seconds);
        }
        config.build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The defaults are those of the .NET and Java engines: updates and the
    /// watcher on, no poll at startup, the Distributor as the source with its
    /// URL formatter and the IP Intelligence download type.
    #[test]
    fn defaults_match_the_other_implementations() {
        let builder = IpIntelligenceOnPremiseEngineBuilder::new("does-not-exist.ipi");
        let config = builder.data_file_configuration();
        assert!(config.automatic_updates_enabled);
        assert!(config.file_system_watcher_enabled);
        assert!(!config.update_on_startup);
        assert_eq!(config.data_update_url.as_deref(), Some(DISTRIBUTOR_URL));
        assert!(config.url_formatter.is_some());
        assert_eq!(
            config.data_download_type.as_deref(),
            Some(DATA_DOWNLOAD_TYPE)
        );
        assert!(config.data_update_license_keys.is_empty());
        assert!(config.license_key_required_for_updates);
        assert_eq!(
            config.data_file_path.as_deref(),
            Some(std::path::Path::new("does-not-exist.ipi"))
        );
        let file = AspectEngineDataFile::new(config);
        assert_eq!(
            file.formatted_update_url().as_deref(),
            Some(
                "https://distributor.51degrees.com/api/v2/download\
                 ?LicenseKeys=&Download=True&Type=IPIV41"
            )
        );
    }

    #[test]
    fn update_options_are_captured() {
        let builder = IpIntelligenceOnPremiseEngineBuilder::new("d.ipi")
            .auto_update(false)
            .file_system_watcher(false)
            .update_on_startup(true)
            .data_update_url("https://example.invalid/d")
            .data_update_license_keys(["KEY1", "KEY2"])
            .polling_interval_seconds(60)
            .max_randomisation_seconds(5);
        let config = builder.data_file_configuration();
        assert!(!config.automatic_updates_enabled);
        assert!(!config.file_system_watcher_enabled);
        assert!(config.update_on_startup);
        assert_eq!(
            config.data_update_url.as_deref(),
            Some("https://example.invalid/d")
        );
        assert_eq!(config.data_update_license_keys, ["KEY1", "KEY2"]);
        assert_eq!(config.polling_interval_seconds, 60);
        assert_eq!(config.max_randomisation_seconds, 5);
        let file = AspectEngineDataFile::new(config);
        assert_eq!(
            file.formatted_update_url().as_deref(),
            Some("https://example.invalid/d?LicenseKeys=KEY1|KEY2&Download=True&Type=IPIV41")
        );
    }

    /// A service set on the builder needs the shared engine, so the plain
    /// build refuses before it opens the file.
    #[test]
    fn build_refuses_a_service_it_cannot_register() {
        let error = IpIntelligenceOnPremiseEngineBuilder::new("does-not-exist.ipi")
            .data_update_service(Arc::new(DataUpdateService::new()))
            .build()
            .err()
            .expect("build refuses");
        assert!(error.to_string().contains("build_shared"), "{error}");
    }

    /// With the default automatic updates and no licence key the build refuses
    /// before it opens the file, so the error is about the key and not the
    /// file. With the updates off the file is opened, and the missing file is
    /// the error.
    #[test]
    fn a_missing_licence_key_refuses_before_the_file_is_opened() {
        let error = IpIntelligenceOnPremiseEngineBuilder::new("does-not-exist.ipi")
            .build()
            .err()
            .expect("the build refuses");
        assert!(error.to_string().contains("licence key"), "{error}");

        let error = IpIntelligenceOnPremiseEngineBuilder::new("does-not-exist.ipi")
            .auto_update(false)
            .build()
            .err()
            .expect("the file does not exist");
        assert!(!error.to_string().contains("licence key"), "{error}");
    }

    #[test]
    fn concurrency_is_unset_until_it_is_set() {
        const THREADS: u16 = 16;
        let builder = IpIntelligenceOnPremiseEngineBuilder::new("d.ipi");
        assert_eq!(builder.concurrency, None);
        assert_eq!(builder.concurrency(THREADS).concurrency, Some(THREADS));
    }

    #[test]
    fn property_restriction_accumulates() {
        let builder = IpIntelligenceOnPremiseEngineBuilder::new("d.ipi")
            .property("RegisteredCountry")
            .properties(["Country", "CountryCode"]);
        assert_eq!(
            builder.requested_properties,
            vec!["RegisteredCountry", "Country", "CountryCode"]
        );
    }
}
