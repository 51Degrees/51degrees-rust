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

//! The time one read of a property takes through the device data, by name
//! and through its typed accessor, once its value has been read, so the
//! figures time finding the property, and the time to build an engine and
//! to process a detection with no names and with every name. They mean
//! something only in an optimised build, so the tests time nothing in a
//! debug build, and `cargo test --release -p
//! fiftyone-device-detection-onpremise --test read_timing -- --nocapture`
//! prints them.

use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use fiftyone_device_detection_onpremise::{
    DeviceDetectionOnPremiseEngine, DeviceDetectionOnPremiseEngineBuilder,
};
use fiftyone_device_detection_shared::{DeviceData, DEVICE_DATA_KEY};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{Evidence, FlowElement, Pipeline};

/// The reads timed for each figure.
const READS: u32 = 1_000_000;

/// The detections timed for each figure, after as many again untimed.
const DETECTIONS: u32 = 20_000;

const USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
    AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";

/// The User-Agents the timed detections take in turn.
const USER_AGENTS: [&str; 3] = [
    USER_AGENT,
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
     Chrome/124.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) \
     Chrome/124.0.0.0 Mobile Safari/537.36",
];

/// An engine over `path` built with `names`, or with none when it is empty,
/// and the time the build took in milliseconds.
fn build(path: &Path, names: &[String]) -> (Arc<DeviceDetectionOnPremiseEngine>, f64) {
    let start = Instant::now();
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(path)
        .performance_profile(PerformanceProfile::HighPerformance)
        .properties(names.iter().cloned())
        .build()
        .expect("the data file loads");
    (engine, start.elapsed().as_secs_f64() * 1000.0)
}

/// The mean time of one detection through `engine`, making the flow data
/// and processing it, in microseconds.
fn mean_detection_microseconds(engine: Arc<DeviceDetectionOnPremiseEngine>) -> f64 {
    let element: Arc<dyn FlowElement> = engine;
    let pipeline = Pipeline::builder()
        .add_element(element)
        .build()
        .expect("the pipeline builds");
    let detect = |detection: u32| {
        let user_agent = USER_AGENTS[detection as usize % USER_AGENTS.len()];
        let mut data = pipeline.create_flow_data_with(
            Evidence::builder()
                .add("header.user-agent", user_agent)
                .build(),
        );
        data.process().expect("processing succeeds");
        black_box(&data);
    };
    (0..DETECTIONS).for_each(detect);
    let start = Instant::now();
    (0..DETECTIONS).for_each(detect);
    start.elapsed().as_secs_f64() * 1_000_000.0 / f64::from(DETECTIONS)
}

#[test]
fn building_and_processing_with_no_names_and_with_every_name() {
    if cfg!(debug_assertions) {
        eprintln!("the detections are timed only in an optimised build; skipping");
        return;
    }
    let Some(path) = data_file() else {
        eprintln!("no Hash data file found; skipping the detection timing");
        return;
    };
    let (none, none_build) = build(&path, &[]);
    let every: Vec<String> = none
        .properties()
        .iter()
        .filter(|property| property.available)
        .map(|property| property.name.clone())
        .collect();
    let (named, every_build) = build(&path, &every);
    let none_detection = mean_detection_microseconds(none);
    let every_detection = mean_detection_microseconds(named);
    println!(
        "No names: built in {none_build:.0} ms, {none_detection:.2} us a detection. \
         Every name ({} names): built in {every_build:.0} ms, {every_detection:.2} us a \
         detection. Each detection figure is the mean of {DETECTIONS}.",
        every.len()
    );
}

/// The Hash data file `_51DEGREES_DD_PATH` names, or the Lite file in the
/// `cxx` submodule.
fn data_file() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("_51DEGREES_DD_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .to_path_buf();
    Some(workspace.join("cxx/data/device-detection/51Degrees-LiteV4.1.hash"))
        .filter(|path| path.is_file())
}

/// The mean time of one call of `read`, in nanoseconds.
fn mean_nanoseconds(mut read: impl FnMut()) -> f64 {
    let start = Instant::now();
    for _ in 0..READS {
        read();
    }
    start.elapsed().as_nanos() as f64 / f64::from(READS)
}

#[test]
fn a_read_by_name_and_a_typed_read() {
    if cfg!(debug_assertions) {
        eprintln!("the reads are timed only in an optimised build; skipping");
        return;
    }
    let Some(path) = data_file() else {
        eprintln!("no Hash data file found; skipping the read timing");
        return;
    };
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(path)
        .performance_profile(PerformanceProfile::HighPerformance)
        .build()
        .expect("the data file loads");
    let element: Arc<dyn FlowElement> = engine;
    let pipeline = Pipeline::builder()
        .add_element(element)
        .build()
        .expect("the pipeline builds");
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", USER_AGENT)
            .build(),
    );
    data.process().expect("processing succeeds");
    let device = data.get(DEVICE_DATA_KEY).expect("device data was produced");
    assert_eq!(
        format!("{:?}", device.is_mobile()),
        format!("{:?}", device.bool_property("IsMobile"))
    );
    let by_name = mean_nanoseconds(|| {
        black_box(device.bool_property(black_box("IsMobile")));
    });
    let typed = mean_nanoseconds(|| {
        black_box(device.is_mobile());
    });
    println!(
        "IsMobile read by name {by_name:.1} ns, typed {typed:.1} ns, the mean of {READS} reads each"
    );
}
