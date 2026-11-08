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

//! The on-premise pipeline leaves `51D_GetHighEntropyValues` evidence to the
//! engine, which the C library turns into client hints, so the pipeline holds
//! no decoder element and its Flow Data no `uach` data for the JSON builder
//! to write. Skips when the Lite Hash data file is absent.

#![cfg(feature = "on-premise")]

use std::path::PathBuf;

use fiftyone_device_detection::{
    DeviceData, DeviceDetectionPipelineBuilder, Evidence, PerformanceProfile, DEVICE_DATA_KEY,
};
use fiftyone_pipeline_core::FlowElement;

/// The high entropy values of Chrome 131 on macOS 15.1.1, in base 64.
const MAC_VALUES: &str = "eyJhcmNoaXRlY3R1cmUiOiJhcm0iLCJicmFuZHMiOlt7ImJyYW5kIjoiR29vZ2xlIENocm9tZSIsInZlcnNpb24iOiIxMzEifSx7ImJyYW5kIjoiQ2hyb21pdW0iLCJ2ZXJzaW9uIjoiMTMxIn0seyJicmFuZCI6Ik5vdF9BIEJyYW5kIiwidmVyc2lvbiI6IjI0In1dLCJmdWxsVmVyc2lvbkxpc3QiOlt7ImJyYW5kIjoiR29vZ2xlIENocm9tZSIsInZlcnNpb24iOiIxMzEuMC42Nzc4LjE0MCJ9LHsiYnJhbmQiOiJDaHJvbWl1bSIsInZlcnNpb24iOiIxMzEuMC42Nzc4LjE0MCJ9LHsiYnJhbmQiOiJOb3RfQSBCcmFuZCIsInZlcnNpb24iOiIyNC4wLjAuMCJ9XSwibW9iaWxlIjpmYWxzZSwibW9kZWwiOiIiLCJwbGF0Zm9ybSI6Im1hY09TIiwicGxhdGZvcm1WZXJzaW9uIjoiMTUuMS4xIn0=";

/// The data key the UA-CH high entropy decoder element writes under.
const DECODER_DATA_KEY: &str = "uach";

/// The data file `51DEGREES_DD_PATH` names, or else the Lite file found from
/// this crate's directory upwards.
fn data_file() -> Option<PathBuf> {
    if let Some(path) = std::env::var("51DEGREES_DD_PATH")
        .ok()
        .map(|path| PathBuf::from(path.trim()))
        .filter(|path| path.is_file())
    {
        return Some(path);
    }
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        let candidate = dir.join("cxx/data/device-detection/51Degrees-LiteV4.1.hash");
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[test]
fn the_on_premise_pipeline_leaves_high_entropy_values_to_the_engine() {
    let Some(path) = data_file() else {
        eprintln!("no Lite Hash data file found; skipping the high entropy values test");
        return;
    };
    let pipeline = DeviceDetectionPipelineBuilder::on_premise(&path)
        .performance_profile(PerformanceProfile::HighPerformance)
        .auto_update(false)
        .file_system_watcher(false)
        .share_usage(false)
        .build()
        .expect("the pipeline builds over the Lite file");

    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("query.51D_GetHighEntropyValues", MAC_VALUES)
            .build(),
    );
    data.process().expect("processing succeeds");

    // The engine alone gives the platform the high entropy values name.
    let device = data.get(DEVICE_DATA_KEY).expect("device data was produced");
    let platform = device.platform_name();
    assert_eq!(platform.value().expect("PlatformName has a value"), "macOS");

    assert!(
        !pipeline
            .flow_elements()
            .iter()
            .any(|element| FlowElement::data_key(&**element) == DECODER_DATA_KEY),
        "the pipeline holds a decoder element"
    );
    assert!(
        !data.data_keys().iter().any(|key| key == DECODER_DATA_KEY),
        "the Flow Data holds uach data, which the JSON builder would write"
    );
}
