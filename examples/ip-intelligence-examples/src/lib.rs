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
//! Enterprise data file, and [`write_data_file_warnings`], the standard lines
//! about the data file in use that every on-premise example prints.

#![warn(missing_docs)]

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

use fiftyone_ip_intelligence::{IpIntelligenceOnPremiseEngine, PerformanceProfile};

/// Properties the Enterprise data file carries and neither free file does.
/// The on-premise examples ask for these alongside the free properties, and
/// say where to ask about the Enterprise data file when the file in use does
/// not carry them.
pub const ENTERPRISE_PROPERTIES: &[&str] = &[
    "Country",
    "CountryCode",
    "Town",
    "Latitude",
    "Longitude",
    "IsHosted",
    "IsVPN",
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
