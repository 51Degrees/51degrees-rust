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

//! The data updates specification says "If the updated data source cannot be
//! used, the refresh MUST fail with an error, and the Engine MUST go on using
//! the data source it had, so that processing is unaffected." A data file
//! that the C library loads but whose metadata cannot be read is one the
//! engine cannot use, so a refresh to it fails and the engine keeps the data
//! set it had, under every profile, from a file and from memory. The tests
//! need the Lite file, from `_51DEGREES_DD_PATH` or the data repository in
//! the `cxx` submodule, and skip with a note without it.

#[allow(dead_code)]
#[path = "../../deprecated-names/environment.rs"]
mod deprecated_names;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use fiftyone_device_detection_onpremise::{
    DeviceDetectionOnPremiseEngine, DeviceDetectionOnPremiseEngineBuilder,
};
use fiftyone_device_detection_shared::{DeviceData, DEVICE_DATA_KEY};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{Evidence, FlowElement, Pipeline};
use fiftyone_pipeline_engines::{DataFileConfiguration, OnPremiseAspectEngine};

/// A mobile Safari on iPhone User-Agent.
const MOBILE_USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
    AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";

/// Every profile, both those that read a copy of the data file and those
/// that hold the data set in memory.
const PROFILES: [PerformanceProfile; 5] = [
    PerformanceProfile::Default,
    PerformanceProfile::Balanced,
    PerformanceProfile::LowMemory,
    PerformanceProfile::HighPerformance,
    PerformanceProfile::InMemory,
];

/// Where the Hash data file's header gives the place of the properties, and
/// where a property's description offset sits in its record, from `hash.h`
/// and `property.h` in 51degrees-cxx.
const PROPERTIES_HEADER: usize = 110;
const PROPERTY_SIZE: usize = 44;
const DESCRIPTION_OFFSET: usize = 16;

/// The Lite file `_51DEGREES_DD_PATH` names, or the one in the data
/// repository inside the `cxx` submodule.
fn lite_data_file() -> Option<PathBuf> {
    if let Ok(path) = crate::deprecated_names::var("_51DEGREES_DD_PATH") {
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

/// The Lite file with its first property's description past the end of the
/// strings. The C library loads it, because loading reads no description,
/// and reading the engine's metadata then fails.
fn with_unreadable_metadata(lite: &Path) -> Vec<u8> {
    let mut bytes = std::fs::read(lite).unwrap();
    let read_u32 = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let start = read_u32(PROPERTIES_HEADER) as usize;
    let length = read_u32(PROPERTIES_HEADER + 4) as usize;
    let count = read_u32(PROPERTIES_HEADER + 8) as usize;
    assert_eq!(length, count * PROPERTY_SIZE, "the properties are found");
    let at = start + DESCRIPTION_OFFSET;
    bytes[at..at + 4].copy_from_slice(&(i32::MAX as u32).to_le_bytes());
    bytes
}

fn is_unreadable_metadata(error: &impl std::fmt::Display) -> bool {
    error
        .to_string()
        .contains("the data file's metadata could not be read (property 0)")
}

/// A new, empty folder holding a copy of `lite`, with a folder for the
/// engine's copies of it.
fn folder_with(lite: &Path, test: &str) -> (PathBuf, PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("fiftyone-dd-keeps-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let temp = dir.join("temp");
    std::fs::create_dir_all(&temp).unwrap();
    let path = dir.join("51Degrees.hash");
    std::fs::copy(lite, &path).unwrap();
    (dir, path, temp)
}

/// An engine over the data file at `path` under `profile`, copying it into
/// `temp` where the profile needs a copy, with no update service.
fn engine(
    path: &Path,
    temp: &Path,
    profile: PerformanceProfile,
) -> Arc<DeviceDetectionOnPremiseEngine> {
    let config = DataFileConfiguration::builder(path)
        .automatic_updates_enabled(false)
        .file_system_watcher_enabled(false)
        .license_key_required_for_updates(false)
        .build();
    Arc::new(
        DeviceDetectionOnPremiseEngineBuilder::new(path)
            .performance_profile(profile)
            .temp_dir_path(temp)
            .auto_update(config, None)
            .build()
            .expect("the data file loads"),
    )
}

/// Whether `engine` detects the mobile User-Agent as a mobile device.
fn is_mobile(engine: &Arc<DeviceDetectionOnPremiseEngine>) -> Option<bool> {
    let element: Arc<dyn FlowElement> = engine.clone();
    let pipeline = Pipeline::builder().add_element(element).build().unwrap();
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", MOBILE_USER_AGENT)
            .build(),
    );
    data.process().unwrap();
    let mobile = data
        .get(DEVICE_DATA_KEY)
        .unwrap()
        .is_mobile()
        .value()
        .ok()
        .copied();
    mobile
}

/// The data set the engine reads is the one it had, being one whose
/// metadata reads, and detections are unaffected.
fn assert_uses_the_data_it_had(engine: &Arc<DeviceDetectionOnPremiseEngine>, context: &str) {
    let properties = engine.data_set().data_file_properties();
    assert!(
        properties.is_ok(),
        "{context}: the engine reads the data set that failed: {}",
        properties.err().map(|e| e.to_string()).unwrap_or_default()
    );
    assert_eq!(is_mobile(engine), Some(true), "{context}");
}

/// A refresh from a data file whose metadata cannot be read fails with an
/// error, and the engine goes on using the data set it had, under every
/// profile.
#[test]
fn a_refresh_from_a_file_that_cannot_be_used_keeps_the_data_set_it_had() {
    let Some(lite) = lite_data_file() else {
        eprintln!("skipping the failed refresh: set _51DEGREES_DD_PATH to the Lite file");
        return;
    };
    let unreadable = with_unreadable_metadata(&lite);
    for profile in PROFILES {
        let (dir, path, temp) = folder_with(&lite, &format!("file-{}", profile.as_str()));
        let engine = engine(&path, &temp, profile);

        std::fs::write(&path, &unreadable).unwrap();
        let error = engine
            .refresh(None)
            .expect_err("the new data file cannot be used");

        assert!(is_unreadable_metadata(&error), "{profile:?}: {error}");
        assert_uses_the_data_it_had(&engine, &format!("{profile:?} from the file"));
        drop(engine);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A refresh from memory holding a data file whose metadata cannot be read
/// fails with an error, and the engine goes on using the data set it had,
/// under every profile.
#[test]
fn a_refresh_from_memory_that_cannot_be_used_keeps_the_data_set_it_had() {
    let Some(lite) = lite_data_file() else {
        eprintln!("skipping the failed refresh: set _51DEGREES_DD_PATH to the Lite file");
        return;
    };
    let unreadable = with_unreadable_metadata(&lite);
    for profile in PROFILES {
        let (dir, path, temp) = folder_with(&lite, &format!("memory-{}", profile.as_str()));
        let engine = engine(&path, &temp, profile);

        let error = engine
            .refresh_from_memory(None, &unreadable)
            .expect_err("the new data cannot be used");

        assert!(is_unreadable_metadata(&error), "{profile:?}: {error}");
        assert_uses_the_data_it_had(&engine, &format!("{profile:?} from memory"));
        drop(engine);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
