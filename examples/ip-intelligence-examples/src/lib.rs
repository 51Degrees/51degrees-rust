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

//! Runnable IP-intelligence examples, plus the helpers shared between them.
//!
//! The example programs live under `src/bin`, each a self-contained binary.
//! This library holds [`data_file_properties`] and [`ENTERPRISE_PROPERTIES`],
//! which the on-premise examples use to say which properties need the
//! Enterprise data file, [`write_data_file_warnings`], the standard lines
//! about the data file in use that every on-premise example prints, and
//! [`profile_arguments`], which reads the data-file path and the
//! [`PROFILE_FLAG`] of the examples that let the person running them choose a
//! faster profile.

#![warn(missing_docs)]

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use fiftyone_ip_intelligence::{IpIntelligenceOnPremiseEngine, PerformanceProfile};

/// Properties the Enterprise data file carries and neither free file does,
/// in the order the examples show them. The on-premise examples ask for
/// these alongside the free properties, and say where to ask about the
/// Enterprise data file when the file in use does not carry them.
pub const ENTERPRISE_PROPERTIES: &[&str] = &[
    "IpRangeStart",
    "IpRangeEnd",
    "Country",
    "CountryCode",
    "CountryCode3",
    "Region",
    "State",
    "Town",
    "Latitude",
    "Longitude",
    "AccuracyRadiusMin",
    "TimeZoneOffset",
    "ConnectionType",
    "IsHosted",
    "IsVPN",
    "IsProxy",
    "IsCellular",
    "IsEu",
    "CountryCodesGeographical",
    "CountryCodesPopulation",
    "Mcc",
];

/// The names of the properties the data file at `data_file` carries.
///
/// This opens a second engine over the same file with the low-memory profile,
/// which leaves the data on disk, and no named properties, and asks it. The
/// pipeline builder does not hand back its engine, and an engine built with
/// named properties lists the names it was given, including any the file
/// lacks.
pub fn data_file_properties(data_file: &Path) -> Result<Vec<String>> {
    Ok(open_for_inspection(data_file)?.available_property_names())
}

/// Write the standard lines about the data file at `data_file`, being its tier
/// and publish date, then any warning that it is free data or more than 30 days
/// old. Opens a second engine over the file, as [`data_file_properties`] does.
pub fn write_data_file_warnings(data_file: &Path, out: &mut dyn Write) -> Result<()> {
    let engine = open_for_inspection(data_file)?;
    writeln!(out, "{}", examples_shared::data_file_info(&engine))?;
    for warning in examples_shared::check_data_file(&engine) {
        writeln!(out, "WARNING: {warning}")?;
    }
    Ok(())
}

/// An engine over `data_file` for reading what the file holds, with the
/// low-memory profile and no updates.
fn open_for_inspection(data_file: &Path) -> Result<IpIntelligenceOnPremiseEngine> {
    IpIntelligenceOnPremiseEngine::builder(data_file)
        .performance_profile(PerformanceProfile::LowMemory)
        .auto_update(false)
        .file_system_watcher(false)
        .build()
        .context("failed to open the data file to read its properties")
}

/// The flag that chooses the performance profile of an example that offers
/// one, as `--profile <name>` or `--profile=<name>`.
pub const PROFILE_FLAG: &str = "--profile";

/// The command-line arguments of an on-premise example that takes a data-file
/// path and a performance profile.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ProfileArguments {
    /// The first argument that is not [`PROFILE_FLAG`] or its value, being the
    /// path of the data file to open.
    pub data_file: Option<PathBuf>,
    /// The profile [`PROFILE_FLAG`] names, when it is given.
    pub profile: Option<PerformanceProfile>,
}

/// Read the arguments that follow the program name into [`ProfileArguments`].
///
/// The name is read by [`PerformanceProfile::parse`], so `HighPerformance` and
/// `high-performance` name the same profile. Fails when [`PROFILE_FLAG`] has no
/// value or its value names no profile.
pub fn profile_arguments<Arguments, Argument>(arguments: Arguments) -> Result<ProfileArguments>
where
    Arguments: IntoIterator<Item = Argument>,
    Argument: AsRef<str>,
{
    let mut parsed = ProfileArguments::default();
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        let argument = argument.as_ref();
        let name = if argument == PROFILE_FLAG {
            arguments
                .next()
                .map(|name| name.as_ref().to_owned())
                .with_context(|| format!("{PROFILE_FLAG} needs a profile name"))?
        } else if let Some(name) = argument
            .strip_prefix(PROFILE_FLAG)
            .and_then(|rest| rest.strip_prefix('='))
        {
            name.to_owned()
        } else {
            if parsed.data_file.is_none() {
                parsed.data_file = Some(PathBuf::from(argument));
            }
            continue;
        };
        let profile = PerformanceProfile::parse(&name).with_context(|| {
            format!(
                "{name} is not a performance profile. Use InMemory, \
                 HighPerformance, LowMemory, Balanced or Default."
            )
        })?;
        parsed.profile = Some(profile);
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_profile_flag_in_both_spellings() {
        let spaced = profile_arguments(["--profile", "HighPerformance"])
            .expect("a known profile should be read");
        assert_eq!(spaced.profile, Some(PerformanceProfile::HighPerformance));
        assert_eq!(spaced.data_file, None);

        let joined = profile_arguments(["--profile=high-performance"])
            .expect("a known profile should be read");
        assert_eq!(joined.profile, Some(PerformanceProfile::HighPerformance));
    }

    #[test]
    fn takes_the_data_file_from_either_side_of_the_flag() {
        for arguments in [
            ["data.ipi", "--profile", "InMemory"],
            ["--profile", "InMemory", "data.ipi"],
        ] {
            let parsed = profile_arguments(arguments).expect("the arguments should be read");
            assert_eq!(parsed.data_file, Some(PathBuf::from("data.ipi")));
            assert_eq!(parsed.profile, Some(PerformanceProfile::InMemory));
        }
    }

    #[test]
    fn no_arguments_give_no_file_and_no_profile() {
        let parsed = profile_arguments(Vec::<String>::new()).expect("no arguments are valid");
        assert_eq!(parsed, ProfileArguments::default());
    }

    #[test]
    fn refuses_a_missing_or_unknown_profile() {
        assert!(profile_arguments(["--profile"]).is_err());
        assert!(profile_arguments(["--profile", "Fastest"]).is_err());
    }
}
