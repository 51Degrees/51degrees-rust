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

//! A match metric whose graph walk fails has no value, with the reason. A
//! walk fails when the data file cannot be read, so the test reads a copy of
//! the Lite file under the low memory profile, which reads the graphs from
//! the file, and empties the copy after the detections. It needs the Lite
//! file `_51DEGREES_DD_PATH` names, and skips with a note without it.

#[allow(dead_code)]
#[path = "../../deprecated-names/environment.rs"]
mod deprecated_names;

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fiftyone_device_detection_onpremise::DeviceDetectionOnPremiseEngineBuilder;
use fiftyone_device_detection_shared::{DeviceData, DeviceDataBase, DEVICE_DATA_KEY};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{Evidence, FlowElement, Pipeline};

/// The reason a match metric has no value when a graph cannot be read.
const METRICS_UNREADABLE: &str =
    "The match metrics could not be read, because a graph could not be read from the data file.";

/// A Chrome on Windows User-Agent.
const DESKTOP_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

fn lite_data_file() -> Option<PathBuf> {
    crate::deprecated_names::var("_51DEGREES_DD_PATH")
        .ok()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
}

/// A new, empty folder holding a copy of `lite`.
fn folder_with_copy(lite: &Path) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("fiftyone-dd-failed-walk-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("51Degrees-LiteV4.1.hash");
    std::fs::copy(lite, &path).unwrap();
    (dir, path)
}

/// Two detections of a desktop User-Agent over the data file at `path`, read
/// under the low memory profile, with no property of either read yet.
fn two_detections(path: &Path) -> (DeviceDataBase, DeviceDataBase) {
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(path)
        .performance_profile(PerformanceProfile::LowMemory)
        .build()
        .expect("the copy loads");
    let element: Arc<dyn FlowElement> = engine;
    let pipeline = Pipeline::builder()
        .add_element(element)
        .build()
        .expect("the pipeline builds");
    let detect = || {
        let mut data = pipeline.create_flow_data_with(
            Evidence::builder()
                .add("header.user-agent", DESKTOP_USER_AGENT)
                .build(),
        );
        data.process().expect("the detection succeeds");
        data.get(DEVICE_DATA_KEY)
            .expect("device data was produced")
            .clone()
    };
    (detect(), detect())
}

/// UserAgents, read first after the data file can no longer be read, has no
/// value with the reason the other match metrics give, as .NET's
/// `GetUserAgents` throws. Method, read first on the other detection, shows
/// the reason they give.
#[test]
fn user_agents_whose_walk_fails_has_no_value_with_the_reason() {
    let Some(lite) = lite_data_file() else {
        eprintln!("no Lite Hash data file found; skipping the failed walk");
        return;
    };
    let (dir, path) = folder_with_copy(&lite);
    {
        let (first, second) = two_detections(&path);
        OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("the copy opens")
            .set_len(0)
            .expect("the copy empties");
        assert_eq!(
            first.user_agents().no_value_message(),
            Some(METRICS_UNREADABLE)
        );
        assert_eq!(second.method().no_value_message(), Some(METRICS_UNREADABLE));
    }
    let _ = std::fs::remove_dir_all(&dir);
}
