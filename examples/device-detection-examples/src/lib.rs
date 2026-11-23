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
//! do not repeat them. Today that is [`profile_line`],
//! [`build_hardware_pipeline`] and [`has_hardware_lookup`], which both the TAC
//! and the native-model cloud examples use, [`print_data_file_warnings`] /
//! [`print_data_file_warnings_for`], the standard data-file introspection the
//! on-premise examples print at the end of a run, and [`available_properties`]
//! and [`data_file_properties`] with [`ENTERPRISE_PROPERTIES`], which the
//! on-premise examples use to say which properties need the Enterprise data
//! file.

#![warn(missing_docs)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};

use fiftyone_device_detection::{
    DeviceData, DeviceDataBase, DeviceDetectionOnPremiseEngineBuilder, FlowElement,
    PerformanceProfile, Pipeline, DEVICE_ELEMENT_DATA_KEY, HARDWARE_ELEMENT_DATA_KEY,
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
/// return. For an engine built without naming properties these are the ones
/// its data file carries. An engine built with named properties lists the
/// names it was given, including any its data file lacks, so use
/// [`data_file_properties`] for one of those.
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
/// from an engine opened over it with the low-memory profile and no named
/// properties. This suits an example whose own engine names its properties,
/// because such an engine lists the names it was given rather than what the
/// file holds.
pub fn data_file_properties(data_file: &Path) -> Result<Vec<String>> {
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(data_file)
        .performance_profile(PerformanceProfile::LowMemory)
        .build()
        .context("failed to open the data file to read its properties")?;
    Ok(engine.available_properties())
}

/// How [`profile_line`] shows a property the cloud did not resolve.
pub const NO_VALUE: &str = "Unknown";

/// Render one matched device profile as a `Vendor Name (Model)` line, reading
/// the hardware properties through the shared typed accessors. A property the
/// cloud did not resolve is shown as [`NO_VALUE`].
///
/// Shared by the TAC and native-model cloud examples: a hardware-profile lookup
/// returns a list of device profiles, and both examples list each one this way.
pub fn profile_line(profile: &DeviceDataBase) -> String {
    let vendor = profile.hardware_vendor();
    let vendor = vendor.value().map(String::as_str).unwrap_or(NO_VALUE);

    let name = profile.hardware_name();
    let name = match name.value() {
        Ok(list) if !list.is_empty() => list.join(", "),
        _ => NO_VALUE.to_owned(),
    };

    let model = profile.hardware_model();
    let model = model.value().map(String::as_str).unwrap_or(NO_VALUE);

    format!("{vendor} {name} ({model})")
}

/// The property the cloud lists under the hardware engine when the resource
/// key includes the hardware lookup, HardwarePlatform.Profiles.
pub const HARDWARE_PROFILES_PROPERTY: &str = "Profiles";

/// The error [`has_hardware_lookup`] gives when the cloud's list of the
/// properties the resource key includes could not be read.
pub const PROPERTY_LIST_UNREADABLE: &str = "the cloud's list of the properties \
    the resource key includes could not be read, because the cloud service \
    could not be reached or did not answer with the list, so check the network \
    connection and the cloud endpoint and run the example again";

/// The hardware engine of a cloud hardware-profile `pipeline`.
fn hardware_engine(pipeline: &Pipeline) -> Result<&Arc<dyn FlowElement>> {
    pipeline
        .flow_elements()
        .iter()
        .find(|element| element.data_key() == HARDWARE_ELEMENT_DATA_KEY)
        .context("the pipeline has no hardware engine")
}

/// Build a cloud hardware-profile pipeline with `build`, and build it once
/// more if the cloud's list of the properties the resource key includes could
/// not be read. A build asks the cloud only for that list and the evidence
/// keys, so no lookup is sent before [`has_hardware_lookup`] decides.
pub fn build_hardware_pipeline(build: impl Fn() -> Result<Arc<Pipeline>>) -> Result<Arc<Pipeline>> {
    let pipeline = build()?;
    if hardware_engine(&pipeline)?.properties_loaded() {
        return Ok(pipeline);
    }
    build()
}

/// Whether the resource key of the cloud hardware-profile `pipeline` includes
/// the hardware lookup, read from the hardware engine's properties before any
/// lookup is made. The cloud lists those properties for the resource key,
/// and they hold [`HARDWARE_PROFILES_PROPERTY`] only when the key includes
/// the lookup.
///
/// When the list could not be read, even by [`build_hardware_pipeline`]'s
/// second build, this fails with [`PROPERTY_LIST_UNREADABLE`] and the cloud's
/// own error, because the answer is then not known. Only processing returns
/// that error, so a flow data with no evidence is processed, which asks for
/// the list a third time and stops when it fails. If that third request
/// succeeds, the cloud request engine also sends one request with no evidence
/// before this decides.
pub fn has_hardware_lookup(pipeline: &Arc<Pipeline>) -> Result<bool> {
    let engine = hardware_engine(pipeline)?;
    if !engine.properties_loaded() {
        let retried = pipeline.create_flow_data().process();
        if !engine.properties_loaded() {
            retried.context(PROPERTY_LIST_UNREADABLE)?;
            bail!(PROPERTY_LIST_UNREADABLE);
        }
    }
    Ok(engine.properties().iter().any(|property| {
        property
            .name
            .eq_ignore_ascii_case(HARDWARE_PROFILES_PROPERTY)
    }))
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
