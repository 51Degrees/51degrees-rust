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

//! @page dd-cloud-metadata-example Metadata (Device Detection, Cloud)
//!
//! Cloud metadata console example. See the descriptive block at the bottom of
//! this file for the full write-up.
//!
//! @snippet dd-cloud-metadata.rs example

use std::sync::Arc;

use anyhow::{Context, Result};

use fiftyone_device_detection::{CloudRequestEngine, DeviceDetectionCloudEngine, FlowElement};

/// The links the example's output ends with.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("Device Detection", "https://51degrees.com/device-detection?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-metadata.rs&utm_term=find-out-more"),
    ("Pricing", "https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-metadata.rs&utm_term=find-out-more"),
    ("Documentation", "https://51degrees.com/documentation/index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-metadata.rs&utm_term=find-out-more"),
];

/// Options the example runs with.
pub struct ExampleOptions {
    /// The cloud resource key whose metadata is listed.
    pub resource_key: String,
    /// An optional override for the cloud endpoint.
    pub endpoint: Option<String>,
}

// [example]
/// Run the cloud metadata example.
///
/// Lists the device properties the resource key grants (from the cloud device
/// engine) and the evidence keys the cloud accepts (from the cloud request
/// engine). Both lists are discovered from the cloud on first use.
pub fn run(options: ExampleOptions) -> Result<()> {
    // Build the cloud request engine. It is the element that actually talks to
    // the cloud, so it owns the accessible-properties and evidence-keys
    // discovery. The device engine is built on top of it and derives its
    // property metadata from it.
    let mut request_builder = CloudRequestEngine::builder().resource_key(options.resource_key);
    if let Some(endpoint) = options.endpoint {
        request_builder = request_builder.endpoint(endpoint);
    }
    let request_engine = Arc::new(
        request_builder
            .build()
            .context("failed to build the cloud request engine")?,
    );

    // Build the device engine, which reads the device product's property list
    // from the accessible properties the request engine fetched when it was
    // built. `refresh_properties` returns how many there are, or an error when
    // the cloud could not be reached then.
    let mut device_engine = DeviceDetectionCloudEngine::builder()
        .cloud_request_engine(request_engine.clone())
        .try_build()
        .context("failed to build the cloud device-detection engine")?;
    let property_count = device_engine
        .refresh_properties()
        .context("failed to fetch the accessible device properties from the cloud")?;

    output_properties(&device_engine, property_count);

    // The evidence keys come from the request engine, not the device engine: the
    // device engine consumes no raw evidence of its own, it only processes the
    // JSON the request engine fetches. The request engine is the one passing the
    // evidence values to the cloud, so it advertises the accepted keys.
    output_evidence_keys(&request_engine)
        .context("failed to fetch the accepted evidence keys from the cloud")?;

    Ok(())
}
// [example]

/// Print the device properties the resource key grants, with their category and
/// value type, one line per property.
fn output_properties(engine: &DeviceDetectionCloudEngine, property_count: usize) {
    println!("Properties available for this resource key ({property_count}):");
    for property in engine.properties() {
        let category = if property.category.is_empty() {
            "Uncategorised"
        } else {
            property.category.as_str()
        };
        // The value type is the core PropertyValueType the cloud type-name was
        // mapped onto when the metadata was built.
        println!(
            "\tProperty - {} [Category: {category}] ({:?})",
            property.name, property.value_type
        );
    }
}

/// Print the evidence keys the cloud accepts for this resource key.
///
/// The request engine's accepted evidence keys are fetched lazily, so this call
/// triggers that fetch if the property refresh above did not already. When the
/// whitelist is available the keys are listed, sorted for a stable display.
fn output_evidence_keys(engine: &Arc<CloudRequestEngine>) -> Result<()> {
    println!();
    let filter = engine.accepted_evidence_keys()?;
    let mut keys: Vec<&str> = filter.whitelist().map(|(key, _)| key).collect();
    keys.sort_unstable();

    if keys.is_empty() {
        // Fall back to the FlowElement filter probe when a whitelist cannot be
        // enumerated.
        println!(
            "No accepted evidence keys were advertised. As an alternative you can \
             probe individual keys: header.user-agent is {}accepted.",
            if engine.evidence_key_filter().include("header.user-agent") {
                ""
            } else {
                "not "
            }
        );
    } else {
        println!("Accepted evidence keys:");
        for key in keys {
            println!("\t{key}");
        }
    }
    Ok(())
}

/// Read the resource key from the command line or the environment, then run.
fn main() -> Result<()> {
    let resource_key = std::env::args()
        .nth(1)
        .or_else(examples_shared::resource_key_from_env);

    let Some(resource_key) = resource_key else {
        eprintln!(
            "No resource key available. Set the _51DEGREES_RESOURCE_KEY environment \
             variable (or pass the key as the first argument). Create a free key at \
             https://51degrees.com/configure?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-metadata.rs&utm_term=resource-key-required."
        );
        examples_shared::print_find_out_more(FIND_OUT_MORE, examples_shared::CLOUD_REPOSITORIES);
        return Ok(());
    };

    let result = run(ExampleOptions {
        resource_key,
        // The cloud endpoint. Unset, the cloud request engine reads
        // _51DEGREES_CLOUD_ENDPOINT itself, and it is passed here as well so the
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

    #[test]
    #[ignore = "requires a live cloud resource key (set _51DEGREES_RESOURCE_KEY)"]
    fn runs_against_the_cloud() {
        let Some(resource_key) = examples_shared::resource_key_from_env() else {
            eprintln!("skipping: no resource key set");
            return;
        };
        run(ExampleOptions {
            resource_key,
            endpoint: examples_shared::cloud_endpoint_from_env(),
        })
        .expect("the cloud metadata example should complete");
    }
}

/*
 * @example dd-cloud-metadata.rs
 *
 * The device-detection cloud metadata console example. It shows how to
 * discover, at runtime, which device properties a resource key grants and which
 * evidence keys the cloud will accept for it.
 *
 * Two different engines own the two kinds of metadata, and the example reads
 * each from the right place:
 *
 * - The list of device properties comes from the `DeviceDetectionCloudEngine`.
 *   That list comes from the resource key's accessible properties, which the
 *   request engine asks the cloud for when it is built. `refresh_properties`
 *   returns the number of properties found, or an error when the cloud could
 *   not be reached then. Each property is then printed with its category and
 *   core value type.
 *
 * - The list of accepted evidence keys comes from the `CloudRequestEngine`, not
 *   the device engine. The device engine consumes no raw evidence of its own. It
 *   simply processes the JSON the request engine fetches. The request engine is
 *   the element that takes the evidence values and sends them to the cloud, so
 *   it is the one that advertises which keys are accepted. When the engine's
 *   evidence filter is an enumerable whitelist the keys are listed. Otherwise
 *   the example falls back to probing a single key for inclusion.
 *
 * No `ShareUsageElement` is added, because this is a console example. A
 * production web deployment should enable usage sharing.
 *
 * The resource key is read from the first command-line argument or the
 * `_51DEGREES_RESOURCE_KEY` environment variable. Create a free key, or a paid key
 * with the full property set, at https://51degrees.com/configure?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-metadata.rs&utm_term=dd-cloud-metadata.
 */
