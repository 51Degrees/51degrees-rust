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
//! figures time finding the property. They mean something only in an
//! optimised build, so the test reads nothing in a debug build, and
//! `cargo test --release -p fiftyone-device-detection-onpremise --test
//! read_timing -- --nocapture` prints them.

use std::hint::black_box;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use fiftyone_device_detection_onpremise::DeviceDetectionOnPremiseEngineBuilder;
use fiftyone_device_detection_shared::{DeviceData, DEVICE_DATA_KEY};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{Evidence, FlowElement, Pipeline};

/// The reads timed for each figure.
const READS: u32 = 1_000_000;

const USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
    AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";

/// The Hash data file `51DEGREES_DD_PATH` names, or the Lite file in the
/// `cxx` submodule.
fn data_file() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("51DEGREES_DD_PATH") {
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
