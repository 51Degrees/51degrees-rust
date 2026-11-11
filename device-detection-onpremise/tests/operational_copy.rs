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

//! A data update replaces the data file of a running engine under every
//! profile, and the copy the engine reads leaves with it, as does the copy of
//! a data file that fails to load. The tests need the Lite file
//! `_51DEGREES_DD_PATH` names, and skip with a note without it.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fiftyone_device_detection_onpremise::{
    DeviceDetectionOnPremiseEngine, DeviceDetectionOnPremiseEngineBuilder,
};
use fiftyone_device_detection_shared::{DeviceData, DEVICE_DATA_KEY};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{Evidence, FlowData, FlowElement, Pipeline};
use fiftyone_pipeline_engines::{
    AutoUpdateStatus, DataFileConfiguration, DataUpdateService, OnPremiseAspectEngine,
};

/// A mobile Safari on iPhone User-Agent.
const MOBILE_USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
    AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";

fn lite_data_file() -> Option<PathBuf> {
    std::env::var("_51DEGREES_DD_PATH")
        .ok()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
}

/// A new, empty folder holding a copy of `lite` named `name`.
fn folder_with(lite: &Path, test: &str, name: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("fiftyone-dd-copy-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::copy(lite, &path).unwrap();
    (dir, path)
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Serve `body` to one request, and return the URL.
fn serve_once(body: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/datafile", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 4096];
        let _ = stream.read(&mut request);
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(&body);
    });
    (url, server)
}

/// The data file configuration of automatic updates from `url`, which the
/// test polls itself.
fn updating_config(path: &Path, url: String) -> DataFileConfiguration {
    DataFileConfiguration::builder(path)
        .data_update_url(url)
        .automatic_updates_enabled(true)
        .polling_interval_seconds(24 * 60 * 60)
        .max_randomisation_seconds(0)
        .file_system_watcher_enabled(false)
        .decompress_content(false)
        .verify_md5(false)
        .license_key_required_for_updates(false)
        .build()
}

/// An engine with automatic updates on from `url`, which the test polls
/// itself.
fn updating_engine(
    path: &Path,
    profile: PerformanceProfile,
    url: String,
    service: &Arc<DataUpdateService>,
) -> Arc<DeviceDetectionOnPremiseEngine> {
    DeviceDetectionOnPremiseEngineBuilder::new(path)
        .performance_profile(profile)
        .auto_update(updating_config(path, url), Some(Arc::clone(service)))
        .build()
        .expect("the data file loads")
}

fn detect(engine: &Arc<DeviceDetectionOnPremiseEngine>) -> FlowData {
    let element: Arc<dyn FlowElement> = engine.clone();
    let pipeline = Pipeline::builder().add_element(element).build().unwrap();
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", MOBILE_USER_AGENT)
            .build(),
    );
    data.process().unwrap();
    data
}

fn is_mobile(data: &FlowData) -> Option<bool> {
    data.get(DEVICE_DATA_KEY)
        .unwrap()
        .is_mobile()
        .value()
        .ok()
        .copied()
}

/// With automatic updates on, an update replaces the data file of a running
/// engine under every profile, including the ones that read the data file
/// while the data set lives, and the engine moves to the new file.
#[test]
fn an_update_replaces_the_data_file_under_every_profile() {
    let Some(lite) = lite_data_file() else {
        eprintln!("skipping operational copies: set _51DEGREES_DD_PATH to the Lite file");
        return;
    };
    for profile in [
        PerformanceProfile::Default,
        PerformanceProfile::Balanced,
        PerformanceProfile::LowMemory,
        PerformanceProfile::HighPerformance,
        PerformanceProfile::InMemory,
    ] {
        let (dir, path) = folder_with(&lite, profile.as_str(), "51Degrees.hash");
        let (url, server) = serve_once(std::fs::read(&lite).unwrap());
        let service = Arc::new(DataUpdateService::new());
        let engine = updating_engine(&path, profile, url, &service);
        let as_engine: Arc<dyn OnPremiseAspectEngine> = engine.clone();

        let status = service.check_for_update(&as_engine, None);
        server.join().unwrap();

        assert_eq!(
            status.map_err(|error| error.to_string()),
            Ok(AutoUpdateStatus::Success),
            "{profile:?}"
        );
        assert_eq!(is_mobile(&detect(&engine)), Some(true), "{profile:?}");
        drop(as_engine);
        drop(engine);
        drop(service);
        assert_eq!(files_in(&dir), ["51Degrees.hash"], "{profile:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// The engine reads one copy at a time, and a copy goes once nothing reads
/// the data set it holds, even when a detection outlives a refresh. None is
/// left after the engine is dropped.
#[test]
fn each_copy_goes_with_the_data_set_read_from_it() {
    let Some(lite) = lite_data_file() else {
        eprintln!("skipping operational copies: set _51DEGREES_DD_PATH to the Lite file");
        return;
    };
    let name = format!("copied-{}.hash", std::process::id());
    let (dir, path) = folder_with(&lite, "copies", &name);
    let copies = || {
        files_in(&std::env::temp_dir())
            .into_iter()
            .filter(|file| file.starts_with(&name) && file.ends_with(".operational"))
            .count()
    };
    let service = Arc::new(DataUpdateService::new());
    let engine = updating_engine(
        &path,
        PerformanceProfile::Default,
        String::from("http://127.0.0.1:9/unused"),
        &service,
    );
    assert_eq!(copies(), 1, "one copy for the data set loaded");

    let early = detect(&engine);
    for _ in 0..3 {
        engine.refresh(None).expect("the data file reloads");
    }
    assert_eq!(
        copies(),
        2,
        "the copy the early detection reads, and the newest"
    );
    assert_eq!(is_mobile(&early), Some(true));
    drop(early);
    assert_eq!(copies(), 1, "the early detection's copy goes with it");

    drop(engine);
    drop(service);
    assert_eq!(copies(), 0, "no copy outlives the engine");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The copy is made in the folder the builder names, on build and on each
/// refresh, and none is left there once the engine is dropped.
#[test]
fn the_copy_goes_in_the_folder_the_builder_names() {
    let Some(lite) = lite_data_file() else {
        eprintln!("skipping operational copies: set _51DEGREES_DD_PATH to the Lite file");
        return;
    };
    let (dir, path) = folder_with(&lite, "named", "51Degrees.hash");
    let temp = dir.join("temp");
    std::fs::create_dir_all(&temp).unwrap();
    let copies = || files_in(&temp).len();

    let engine = DeviceDetectionOnPremiseEngineBuilder::new(&path)
        .temp_dir_path(&temp)
        .auto_update(
            updating_config(&path, String::from("http://127.0.0.1:9/unused")),
            None,
        )
        .build()
        .expect("the data file loads");
    assert_eq!(copies(), 1, "the copy is in the named folder");

    engine.refresh(None).expect("the data file reloads");
    assert_eq!(copies(), 1, "the refresh copies into the named folder too");

    drop(engine);
    assert_eq!(copies(), 0, "no copy outlives the engine");
    let _ = std::fs::remove_dir_all(&dir);
}

/// With reuse on, a refresh of an unchanged data file reads the copy the
/// engine already reads rather than making another, and none is left once
/// the engine is dropped. The old data set's removal of the copy it shares
/// unlinks the copy on Linux and macOS, where the new data set keeps the
/// handles it opened, and fails on Windows while the copy is open, so the
/// folder then holds that copy or nothing.
#[test]
fn a_refresh_with_reuse_reads_the_copy_already_made() {
    let Some(lite) = lite_data_file() else {
        eprintln!("skipping operational copies: set _51DEGREES_DD_PATH to the Lite file");
        return;
    };
    let (dir, path) = folder_with(&lite, "reuse-refresh", "51Degrees.hash");
    let temp = dir.join("temp");
    std::fs::create_dir_all(&temp).unwrap();
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(&path)
        .temp_dir_path(&temp)
        .create_temp_data_copy(true)
        .reuse_temp_file(true)
        .build()
        .expect("the data file loads");
    let copied = files_in(&temp);
    assert_eq!(copied.len(), 1, "one copy for the data set loaded");

    engine.refresh(None).expect("the data file reloads");
    let refreshed = files_in(&temp);
    assert!(
        refreshed.iter().all(|name| copied.contains(name)),
        "no new copy: {copied:?} then {refreshed:?}"
    );
    assert_eq!(is_mobile(&detect(&engine)), Some(true));

    drop(engine);
    assert_eq!(
        files_in(&temp),
        Vec::<String>::new(),
        "no copy outlives the engine"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The profiles that keep the data file open, so the engine reads a copy.
const COPYING_PROFILES: [PerformanceProfile; 4] = [
    PerformanceProfile::Default,
    PerformanceProfile::Balanced,
    PerformanceProfile::LowMemory,
    PerformanceProfile::HighPerformance,
];

/// Where the Hash data file's header gives the place of the properties, and
/// where a property's description offset sits in its record, from `hash.h`
/// and `property.h` in 51degrees-cxx.
const PROPERTIES_HEADER: usize = 110;
const PROPERTY_SIZE: usize = 44;
const DESCRIPTION_OFFSET: usize = 16;

/// The Lite file with its first property's description past the end of the
/// strings. The data set loads, because loading reads no description, and
/// reading the engine's metadata then fails.
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

/// A build that fails after the data set is open leaves no copy, because the
/// data set is freed before its copy is removed. Windows cannot remove a file
/// that is open, so there the order decides whether the copy is left.
#[test]
fn a_build_that_fails_leaves_no_copy() {
    let Some(lite) = lite_data_file() else {
        eprintln!("skipping operational copies: set _51DEGREES_DD_PATH to the Lite file");
        return;
    };
    let unreadable = with_unreadable_metadata(&lite);
    for profile in COPYING_PROFILES {
        let test = format!("failed-build-{}", profile.as_str());
        let (dir, path) = folder_with(&lite, &test, "51Degrees.hash");
        std::fs::write(&path, &unreadable).unwrap();
        let temp = dir.join("temp");
        std::fs::create_dir_all(&temp).unwrap();

        let error = DeviceDetectionOnPremiseEngineBuilder::new(&path)
            .performance_profile(profile)
            .temp_dir_path(&temp)
            .auto_update(
                updating_config(&path, String::from("http://127.0.0.1:9/unused")),
                None,
            )
            .build()
            .err()
            .expect("the metadata cannot be read");

        assert!(is_unreadable_metadata(&error), "{profile:?}: {error}");
        assert_eq!(files_in(&temp), Vec::<String>::new(), "{profile:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A refresh that fails after the new data set is open leaves only the copy
/// the engine still reads, and the engine keeps detecting from it.
#[test]
fn a_refresh_that_fails_leaves_only_the_copy_in_use() {
    let Some(lite) = lite_data_file() else {
        eprintln!("skipping operational copies: set _51DEGREES_DD_PATH to the Lite file");
        return;
    };
    let unreadable = with_unreadable_metadata(&lite);
    for profile in COPYING_PROFILES {
        let test = format!("failed-refresh-{}", profile.as_str());
        let (dir, path) = folder_with(&lite, &test, "51Degrees.hash");
        let temp = dir.join("temp");
        std::fs::create_dir_all(&temp).unwrap();
        let engine = DeviceDetectionOnPremiseEngineBuilder::new(&path)
            .performance_profile(profile)
            .temp_dir_path(&temp)
            .auto_update(
                updating_config(&path, String::from("http://127.0.0.1:9/unused")),
                None,
            )
            .build()
            .expect("the data file loads");
        let in_use = files_in(&temp);
        assert_eq!(in_use.len(), 1, "{profile:?}");

        std::fs::write(&path, &unreadable).unwrap();
        let error = engine
            .refresh(None)
            .expect_err("the new metadata cannot be read");

        assert!(is_unreadable_metadata(&error), "{profile:?}: {error}");
        assert_eq!(files_in(&temp), in_use, "{profile:?}");
        assert_eq!(is_mobile(&detect(&engine)), Some(true), "{profile:?}");
        drop(engine);
        assert_eq!(files_in(&temp), Vec::<String>::new(), "{profile:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
