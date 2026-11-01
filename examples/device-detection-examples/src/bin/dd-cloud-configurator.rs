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

//! @page dd-cloud-configurator-example Configurator (Device Detection, Cloud)
//!
//! Cloud configurator console example. See the descriptive block at the bottom
//! of this file for the full write-up.
//!
//! @snippet dd-cloud-configurator.rs example

use std::sync::Arc;

use anyhow::{Context, Result};

use fiftyone_device_detection::{
    CloudRequestEngine, DeviceData, DeviceDetectionCloudEngine, Pipeline, DEVICE_DATA_KEY,
};
use fiftyone_pipeline_core::{Evidence, FlowElement};

/// The cloud request origin the configured key is locked to.
///
/// A resource key created in the configurator can be restricted to one or more
/// origins. The matching `Origin` header value is sent with every request so the
/// cloud accepts it.
const CLOUD_REQUEST_ORIGIN: &str = "www.51degrees-example.com";

/// Options the example runs with.
pub struct ExampleOptions {
    /// The cloud resource key created in the configurator.
    pub resource_key: String,
    /// An optional override for the cloud endpoint.
    pub endpoint: Option<String>,
    /// The cloud request origin to send. Defaults to [`CLOUD_REQUEST_ORIGIN`].
    /// Set to `None` to send no origin header.
    pub cloud_request_origin: Option<String>,
}

impl ExampleOptions {
    /// Default options for a resource key: the sample origin and the cloud
    /// endpoint from the environment, or the public cloud when unset.
    pub fn with_resource_key(resource_key: String) -> Self {
        ExampleOptions {
            resource_key,
            // The cloud endpoint. Unset, the cloud request engine reads
            // 51DEGREES_CLOUD_ENDPOINT itself, and it is passed here as well so the
            // choice is visible. A host other than cloud.51degrees.com is an on
            // premise web server or a privately hosted 51Degrees cloud (see
            // examples_shared::CLOUD_ENDPOINT_ENV_VAR).
            endpoint: examples_shared::cloud_endpoint_from_env(),
            cloud_request_origin: Some(CLOUD_REQUEST_ORIGIN.to_owned()),
        }
    }
}

// [example]
/// Run the cloud configurator example.
///
/// Builds the cloud pipeline by hand so the full resource-key configuration is
/// visible (the resource key, the cloud endpoint, and the cloud request origin),
/// then runs a single User-Agent Client Hints detection through it and prints
/// `IsMobile`.
pub fn run(options: ExampleOptions) -> Result<()> {
    let pipeline = build_pipeline(&options)?;

    // Evidence from a Windows 11 device using a browser that supports User-Agent
    // Client Hints. The shared helper supplies a representative UACH header set.
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add_all(examples_shared::evidence::uach_header_evidence())
            .build(),
    );
    data.process().context("cloud pipeline processing failed")?;

    let device = data.get(DEVICE_DATA_KEY).context(
        "no device data was produced (the cloud request may have failed, or the \
         origin does not match the resource key)",
    )?;

    // Print IsMobile, handling the no-value case rather than assuming a value.
    match device.is_mobile().value() {
        Ok(is_mobile) => println!("device.IsMobile: {is_mobile}"),
        Err(_) => println!(
            "device.IsMobile: {}",
            device.is_mobile().no_value_message().unwrap_or("No value")
        ),
    }

    Ok(())
}
// [example]

/// Assemble the cloud pipeline from the resource-key configuration.
///
/// The pipeline is built by hand (rather than through the convenience builder)
/// so every piece of the resource-key configuration is explicit: the resource
/// key and endpoint on the request engine, the cloud request origin, and the
/// device engine that turns the response into a result. The convenience
/// `DeviceDetectionPipelineBuilder::cloud` does the same wiring but does not
/// expose the origin, which this example specifically illustrates.
fn build_pipeline(options: &ExampleOptions) -> Result<Arc<Pipeline>> {
    let mut request_builder =
        CloudRequestEngine::builder().resource_key(options.resource_key.clone());
    if let Some(endpoint) = &options.endpoint {
        request_builder = request_builder.endpoint(endpoint.clone());
    }
    if let Some(origin) = &options.cloud_request_origin {
        request_builder = request_builder.cloud_request_origin(origin.clone());
    }
    let request_engine = Arc::new(
        request_builder
            .build()
            .context("failed to build the cloud request engine")?,
    );

    // The device engine reads the request engine's JSON and produces the shared
    // DeviceData result. It is given the same Arc that is added to the pipeline.
    let device_engine = DeviceDetectionCloudEngine::builder()
        .cloud_request_engine(request_engine.clone())
        .try_build()
        .context("failed to build the cloud device-detection engine")?;

    // No ShareUsageElement is added: usage sharing is forbidden for console
    // examples and required only for web examples.
    Pipeline::builder()
        .add_element(request_engine as Arc<dyn FlowElement>)
        .add_element(Arc::new(device_engine))
        .build()
        .context("failed to build the cloud pipeline")
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
             https://configure.51degrees.com?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-configurator.rs&utm_term=resource-key-required."
        );
        return Ok(());
    };

    run(ExampleOptions::with_resource_key(resource_key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_builds_offline_with_injected_state() {
        // The request engine fetches its discovery state from the cloud as it
        // builds, so an offline build needs a previously exported state. With one
        // injected, the same two-element pipeline assembles without a network call.
        use fiftyone_device_detection::CloudEngineState;

        let request_engine = Arc::new(
            CloudRequestEngine::builder()
                .resource_key("resource-key-placeholder")
                .cloud_request_origin("https://example.com")
                .set_state(CloudEngineState::default())
                .build()
                .expect("the request engine should build offline from an injected state"),
        );
        let device_engine = DeviceDetectionCloudEngine::builder()
            .cloud_request_engine(request_engine.clone())
            .try_build()
            .expect("the device engine should build");
        let pipeline = Pipeline::builder()
            .add_element(request_engine as Arc<dyn FlowElement>)
            .add_element(Arc::new(device_engine))
            .build()
            .expect("the cloud pipeline should build");
        // Two elements: the request engine and the device engine.
        assert_eq!(pipeline.flow_elements().len(), 2);
    }

    #[test]
    #[ignore = "requires a live cloud resource key (set 51DEGREES_RESOURCE_KEY)"]
    fn runs_against_the_cloud() {
        let Some(resource_key) = examples_shared::resource_key_from_env() else {
            eprintln!("skipping: no resource key set");
            return;
        };
        // The sample origin will only be accepted if the resource key is locked
        // to it, so send no origin for a generic key under test.
        let mut options = ExampleOptions::with_resource_key(resource_key);
        options.cloud_request_origin = None;
        run(options).expect("the cloud configurator example should complete");
    }
}

/*
 * @example dd-cloud-configurator.rs
 *
 * The device-detection cloud configurator console example. It illustrates how a
 * resource key created in the 51Degrees configurator (https://configure.51degrees.com?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-cloud-configurator.rs&utm_term=dd-cloud-configurator) is
 * turned into a configured, ready-to-run cloud pipeline.
 *
 * The configurator lets you choose exactly which properties a key returns and,
 * optionally, lock the key to one or more request origins. This example makes
 * that configuration explicit by building the pipeline by hand rather than
 * through the convenience builder:
 *
 * - a `CloudRequestEngine` is built with the resource key (and, if overridden,
 *   the cloud endpoint), and with the cloud request origin set so the `Origin`
 *   header the cloud checks matches the key's allowed origins,
 * - a `DeviceDetectionCloudEngine` is built on top of it to turn the cloud's
 *   JSON response into the shared `DeviceData` result,
 * - the two are added to a `Pipeline` in order.
 *
 * The convenience `DeviceDetectionPipelineBuilder::cloud(resource_key)` performs
 * the same wiring for the common case, but it does not expose the request
 * origin, which is the piece this example exists to show.
 *
 * A single User-Agent Client Hints detection is then run through the pipeline
 * and `IsMobile` is printed, with the no-value case handled explicitly. The
 * sample origin (`www.51degrees-example.com`) is only accepted by the cloud if
 * the resource key is locked to it, so the live test sends no origin for a
 * generic key.
 *
 * No `ShareUsageElement` is added, because this is a console example. A
 * production web deployment should enable usage sharing.
 *
 * The resource key is read from the first command-line argument or the
 * `51DEGREES_RESOURCE_KEY` environment variable.
 */
