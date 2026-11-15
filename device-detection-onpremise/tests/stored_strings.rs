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

//! The on-premise engine's device data gives each property's stored string
//! beside the parsed value, as the
//! [values stored as strings](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/value-types.md#values-stored-as-strings)
//! section of the specification describes.
//!
//! These run against the Lite Hash data file, and skip cleanly when it
//! cannot be found or will not load, as the other integration tests do.

#[allow(dead_code)]
#[path = "../../deprecated-names/environment.rs"]
mod deprecated_names;

use std::path::PathBuf;
use std::sync::Arc;

use fiftyone_device_detection_onpremise::{
    DeviceDetectionOnPremiseEngine, DeviceDetectionOnPremiseEngineBuilder,
};
use fiftyone_device_detection_shared::{DeviceData, DEVICE_DATA_KEY};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{Evidence, FlowData, Pipeline};

/// A representative mobile Safari on iPhone User-Agent.
const MOBILE_USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
    AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";

/// The Lite Hash data file, from `_51DEGREES_DD_PATH` or the `cxx` submodule.
fn lite_data_file() -> Option<PathBuf> {
    if let Ok(path) = crate::deprecated_names::var("_51DEGREES_DD_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .join("cxx")
        .join("data")
        .join("device-detection")
        .join("51Degrees-LiteV4.1.hash");
    file.is_file().then_some(file)
}

/// The engine over the Lite data file, or `None` to skip.
fn build_engine() -> Option<Arc<DeviceDetectionOnPremiseEngine>> {
    let Some(data_file) = lite_data_file() else {
        eprintln!("the Lite Hash data file was not found; skipping");
        return None;
    };
    match DeviceDetectionOnPremiseEngineBuilder::new(data_file)
        .performance_profile(PerformanceProfile::HighPerformance)
        .build()
    {
        Ok(engine) => Some(engine),
        Err(e) => {
            eprintln!("the native Hash data set did not load ({e}); skipping");
            None
        }
    }
}

/// A processed flow data for the evidence given.
fn processed(engine: &Arc<DeviceDetectionOnPremiseEngine>, evidence: Evidence) -> FlowData {
    let pipeline = Pipeline::builder()
        .add_element(engine.clone())
        .build()
        .expect("the pipeline builds");
    let mut data = pipeline.create_flow_data_with(evidence);
    data.process().expect("processing succeeds");
    data
}

#[test]
fn each_value_is_given_as_the_string_the_data_file_stores() {
    let Some(engine) = build_engine() else {
        return;
    };
    let evidence = Evidence::builder()
        .add("header.user-agent", MOBILE_USER_AGENT)
        .build();
    let data = processed(&engine, evidence);
    let device = data.get(DEVICE_DATA_KEY).expect("device data");
    let stored = device
        .values_as_string()
        .expect("the on-premise engine holds its values as strings");

    // The data file stores a Boolean as the text True or False.
    assert!(*device.is_mobile().value().unwrap());
    assert_eq!(stored.value_as_string("IsMobile").value().unwrap(), "True");
    assert_eq!(
        stored.value_as_string("PlatformName").value().ok(),
        device.platform_name().value().ok()
    );
}

#[test]
fn a_property_with_no_value_gives_the_reason_the_typed_accessor_gives() {
    let Some(engine) = build_engine() else {
        return;
    };
    let data = processed(&engine, Evidence::default());
    let device = data.get(DEVICE_DATA_KEY).expect("device data");
    let stored = device.values_as_string().expect("strings are offered");

    let is_mobile = stored.value_as_string("IsMobile");
    assert!(!is_mobile.has_value());
    assert_eq!(
        is_mobile.no_value_message(),
        device.is_mobile().no_value_message()
    );

    let unknown = stored.value_as_string("NotAProperty");
    assert!(!unknown.has_value());
    assert_eq!(
        unknown.no_value_message(),
        device.string_property("NotAProperty").no_value_message()
    );
}
