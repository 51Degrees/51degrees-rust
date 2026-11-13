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

//! Tests of the data update service's file system watcher, which refreshes an
//! engine when its data file changes on disk.
#![cfg(feature = "data-update")]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fiftyone_pipeline_core::{
    EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData, Result,
};
use fiftyone_pipeline_engines::{
    AspectEngine, AspectEngineDataFile, AspectPropertyMetaData, DataFileConfiguration,
    DataUpdateService, OnPremiseAspectEngine,
};

/// How long a test waits for a refresh it expects.
const REFRESH_WAIT: Duration = Duration::from_secs(10);

/// How long a test waits before deciding a refresh it does not expect has
/// not happened.
const QUIET_WAIT: Duration = Duration::from_secs(2);

/// How long the watcher is given to start before the file changes.
const WATCHER_START: Duration = Duration::from_millis(300);

/// An engine that counts its refreshes.
struct CountingEngine {
    filter: EvidenceKeyFilterWhitelist,
    data_files: Vec<Arc<AspectEngineDataFile>>,
    refreshes: Arc<AtomicUsize>,
}

impl FlowElement for CountingEngine {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
        Ok(())
    }
    fn data_key(&self) -> &str {
        "counting"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

impl AspectEngine for CountingEngine {
    fn data_source_tier(&self) -> &str {
        "Lite"
    }
    fn aspect_properties(&self) -> &[AspectPropertyMetaData] {
        &[]
    }
}

impl OnPremiseAspectEngine for CountingEngine {
    fn data_files(&self) -> &[Arc<AspectEngineDataFile>] {
        &self.data_files
    }
    fn refresh(&self, _identifier: Option<&str>) -> Result<()> {
        self.refreshes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

/// A new, empty folder for one test, holding a data file.
fn folder(test: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("fiftyone-watcher-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("device.dat");
    std::fs::write(&path, b"version 1").unwrap();
    (dir, path)
}

/// An engine whose data file at `path` is watched, with automatic updates on
/// or off. There is no update URL, so a poll makes no request.
fn watched_engine(path: &Path, automatic_updates: bool) -> Arc<CountingEngine> {
    let config = DataFileConfiguration::builder(path)
        .automatic_updates_enabled(automatic_updates)
        .file_system_watcher_enabled(true)
        .build();
    Arc::new(CountingEngine {
        filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        data_files: vec![Arc::new(AspectEngineDataFile::new(config))],
        refreshes: Arc::new(AtomicUsize::new(0)),
    })
}

fn register(service: &DataUpdateService, engine: &Arc<CountingEngine>) {
    let data_file = Arc::clone(&engine.data_files[0]);
    let engine: Arc<dyn OnPremiseAspectEngine> = engine.clone();
    service.register(engine, data_file).unwrap();
    std::thread::sleep(WATCHER_START);
}

/// The refresh count once it reaches `at_least`, or after `limit`.
fn refreshes_after(count: &AtomicUsize, at_least: usize, limit: Duration) -> usize {
    let start = Instant::now();
    while count.load(Ordering::SeqCst) < at_least && start.elapsed() < limit {
        std::thread::sleep(Duration::from_millis(20));
    }
    count.load(Ordering::SeqCst)
}

/// Dropping the service releases the engines registered with it and stops
/// their watchers, so the data set is freed and a later change to the file
/// refreshes nothing.
#[test]
fn dropping_the_service_releases_the_engine_and_stops_the_watcher() {
    let (dir, path) = folder("drop");
    let engine = watched_engine(&path, true);
    let refreshes = Arc::clone(&engine.refreshes);
    let service = DataUpdateService::new();
    register(&service, &engine);
    let released = Arc::downgrade(&engine);

    drop(engine);
    drop(service);

    assert!(
        released.upgrade().is_none(),
        "the engine is still alive after the service and its caller dropped it"
    );
    std::fs::write(&path, b"version 2").unwrap();
    assert_eq!(refreshes_after(&refreshes, 1, QUIET_WAIT), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The service holds an engine weakly, as a .NET engine unregisters its data
/// files when disposed. Dropping the engine, or the pipeline holding it,
/// frees its data set and ends its registration while the service lives on,
/// so a later change to the file refreshes nothing. The service's unit tests
/// check that the registration's watcher goes with it.
#[test]
fn dropping_the_engine_ends_its_registration() {
    let (dir, path) = folder("engine");
    let engine = watched_engine(&path, true);
    let refreshes = Arc::clone(&engine.refreshes);
    let service = DataUpdateService::new();
    register(&service, &engine);
    assert_eq!(service.registered_count(), 1);
    let released = Arc::downgrade(&engine);

    drop(engine);

    assert!(
        released.upgrade().is_none(),
        "the service kept the engine alive after its caller dropped it"
    );
    assert_eq!(service.registered_count(), 0);
    std::fs::write(&path, b"version 2").unwrap();
    assert_eq!(refreshes_after(&refreshes, 1, QUIET_WAIT), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Automatic updates are the master switch for the watcher, as the
/// specification says. With them off a change to the file refreshes nothing,
/// and with them on it refreshes the engine.
#[test]
fn the_watcher_runs_only_with_automatic_updates_on() {
    let (dir, path) = folder("switch");
    let service = DataUpdateService::new();

    let off = watched_engine(&path, false);
    register(&service, &off);
    std::fs::write(&path, b"version 2").unwrap();
    assert_eq!(
        refreshes_after(&off.refreshes, 1, QUIET_WAIT),
        0,
        "the watcher refreshed an engine with automatic updates off"
    );

    let on = watched_engine(&path, true);
    register(&service, &on);
    std::fs::write(&path, b"version 3").unwrap();
    assert!(refreshes_after(&on.refreshes, 1, REFRESH_WAIT) >= 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The watcher refreshes the engine when its data file changes, and not when
/// another file in the same folder does.
#[test]
fn the_watcher_ignores_other_files_in_the_folder() {
    let (dir, path) = folder("others");
    let service = DataUpdateService::new();
    let engine = watched_engine(&path, true);
    register(&service, &engine);

    std::fs::write(dir.join("application.log"), b"a line").unwrap();
    assert_eq!(
        refreshes_after(&engine.refreshes, 1, QUIET_WAIT),
        0,
        "another file in the folder refreshed the engine"
    );

    std::fs::write(&path, b"version 2").unwrap();
    assert!(refreshes_after(&engine.refreshes, 1, REFRESH_WAIT) >= 1);
    let _ = std::fs::remove_dir_all(&dir);
}
