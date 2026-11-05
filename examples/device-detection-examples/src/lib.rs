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

//! Runnable device-detection examples, plus the helpers shared between them.
//!
//! The example programs live under `src/bin`, each a self-contained binary. This
//! library holds the small pieces more than one of them needs, so the binaries
//! do not repeat them. Today that is [`profile_line`], the hardware-profile line
//! renderer used by both the TAC and the native-model cloud examples,
//! [`print_data_file_warnings`] / [`print_data_file_warnings_for`], the standard
//! data-file introspection the on-premise examples print at the end of a run,
//! and [`available_properties`] and [`data_file_properties`] with
//! [`ENTERPRISE_PROPERTIES`], which the on-premise examples use to say which
//! properties need the Enterprise data file.

#![warn(missing_docs)]

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use fiftyone_device_detection::{
    DeviceData, DeviceDataBase, DeviceDetectionOnPremiseEngineBuilder, FlowElement,
    PerformanceProfile, Pipeline, DEVICE_ELEMENT_DATA_KEY,
};
use fiftyone_pipeline_engines::OnPremiseAspectEngine;

/// Properties the Enterprise data file carries and the free Lite file does
/// not. The on-premise examples ask for these alongside the Lite properties,
/// and say where to ask about the Enterprise data file when the file in use
/// does not carry them.
pub const ENTERPRISE_PROPERTIES: &[&str] = &[
    "HardwareVendor",
    "HardwareModel",
    "DeviceType",
    "PriceBand",
    "ScreenInchesDiagonal",
];

/// The names of the properties the Device Detection engine in `pipeline` can
/// return, which are those its data file carries.
pub fn available_properties(pipeline: &Pipeline) -> Vec<String> {
    pipeline
        .flow_elements()
        .iter()
        .filter(|element| element.data_key() == DEVICE_ELEMENT_DATA_KEY)
        .flat_map(|element| element.properties().iter())
        .map(|property| property.name.clone())
        .collect()
}

/// The names of the properties the data file at `data_file` carries, read
/// from an engine opened over it with the low-memory profile. This suits an
/// example whose own engine asks for fewer properties than the file carries,
/// so its pipeline cannot say what the file holds.
pub fn data_file_properties(data_file: &Path) -> Result<Vec<String>> {
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(data_file)
        .performance_profile(PerformanceProfile::LowMemory)
        .build()
        .context("failed to open the data file to read its properties")?;
    Ok(engine.available_properties())
}

/// Render one matched device profile as a `Vendor Name (Model)` line, reading
/// the hardware properties through the shared typed accessors. A property the
/// cloud did not resolve is shown as `Unknown`.
///
/// Shared by the TAC and native-model cloud examples: a hardware-profile lookup
/// returns a list of device profiles, and both examples list each one this way.
pub fn profile_line(profile: &DeviceDataBase) -> String {
    let vendor = profile.hardware_vendor();
    let vendor = vendor.value().map(String::as_str).unwrap_or("Unknown");

    let name = profile.hardware_name();
    let name = match name.value() {
        Ok(list) if !list.is_empty() => list.join(", "),
        _ => "Unknown".to_owned(),
    };

    let model = profile.hardware_model();
    let model = model.value().map(String::as_str).unwrap_or("Unknown");

    format!("{vendor} {name} ({model})")
}

/// Build a sibling on-premise engine over `data_file`, then print the standard
/// data-file introspection: the tier and publish date line followed by the
/// Lite-tier and out-of-date warnings.
///
/// The on-premise examples run detection through a pipeline, but the pipeline
/// does not hand back the concrete engine, and the introspection helpers in
/// [`examples_shared`] take a `&dyn OnPremiseAspectEngine`. Building a throwaway
/// engine over the same file with the same [`PerformanceProfile`] is the simplest
/// way to read the data-file metadata, so each example calls this once at the end
/// of its run. An example that already holds an engine should call
/// [`print_data_file_warnings_for`] instead, avoiding the second build.
pub fn print_data_file_warnings(data_file: &PathBuf, profile: PerformanceProfile) -> Result<()> {
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(data_file)
        .performance_profile(profile)
        .build()
        .context("failed to build an engine for data-file introspection")?;

    print_data_file_warnings_for(engine.as_ref());
    Ok(())
}

/// Print the standard data-file introspection for an engine the caller already
/// holds: the tier and publish date line followed by the Lite-tier and
/// out-of-date warnings.
///
/// This is the half of [`print_data_file_warnings`] that does not build an
/// engine, factored out so the metadata and update-data-file examples, which have
/// already built an engine of their own, can print the same lines without a
/// second build.
pub fn print_data_file_warnings_for(engine: &dyn OnPremiseAspectEngine) {
    println!("{}", examples_shared::data_file_info(engine));
    for warning in examples_shared::check_data_file(engine) {
        println!("WARNING: {warning}");
    }
}
