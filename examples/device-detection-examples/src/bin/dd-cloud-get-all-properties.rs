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

//! @page dd-cloud-get-all-properties-example Get All Properties (Device Detection, Cloud)
//!
//! Cloud get-all-properties console example. See the descriptive block at the
//! bottom of this file for the full write-up.
//!
//! @snippet dd-cloud-get-all-properties.rs example

use anyhow::{Context, Result};

use fiftyone_device_detection::{DeviceDetectionPipelineBuilder, Evidence, DEVICE_DATA_KEY};
// ElementData (which provides `keys()` and `get()` on the result bag) is not
// re-exported by the facade, so it is brought in from the core crate directly.
use fiftyone_pipeline_core::ElementData;

/// The links the example's output ends with.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("Device Detection", "https://51degrees.com/device-detection?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-get-all-properties.rs&utm_term=find-out-more"),
    ("Pricing", "https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-get-all-properties.rs&utm_term=find-out-more"),
    ("Documentation", "https://51degrees.com/documentation/index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-get-all-properties.rs&utm_term=find-out-more"),
];

/// The maximum length any single property value is printed at, so a long list
/// or JavaScript value does not flood the console. Values are truncated at 200
/// characters.
const MAX_VALUE_LENGTH: usize = 200;

/// Options the example runs with.
pub struct ExampleOptions {
    /// The cloud resource key the pipeline authenticates with.
    pub resource_key: String,
    /// An optional override for the cloud endpoint.
    pub endpoint: Option<String>,
    /// The User-Agent to detect and dump every property for.
    pub user_agent: String,
}

impl ExampleOptions {
    /// Default options for a given resource key: the sample mobile User-Agent
    /// and the cloud endpoint from the environment, or the public cloud when
    /// unset.
    pub fn with_resource_key(resource_key: String) -> Self {
        ExampleOptions {
            resource_key,
            // The cloud endpoint. Unset, the cloud request engine reads
            // 51DEGREES_CLOUD_ENDPOINT itself, and it is passed here as well so the
            // choice is visible. A host other than cloud.51degrees.com is an on
            // premise web server or a privately hosted 51Degrees cloud (see
            // examples_shared::CLOUD_ENDPOINT_ENV_VAR).
            endpoint: examples_shared::cloud_endpoint_from_env(),
            user_agent: examples_shared::evidence::MOBILE_USER_AGENT.to_owned(),
        }
    }
}

// [example]
/// Run the cloud get-all-properties example.
///
/// Processes a single User-Agent and prints every property the resource key
/// returns for it, reading the values straight out of the dynamic result bag
/// rather than from a hard-coded list. This way the output reflects exactly the
/// properties the configured key grants.
pub fn run(options: ExampleOptions) -> Result<()> {
    let mut builder = DeviceDetectionPipelineBuilder::cloud(options.resource_key);
    if let Some(endpoint) = options.endpoint {
        builder = builder.endpoint(endpoint);
    }
    // Usage sharing is off because this is a console example, whose evidence
    // 51Degrees already holds. In normal operation leave it on, which is the
    // builder's default.
    let pipeline = builder
        .share_usage(false)
        .build()
        .context("failed to build the cloud device-detection pipeline")?;

    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", options.user_agent.clone())
            .build(),
    );
    data.process().context("cloud pipeline processing failed")?;

    println!(
        "What property values are associated with the User-Agent '{}'?",
        options.user_agent
    );

    let device = data.get(DEVICE_DATA_KEY).context(
        "no device data was produced (the cloud request may have failed, or the \
         resource key grants no device properties)",
    )?;

    // Ask the result for the names of every property it actually holds, then
    // read each back by name. `keys()` reflects the live result, so the output
    // tracks whatever the resource key returns with no hard-coded list.
    let mut names = device.keys();
    names.sort_unstable();

    if names.is_empty() {
        println!(
            "(no properties were returned; check that the resource key grants \
             device-detection properties)"
        );
        return Ok(());
    }

    for name in names {
        // get_property_as_string renders any value type to a display string and
        // returns the no-value marker for an absent or no-value property.
        let value = examples_shared::get_property_as_string(device, &name);
        println!("{name} = {}", truncate(&value));
    }

    Ok(())
}
// [example]

/// Truncate a long value for display, appending an ellipsis when it is cut.
fn truncate(value: &str) -> String {
    // Count by characters, not bytes, so a multi-byte boundary is never split.
    if value.chars().count() <= MAX_VALUE_LENGTH {
        return value.to_owned();
    }
    let truncated: String = value.chars().take(MAX_VALUE_LENGTH).collect();
    format!("{truncated}...")
}

/// Read the resource key from the command line or the environment, then run.
fn main() -> Result<()> {
    let resource_key = std::env::args()
        .nth(1)
        .or_else(examples_shared::resource_key_from_env);

    let Some(resource_key) = resource_key else {
        eprintln!(
            "No resource key available. Set the 51DEGREES_RESOURCE_KEY environment \
             variable (or pass the key as the first argument). Create a free key at \
             https://configure.51degrees.com?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-get-all-properties.rs&utm_term=resource-key-required, making sure to include all the \
             properties you want this example to display."
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

    #[test]
    fn truncate_leaves_short_values_alone() {
        assert_eq!(truncate("Chrome"), "Chrome");
    }

    #[test]
    fn truncate_cuts_long_values() {
        let long = "x".repeat(MAX_VALUE_LENGTH + 50);
        let result = truncate(&long);
        assert!(result.ends_with("..."));
        assert_eq!(result.chars().count(), MAX_VALUE_LENGTH + 3);
    }

    #[test]
    #[ignore = "requires a live cloud resource key (set 51DEGREES_RESOURCE_KEY)"]
    fn runs_against_the_cloud() {
        let Some(resource_key) = examples_shared::resource_key_from_env() else {
            eprintln!("skipping: no resource key set");
            return;
        };
        run(ExampleOptions::with_resource_key(resource_key))
            .expect("the cloud get-all-properties example should complete");
    }
}

/*
 * @example dd-cloud-get-all-properties.rs
 *
 * The device-detection cloud get-all-properties console example. It detects a
 * single User-Agent through the cloud and then prints every property the result
 * holds, rather than a fixed, hard-coded selection.
 *
 * The important detail is that the property list is taken from the result
 * itself. After processing, the example asks the device result for the names of
 * every property it contains (`ElementData::keys`), sorts them for a stable
 * display, and reads each value back by name with the shared
 * `get_property_as_string` helper. That helper renders any value type (boolean,
 * integer, string, list and so on) to a display string and returns the no-value
 * marker for a property that is absent or has no value. Because the names come
 * from the live result, the output automatically reflects exactly the set of
 * properties the configured resource key grants, with no list to keep in sync.
 *
 * Long values are truncated to a fixed length for readability, a 200-character
 * cut-off.
 *
 * Usage sharing is turned off with `.share_usage(false)`, because this is a
 * console example. In normal operation leave it on, which is the builder's
 * default, so 51Degrees receives evidence that improves detection.
 *
 * The resource key is read from the first command-line argument or the
 * `51DEGREES_RESOURCE_KEY` environment variable. To see the widest set of
 * properties, create a key that includes them at https://configure.51degrees.com?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-get-all-properties.rs&utm_term=dd-cloud-get-all-properties.
 */
