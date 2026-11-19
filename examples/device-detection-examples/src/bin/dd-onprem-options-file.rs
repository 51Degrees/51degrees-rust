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

//! @page dd-onprem-options-file-example Options File (Device Detection, On-premise)
//!
//! Builds an on-premise Device Detection pipeline from an options file, in
//! the shape of the `PipelineOptions` section of a .NET `appsettings.json`
//! file, rather than with the fluent builder the getting-started example
//! uses. It then reads device properties back from processed User-Agent
//! evidence, along with properties only the Enterprise data file carries.
//! The core of the example:
//!
//! @snippet dd-onprem-options-file.rs example

use std::fmt::{Display, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};

use fiftyone_device_detection::configuration::{self, PipelineOptions};
use fiftyone_device_detection::{
    DeviceData, Evidence, PerformanceProfile, Pipeline, PipelineBuilder, DEVICE_DATA_KEY,
};
use fiftyone_pipeline_core::configuration::ConfigValue;
use fiftyone_pipeline_engines::AspectPropertyValue;

use device_detection_examples::ENTERPRISE_PROPERTIES;

/// The options file the example reads, beside this crate's manifest.
const OPTIONS_FILE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/options/dd-onprem-options.json"
);

/// The name the options file gives the Hash engine's builder.
const HASH_ENGINE_BUILDER: &str = "DeviceDetectionHashEngineBuilder";

/// The build parameter that names the data file.
const DATA_FILE_PARAMETER: &str = "DataFile";

/// The performance profile the options file gives, used again for the data
/// file warnings.
const PROFILE: PerformanceProfile = PerformanceProfile::LowMemory;

/// Where to ask about a copy of the Enterprise data file.
const CONTACT_US_URL: &str = "https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-onprem-options-file.rs&utm_term=enterprise-data-file";

/// The links the example's output ends with.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("Device Detection", "https://51degrees.com/device-detection?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-onprem-options-file.rs&utm_term=find-out-more"),
    ("Pricing", "https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-onprem-options-file.rs&utm_term=find-out-more"),
    ("Documentation", "https://51degrees.com/documentation/index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-onprem-options-file.rs&utm_term=find-out-more"),
];

/// Options the example runs with. Holding the inputs in one struct lets `main`
/// and the test feed the same `run` entry point.
pub struct ExampleOptions {
    /// The options file the pipeline is built from.
    pub options_file: PathBuf,
    /// The path to the on-premise Hash data file, which replaces the one the
    /// options file names.
    pub data_file: PathBuf,
}

// [example]
/// Run the on-premise options file example.
///
/// Reads the options file, points its `DataFile` at the data file found on
/// this machine, and builds the pipeline from it with the builders the
/// Device Detection crate registers under their .NET names. It then runs a
/// desktop and a mobile User-Agent through the pipeline, printing device
/// properties for each, then the Enterprise properties. Finishes by printing
/// the data file warnings, the Enterprise properties the data file does not
/// carry with where to ask about a copy, and the "Find out more" links.
pub fn run(options: ExampleOptions) -> Result<()> {
    let text = std::fs::read_to_string(&options.options_file).with_context(|| {
        format!(
            "failed to read the options file {}",
            options.options_file.display()
        )
    })?;
    let mut pipeline_options =
        PipelineOptions::from_json(&text).context("failed to read the pipeline options")?;
    use_data_file(&mut pipeline_options, &options.data_file);

    // The registry holds the builders .NET's Device Detection package gives,
    // under .NET's names, so the options file names them as .NET does.
    let pipeline = PipelineBuilder::new()
        .build_from_configuration(&configuration::registry(), &pipeline_options)
        .context("failed to build the pipeline from the options file")?;

    let available = device_detection_examples::available_properties(&pipeline);
    for (label, pairs) in representative_evidence() {
        analyse_evidence(&pipeline, label, &pairs, &available)?;
    }

    device_detection_examples::print_data_file_warnings(&options.data_file, PROFILE)?;

    let missing = examples_shared::missing_properties(ENTERPRISE_PROPERTIES, &available);
    if let Some(message) = examples_shared::enterprise_data_file_message(&missing, CONTACT_US_URL) {
        println!();
        println!("{message}");
    }

    examples_shared::print_find_out_more(FIND_OUT_MORE, examples_shared::ON_PREMISE_REPOSITORIES);
    Ok(())
}

/// Point the Hash engine's `DataFile` at `data_file`, so the options file
/// works wherever the data file was found or downloaded.
fn use_data_file(options: &mut PipelineOptions, data_file: &Path) {
    for element in &mut options.elements {
        let is_hash_engine = element
            .builder_name
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(HASH_ENGINE_BUILDER));
        if is_hash_engine {
            element
                .build_parameters
                .retain(|(name, _)| !name.eq_ignore_ascii_case(DATA_FILE_PARAMETER));
            element.build_parameters.push((
                DATA_FILE_PARAMETER.to_owned(),
                ConfigValue::from(data_file.display().to_string()),
            ));
        }
    }
}
// [example]

/// The representative evidence the example processes: a desktop and a mobile
/// User-Agent, each as a labelled `(key, value)` pair list.
fn representative_evidence() -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    vec![
        (
            "Desktop User-Agent",
            examples_shared::evidence::desktop_user_agent_evidence(),
        ),
        (
            "Mobile User-Agent",
            examples_shared::evidence::mobile_user_agent_evidence(),
        ),
    ]
}

/// Process one evidence set and print the key device properties. `available`
/// holds the properties the data file carries.
fn analyse_evidence(
    pipeline: &Arc<Pipeline>,
    label: &str,
    pairs: &[(&str, &str)],
    available: &[String],
) -> Result<()> {
    let mut message = String::new();
    let _ = writeln!(message, "--- {label} ---");
    let _ = writeln!(message, "Input values:");
    for (key, value) in pairs {
        let _ = writeln!(message, "\t{key}: {value}");
    }

    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add_all(pairs.iter().map(|(k, v)| (*k, *v)))
            .build(),
    );
    data.process().context("pipeline processing failed")?;

    let _ = writeln!(message, "Results:");
    let device = data
        .get(DEVICE_DATA_KEY)
        .context("the Hash engine should have produced device data")?;
    output_value("Mobile Device", &device.is_mobile(), &mut message);
    output_value("Platform Name", &device.platform_name(), &mut message);
    output_value("Browser Name", &device.browser_name(), &mut message);
    output_value("Device Id", &device.device_id(), &mut message);

    // Properties only the Enterprise data file carries. With the free Lite file
    // each one says so.
    let enterprise = [
        (
            "Hardware Vendor",
            "HardwareVendor",
            device.hardware_vendor(),
        ),
        ("Hardware Model", "HardwareModel", device.hardware_model()),
        ("Device Type", "DeviceType", device.device_type()),
    ];
    for (name, property, value) in &enterprise {
        output_enterprise(name, property, value, available, &mut message);
    }
    let _ = writeln!(message);
    print!("{message}");
    Ok(())
}

/// Append one typed property to the output, or the reason it has no value.
fn output_value<T: Display>(name: &str, value: &AspectPropertyValue<T>, message: &mut String) {
    match value.value() {
        Ok(value) => {
            let _ = writeln!(message, "\t{name}: {value}");
        }
        Err(_) => {
            let reason = value.no_value_message().unwrap_or("No value");
            let _ = writeln!(message, "\t{name}: {reason}");
        }
    }
}

/// Append a property only the Enterprise data file carries, or say that it
/// needs the Enterprise data file when the data file in use does not carry it.
fn output_enterprise<T: Display>(
    name: &str,
    property: &str,
    value: &AspectPropertyValue<T>,
    available: &[String],
    message: &mut String,
) {
    if examples_shared::is_available(property, available) {
        output_value(name, value, message);
    } else {
        let _ = writeln!(
            message,
            "\t{name}: {}",
            examples_shared::NEEDS_ENTERPRISE_DATA_FILE
        );
    }
}

/// Resolve the data file then run the example. The data file is taken from the
/// first command-line argument, the `_51DEGREES_DD_PATH` environment variable, or
/// the Lite Hash file in the data repository inside the `cxx` submodule, which
/// is downloaded when it is missing.
fn main() -> Result<()> {
    let data_file = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .or_else(examples_shared::dd_data_path);

    let Some(data_file) = data_file else {
        anyhow::bail!(
            "No device-detection data file found. Set _51DEGREES_DD_PATH, or pass \
             the path as the first argument."
        );
    };

    run(ExampleOptions {
        options_file: PathBuf::from(OPTIONS_FILE),
        data_file,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run the example end to end against the Lite Hash data file, skipped
    /// only when no data file can be located.
    #[test]
    fn runs_against_the_lite_data_file() {
        let Some(data_file) = examples_shared::dd_data_path() else {
            eprintln!("skipping: no on-premise data file found (set _51DEGREES_DD_PATH)");
            return;
        };
        run(ExampleOptions {
            options_file: PathBuf::from(OPTIONS_FILE),
            data_file,
        })
        .expect("the on-premise options file example should complete");
    }

    /// The options file builds the Hash engine with the settings it gives,
    /// and its `DataFile` is replaced by the one passed in.
    #[test]
    fn the_options_file_names_the_hash_engine_and_its_data_file_is_replaced() {
        let text = std::fs::read_to_string(OPTIONS_FILE).expect("the options file");
        let mut options = PipelineOptions::from_json(&text).expect("the options");
        use_data_file(&mut options, Path::new("elsewhere.hash"));

        assert_eq!(options.elements.len(), 1);
        let element = &options.elements[0];
        assert_eq!(element.builder_name.as_deref(), Some(HASH_ENGINE_BUILDER));
        let data_files: Vec<&ConfigValue> = element
            .build_parameters
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case(DATA_FILE_PARAMETER))
            .map(|(_, value)| value)
            .collect();
        assert_eq!(
            data_files,
            [&ConfigValue::from("elsewhere.hash")],
            "one DataFile, the one passed in"
        );
    }
}

/*
 * @example dd-onprem-options-file.rs
 *
 * The device-detection on-premise options file console example. It builds the
 * same kind of pipeline as dd-onprem-getting-started, but from an options file
 * rather than with the fluent builder.
 *
 * The options file, options/dd-onprem-options.json beside this crate's
 * manifest, has the shape of the PipelineOptions section of a .NET
 * appsettings.json file. It names the Hash engine's builder,
 * DeviceDetectionHashEngineBuilder, with the data file, the low-memory
 * performance profile and automatic updates off, and turns usage sharing off.
 * The example reads it with PipelineOptions::from_json, points DataFile at the
 * data file it found, and builds the pipeline with
 * PipelineBuilder::build_from_configuration and the registry the Device
 * Detection crate gives, which holds its builders under their .NET names.
 *
 * For a desktop and a mobile User-Agent the example then reads IsMobile,
 * PlatformName, BrowserName and DeviceId, and the properties only the
 * Enterprise data file carries, HardwareVendor, HardwareModel and DeviceType.
 * With the free Lite file each of these prints "needs the Enterprise data
 * file", and the run ends by naming them with a link to contact 51Degrees
 * about a copy of the Enterprise data file, then a "Find out more" section.
 *
 * The data file is read from the first command-line argument, the
 * `_51DEGREES_DD_PATH` environment variable, or the Lite Hash file in the data
 * repository inside the cxx submodule, which the example downloads when it is
 * missing. The Enterprise file with the full property set is described at
 * https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-onprem-options-file.rs&utm_term=dd-onprem-options-file.
 */
