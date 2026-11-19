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

//! Refreshes run at once from several threads, and while other threads
//! process, as the data updates specification requires: "This mechanism MUST
//! be thread safe." and "The Pipeline that contains the Aspect Engine MUST
//! still be capable of processing Flow Data with minimal performance impact
//! while data refresh is happening." No test here measures time. Each checks
//! that every refresh and every detection made while refreshes run succeeds
//! and gives the right answer. The tests need the Lite file, from
//! `_51DEGREES_DD_PATH` or the data repository in the `cxx` submodule, and
//! skip with a note without it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use fiftyone_device_detection_onpremise::{
    DeviceDetectionOnPremiseEngine, DeviceDetectionOnPremiseEngineBuilder,
};
use fiftyone_device_detection_shared::{DeviceData, DEVICE_DATA_KEY};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{Evidence, FlowElement, Pipeline};
use fiftyone_pipeline_engines::OnPremiseAspectEngine;

/// A mobile Safari on iPhone User-Agent.
const MOBILE_USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
    AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";

/// A desktop Chrome on Windows User-Agent.
const DESKTOP_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// The threads that refresh, and the refreshes each makes.
const REFRESHING_THREADS: usize = 4;
const REFRESHES_EACH: usize = 3;

/// The threads that process while the refreshes run.
const PROCESSING_THREADS: usize = 4;

/// The Lite file `_51DEGREES_DD_PATH` names, or the one in the data
/// repository inside the `cxx` submodule.
fn lite_data_file() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("_51DEGREES_DD_PATH") {
        let path = PathBuf::from(path.trim());
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

/// An engine over the Lite file under `profile`, in a pipeline of its own.
fn engine_and_pipeline(
    path: &PathBuf,
    profile: PerformanceProfile,
) -> (Arc<DeviceDetectionOnPremiseEngine>, Arc<Pipeline>) {
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(path)
        .performance_profile(profile)
        .build()
        .expect("the data file loads");
    let element: Arc<dyn FlowElement> = engine.clone();
    let pipeline = Pipeline::builder().add_element(element).build().unwrap();
    (engine, pipeline)
}

/// Whether the pipeline detects `user_agent` as a mobile device.
fn is_mobile(pipeline: &Arc<Pipeline>, user_agent: &str) -> Option<bool> {
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", user_agent)
            .build(),
    );
    data.process().expect("processing succeeds");
    let mobile = data
        .get(DEVICE_DATA_KEY)
        .expect("the device data is there")
        .is_mobile()
        .value()
        .ok()
        .copied();
    mobile
}

/// Run `REFRESHES_EACH` refreshes on each of `REFRESHING_THREADS` threads at
/// once, half from the file and half from memory.
fn refresh_from_several_threads(engine: &DeviceDetectionOnPremiseEngine, bytes: &[u8]) {
    std::thread::scope(|scope| {
        for thread in 0..REFRESHING_THREADS {
            scope.spawn(move || {
                for refresh in 0..REFRESHES_EACH {
                    let result = if thread % 2 == 0 {
                        engine.refresh(None)
                    } else {
                        engine.refresh_from_memory(None, bytes)
                    };
                    result.unwrap_or_else(|error| {
                        panic!("refresh {refresh} on thread {thread} fails: {error}")
                    });
                }
            });
        }
    });
}

/// Refreshes made at once from several threads, from the file and from
/// memory, all succeed, and the engine then reads a whole data set and
/// detects as before.
#[test]
fn refreshes_made_at_once_from_several_threads_all_succeed() {
    let Some(path) = lite_data_file() else {
        eprintln!("skipping refreshes at once: set _51DEGREES_DD_PATH to the Lite file");
        return;
    };
    let bytes = std::fs::read(&path).unwrap();
    for profile in [
        PerformanceProfile::HighPerformance,
        PerformanceProfile::LowMemory,
    ] {
        let (engine, pipeline) = engine_and_pipeline(&path, profile);

        refresh_from_several_threads(&engine, &bytes);

        assert!(
            engine.data_set().data_file_properties().is_ok(),
            "{profile:?}: the data set reads"
        );
        assert_eq!(
            is_mobile(&pipeline, MOBILE_USER_AGENT),
            Some(true),
            "{profile:?}"
        );
        assert_eq!(
            is_mobile(&pipeline, DESKTOP_USER_AGENT),
            Some(false),
            "{profile:?}"
        );
    }
}

/// Threads go on processing while other threads refresh, and every
/// detection made meanwhile succeeds and gives the answer a detection gives
/// with no refresh running.
#[test]
fn processing_goes_on_while_refreshes_run() {
    let Some(path) = lite_data_file() else {
        eprintln!("skipping processing during refresh: set _51DEGREES_DD_PATH to the Lite file");
        return;
    };
    let bytes = std::fs::read(&path).unwrap();
    for profile in [
        PerformanceProfile::HighPerformance,
        PerformanceProfile::LowMemory,
    ] {
        let (engine, pipeline) = engine_and_pipeline(&path, profile);
        let refreshed = AtomicBool::new(false);
        let detections = AtomicUsize::new(0);

        std::thread::scope(|scope| {
            for thread in 0..PROCESSING_THREADS {
                let (pipeline, refreshed, detections) = (&pipeline, &refreshed, &detections);
                scope.spawn(move || loop {
                    // Read before processing, so the last loop starts after
                    // every refresh has ended.
                    let last = refreshed.load(Ordering::Acquire);
                    let (user_agent, mobile) = if thread % 2 == 0 {
                        (MOBILE_USER_AGENT, true)
                    } else {
                        (DESKTOP_USER_AGENT, false)
                    };
                    assert_eq!(
                        is_mobile(pipeline, user_agent),
                        Some(mobile),
                        "{profile:?} thread {thread}"
                    );
                    detections.fetch_add(1, Ordering::Relaxed);
                    if last {
                        break;
                    }
                });
            }
            refresh_from_several_threads(&engine, &bytes);
            refreshed.store(true, Ordering::Release);
        });

        assert!(
            detections.load(Ordering::Relaxed) >= PROCESSING_THREADS,
            "{profile:?}: every processing thread detected"
        );
    }
}
