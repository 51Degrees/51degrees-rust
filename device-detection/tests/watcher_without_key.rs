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

//! The file system watcher runs on its own setting and needs no licence key.
//! A pipeline built with automatic updates off and no key reloads its engine
//! when a new data file replaces the one it was built with. The test needs
//! the Lite file `_51DEGREES_DD_PATH` names, or the one at
//! `cxx/data/device-detection`, and skips with a note without it.
#![cfg(feature = "on-premise")]

#[allow(dead_code)]
#[path = "../../deprecated-names/environment.rs"]
mod deprecated_names;

use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant, SystemTime};

use fiftyone_device_detection::{DeviceDetectionOnPremisePipelineBuilder, PerformanceProfile};
use fiftyone_pipeline_engines::{AutoUpdateStatus, DataUpdateService};

/// How long the watcher is given to start before the file is replaced.
const WATCHER_START: Duration = Duration::from_secs(1);

/// How long the test waits for the engine to reload. The wait ends as soon
/// as it reloads, so the limit only matters when it never does.
const RELOAD_WAIT: Duration = Duration::from_secs(60);

/// The Lite file `_51DEGREES_DD_PATH` names, or the one under `cxx/data`, or
/// `None` with a note that the test is skipped.
fn lite_file() -> Option<PathBuf> {
    let bundled = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cxx/data/device-detection/51Degrees-LiteV4.1.hash");
    let found = crate::deprecated_names::var("_51DEGREES_DD_PATH")
        .ok()
        .map(PathBuf::from)
        .into_iter()
        .chain([bundled])
        .find(|path| path.is_file());
    if found.is_none() {
        eprintln!("skipped: no Lite file at _51DEGREES_DD_PATH or cxx/data");
    }
    found
}

/// Replace the file at `path` with a copy of `source`, as a deployment puts
/// a new data file in place, by renaming a new file over it. The new file is
/// given the current time, so it is newer than the one the engine loaded.
fn replace(path: &Path, source: &Path) {
    let staged = path.with_extension("new");
    std::fs::copy(source, &staged).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&staged)
        .unwrap()
        .set_modified(SystemTime::now())
        .unwrap();
    if let Err(error) = std::fs::rename(&staged, path) {
        panic!(
            "the new data file could not replace the one the engine was built \
             with, so the engine holds that file open rather than reading a \
             copy: {error}"
        );
    }
}

/// With automatic updates off, no licence key and the watcher left on, as
/// it is by default, replacing the data file reloads the engine, and the
/// pipeline goes on processing.
#[test]
fn the_watcher_reloads_the_engine_with_no_key_and_automatic_updates_off() {
    let Some(lite) = lite_file() else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("fiftyone-dd-watcher-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("51Degrees-LiteV4.1.hash");
    std::fs::copy(&lite, &path).unwrap();

    let service = Arc::new(DataUpdateService::new());
    let (sender, statuses) = mpsc::channel();
    service.on_update_complete(move |complete| {
        let _ = sender.send(complete.status);
    });
    let pipeline = DeviceDetectionOnPremisePipelineBuilder::new(&path)
        .performance_profile(PerformanceProfile::Balanced)
        .auto_update(false)
        .share_usage(false)
        .data_update_service(Arc::clone(&service))
        .build()
        .expect("a pipeline with automatic updates off needs no licence key");
    std::thread::sleep(WATCHER_START);

    replace(&path, &lite);

    let start = Instant::now();
    let mut seen = Vec::new();
    while !seen.contains(&AutoUpdateStatus::Success) {
        let left = RELOAD_WAIT.saturating_sub(start.elapsed());
        match statuses.recv_timeout(left) {
            Ok(status) => seen.push(status),
            Err(_) => panic!(
                "the engine did not reload within {RELOAD_WAIT:?} of the data file \
                 being replaced, with the watcher on and automatic updates off. \
                 Checks reported: {seen:?}"
            ),
        }
    }

    let mut flow_data = pipeline.create_flow_data();
    flow_data.add_evidence("header.user-agent", "Mozilla/5.0");
    flow_data.process().expect("the reloaded engine processes");
    drop(pipeline);
    drop(service);
    let _ = std::fs::remove_dir_all(&dir);
}
