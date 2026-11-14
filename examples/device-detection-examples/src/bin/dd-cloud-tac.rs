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

use device_detection_examples::profile_line;
use fiftyone_device_detection::{DeviceDetectionPipelineBuilder, Evidence, HARDWARE_DATA_KEY};

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
    /// The cloud resource key. TAC lookup requires a paid subscription.
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
/// read through the same typed accessors as a single-device detection.
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
    // on, which is the builder's default.
    let mut builder =
        DeviceDetectionPipelineBuilder::cloud(options.resource_key).hardware_profile();
    if let Some(endpoint) = options.endpoint {
        builder = builder.endpoint(endpoint);
    }
    let pipeline = builder
        .share_usage(false)
        .build()
        .context("failed to build the cloud hardware-profile pipeline")?;

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
            println!(
                "\tNo matching device profiles were returned for this TAC. TAC lookup \
                 requires a resource key with the hardware-profile-lookup product."
            );
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
             with the required properties at https://51degrees.com/configure?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-tac.rs&utm_term=resource-key-required."
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

    #[test]
    #[ignore = "requires a live cloud resource key with TAC lookup (paid tier)"]
    fn runs_against_the_cloud() {
        let Some(resource_key) = examples_shared::resource_key_from_env() else {
            eprintln!("skipping: no resource key set");
            return;
        };
        run(ExampleOptions::with_resource_key(resource_key))
            .expect("the cloud TAC example should complete");
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
 * TAC lookup requires a resource key with the hardware-profile-lookup product (a
 * paid subscription). A key without it returns the standard single-device
 * `device` block with no `hardware.profiles`, so the lookup yields no profiles
 * and the example reports that. The example's test is therefore ignored by
 * default, so a plain `cargo test` stays green offline, and it skips cleanly when
 * no resource key is set.
 *
 * Usage sharing is turned off with `.share_usage(false)`, because this is a
 * console example. In normal operation leave it on, which is the builder's
 * default, so 51Degrees receives evidence that improves detection.
 *
 * The resource key is read from the first command-line argument or the
 * `_51DEGREES_RESOURCE_KEY` environment variable. See https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-tac.rs&utm_term=dd-cloud-tac
 * for a paid subscription and https://51degrees.com/configure?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-tac.rs&utm_term=dd-cloud-tac to create a key.
 */
