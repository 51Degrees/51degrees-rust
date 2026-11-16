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

//! Both Device Detection pipeline builders offer the option that records a
//! processing error on the flow data instead of returning it, which the
//! specification says must be available when the Pipeline is created. The
//! on-premise cases skip when the Lite Hash data file is absent, which CI
//! checks for before it runs the tests.

/// Where the Lite Hash data file is, from the environment or the data
/// submodule, or `None` when neither holds one.
#[cfg(feature = "on-premise")]
fn data_file() -> Option<std::path::PathBuf> {
    if let Ok(path) = std::env::var("_51DEGREES_DD_PATH") {
        let path = std::path::PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cxx/data/device-detection/51Degrees-LiteV4.1.hash");
    path.is_file().then_some(path)
}

#[cfg(feature = "on-premise")]
#[test]
fn the_on_premise_pipeline_builder_suppresses_process_errors_when_asked() {
    use fiftyone_device_detection::DeviceDetectionPipelineBuilder;

    let Some(file) = data_file() else {
        eprintln!("skipped: no Lite file");
        return;
    };
    let pipeline = DeviceDetectionPipelineBuilder::on_premise(&file)
        .auto_update(false)
        .suppress_process_exceptions(true)
        .build()
        .expect("the on-premise pipeline builds");
    assert!(
        pipeline.is_suppress_process_exceptions(),
        "the pipeline suppresses process errors when the builder is asked to"
    );
}

#[cfg(feature = "on-premise")]
#[test]
fn the_on_premise_pipeline_builder_returns_process_errors_by_default() {
    use fiftyone_device_detection::DeviceDetectionPipelineBuilder;

    let Some(file) = data_file() else {
        eprintln!("skipped: no Lite file");
        return;
    };
    let pipeline = DeviceDetectionPipelineBuilder::on_premise(&file)
        .auto_update(false)
        .build()
        .expect("the on-premise pipeline builds");
    assert!(
        !pipeline.is_suppress_process_exceptions(),
        "the default is unchanged, so an error is returned from process"
    );
}

// Gated on `reqwest-client` as well as `cloud`, because the cloud request
// engine needs an HTTP transport even when a given state means it makes no
// call, which is what the builder's own offline test does.
#[cfg(all(feature = "cloud", feature = "reqwest-client"))]
#[test]
fn the_cloud_pipeline_builder_suppresses_process_errors_when_asked() {
    use fiftyone_device_detection::{CloudEngineState, DeviceDetectionPipelineBuilder};

    let pipeline = DeviceDetectionPipelineBuilder::cloud("resource-key-placeholder")
        .set_state(CloudEngineState::default())
        .suppress_process_exceptions(true)
        .build()
        .expect("the cloud pipeline builds from the given state");
    assert!(
        pipeline.is_suppress_process_exceptions(),
        "the pipeline suppresses process errors when the builder is asked to"
    );

    let pipeline = DeviceDetectionPipelineBuilder::cloud("resource-key-placeholder")
        .set_state(CloudEngineState::default())
        .build()
        .expect("the cloud pipeline builds from the given state");
    assert!(
        !pipeline.is_suppress_process_exceptions(),
        "the default is unchanged, so an error is returned from process"
    );
}
