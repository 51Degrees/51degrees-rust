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

//! Data-file configuration and run-time state for on-premise engines.
//!
//! [`DataFileConfiguration`] is the set of options that control how a data file
//! is updated. [`AspectEngineDataFile`] bundles that configuration with the
//! run-time state the [`crate::DataUpdateService`] tracks for one data file (its
//! identifier, the publish and expected-update timestamps, and so on).
//!
//! These types are shared (`Arc`-wrapped, fields behind locks) because the
//! update service touches them from background threads while the owning engine
//! reads them on the request path.

use std::collections::hash_map::RandomState;
use std::fmt::Debug;
use std::hash::BuildHasher;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, TimeZone, Utc};
use fiftyone_pipeline_core::redact::{mask, redact_with};
use fiftyone_pipeline_core::{Error, Result};

/// The default polling interval, 30 minutes.
pub const DEFAULT_POLLING_INTERVAL_SECONDS: u64 = 30 * 60;

/// The 51Degrees Distributor's download endpoint, the update URL the 51Degrees
/// engines use unless another is configured.
pub const DISTRIBUTOR_URL: &str = "https://distributor.51degrees.com/api/v2/download";

/// Adds query parameters to a data file's update URL before each request.
///
/// The update service asks the formatter for the URL it requests, so a
/// download service that needs the licence key or the file type in the query
/// string gets them without the configured URL carrying them. A static URL
/// that serves a file needs no formatter.
pub trait DataUpdateUrlFormatter: Debug + Send + Sync {
    /// The URL to request for `data_file`, built from its configured update URL.
    fn format_url(&self, data_file: &AspectEngineDataFile) -> String;
}

/// The formatter for the 51Degrees Distributor.
///
/// Appends `LicenseKeys`, with the configured keys separated by `|`,
/// `Download=True`, and `Type` with the data file's download type, as the .NET
/// and Java implementations do.
#[derive(Debug, Clone, Copy, Default)]
pub struct FiftyOneUrlFormatter;

impl DataUpdateUrlFormatter for FiftyOneUrlFormatter {
    fn format_url(&self, data_file: &AspectEngineDataFile) -> String {
        let config = data_file.configuration();
        let url = config.data_update_url.as_deref().unwrap_or_default();
        let mut formatted = format!(
            "{url}?LicenseKeys={}&Download=True",
            config.data_update_license_keys.join("|")
        );
        if let Some(download_type) = &config.data_download_type {
            formatted.push_str("&Type=");
            formatted.push_str(download_type);
        }
        formatted
    }
}

/// The default maximum polling randomisation, 10 minutes.
pub const DEFAULT_MAX_RANDOMISATION_SECONDS: u64 = 10 * 60;

/// The default data-file identifier.
pub const DEFAULT_IDENTIFIER: &str = "Default";

/// Options controlling automatic updates of one data file.
///
/// Build one with [`DataFileConfiguration::builder`]. The defaults are that
/// automatic updates and the file-system watcher are on, MD5 verification and
/// the `If-Modified-Since` header are on, update-on-startup is off, the
/// content is assumed gzip-compressed, and a licence key is required before
/// automatic updates or update on startup run.
///
/// The `Debug` form shows each licence key, in the list and in the update
/// URL, by its last 4 characters only.
#[derive(Clone)]
pub struct DataFileConfiguration {
    /// Identifier used to tell apart the data files of a multi-file engine.
    /// Ignored by single-file engines.
    pub identifier: String,
    /// The path to the data file on disk. `None` for a memory-only data source.
    pub data_file_path: Option<PathBuf>,
    /// The URL to poll for an updated data file. `None` to disable remote
    /// polling.
    pub data_update_url: Option<String>,
    /// Whether the service should periodically poll [`Self::data_update_url`].
    /// This is the master switch for automatic updates, so with it off there
    /// is no file-system watcher either.
    pub automatic_updates_enabled: bool,
    /// Whether a file-system watcher should refresh the engine when the file at
    /// [`Self::data_file_path`] changes on disk. The watcher runs only when
    /// [`Self::automatic_updates_enabled`] is on as well.
    pub file_system_watcher_enabled: bool,
    /// Whether the service should poll for an update once, synchronously, when
    /// the data file is first registered.
    pub update_on_startup: bool,
    /// The interval between remote polls.
    pub polling_interval_seconds: u64,
    /// The maximum random extra delay added to each polling interval, used to
    /// stagger requests from many instances. A value between zero and this is
    /// added to each interval.
    pub max_randomisation_seconds: u64,
    /// Whether the downloaded content is gzip-compressed and must be
    /// decompressed before use.
    pub decompress_content: bool,
    /// Whether the response is expected to carry a `Content-MD5` header whose
    /// hash is verified against the download.
    pub verify_md5: bool,
    /// Whether the poll request carries an `If-Modified-Since` header so the
    /// server can answer `304 Not Modified` when nothing is newer.
    pub verify_modified_since: bool,
    /// License keys the URL formatter adds to the update URL.
    pub data_update_license_keys: Vec<String>,
    /// The formatter that turns [`Self::data_update_url`] into the URL each poll
    /// requests. `None` requests the URL as configured.
    pub url_formatter: Option<Arc<dyn DataUpdateUrlFormatter>>,
    /// The download type the URL formatter sends, which names the data file
    /// format the Distributor should serve, such as `HashV41` or `IPIV41`.
    pub data_download_type: Option<String>,
    /// Whether an engine refuses to build with automatic updates or update on
    /// startup on and no licence key, as the Distributor serves nothing
    /// without one. On unless the update source needs no key.
    pub license_key_required_for_updates: bool,
}

impl DataFileConfiguration {
    /// Start building a configuration for the data file at `path`.
    pub fn builder(path: impl Into<PathBuf>) -> DataFileConfigurationBuilder {
        DataFileConfigurationBuilder::new(Some(path.into()))
    }

    /// Start building a configuration for a memory-only data source (no file on
    /// disk).
    pub fn memory_builder() -> DataFileConfigurationBuilder {
        DataFileConfigurationBuilder::new(None)
    }

    /// Checks that the licence key the update source needs is set.
    ///
    /// Fails when [`Self::license_key_required_for_updates`] is on, automatic
    /// updates or update on startup are on, and no key is set or every key is
    /// blank. The engine builders call this before they open the data file,
    /// as .NET does, so a deployment without a key fails at startup instead
    /// of being refused at every poll.
    pub fn check_license_keys(&self) -> Result<()> {
        if !self.license_key_required_for_updates {
            return Ok(());
        }
        let wanted: Vec<&str> = [
            (self.automatic_updates_enabled, "automatic updates"),
            (self.update_on_startup, "update on startup"),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, name)| name)
        .collect();
        let has_key = self
            .data_update_license_keys
            .iter()
            .any(|key| !key.trim().is_empty());
        if wanted.is_empty() || has_key {
            return Ok(());
        }
        Err(Error::configuration(format!(
            "A licence key is needed for {} and none is set. Set it with \
             data_update_license_keys, or turn the updates off, with auto_update(false) \
             and update_on_startup(false) on a pipeline or engine builder or with \
             automatic_updates_enabled(false) and update_on_startup(false) on a data \
             file configuration, for a data file no licence serves, such as the free \
             Lite file.",
            wanted.join(" and ")
        )))
    }
}

impl Debug for DataFileConfiguration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let keys = &self.data_update_license_keys;
        let masked_keys: Vec<String> = keys.iter().map(String::as_str).map(mask).collect();
        let masked_url = self
            .data_update_url
            .as_deref()
            .map(|url| redact_with(url, keys));
        f.debug_struct("DataFileConfiguration")
            .field("identifier", &self.identifier)
            .field("data_file_path", &self.data_file_path)
            .field("data_update_url", &masked_url)
            .field("automatic_updates_enabled", &self.automatic_updates_enabled)
            .field(
                "file_system_watcher_enabled",
                &self.file_system_watcher_enabled,
            )
            .field("update_on_startup", &self.update_on_startup)
            .field("polling_interval_seconds", &self.polling_interval_seconds)
            .field("max_randomisation_seconds", &self.max_randomisation_seconds)
            .field("decompress_content", &self.decompress_content)
            .field("verify_md5", &self.verify_md5)
            .field("verify_modified_since", &self.verify_modified_since)
            .field("data_update_license_keys", &masked_keys)
            .field("url_formatter", &self.url_formatter)
            .field("data_download_type", &self.data_download_type)
            .field(
                "license_key_required_for_updates",
                &self.license_key_required_for_updates,
            )
            .finish()
    }
}

/// A fluent builder for [`DataFileConfiguration`].
#[derive(Debug, Clone)]
pub struct DataFileConfigurationBuilder {
    config: DataFileConfiguration,
}

impl DataFileConfigurationBuilder {
    fn new(data_file_path: Option<PathBuf>) -> Self {
        DataFileConfigurationBuilder {
            config: DataFileConfiguration {
                identifier: DEFAULT_IDENTIFIER.to_owned(),
                data_file_path,
                data_update_url: None,
                automatic_updates_enabled: true,
                file_system_watcher_enabled: true,
                update_on_startup: false,
                polling_interval_seconds: DEFAULT_POLLING_INTERVAL_SECONDS,
                max_randomisation_seconds: DEFAULT_MAX_RANDOMISATION_SECONDS,
                decompress_content: true,
                verify_md5: true,
                verify_modified_since: true,
                data_update_license_keys: Vec::new(),
                url_formatter: None,
                data_download_type: None,
                license_key_required_for_updates: true,
            },
        }
    }

    /// Set the data-file identifier.
    pub fn identifier(mut self, identifier: impl Into<String>) -> Self {
        self.config.identifier = identifier.into();
        self
    }

    /// Set the remote update URL and enable remote polling.
    pub fn data_update_url(mut self, url: impl Into<String>) -> Self {
        self.config.data_update_url = Some(url.into());
        self
    }

    /// Enable or disable automatic updates, the master switch for remote
    /// polling and the file-system watcher.
    pub fn automatic_updates_enabled(mut self, enabled: bool) -> Self {
        self.config.automatic_updates_enabled = enabled;
        self
    }

    /// Enable or disable the file-system watcher, which runs only while
    /// automatic updates are on.
    pub fn file_system_watcher_enabled(mut self, enabled: bool) -> Self {
        self.config.file_system_watcher_enabled = enabled;
        self
    }

    /// Enable or disable the synchronous update-on-startup poll.
    pub fn update_on_startup(mut self, enabled: bool) -> Self {
        self.config.update_on_startup = enabled;
        self
    }

    /// Set the remote polling interval in seconds.
    pub fn polling_interval_seconds(mut self, seconds: u64) -> Self {
        self.config.polling_interval_seconds = seconds;
        self
    }

    /// Set the maximum random extra delay added to each polling interval.
    pub fn max_randomisation_seconds(mut self, seconds: u64) -> Self {
        self.config.max_randomisation_seconds = seconds;
        self
    }

    /// Set whether downloaded content is gzip-compressed.
    pub fn decompress_content(mut self, enabled: bool) -> Self {
        self.config.decompress_content = enabled;
        self
    }

    /// Set whether the `Content-MD5` hash is verified.
    pub fn verify_md5(mut self, enabled: bool) -> Self {
        self.config.verify_md5 = enabled;
        self
    }

    /// Set whether the `If-Modified-Since` header is sent.
    pub fn verify_modified_since(mut self, enabled: bool) -> Self {
        self.config.verify_modified_since = enabled;
        self
    }

    /// Set the license keys the URL formatter adds to the update URL.
    pub fn data_update_license_keys<I, S>(mut self, keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.config.data_update_license_keys = keys.into_iter().map(Into::into).collect();
        self
    }

    /// Set the formatter that builds the URL each poll requests, or `None` to
    /// request the configured URL as it is.
    pub fn url_formatter(mut self, formatter: Option<Arc<dyn DataUpdateUrlFormatter>>) -> Self {
        self.config.url_formatter = formatter;
        self
    }

    /// Set the download type the URL formatter sends.
    pub fn data_download_type(mut self, download_type: impl Into<String>) -> Self {
        self.config.data_download_type = Some(download_type.into());
        self
    }

    /// Set whether an engine refuses to build with automatic updates or update
    /// on startup on and no licence key. On by default. Turn it off for an
    /// update source that serves the file without one.
    pub fn license_key_required_for_updates(mut self, required: bool) -> Self {
        self.config.license_key_required_for_updates = required;
        self
    }

    /// Finish building the configuration.
    pub fn build(self) -> DataFileConfiguration {
        self.config
    }
}

/// The configuration plus run-time state for one data file used by an engine.
///
/// The mutable timestamps are held behind a [`Mutex`] so the update service can
/// write them from a background
/// thread while the engine reads them on the request path. An instance is
/// shared as an [`std::sync::Arc<AspectEngineDataFile>`] between the engine and
/// the [`crate::DataUpdateService`].
#[derive(Debug)]
pub struct AspectEngineDataFile {
    configuration: DataFileConfiguration,
    state: Mutex<DataFileState>,
}

#[derive(Debug, Default)]
struct DataFileState {
    /// When the current data was published, parsed from the data file. The
    /// `If-Modified-Since` header is set from this.
    data_published: Option<DateTime<Utc>>,
    /// How many times the publish date has been set, so the data update
    /// service can tell whether an engine recorded one during a refresh.
    #[cfg(feature = "data-update")]
    published_changes: u64,
    /// When an updated data file is expected to be available. The scheduled
    /// poll after registration, and after a scheduled poll or
    /// `check_for_update` applies an update, waits for it.
    update_available: Option<DateTime<Utc>>,
    /// The last-modified time of the file the service most recently applied,
    /// used to debounce duplicate file-system-watcher events. Only the
    /// data-update service touches it, so it is gated on that feature.
    #[cfg(feature = "data-update")]
    last_applied_modified: Option<DateTime<Utc>>,
    /// True once the file has been registered with a data update service.
    registered: bool,
}

impl AspectEngineDataFile {
    /// Create run-time state for the supplied configuration.
    pub fn new(configuration: DataFileConfiguration) -> Self {
        AspectEngineDataFile {
            configuration,
            state: Mutex::new(DataFileState::default()),
        }
    }

    /// The data file's identifier.
    pub fn identifier(&self) -> &str {
        &self.configuration.identifier
    }

    /// Borrow the data file's configuration.
    pub fn configuration(&self) -> &DataFileConfiguration {
        &self.configuration
    }

    /// The path to the data file on disk, if any.
    pub fn data_file_path(&self) -> Option<&std::path::Path> {
        self.configuration.data_file_path.as_deref()
    }

    /// The remote update URL, if configured.
    pub fn data_update_url(&self) -> Option<&str> {
        self.configuration.data_update_url.as_deref()
    }

    /// The URL a poll requests: the configured update URL as the configured
    /// formatter rewrites it, or as it is when there is no formatter. `None`
    /// when no update URL is configured.
    pub fn formatted_update_url(&self) -> Option<String> {
        self.data_update_url()?;
        Some(match &self.configuration.url_formatter {
            Some(formatter) => formatter.format_url(self),
            None => self
                .configuration
                .data_update_url
                .clone()
                .unwrap_or_default(),
        })
    }

    /// Whether automatic remote updates are enabled for this file.
    pub fn automatic_updates_enabled(&self) -> bool {
        self.configuration.automatic_updates_enabled
    }

    /// When the current data was published, if known.
    pub fn data_published(&self) -> Option<DateTime<Utc>> {
        self.lock().data_published
    }

    /// Set the publish time of the current data.
    pub fn set_data_published(&self, when: DateTime<Utc>) {
        let mut state = self.lock();
        state.data_published = Some(when);
        #[cfg(feature = "data-update")]
        {
            state.published_changes = state.published_changes.wrapping_add(1);
        }
    }

    /// Clear the publish time, for data that holds none. This does not count
    /// as setting one, so after a download the data update service still
    /// records the time of the download.
    pub fn clear_data_published(&self) {
        self.lock().data_published = None;
    }

    /// How many times the publish date has been set.
    #[cfg(feature = "data-update")]
    pub(crate) fn published_changes(&self) -> u64 {
        self.lock().published_changes
    }

    /// When an updated data file is expected to be available, if known.
    pub fn update_available_time(&self) -> Option<DateTime<Utc>> {
        self.lock().update_available
    }

    /// Set the time an updated data file is expected to be available.
    pub fn set_update_available_time(&self, when: DateTime<Utc>) {
        self.lock().update_available = Some(when);
    }

    /// Clear the time an updated data file is expected, for data that holds
    /// none.
    pub fn clear_update_available_time(&self) {
        self.lock().update_available = None;
    }

    /// Record the dates a data file's header holds, each as a year, month
    /// and day, once an engine has loaded the file, as .NET's engines do
    /// after each load. The publish date, at midnight UTC, goes in the
    /// update service's `If-Modified-Since` header. The next update date, at
    /// noon UTC and a random minute as .NET gives it, is when the update
    /// service next polls. A date the header does not hold, or that is not a
    /// day of the calendar, is cleared, so no date of data no longer loaded
    /// is kept.
    pub fn record_header_dates(
        &self,
        published: Option<(i32, u32, u32)>,
        next_update: Option<(i32, u32, u32)>,
    ) {
        let published = published.and_then(|(year, month, day)| {
            Utc.with_ymd_and_hms(year, month, day, 0, 0, 0).single()
        });
        match published {
            Some(published) => self.set_data_published(published),
            None => self.clear_data_published(),
        }
        let next = next_update.and_then(|(year, month, day)| {
            Utc.with_ymd_and_hms(year, month, day, NEXT_UPDATE_HOUR, random_minute(), 0)
                .single()
        });
        match next {
            Some(next) => self.set_update_available_time(next),
            None => self.clear_update_available_time(),
        }
    }

    /// True once this file has been registered with a data update service.
    pub fn is_registered(&self) -> bool {
        self.lock().registered
    }

    /// Mark this file as registered (or not) with a data update service.
    #[cfg(feature = "data-update")]
    pub(crate) fn set_registered(&self, registered: bool) {
        self.lock().registered = registered;
    }

    /// The last-modified time of the file most recently applied by the service.
    #[cfg(feature = "data-update")]
    pub(crate) fn last_applied_modified(&self) -> Option<DateTime<Utc>> {
        self.lock().last_applied_modified
    }

    /// Record the last-modified time of the file just applied, used to debounce
    /// duplicate watcher events.
    #[cfg(feature = "data-update")]
    pub(crate) fn set_last_applied_modified(&self, when: DateTime<Utc>) {
        self.lock().last_applied_modified = Some(when);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, DataFileState> {
        // The lock guards only small `Copy` timestamps, so a poisoned lock
        // (from a panic while holding it) cannot leave inconsistent state.
        // Recover the guard rather than propagating the poison.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The hour, UTC, of the day the data file header gives for the next data
/// file, at which the update service is told to expect it.
const NEXT_UPDATE_HOUR: u32 = 12;

/// The number of minutes in an hour, the range the minute of
/// [`NEXT_UPDATE_HOUR`] is chosen from.
const MINUTES_PER_HOUR: u64 = 60;

/// A minute of the hour chosen at random, so that servers loading the same
/// data file do not all start to poll at once.
fn random_minute() -> u32 {
    let random = RandomState::new().hash_one(std::time::SystemTime::now());
    u32::try_from(random % MINUTES_PER_HOUR).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The publish date is recorded at midnight UTC and the next update date
    /// at noon UTC and a whole minute, as .NET's engines record them, and a
    /// date the header lacks, or one that is not a day of the calendar,
    /// clears the date recorded before.
    #[test]
    fn header_dates_are_recorded_as_dotnet_records_them() {
        use chrono::Timelike;

        let file = AspectEngineDataFile::new(DataFileConfiguration::builder("d.dat").build());
        file.record_header_dates(Some((2001, 2, 3)), Some((2001, 2, 4)));
        assert_eq!(
            file.data_published(),
            Utc.with_ymd_and_hms(2001, 2, 3, 0, 0, 0).single()
        );
        let next = file.update_available_time().expect("no next update date");
        assert_eq!(
            next.date_naive(),
            chrono::NaiveDate::from_ymd_opt(2001, 2, 4).unwrap()
        );
        assert_eq!((next.hour(), next.second(), next.nanosecond()), (12, 0, 0));

        file.record_header_dates(None, Some((2001, 2, 30)));
        assert!(file.data_published().is_none());
        assert!(file.update_available_time().is_none());
    }

    /// The minute of the next update date is drawn at random, so servers
    /// loading the same data file poll at different times. 200 draws from 60
    /// minutes give about 58 different minutes.
    #[test]
    fn the_next_update_minute_is_drawn_at_random() {
        let minutes: std::collections::BTreeSet<u32> = (0..200).map(|_| random_minute()).collect();
        assert!(
            minutes.len() > 20,
            "{} different minutes in 200: {minutes:?}",
            minutes.len()
        );
        assert!(minutes.iter().all(|minute| *minute < 60));
    }

    #[test]
    fn builder_uses_documented_defaults() {
        let cfg = DataFileConfiguration::builder("data.dat").build();
        assert_eq!(cfg.identifier, "Default");
        assert!(cfg.automatic_updates_enabled);
        assert!(cfg.file_system_watcher_enabled);
        assert!(!cfg.update_on_startup);
        assert_eq!(cfg.polling_interval_seconds, 30 * 60);
        assert_eq!(cfg.max_randomisation_seconds, 10 * 60);
        assert!(cfg.decompress_content);
        assert!(cfg.verify_md5);
        assert!(cfg.verify_modified_since);
        assert!(cfg.license_key_required_for_updates);
        assert_eq!(
            cfg.data_file_path.as_deref(),
            Some(std::path::Path::new("data.dat"))
        );
    }

    /// A licence key is required once automatic updates or update on startup
    /// are on, a blank key counts as none, and a source that needs no key or a
    /// configuration with both off passes.
    #[test]
    fn a_licence_key_is_required_for_updates() {
        let missing = DataFileConfiguration::builder("d.dat")
            .build()
            .check_license_keys()
            .expect_err("automatic updates need a key")
            .to_string();
        assert!(missing.contains("automatic updates"), "{missing}");
        assert!(missing.contains("auto_update(false)"), "{missing}");

        assert!(DataFileConfiguration::builder("d.dat")
            .data_update_license_keys([" "])
            .build()
            .check_license_keys()
            .is_err());

        let startup = DataFileConfiguration::builder("d.dat")
            .automatic_updates_enabled(false)
            .update_on_startup(true)
            .build()
            .check_license_keys()
            .expect_err("update on startup needs a key")
            .to_string();
        assert!(startup.contains("update on startup"), "{startup}");

        assert!(DataFileConfiguration::builder("d.dat")
            .data_update_license_keys(["KEY1"])
            .build()
            .check_license_keys()
            .is_ok());
        assert!(DataFileConfiguration::builder("d.dat")
            .license_key_required_for_updates(false)
            .build()
            .check_license_keys()
            .is_ok());
        assert!(DataFileConfiguration::builder("d.dat")
            .automatic_updates_enabled(false)
            .build()
            .check_license_keys()
            .is_ok());
    }

    #[test]
    fn builder_overrides() {
        let cfg = DataFileConfiguration::builder("d.dat")
            .identifier("primary")
            .data_update_url("https://example.invalid/d")
            .update_on_startup(true)
            .polling_interval_seconds(60)
            .verify_md5(false)
            .data_update_license_keys(["KEY1", "KEY2"])
            .build();
        assert_eq!(cfg.identifier, "primary");
        assert_eq!(
            cfg.data_update_url.as_deref(),
            Some("https://example.invalid/d")
        );
        assert!(cfg.update_on_startup);
        assert_eq!(cfg.polling_interval_seconds, 60);
        assert!(!cfg.verify_md5);
        assert_eq!(cfg.data_update_license_keys, ["KEY1", "KEY2"]);
        assert!(cfg.url_formatter.is_none());
        assert!(cfg.data_download_type.is_none());
    }

    /// The Distributor formatter writes the licence keys, the download flag
    /// and the type as the .NET and Java formatters do.
    #[test]
    fn fiftyone_formatter_writes_the_distributor_query() {
        let file = AspectEngineDataFile::new(
            DataFileConfiguration::builder("d.dat")
                .data_update_url(DISTRIBUTOR_URL)
                .data_update_license_keys(["KEY1", "KEY2"])
                .data_download_type("HashV41")
                .url_formatter(Some(Arc::new(FiftyOneUrlFormatter)))
                .build(),
        );
        assert_eq!(
            file.formatted_update_url().as_deref(),
            Some(
                "https://distributor.51degrees.com/api/v2/download\
                 ?LicenseKeys=KEY1|KEY2&Download=True&Type=HashV41"
            )
        );
    }

    /// Without keys or a type the parameters are still present, as .NET writes
    /// them, so the Distributor answers with its own refusal.
    #[test]
    fn fiftyone_formatter_without_keys_or_type() {
        let file = AspectEngineDataFile::new(
            DataFileConfiguration::builder("d.dat")
                .data_update_url("https://example.invalid/d")
                .url_formatter(Some(Arc::new(FiftyOneUrlFormatter)))
                .build(),
        );
        assert_eq!(
            file.formatted_update_url().as_deref(),
            Some("https://example.invalid/d?LicenseKeys=&Download=True")
        );
    }

    /// With no formatter the configured URL is requested as it is, and with
    /// no URL nothing is.
    #[test]
    fn no_formatter_requests_the_configured_url() {
        let file = AspectEngineDataFile::new(
            DataFileConfiguration::builder("d.dat")
                .data_update_url("https://example.invalid/file.hash")
                .data_update_license_keys(["KEY1"])
                .build(),
        );
        assert_eq!(
            file.formatted_update_url().as_deref(),
            Some("https://example.invalid/file.hash")
        );
        let none = AspectEngineDataFile::new(DataFileConfiguration::builder("d.dat").build());
        assert!(none.formatted_update_url().is_none());
    }

    /// The `Debug` form, which a log line or a panic message can carry, shows
    /// each licence key by its last 4 characters only, in the list of keys and
    /// in the update URL. The builder and the data file show the
    /// configuration the same way.
    #[test]
    fn the_debug_form_shows_only_the_last_four_characters_of_each_key() {
        let builder = DataFileConfiguration::builder("d.dat")
            .data_update_url("https://example.invalid/d?LicenseKeys=QRSTUVWXYZ012345")
            .data_update_license_keys(["ABCDEFGHIJKLMNOP"]);
        let configuration = builder.clone().build();
        let file = AspectEngineDataFile::new(configuration.clone());
        for shown in [
            format!("{builder:?}"),
            format!("{configuration:?}"),
            format!("{file:?}"),
        ] {
            assert!(
                !shown.contains("ABCDEFGHIJKLMNOP"),
                "a key was shown: {shown}"
            );
            assert!(
                !shown.contains("QRSTUVWXYZ012345"),
                "the key in the URL was shown: {shown}"
            );
            assert!(
                shown.contains("\"************MNOP\""),
                "the masked key was lost: {shown}"
            );
            assert!(
                shown.contains("LicenseKeys=************2345"),
                "the masked URL was lost: {shown}"
            );
        }
    }

    #[test]
    fn state_round_trips() {
        let file = AspectEngineDataFile::new(DataFileConfiguration::builder("d.dat").build());
        assert!(!file.is_registered());
        assert!(file.data_published().is_none());

        let now = Utc::now();
        file.set_data_published(now);
        assert_eq!(file.data_published(), Some(now));
        file.set_update_available_time(now);
        assert_eq!(file.update_available_time(), Some(now));

        #[cfg(feature = "data-update")]
        let changes = file.published_changes();
        file.clear_data_published();
        file.clear_update_available_time();
        assert!(file.data_published().is_none());
        assert!(file.update_available_time().is_none());
        // Clearing the publish date is not setting one.
        #[cfg(feature = "data-update")]
        {
            assert_eq!(file.published_changes(), changes);
        }

        // set_registered exists only with the data-update service.
        #[cfg(feature = "data-update")]
        {
            file.set_registered(true);
            assert!(file.is_registered());
        }
    }
}
