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

//! The data file settings every on-premise engine's builder takes from a
//! configuration, by the names .NET's `SingleFileAspectEngineBuilderBase`
//! and `OnPremiseAspectEngineBuilderBase` give them. See
//! [`fiftyone_pipeline_core::configuration`].
//!
//! [`add_data_file_settings`] adds them to an engine's builder definition
//! in .NET's order:
//!
//! - `SetDataUpdateUrl`, refusing an empty URL or one without a scheme with
//!   .NET's messages, and `SetDataUpdateUseUrlFormatter`, where false
//!   requests the URL as it is configured.
//! - `SetDataUpdateVerifyMd5`, `SetDataUpdateDecompress`, `SetAutoUpdate`,
//!   `SetDataFileSystemWatcher`, `SetVerifyIfModifiedSince` and
//!   `SetDataUpdateOnStartup`.
//! - `SetUpdatePollingInterval` and `SetUpdateRandomisationMax`, as a whole
//!   number of seconds or as the text of a .NET `TimeSpan`, such as
//!   `00:30:00`, whose fraction of a second is dropped. A negative number is
//!   refused, because the update service counts whole seconds from now.
//! - `SetDataUpdateLicenseKey`, adding one key, and
//!   `SetDataUpdateLicenseKeys`, adding a list of them.
//! - `SetTempDirPath`, the folder a copy of the data file is made in.
//! - `SetDataUpdateUrl` given a `Uri`, `SetDataUpdateUrlFormatter`,
//!   `SetDataUpdateUseUrlFormatter` given a formatter, and `AddDataFile`
//!   take their values in code only, so naming one fails as it does in
//!   .NET.
//!
//! The engine's builder then calls [`DataFileSettings::check`], which
//! refuses updates that cannot run as .NET refuses them, and makes its data
//! file with [`DataFileSettings::configuration`].
//!
//! [`performance_profile_number`] reads the text of an on-premise engine's
//! `SetPerformanceProfile` as .NET does.

use std::path::PathBuf;
use std::sync::Arc;

use fiftyone_pipeline_core::configuration::{BuilderDefinition, ConfigType, ParseFailure};
use fiftyone_pipeline_core::{Error, Result};

use crate::{AspectEngineDataFile, DataFileConfiguration, DataUpdateUrlFormatter};

/// .NET's default polling interval, 30 minutes.
const DEFAULT_POLLING_INTERVAL_SECONDS: i32 = 30 * 60;

/// .NET's default most random delay added to each poll, 10 minutes.
const DEFAULT_RANDOMISATION_SECONDS: i32 = 10 * 60;

/// The data file settings read from a configuration, held as .NET's
/// `DataFileConfigurationBuilder` holds them until the engine is made. The
/// defaults are .NET's, being automatic updates and the file system watcher
/// on, update on startup off, a poll every 30 minutes plus up to 10, MD5
/// verification, decompression and `If-Modified-Since` on, and the
/// Distributor's URL formatter.
#[derive(Debug, Clone)]
pub struct DataFileSettings {
    data_update_url: Option<String>,
    use_url_formatter: bool,
    verify_md5: bool,
    decompress: bool,
    auto_update: bool,
    file_system_watcher: bool,
    polling_interval_seconds: i32,
    randomisation_seconds: i32,
    verify_modified_since: bool,
    license_keys: Vec<String>,
    update_on_startup: bool,
    temp_dir_path: Option<String>,
}

impl Default for DataFileSettings {
    fn default() -> Self {
        DataFileSettings {
            data_update_url: None,
            use_url_formatter: true,
            verify_md5: true,
            decompress: true,
            auto_update: true,
            file_system_watcher: true,
            polling_interval_seconds: DEFAULT_POLLING_INTERVAL_SECONDS,
            randomisation_seconds: DEFAULT_RANDOMISATION_SECONDS,
            verify_modified_since: true,
            license_keys: Vec::new(),
            update_on_startup: false,
            temp_dir_path: None,
        }
    }
}

impl DataFileSettings {
    /// The settings with .NET's defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// The folder `SetTempDirPath` gave, if any.
    pub fn temp_dir_path(&self) -> Option<&str> {
        self.temp_dir_path.as_deref()
    }

    /// Whether any update is on, being automatic updates, update on startup
    /// or the file system watcher, so the data file is registered with the
    /// update service.
    pub fn updates_requested(&self) -> bool {
        !self.update_features().is_empty()
    }

    /// The updates that are on, by the names .NET's messages give them.
    fn update_features(&self) -> Vec<&'static str> {
        [
            (self.auto_update, "auto update"),
            (self.update_on_startup, "update on startup"),
            (self.file_system_watcher, "file system watcher"),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, name)| name)
        .collect()
    }

    /// Refuse updates that cannot run, with .NET's messages. Any update needs
    /// an update service, which an application gives the pipeline builder as
    /// a resource, and automatic updates or update on startup need a license
    /// key, as the
    /// [specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/data-updates.md#defaults)
    /// requires.
    pub fn check(&self, has_update_service: bool) -> Result<()> {
        let features = self.update_features();
        if !features.is_empty() && !has_update_service {
            return Err(Error::configuration(format!(
                "Some enabled features ({}) require an IDataUpdateService but one has not been \
                 supplied. This can be corrected by passing an IDataUpdateService instance to \
                 the engine builder constructor. If building from configuration, an \
                 IServiceProvider instance, able to resolve an IDataUpdateService, must be \
                 passed to the pipeline builder.",
                features.join(", ")
            )));
        }
        let needing_key: Vec<&str> = features
            .into_iter()
            .filter(|feature| *feature != "file system watcher")
            .collect();
        if !needing_key.is_empty() && self.license_keys.iter().all(String::is_empty) {
            return Err(Error::configuration(format!(
                "Some enabled features ({}) require a license key but one has not been \
                 supplied. This can be corrected by passing a license key to the engine \
                 builder. If building from configuration, check the associated builder \
                 implementation to determine the correct parameter name to use.",
                needing_key.join(", ")
            )));
        }
        Ok(())
    }

    /// The configuration of the data file at `data_file_path`. The update URL,
    /// the URL formatter and the download type are left to the engine's
    /// defaults where no setting gave them.
    pub fn configuration(&self, data_file_path: impl Into<PathBuf>) -> DataFileConfiguration {
        let mut builder = DataFileConfiguration::builder(data_file_path)
            .automatic_updates_enabled(self.auto_update)
            .file_system_watcher_enabled(self.file_system_watcher)
            .update_on_startup(self.update_on_startup)
            .polling_interval_seconds(u64::try_from(self.polling_interval_seconds).unwrap_or(0))
            .max_randomisation_seconds(u64::try_from(self.randomisation_seconds).unwrap_or(0))
            .decompress_content(self.decompress)
            .verify_md5(self.verify_md5)
            .verify_modified_since(self.verify_modified_since)
            .data_update_license_keys(self.license_keys.iter().cloned());
        if let Some(url) = &self.data_update_url {
            builder = builder.data_update_url(url.clone());
        }
        if !self.use_url_formatter {
            builder = builder.url_formatter(Some(Arc::new(UnformattedUrl)));
        }
        builder.build()
    }
}

/// Requests the update URL as it is configured, as .NET does with no URL
/// formatter.
#[derive(Debug)]
struct UnformattedUrl;

impl DataUpdateUrlFormatter for UnformattedUrl {
    fn format_url(&self, data_file: &AspectEngineDataFile) -> String {
        data_file.data_update_url().unwrap_or_default().to_string()
    }
}

/// A period read as .NET's `TimeSpan.TryParse` reads it, in seconds.
struct TimeSpan(f64);

impl ConfigType for TimeSpan {
    const TYPE_NAME: &'static str = "TimeSpan";

    fn parse_text(text: &str) -> std::result::Result<Self, ParseFailure> {
        parse_time_span(text)
            .map(TimeSpan)
            .ok_or_else(ParseFailure::text)
    }
}

/// The ticks of 100 nanoseconds in one second, .NET's unit for a period.
const TICKS_PER_SECOND: i128 = 10_000_000;

/// The most days a .NET `TimeSpan` holds.
const MAX_DAYS: u64 = 10_675_199;

/// The most digits of a fraction of a second, in ticks.
const MAX_FRACTION_DIGITS: usize = 7;

/// Read a period as .NET's `TimeSpan.TryParse` does in the invariant forms,
/// giving its seconds. Space around the text and a leading `-` are allowed.
/// One number is days, `h:m` hours and minutes, `h:m:s` hours, minutes and
/// seconds or, when the hours are more than 23, days, hours and minutes,
/// and `d.h:m`, `d.h:m:s`, `d:h:m:s` and `h:m:s.f` with `d.h:m:s.f` and
/// `d:h:m:s.f` add days and a fraction of up to seven digits.
fn parse_time_span(text: &str) -> Option<f64> {
    let text = text.trim_matches(char::is_whitespace);
    let (negative, body) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let mut numbers = Vec::new();
    let mut separators = Vec::new();
    let mut start = 0;
    for (index, c) in body.char_indices() {
        if c == ':' || c == '.' {
            numbers.push(&body[start..index]);
            separators.push(c);
            start = index + 1;
        } else if !c.is_ascii_digit() {
            return None;
        }
    }
    numbers.push(&body[start..]);
    if numbers.iter().any(|n| n.is_empty()) {
        return None;
    }
    let fraction = |digits: &str| -> Option<u64> {
        if digits.len() > MAX_FRACTION_DIGITS {
            return None;
        }
        let padded = format!("{digits:0<width$}", width = MAX_FRACTION_DIGITS);
        padded.parse().ok()
    };
    let value = |digits: &str| -> Option<u64> { digits.parse().ok() };
    // Days, hours, minutes, seconds and the fraction in ticks.
    let parts: [u64; 5] = match (numbers.as_slice(), separators.as_slice()) {
        ([d], []) => [value(d)?, 0, 0, 0, 0],
        ([h, m], [':']) => [0, value(h)?, value(m)?, 0, 0],
        ([a, b, c], [':', ':']) => {
            let (a, b, c) = (value(a)?, value(b)?, value(c)?);
            if valid(0, a, b, c) {
                [0, a, b, c, 0]
            } else {
                [a, b, c, 0, 0]
            }
        }
        ([d, h, m], ['.', ':']) => [value(d)?, value(h)?, value(m)?, 0, 0],
        ([d, h, m, s], ['.' | ':', ':', ':']) => [value(d)?, value(h)?, value(m)?, value(s)?, 0],
        ([h, m, s, f], [':', ':', '.']) => [0, value(h)?, value(m)?, value(s)?, fraction(f)?],
        ([d, h, m, s, f], ['.' | ':', ':', ':', '.']) => {
            [value(d)?, value(h)?, value(m)?, value(s)?, fraction(f)?]
        }
        _ => return None,
    };
    let [days, hours, minutes, seconds, ticks] = parts;
    if !valid(days, hours, minutes, seconds) {
        return None;
    }
    let whole = ((i128::from(days) * 24 + i128::from(hours)) * 60 + i128::from(minutes)) * 60
        + i128::from(seconds);
    let magnitude = whole * TICKS_PER_SECOND + i128::from(ticks);
    let total = if negative { -magnitude } else { magnitude };
    if total > i128::from(i64::MAX) || total < i128::from(i64::MIN) {
        return None;
    }
    Some(total as f64 / TICKS_PER_SECOND as f64)
}

/// True when each part of a period is in its range.
fn valid(days: u64, hours: u64, minutes: u64, seconds: u64) -> bool {
    days <= MAX_DAYS && hours <= 23 && minutes <= 59 && seconds <= 59
}

/// The whole seconds of a period given as a `TimeSpan`, as .NET's builder
/// takes them, refusing more than `i32::MAX` with .NET's message for the
/// setting.
fn whole_seconds(period: &TimeSpan, too_large: &str, parameter: &str) -> Result<i32> {
    let seconds = period.0;
    if seconds > f64::from(i32::MAX) {
        return Err(Error::configuration(format!(
            "{too_large} timespan too large. Must be less than {} seconds but is {seconds} \
             seconds (Parameter '{parameter}')",
            i32::MAX
        )));
    }
    // As .NET's cast, which drops the fraction and holds a period below
    // i32::MIN at i32::MIN.
    Ok(seconds as i32)
}

/// Refuse a negative number of seconds for the setting called `setting`.
fn not_negative(seconds: i32, setting: &str) -> Result<i32> {
    if seconds < 0 {
        return Err(Error::configuration(format!(
            "The setting '{setting}' must be 0 or more seconds, but is {seconds}."
        )));
    }
    Ok(seconds)
}

/// The text of `SetDataUpdateUrl`, refused with .NET's messages when it is
/// empty or has no scheme, as .NET's `Uri` refuses it.
fn data_update_url(url: String) -> Result<String> {
    if url.is_empty() {
        return Err(Error::configuration("Invalid URI: The URI is empty."));
    }
    let scheme = url.split(':').next().unwrap_or_default();
    let has_scheme = url.contains(':')
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !has_scheme {
        return Err(Error::configuration(
            "Invalid URI: The format of the URI could not be determined.",
        ));
    }
    Ok(url)
}

/// .NET's performance profiles, with their numbers, in number order.
const PERFORMANCE_PROFILES: [(&str, i64); 5] = [
    ("LowMemory", 0),
    ("MaxPerformance", 1),
    ("HighPerformance", 2),
    ("Balanced", 3),
    ("BalancedTemp", 4),
];

/// The number .NET's `Enum.TryParse` gives `text` for the performance
/// profiles, being a whole number, or names matched exactly and joined by
/// commas, whose numbers are combined, with space around each allowed.
fn profile_number(text: &str) -> Option<i64> {
    let trimmed = text.trim_matches(char::is_whitespace);
    if trimmed.starts_with(|c: char| c.is_ascii_digit() || c == '+' || c == '-') {
        let digits = trimmed.strip_prefix('+').unwrap_or(trimmed);
        return digits.parse::<i32>().ok().map(i64::from);
    }
    trimmed.split(',').try_fold(0, |number, name| {
        let name = name.trim_matches(char::is_whitespace);
        PERFORMANCE_PROFILES
            .iter()
            .find(|(profile, _)| *profile == name)
            .map(|(_, value)| number | value)
    })
}

/// The performance profile the text of `SetPerformanceProfile` names, as
/// .NET's on-premise engine builders read it, being .NET's number for it:
/// `0` `LowMemory`, `1` `MaxPerformance`, `2` `HighPerformance`, `3`
/// `Balanced` or `4` `BalancedTemp`. The text is a number or the exact
/// names, names joined by commas being combined. Text naming no profile is
/// refused with .NET's message, and a number that is none of the five with
/// .NET's message for the engine called `engine`.
pub fn performance_profile_number(text: &str, engine: &str) -> Result<u8> {
    let Some(number) = profile_number(text) else {
        let available: Vec<String> = PERFORMANCE_PROFILES
            .iter()
            .map(|(name, _)| format!("'{name}'"))
            .collect();
        return Err(Error::configuration(format!(
            "'{text}' is not a valid performance profile. Available profiles are {}.",
            available.join(", ")
        )));
    };
    u8::try_from(number)
        .ok()
        .filter(|number| *number <= 4)
        .ok_or_else(|| {
            Error::configuration(format!(
                "The performance profile '{number}' is not valid for a {engine}. (Parameter \
                 'profile')"
            ))
        })
}

/// Add the data file settings to `definition`, in the order .NET's
/// `SingleFileAspectEngineBuilderBase` and `OnPremiseAspectEngineBuilderBase`
/// declare them. `settings` gives the [`DataFileSettings`] the builder `B`
/// holds.
pub fn add_data_file_settings<B: Send + 'static>(
    definition: BuilderDefinition<B>,
    settings: fn(&mut B) -> &mut DataFileSettings,
) -> BuilderDefinition<B> {
    definition
        .try_setting("SetDataUpdateUrl", move |mut builder: B, url: String| {
            settings(&mut builder).data_update_url = Some(data_update_url(url)?);
            Ok(builder)
        })
        .code_only_setting("SetDataUpdateUrl", "Uri")
        .code_only_setting("SetDataUpdateUrlFormatter", "IDataUpdateUrlFormatter")
        .code_only_setting("SetDataUpdateUseUrlFormatter", "IDataUpdateUrlFormatter")
        .setting(
            "SetDataUpdateUseUrlFormatter",
            move |mut builder: B, use_formatter: bool| {
                // As in .NET, true keeps whatever formatter is set.
                if !use_formatter {
                    settings(&mut builder).use_url_formatter = false;
                }
                builder
            },
        )
        .setting("SetDataUpdateVerifyMd5", move |mut builder: B, on: bool| {
            settings(&mut builder).verify_md5 = on;
            builder
        })
        .setting(
            "SetDataUpdateDecompress",
            move |mut builder: B, on: bool| {
                settings(&mut builder).decompress = on;
                builder
            },
        )
        .setting("SetAutoUpdate", move |mut builder: B, on: bool| {
            settings(&mut builder).auto_update = on;
            builder
        })
        .setting(
            "SetDataFileSystemWatcher",
            move |mut builder: B, on: bool| {
                settings(&mut builder).file_system_watcher = on;
                builder
            },
        )
        .try_setting(
            "SetUpdatePollingInterval",
            move |mut builder: B, seconds: i32| {
                settings(&mut builder).polling_interval_seconds =
                    not_negative(seconds, "SetUpdatePollingInterval")?;
                Ok(builder)
            },
        )
        .try_setting(
            "SetUpdatePollingInterval",
            move |mut builder: B, period: TimeSpan| {
                let seconds = whole_seconds(&period, "Polling interval", "pollingInterval")?;
                settings(&mut builder).polling_interval_seconds =
                    not_negative(seconds, "SetUpdatePollingInterval")?;
                Ok(builder)
            },
        )
        .try_setting(
            "SetUpdateRandomisationMax",
            move |mut builder: B, seconds: i32| {
                settings(&mut builder).randomisation_seconds =
                    not_negative(seconds, "SetUpdateRandomisationMax")?;
                Ok(builder)
            },
        )
        .try_setting(
            "SetUpdateRandomisationMax",
            move |mut builder: B, period: TimeSpan| {
                let seconds = whole_seconds(&period, "Update randomization", "maximumDeviation")?;
                settings(&mut builder).randomisation_seconds =
                    not_negative(seconds, "SetUpdateRandomisationMax")?;
                Ok(builder)
            },
        )
        .setting(
            "SetVerifyIfModifiedSince",
            move |mut builder: B, on: bool| {
                settings(&mut builder).verify_modified_since = on;
                builder
            },
        )
        .setting(
            "SetDataUpdateLicenseKey",
            move |mut builder: B, key: String| {
                settings(&mut builder).license_keys.push(key);
                builder
            },
        )
        .setting(
            "SetDataUpdateLicenseKeys",
            move |mut builder: B, keys: Box<[String]>| {
                settings(&mut builder).license_keys.extend(keys);
                builder
            },
        )
        .setting("SetDataUpdateOnStartup", move |mut builder: B, on: bool| {
            settings(&mut builder).update_on_startup = on;
            builder
        })
        .code_only_setting("AddDataFile", "IDataFileConfiguration")
        .setting("SetTempDirPath", move |mut builder: B, path: String| {
            settings(&mut builder).temp_dir_path = Some(path);
            builder
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What .NET 8's `TimeSpan.TryParse` gave for each text, in the en-GB
    /// culture, as its seconds printed in the shortest form that reads back,
    /// or `None` where it failed.
    const DOTNET: &[(&str, Option<&str>)] = &[
        ("00:02:00", Some("120")),
        ("1.00:00:00", Some("86400")),
        ("1:00:00:00", Some("86400")),
        ("2:30", Some("9000")),
        ("-00:01:00", Some("-60")),
        ("00:00:01.5", Some("1.5")),
        ("24:00:00", Some("2073600")),
        ("23:59:59", Some("86399")),
        ("00:60:00", None),
        ("1.2:3:4", Some("93784")),
        ("5", Some("432000")),
        (" 00:01:00 ", Some("60")),
        ("1:2:3:4:5", None),
        ("0:0:0.12345678", None),
        ("10675199.02:48:05.4775807", Some("922337203685.4775")),
        ("10675200.00:00:00", None),
        ("1.", None),
        ("1e3", None),
        (":30", None),
        ("00:01", Some("60")),
        ("-1.00:00:00", Some("-86400")),
        ("+00:01:00", None),
        ("00:00:00.9999999", Some("0.9999999")),
        ("00:00:59.9", Some("59.9")),
        ("0:0", Some("0")),
        ("1:2:3", Some("3723")),
        ("01:02:03:04", Some("93784")),
        ("1.2:3", Some("93780")),
        ("\t00:01:00\n", Some("60")),
        ("00:01:00\0", None),
        ("-", None),
        ("1.1.1", None),
        ("00:00:60", None),
        ("00:59", Some("3540")),
        ("99:00", None),
        ("1:24:00:00", None),
        ("1:0:60", None),
        ("0.23:59:59", Some("86399")),
        ("00:00:00,5", None),
        ("1 00:00:00", None),
        ("-0:0:1", Some("-1")),
        ("00:00:01.0000001", Some("1.0000001")),
        ("2147483647", None),
        ("24855.03:14:07", Some("2147483647")),
        ("24855.03:14:08", Some("2147483648")),
        ("-24855.03:14:08", Some("-2147483648")),
        ("-24855.03:14:09", Some("-2147483649")),
        ("1:2.5", None),
        ("1.2:3:4.5", Some("93784.5")),
        ("1:2:3:4.5", Some("93784.5")),
        ("1.2.3", None),
        ("00:00:00.", None),
        (".5", None),
        ("1:2:3.", None),
        ("0000000000001", Some("86400")),
        ("000:000:000", Some("0")),
        ("1:02:3", Some("3723")),
        ("12345678901234567890", None),
        ("-10675199.02:48:05.4775808", Some("-922337203685.4775")),
        ("-10675199.02:48:05.4775809", None),
        ("10675199.02:48:05.4775808", None),
        ("10675199", Some("922337193600")),
        ("10675199.23:59:59", None),
        ("0:0:0:0", Some("0")),
        ("00:00:00.0000000", Some("0")),
        ("1 ", Some("86400")),
        ("\u{a0}00:01:00", Some("60")),
        ("00:01:00\u{a0}", Some("60")),
        ("\u{ff11}:00", None),
        ("00:01:00\r\n", Some("60")),
        ("--1", None),
        ("-+1", None),
        ("1:2:3:4:5.6", None),
        ("1.2:3:4:5", None),
        ("1:2:3.4.5", None),
    ];

    #[test]
    fn a_time_span_is_read_as_dotnet_reads_it() {
        for (text, expected) in DOTNET {
            assert_eq!(
                parse_time_span(text).map(|seconds| seconds.to_string()),
                expected.map(str::to_string),
                "{text:?}"
            );
        }
    }

    /// .NET's cast to whole seconds drops the fraction and holds a period
    /// below `i32::MIN` there, and the builder refuses more than `i32::MAX`
    /// with these messages, as .NET 4.5.180's Hash engine builder gave.
    #[test]
    fn a_time_span_gives_whole_seconds_as_dotnet_casts_them() {
        let seconds = |text: &str| {
            whole_seconds(
                &TimeSpan(parse_time_span(text).expect(text)),
                "Polling interval",
                "pollingInterval",
            )
        };
        assert_eq!(seconds("00:00:01.5").unwrap(), 1);
        assert_eq!(seconds("00:00:00.9999999").unwrap(), 0);
        assert_eq!(seconds("24855.03:14:07").unwrap(), i32::MAX);
        assert_eq!(seconds("-24855.03:14:09").unwrap(), i32::MIN);
        assert_eq!(
            seconds("24855.03:14:08").unwrap_err().message(),
            "Polling interval timespan too large. Must be less than 2147483647 seconds but is \
             2147483648 seconds (Parameter 'pollingInterval')"
        );
        assert_eq!(
            whole_seconds(
                &TimeSpan(2147483648.0),
                "Update randomization",
                "maximumDeviation"
            )
            .unwrap_err()
            .message(),
            "Update randomization timespan too large. Must be less than 2147483647 seconds but \
             is 2147483648 seconds (Parameter 'maximumDeviation')"
        );
    }

    /// A URL .NET's `Uri` reads keeps its text, and one it refuses gives
    /// .NET's message.
    #[test]
    fn a_data_update_url_needs_a_scheme() {
        for url in [
            "https://example.com/data",
            "ftp://x",
            "C:\\data",
            "HTTPS://example.com:443/a/../b?x=1#f",
            "mailto:a",
            "a+b-c.d:x",
        ] {
            assert_eq!(data_update_url(url.to_string()).unwrap(), url);
        }
        assert_eq!(
            data_update_url(String::new()).unwrap_err().message(),
            "Invalid URI: The URI is empty."
        );
        for url in [
            "not a url",
            "example.com/data",
            "/data",
            "1http://x",
            "data",
        ] {
            assert_eq!(
                data_update_url(url.to_string()).unwrap_err().message(),
                "Invalid URI: The format of the URI could not be determined.",
                "{url}"
            );
        }
    }

    /// The number each text gave in .NET 4.5.180's Hash engine and 4.5.155's
    /// IP Intelligence engine, or the message it refused the text with.
    #[test]
    fn a_performance_profile_is_read_as_dotnet_reads_it() {
        for (text, number) in [
            ("LowMemory", 0),
            ("MaxPerformance", 1),
            ("HighPerformance", 2),
            ("Balanced", 3),
            ("BalancedTemp", 4),
            (" HighPerformance ", 2),
            ("0", 0),
            ("4", 4),
            ("+2", 2),
            ("LowMemory,Balanced", 3),
            ("LowMemory, Balanced", 3),
        ] {
            assert_eq!(
                performance_profile_number(text, "Engine").unwrap(),
                number,
                "{text}"
            );
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
                performance_profile_number(text, "Engine")
                    .unwrap_err()
                    .message(),
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
                performance_profile_number(text, "IpiOnPremiseEngine")
                    .unwrap_err()
                    .message(),
                format!(
                    "The performance profile '{number}' is not valid for a \
                     IpiOnPremiseEngine. (Parameter 'profile')"
                ),
                "{text}"
            );
        }
    }
}
