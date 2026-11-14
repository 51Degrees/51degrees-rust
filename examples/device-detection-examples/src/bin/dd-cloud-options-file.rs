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

//! @page dd-cloud-options-file-example Options File (Device Detection, Cloud)
//!
//! Builds a cloud Device Detection pipeline from an options file, in the
//! shape of the `PipelineOptions` section of a .NET `appsettings.json` file,
//! rather than with the fluent builder the getting-started example uses.
//! See the descriptive block at the bottom of this file for the full
//! write-up.
//!
//! @snippet dd-cloud-options-file.rs example

use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};

use fiftyone_device_detection::configuration::{self, PipelineOptions};
use fiftyone_device_detection::{DeviceData, Evidence, Pipeline, PipelineBuilder, DEVICE_DATA_KEY};
use fiftyone_pipeline_core::configuration::ConfigValue;
use fiftyone_pipeline_engines::AspectPropertyValue;

/// The options file the example reads, beside this crate's manifest.
const OPTIONS_FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/options/dd-cloud-options.json");

/// The name the options file gives the cloud request engine's builder.
const CLOUD_REQUEST_ENGINE_BUILDER: &str = "CloudRequestEngineBuilder";

/// The build parameter that holds the resource key.
const RESOURCE_KEY_PARAMETER: &str = "ResourceKey";

/// The links the example's output ends with.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("Device Detection", "https://51degrees.com/device-detection?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-options-file.rs&utm_term=find-out-more"),
    ("Pricing", "https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-options-file.rs&utm_term=find-out-more"),
    ("Documentation", "https://51degrees.com/documentation/index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-options-file.rs&utm_term=find-out-more"),
];

/// The pricing message printed when one of the requested properties came back
/// without a value, which usually means the resource key does not include that
/// property.
const PRICING_MESSAGE: &str =
    "Some values were not present in the results. This is usually because the \
     resource key in use does not include the property. A resource key that \
     includes all the properties used by this example can be created at \
     https://51degrees.com/configure?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-options-file.rs&utm_term=pricing-message.";

/// Options the example runs with. Keeping the inputs in one struct lets `main`
/// and the test feed the same `run` entry point.
pub struct ExampleOptions {
    /// The options file the pipeline is built from.
    pub options_file: PathBuf,
    /// The cloud resource key, which replaces the one the options file holds.
    pub resource_key: String,
}

// [example]
/// Run the cloud options file example.
///
/// Reads the options file, puts the resource key into the cloud request
/// engine's build parameters, and builds the pipeline from it with the
/// builders the Device Detection crate registers under their .NET names.
/// It then runs a desktop and a mobile User-Agent through the pipeline,
/// printing `IsMobile`, `HardwareVendor`, `BrowserName` and `PlatformName`
/// for each with explicit no-value handling.
pub fn run(options: ExampleOptions) -> Result<()> {
    let text = std::fs::read_to_string(&options.options_file).with_context(|| {
        format!(
            "failed to read the options file {}",
            options.options_file.display()
        )
    })?;
    let mut pipeline_options =
        PipelineOptions::from_json(&text).context("failed to read the pipeline options")?;
    use_resource_key(&mut pipeline_options, &options.resource_key);

    // The registry holds the builders .NET's Device Detection package gives,
    // under .NET's names, so the options file names them as .NET does. The
    // cloud endpoint is left to the cloud request engine, which reads
    // _51DEGREES_CLOUD_ENDPOINT and defaults to the public cloud.
    let pipeline = PipelineBuilder::new()
        .build_from_configuration(&configuration::registry(), &pipeline_options)
        .context("failed to build the pipeline from the options file")?;

    let mut any_value_missing = false;
    for (label, pairs) in [
        (
            "Desktop User-Agent",
            examples_shared::evidence::desktop_user_agent_evidence(),
        ),
        (
            "Mobile User-Agent",
            examples_shared::evidence::mobile_user_agent_evidence(),
        ),
    ] {
        any_value_missing |= analyse_evidence(&pipeline, label, &pairs)?;
    }
    if any_value_missing {
        println!("{PRICING_MESSAGE}");
    }
    Ok(())
}

/// Put `resource_key` into the cloud request engine's build parameters, in
/// place of the one the options file holds.
fn use_resource_key(options: &mut PipelineOptions, resource_key: &str) {
    for element in &mut options.elements {
        let is_cloud_request_engine = element
            .builder_name
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(CLOUD_REQUEST_ENGINE_BUILDER));
        if is_cloud_request_engine {
            element
                .build_parameters
                .retain(|(name, _)| !name.eq_ignore_ascii_case(RESOURCE_KEY_PARAMETER));
            element.build_parameters.push((
                RESOURCE_KEY_PARAMETER.to_owned(),
                ConfigValue::from(resource_key),
            ));
        }
    }
}
// [example]

/// Process one evidence set and print the key device properties. Returns
/// `true` if any of them came back without a value.
fn analyse_evidence(pipeline: &Arc<Pipeline>, label: &str, pairs: &[(&str, &str)]) -> Result<bool> {
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
    data.process().context("cloud pipeline processing failed")?;

    let _ = writeln!(message, "Results:");
    let mut any_value_missing = false;
    match data.get(DEVICE_DATA_KEY) {
        Some(device) => {
            any_value_missing |= output_value("Mobile Device", &device.is_mobile(), &mut message);
            any_value_missing |=
                output_value("Hardware Vendor", &device.hardware_vendor(), &mut message);
            any_value_missing |= output_value("Browser Name", &device.browser_name(), &mut message);
            any_value_missing |=
                output_value("Platform Name", &device.platform_name(), &mut message);
        }
        None => {
            let _ = writeln!(
                message,
                "\tNo device data was produced (the cloud request may have failed)."
            );
            any_value_missing = true;
        }
    }
    let _ = writeln!(message);
    print!("{message}");
    Ok(any_value_missing)
}

/// Append one typed property to the output, or the reason it has no value.
/// Returns `true` when the property has no value.
fn output_value<T: std::fmt::Display>(
    name: &str,
    value: &AspectPropertyValue<T>,
    message: &mut String,
) -> bool {
    match value.value() {
        Ok(value) => {
            let _ = writeln!(message, "\t{name}: {value}");
            false
        }
        Err(_) => {
            let reason = value.no_value_message().unwrap_or("No value");
            let _ = writeln!(message, "\t{name}: {reason}");
            true
        }
    }
}

/// Read the resource key from the command line (first argument) or the
/// environment, then run the example. Without a key the example prints how to
/// get one and exits successfully, so an offline run is not a hard error.
fn main() -> Result<()> {
    let resource_key = std::env::args()
        .nth(1)
        .or_else(examples_shared::resource_key_from_env);

    let Some(resource_key) = resource_key else {
        eprintln!(
            "No resource key available. Set the _51DEGREES_RESOURCE_KEY environment \
             variable (or pass the key as the first argument). The 51Degrees cloud \
             service is accessed using a resource key, which you can create for free \
             at https://51degrees.com/configure?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-options-file.rs&utm_term=resource-key-required."
        );
        examples_shared::print_find_out_more(FIND_OUT_MORE, examples_shared::CLOUD_REPOSITORIES);
        return Ok(());
    };

    let result = run(ExampleOptions {
        options_file: PathBuf::from(OPTIONS_FILE),
        resource_key,
    });
    examples_shared::print_find_out_more(FIND_OUT_MORE, examples_shared::CLOUD_REPOSITORIES);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run the example end to end against the live cloud. Ignored by default
    /// so a plain `cargo test` stays green offline, and skipped when no
    /// resource key is set.
    #[test]
    #[ignore = "requires a live cloud resource key (set _51DEGREES_RESOURCE_KEY)"]
    fn runs_against_the_cloud() {
        let Some(resource_key) = examples_shared::resource_key_from_env() else {
            eprintln!("skipping: no resource key set");
            return;
        };
        run(ExampleOptions {
            options_file: PathBuf::from(OPTIONS_FILE),
            resource_key,
        })
        .expect("the cloud options file example should complete");
    }

    /// The options file names the cloud request engine and the Device
    /// Detection cloud engine, and its resource key is replaced by the one
    /// passed in.
    #[test]
    fn the_options_file_names_the_cloud_engines_and_its_key_is_replaced() {
        let text = std::fs::read_to_string(OPTIONS_FILE).expect("the options file");
        let mut options = PipelineOptions::from_json(&text).expect("the options");
        use_resource_key(&mut options, "the-key");

        let builders: Vec<Option<&str>> = options
            .elements
            .iter()
            .map(|element| element.builder_name.as_deref())
            .collect();
        assert_eq!(
            builders,
            [
                Some(CLOUD_REQUEST_ENGINE_BUILDER),
                Some("DeviceDetectionCloudEngineBuilder")
            ]
        );
        let keys: Vec<&ConfigValue> = options.elements[0]
            .build_parameters
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case(RESOURCE_KEY_PARAMETER))
            .map(|(_, value)| value)
            .collect();
        assert_eq!(
            keys,
            [&ConfigValue::from("the-key")],
            "one ResourceKey, the one passed in"
        );
    }
}

/*
 * @example dd-cloud-options-file.rs
 *
 * The device-detection cloud options file console example. It builds the same
 * kind of pipeline as dd-cloud-getting-started, but from an options file
 * rather than with the fluent builder.
 *
 * The options file, options/dd-cloud-options.json beside this crate's
 * manifest, has the shape of the PipelineOptions section of a .NET
 * appsettings.json file. It names the cloud request engine's builder,
 * CloudRequestEngineBuilder, with a resource key, then the Device Detection
 * cloud engine's builder, DeviceDetectionCloudEngineBuilder, and turns usage
 * sharing off. The example reads it with PipelineOptions::from_json, puts the
 * resource key it was given into the cloud request engine's build parameters,
 * and builds the pipeline with PipelineBuilder::build_from_configuration and
 * the registry the Device Detection crate gives, which holds its builders
 * under their .NET names.
 *
 * For a desktop and a mobile User-Agent the example reads IsMobile,
 * HardwareVendor, BrowserName and PlatformName. If any property comes back
 * without a value, usually because the resource key does not include it, a
 * pricing message points to the configurator where a key with more
 * properties can be created.
 *
 * The resource key is read from the first command-line argument or the
 * `_51DEGREES_RESOURCE_KEY` environment variable. Without one the example
 * says how to create a key, prints its "Find out more" section and exits
 * successfully. Create a free key, or a paid key with the full property set,
 * at https://51degrees.com/configure?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-options-file.rs&utm_term=dd-cloud-options-file.
 */
