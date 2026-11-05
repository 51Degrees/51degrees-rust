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

//! @page dd-cloud-getting-started-example Getting Started (Device Detection, Cloud)
//!
//! Cloud getting-started console example. See the descriptive block at the
//! bottom of this file for the full write-up.
//!
//! @snippet dd-cloud-getting-started.rs example

use std::fmt::Write as _;
use std::sync::Arc;

use anyhow::{Context, Result};

use fiftyone_device_detection::{
    DeviceData, DeviceDetectionPipelineBuilder, Evidence, Pipeline, DEVICE_DATA_KEY,
};
use fiftyone_pipeline_engines::AspectPropertyValue;

/// The links the example's output ends with.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("Device Detection", "https://51degrees.com/device-detection?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-getting-started.rs&utm_term=find-out-more"),
    ("Pricing", "https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-getting-started.rs&utm_term=find-out-more"),
    ("Documentation", "https://51degrees.com/documentation/index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-getting-started.rs&utm_term=find-out-more"),
];

/// The pricing message printed when one of the requested properties came back
/// without a value, which usually means the resource key does not include that
/// property.
const PRICING_MESSAGE: &str =
    "Some values were not present in the results. This is usually because the \
     resource key in use does not include the property. A resource key that \
     includes all the properties used by this example can be created at \
     https://configure.51degrees.com?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-getting-started.rs&utm_term=pricing-message.";

/// Options the example runs with. Keeping the inputs in one struct lets `main`
/// and the test feed the same `run` entry point.
pub struct ExampleOptions {
    /// The cloud resource key the pipeline authenticates with.
    pub resource_key: String,
    /// An optional override for the cloud endpoint, read from
    /// `51DEGREES_CLOUD_ENDPOINT`. `None` leaves the choice to the cloud request
    /// engine, which reads the same variable and defaults to the public cloud.
    pub endpoint: Option<String>,
}

// [example]
/// Run the cloud getting-started example.
///
/// Builds a cloud device-detection pipeline from the resource key, then runs the
/// representative sample evidence (a desktop and a mobile User-Agent, a full UACH
/// header set, and a high-entropy client-hints blob) through it, printing the
/// strongly-typed `IsMobile`, `HardwareVendor`, `BrowserName` and `PlatformName`
/// results for each with explicit no-value handling.
pub fn run(options: ExampleOptions) -> Result<()> {
    // Build the cloud pipeline. Usage sharing is off because this is a console
    // example, whose evidence 51Degrees already holds. In normal operation
    // leave it on, which is the builder's default.
    let mut builder = DeviceDetectionPipelineBuilder::cloud(options.resource_key);
    if let Some(endpoint) = options.endpoint {
        builder = builder.endpoint(endpoint);
    }
    let pipeline = builder
        .share_usage(false)
        .build()
        .context("failed to build the cloud device-detection pipeline")?;

    // Track whether any requested property came back without a value, so the
    // pricing message can be shown once at the end.
    let mut any_value_missing = false;

    // Run each representative evidence set through the pipeline. The shared
    // sample set covers the desktop, mobile, UACH and high-entropy paths.
    for (label, pairs) in examples_shared::evidence::all_sample_evidence() {
        // The IP-address sample sets are for IP Intelligence, not device
        // detection, so they are skipped here.
        if pairs.iter().all(|(key, _)| key.contains("client-ip")) {
            continue;
        }
        any_value_missing |= analyse_evidence(&pipeline, label, &pairs)?;
    }

    if any_value_missing {
        println!("{PRICING_MESSAGE}");
    }

    Ok(())
}
// [example]

/// Process one evidence set and print the key device properties.
///
/// Returns `true` if any of the four properties came back without a value, which
/// the caller accumulates to decide whether to print the pricing message.
fn analyse_evidence(pipeline: &Arc<Pipeline>, label: &str, pairs: &[(&str, &str)]) -> Result<bool> {
    // Echo the inputs so the output is self-explanatory.
    let mut message = String::new();
    let _ = writeln!(message, "--- {label} ---");
    let _ = writeln!(message, "Input values:");
    for (key, value) in pairs {
        let _ = writeln!(message, "\t{key}: {value}");
    }
    print!("{message}");

    // FlowData carries the evidence in and the results back out. Build it with
    // the evidence for this scenario, then process it through the pipeline.
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add_all(pairs.iter().map(|(k, v)| (*k, *v)))
            .build(),
    );
    data.process().context("cloud pipeline processing failed")?;

    // Read the device result back through the shared key. A cloud failure that
    // was suppressed would leave the device data absent, so handle that case
    // rather than unwrapping.
    let mut message = String::new();
    let _ = writeln!(message, "Results:");
    let mut any_value_missing = false;
    match data.get(DEVICE_DATA_KEY) {
        Some(device) => {
            // Each accessor returns an AspectPropertyValue, the no-value-aware
            // wrapper the typed getters use. `output_value` prints either the
            // value or the no-value explanation.
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

/// Append one typed property to the output, handling the no-value case.
///
/// An [`AspectPropertyValue`] behaves like a nullable type: accessing the value
/// of a no-value instance is an error, so the no-value message is printed
/// instead. Returns `true` when the property has no value.
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
/// environment, then run the example. A cloud example without a key prints a
/// clear message and exits successfully so an offline run is not a hard error.
fn main() -> Result<()> {
    let resource_key = std::env::args()
        .nth(1)
        .or_else(examples_shared::resource_key_from_env);

    let Some(resource_key) = resource_key else {
        eprintln!(
            "No resource key available. Set the 51DEGREES_RESOURCE_KEY environment \
             variable (or pass the key as the first argument). The 51Degrees cloud \
             service is accessed using a resource key, which you can create for free \
             at https://configure.51degrees.com?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-getting-started.rs&utm_term=resource-key-required."
        );
        examples_shared::print_find_out_more(FIND_OUT_MORE, examples_shared::CLOUD_REPOSITORIES);
        return Ok(());
    };

    let result = run(ExampleOptions {
        resource_key,
        // The cloud endpoint. Unset, the cloud request engine reads
        // 51DEGREES_CLOUD_ENDPOINT itself, and it is passed here as well so the
        // choice is visible. A host other than cloud.51degrees.com is an on
        // premise web server or a privately hosted 51Degrees cloud (see
        // examples_shared::CLOUD_ENDPOINT_ENV_VAR).
        endpoint: examples_shared::cloud_endpoint_from_env(),
    });
    examples_shared::print_find_out_more(FIND_OUT_MORE, examples_shared::CLOUD_REPOSITORIES);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run the example end to end against the live cloud.
    ///
    /// This is ignored by default so a plain `cargo test` stays green offline.
    /// It is skipped (not failed) when no resource key is configured, and runs
    /// for real when one is present and the test is invoked with `--ignored`.
    #[test]
    #[ignore = "requires a live cloud resource key (set 51DEGREES_RESOURCE_KEY)"]
    fn runs_against_the_cloud() {
        let Some(resource_key) = examples_shared::resource_key_from_env() else {
            eprintln!("skipping: no resource key set");
            return;
        };
        run(ExampleOptions {
            resource_key,
            endpoint: examples_shared::cloud_endpoint_from_env(),
        })
        .expect("the cloud getting-started example should complete");
    }
}

/*
 * @example dd-cloud-getting-started.rs
 *
 * The device-detection cloud getting-started console example. It shows the
 * simplest end-to-end use of the 51Degrees cloud service for device detection.
 *
 * The example builds a cloud pipeline from a resource key with the convenience
 * `DeviceDetectionPipelineBuilder::cloud(resource_key)`. The builder assembles a
 * `CloudRequestEngine` (which makes one HTTP call per flow data to the 51Degrees
 * cloud) followed by a `DeviceDetectionCloudEngine` (which turns the cloud's JSON
 * response into the shared `DeviceData` result). Because the result type and key
 * are identical to the on-premise engine, the result-reading code below is the
 * same whichever deployment produced it.
 *
 * For each representative evidence set (a desktop User-Agent, a mobile
 * User-Agent, a full User-Agent Client Hints header set, and a base64
 * high-entropy `getHighEntropyValues` blob) the example creates a `FlowData`,
 * adds the evidence, processes it, then reads four common device properties back
 * through their strongly-typed accessors:
 *
 * - `IsMobile` (a boolean),
 * - `HardwareVendor` (a string),
 * - `BrowserName` (a string),
 * - `PlatformName` (a string).
 *
 * Each accessor returns an `AspectPropertyValue<T>`, the no-value-aware wrapper
 * the typed getters use. Rather than assuming a value is present, the example
 * prints the value when one is available and the engine's no-value explanation
 * when it is not. If any property is left without a value (usually because the
 * resource key does not grant it) a pricing message is printed once, pointing to
 * the configurator where a key with more properties can be created.
 *
 * Usage sharing is turned off with `.share_usage(false)`, because this is a
 * console example. In normal operation leave it on, which is the builder's
 * default, so 51Degrees receives evidence that improves detection.
 *
 * The resource key is read from the first command-line argument or the
 * `51DEGREES_RESOURCE_KEY` environment variable. Create a free key, or a paid key
 * with the full property set, at https://configure.51degrees.com?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-getting-started.rs&utm_term=dd-cloud-getting-started.
 */
