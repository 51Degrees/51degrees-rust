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

//! The on-premise pipeline builder passes the Hash engine's matching
//! settings to the engine. Skips when the Lite Hash data file is absent.

#![cfg(feature = "on-premise")]

use std::path::PathBuf;
use std::sync::Arc;

use fiftyone_device_detection::{
    DeviceData, DeviceDetectionOnPremisePipelineBuilder, DeviceDetectionPipelineBuilder, Evidence,
    PerformanceProfile, Pipeline, DEVICE_DATA_KEY,
};

/// A desktop Chrome on Windows User-Agent.
const DESKTOP_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// A User-Agent the Lite file matches in more components with a difference
/// of 10 than with none.
const NOTHING_TO_MATCH: &str = "nothing to match";

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

/// A pipeline builder with no updates and no usage sharing.
fn builder(path: &PathBuf) -> DeviceDetectionOnPremisePipelineBuilder {
    DeviceDetectionPipelineBuilder::on_premise(path)
        .performance_profile(PerformanceProfile::HighPerformance)
        .auto_update(false)
        .file_system_watcher(false)
        .share_usage(false)
}

/// The match method, device id and difference for a User-Agent.
fn detect(pipeline: &Arc<Pipeline>, user_agent: &str) -> (String, String, i64) {
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", user_agent)
            .build(),
    );
    data.process().unwrap();
    let device = data.get(DEVICE_DATA_KEY).unwrap();
    (
        device.method().value().unwrap().clone(),
        device.device_id().value().unwrap().clone(),
        *device.difference().value().unwrap(),
    )
}

#[test]
fn the_matching_settings_reach_the_engine() {
    let Some(path) = data_file() else {
        eprintln!("no Lite Hash data file found; skipping the matching settings");
        return;
    };
    let defaults = builder(&path).build().unwrap();
    assert_eq!(detect(&defaults, DESKTOP_USER_AGENT).0, "PREDICTIVE");
    assert_eq!(detect(&defaults, "").1, "0-0-0-0");
    assert_eq!(detect(&defaults, NOTHING_TO_MATCH).2, 0);

    let performance = builder(&path)
        .use_performance_graph(true)
        .use_predictive_graph(false)
        .build()
        .unwrap();
    assert_eq!(detect(&performance, DESKTOP_USER_AGENT).0, "PERFORMANCE");

    // With the performance graph off by default, this leaves no graph.
    assert!(builder(&path).use_predictive_graph(false).build().is_err());

    let unmatched = builder(&path).allow_unmatched(true).build().unwrap();
    let (_, device_id, _) = detect(&unmatched, "");
    assert!(device_id.split('-').all(|id| id != "0"), "{device_id}");

    let loose = builder(&path).difference(10).build().unwrap();
    assert!(detect(&loose, NOTHING_TO_MATCH).2 > 0);
}
