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

//! @page dd-cloud-tac-example TAC (Device Detection, Cloud)
//!
//! Cloud TAC-lookup console example. See the descriptive block at the bottom of
//! this file for the full write-up.
//!
//! @snippet dd-cloud-tac.rs example

use anyhow::{Context, Result};

use device_detection_examples::{build_hardware_pipeline, has_hardware_lookup, profile_line};
use fiftyone_device_detection::{DeviceDetectionPipelineBuilder, Evidence, HARDWARE_DATA_KEY};

/// The line the example prints in place of the lookups when the resource key
/// does not include the hardware lookup.
const NO_HARDWARE_LOOKUP: &str = "The resource key does not include \
    HardwarePlatform.Profiles, the hardware lookup this example needs, so no \
    devices are listed. Create a resource key that includes it at \
    https://51degrees.com/configure/QKyYH5XT?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-tac.rs&utm_term=no-hardware-lookup, \
    or contact us about it at \
    https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-tac.rs&utm_term=no-hardware-lookup.";

/// The links the example's output ends with.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("Device Detection", "https://51degrees.com/device-detection?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-tac.rs&utm_term=find-out-more"),
    ("Pricing", "https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-tac.rs&utm_term=find-out-more"),
    ("Documentation", "https://51degrees.com/documentation/index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-tac.rs&utm_term=find-out-more"),
];

/// The cloud evidence key a Type Allocation Code is supplied under, `query.tac`.
/// TAC is an input-only value, supplied as evidence and never returned in the
/// response.
const EVIDENCE_QUERY_TAC_KEY: &str = "query.tac";

/// The two sample TACs the example looks up.
const SAMPLE_TACS: [&str; 2] = ["35925406", "86386802"];

/// Options the example runs with.
pub struct ExampleOptions {
    /// The cloud resource key. TAC lookup needs a key that includes
    /// HardwarePlatform.Profiles.
    pub resource_key: String,
    /// An optional override for the cloud endpoint.
    pub endpoint: Option<String>,
    /// The TACs to look up. Defaults to [`SAMPLE_TACS`].
    pub tacs: Vec<String>,
}

impl ExampleOptions {
    /// Default options for a resource key: the two sample TACs and the cloud
    /// endpoint from the environment, or the public cloud when unset.
    pub fn with_resource_key(resource_key: String) -> Self {
        ExampleOptions {
            resource_key,
            // The cloud endpoint. Unset, the cloud request engine reads
            // _51DEGREES_CLOUD_ENDPOINT itself, and it is passed here as well so the
            // choice is visible. A host other than cloud.51degrees.com is an on
            // premise web server or a privately hosted 51Degrees cloud (see
            // examples_shared::CLOUD_ENDPOINT_ENV_VAR).
            endpoint: examples_shared::cloud_endpoint_from_env(),
            tacs: SAMPLE_TACS.iter().map(|t| (*t).to_owned()).collect(),
        }
    }
}

// [example]
/// Run the cloud TAC-lookup example.
///
/// A TAC identifies a class of device, so a lookup can match several device
/// profiles. The pipeline carries a hardware-profile cloud engine, which exposes
/// the matches under [`HARDWARE_DATA_KEY`] as a list of device profiles, each
/// read through the same typed accessors as a single-device detection. When
/// the resource key does not include the hardware lookup, the example says so
/// and makes no lookup.
pub fn run(options: ExampleOptions) -> Result<()> {
    println!(
        "This example shows the details of devices associated with a given \
         'Type Allocation Code' or 'TAC'."
    );
    println!(
        "More background information on TACs can be found through various online \
         sources such as Wikipedia: https://en.wikipedia.org/wiki/Type_Allocation_Code"
    );
    println!("----------------------------------------");

    // The hardware-profile lookup uses a dedicated cloud engine. Usage sharing
    // is off because this is a console example. In normal operation leave it
    // on, which is the builder's default. The pipeline is built once more if
    // the cloud's list of the properties the key includes could not be read.
    let pipeline = build_hardware_pipeline(|| {
        let mut builder =
            DeviceDetectionPipelineBuilder::cloud(options.resource_key.clone()).hardware_profile();
        if let Some(endpoint) = &options.endpoint {
            builder = builder.endpoint(endpoint.clone());
        }
        builder
            .share_usage(false)
            .build()
            .context("failed to build the cloud hardware-profile pipeline")
    })?;

    // The hardware engine's properties are the ones the cloud lists for the
    // resource key. Without HardwarePlatform.Profiles no lookup can return a
    // device, so say what the key lacks and where to get one that has it.
    if !has_hardware_lookup(&pipeline)? {
        println!("{NO_HARDWARE_LOOKUP}");
        return Ok(());
    }

    for tac in &options.tacs {
        // Supply the TAC as query evidence and process it. The TAC is an input
        // only; the matching profiles come back under the hardware data key.
        let mut data = pipeline.create_flow_data_with(
            Evidence::builder()
                .add(EVIDENCE_QUERY_TAC_KEY, tac.clone())
                .build(),
        );
        data.process().context("cloud pipeline processing failed")?;

        let hardware = data.get(HARDWARE_DATA_KEY).context(
            "no hardware-profile data was produced; the cloud request engine did not run",
        )?;

        println!("Which devices are associated with the TAC '{tac}'?");
        if hardware.profiles().is_empty() {
            println!("\tNo devices are associated with this TAC.");
        } else {
            for profile in hardware.profiles() {
                println!("\t{}", profile_line(profile));
            }
        }
    }

    Ok(())
}
// [example]

/// Read the resource key from the command line or the environment, then run.
fn main() -> Result<()> {
    let resource_key = std::env::args()
        .nth(1)
        .or_else(examples_shared::resource_key_from_env);

    let Some(resource_key) = resource_key else {
        eprintln!(
            "No resource key available. Set the _51DEGREES_RESOURCE_KEY environment \
             variable (or pass the key as the first argument). TAC lookup requires a \
             paid subscription; see https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-tac.rs&utm_term=resource-key-required and create a key \
             with the required properties at https://51degrees.com/configure/QKyYH5XT?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-tac.rs&utm_term=resource-key-required."
        );
        examples_shared::print_find_out_more(FIND_OUT_MORE, examples_shared::CLOUD_REPOSITORIES);
        return Ok(());
    };

    let result = run(ExampleOptions::with_resource_key(resource_key));
    examples_shared::print_find_out_more(FIND_OUT_MORE, examples_shared::CLOUD_REPOSITORIES);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use fiftyone_device_detection::DeviceDataBase;

    #[test]
    fn profile_line_renders_vendor_name_model() {
        let profile = DeviceDataBase::new()
            .set("HardwareVendor", "Apple")
            .set("HardwareName", vec!["iPhone 11".to_owned()])
            .set("HardwareModel", "iPhone11,8");
        assert_eq!(profile_line(&profile), "Apple iPhone 11 (iPhone11,8)");
    }

    #[test]
    fn profile_line_falls_back_to_unknown() {
        // A profile with no hardware properties renders each field as Unknown
        // rather than panicking.
        let profile = DeviceDataBase::new();
        assert_eq!(profile_line(&profile), "Unknown Unknown (Unknown)");
    }
}

/*
 * @example dd-cloud-tac.rs
 *
 * The device-detection cloud TAC-lookup console example. It shows how to use
 * the 51Degrees cloud to look up the devices associated with a given Type
 * Allocation Code (TAC). Background on TACs is at
 * https://en.wikipedia.org/wiki/Type_Allocation_Code.
 *
 * A TAC identifies a class of device, so a single lookup can match more than one
 * device profile. The pipeline carries a `HardwareProfileCloudEngine`, which
 * parses the cloud response's `hardware.profiles` array into a
 * `MultiDeviceData`. The example reads the matches back through
 * `HARDWARE_DATA_KEY` and prints each as a `Vendor Name (Model)` line, using the
 * same typed accessors as single-device detection.
 *
 * TAC is an input-only property, supplied as `query.tac` evidence and never
 * returned in the response. The TAC is added as evidence and the resulting
 * profiles are read back.
 *
 * TAC lookup requires a resource key that includes HardwarePlatform.Profiles (a
 * paid subscription). Before any lookup the example reads the hardware
 * engine's properties, which are the ones the cloud lists for the resource key.
 * When they do not hold `Profiles` it prints one line saying so, with links to
 * create a key that includes it and to contact 51Degrees, and ends normally.
 * When the cloud's list of those properties could not be read, it builds the
 * pipeline once more, and if the list still cannot be read it ends with the
 * cloud's error and what to check. A TAC that matches no device prints a line
 * saying so under its heading.
 *
 * The tests in `tests/hardware_lookup.rs` run the example as a program. The
 * one against the cloud is ignored by default, so a plain `cargo test` stays
 * green offline, and checks nothing when no resource key is set or the key is
 * blank. Others run it against a local stand-in for the cloud, with a key that
 * does not include the lookup, with lookups that match no device, with each of
 * those after one failed request for the list of properties, and with a list
 * of properties that cannot be read.
 *
 * Usage sharing is turned off with `.share_usage(false)`, because this is a
 * console example. In normal operation leave it on, which is the builder's
 * default, so 51Degrees receives evidence that improves detection.
 *
 * The resource key is read from the first command-line argument or the
 * `_51DEGREES_RESOURCE_KEY` environment variable. See https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-tac.rs&utm_term=dd-cloud-tac
 * for a paid subscription and https://51degrees.com/configure/QKyYH5XT?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-tac.rs&utm_term=dd-cloud-tac to create a key.
 */
