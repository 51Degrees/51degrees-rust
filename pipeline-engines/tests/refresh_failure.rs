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

//! A refresh that fails, by each route the data update service refreshes an
//! engine: a programmatic update, an update from memory, the update on
//! start-up, a scheduled poll and a change to a watched file. Each failure is
//! logged once, at error level, with the message the specification's
//! [errors table](https://github.com/51Degrees/specifications/blob/16ecea439490552e3d1f13c17dc48ac8ae8d9684/pipeline-specification/features/data-updates.md#errors)
//! gives for a refresh, and is reported to the update callbacks as a
//! failure, never a success. Polling and watching go on, so a good update
//! that follows is applied. Kept in its own test binary because a logger is
//! installed once for the whole process.
#![cfg(feature = "data-update")]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fiftyone_pipeline_core::{
    Error, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData,
    Result,
};
use fiftyone_pipeline_engines::{
    AspectEngine, AspectEngineDataFile, AspectPropertyMetaData, AutoUpdateStatus,
    DataFileConfiguration, DataUpdateError, DataUpdateService, OnPremiseAspectEngine,
};

/// How long a test waits for something it expects of the service.
const WAIT: Duration = Duration::from_secs(10);

/// How often a test checks again for something it is waiting on.
const CHECK_AGAIN: Duration = Duration::from_millis(20);

/// How long the watcher is given to start before the file changes.
const WATCHER_START: Duration = Duration::from_millis(300);

/// Long enough for a file's modified time to move on between two writes.
const NEXT_WRITE: Duration = Duration::from_millis(100);

/// The error a failing refresh of [`FailingEngine`] gives.
const REFUSED: &str = "the new data cannot be used";

/// What a callback that panics says.
const PANIC: &str = "a callback that panics";

/// What a refresh of [`FailingEngine`] that panics says.
const REFRESH_PANIC: &str = "a refresh that panics";

/// The start of the record logged for a refresh that failed.
const APPLYING: &str = "ERROR An error occurred while applying a data file update to failing.";

/// How much older than the file last applied a file put in place is.
const OLDER: Duration = Duration::from_secs(60);

/// The start of the record logged for a callback that panicked.
const PANIC_RECORD: &str = "ERROR An update callback for failing panicked.";

/// Log records at warning level or above, as "LEVEL message".
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Keeps every record at warning level or above in [`LOGGED`].
struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            LOGGED
                .lock()
                .unwrap()
                .push(format!("{} {}", record.level(), record.args()));
        }
    }

    fn flush(&self) {}
}

static LOGGER: TestLogger = TestLogger;

/// The records logged since the last call, which clears them.
fn take() -> Vec<String> {
    std::mem::take(&mut *LOGGED.lock().unwrap())
}

/// An engine with one data file whose first refreshes panic or fail and
/// whose later ones work, counting them all.
struct FailingEngine {
    filter: EvidenceKeyFilterWhitelist,
    data_files: Vec<Arc<AspectEngineDataFile>>,
    panics: AtomicUsize,
    failures: Mutex<usize>,
    refreshes: AtomicUsize,
}

impl FailingEngine {
    /// One refresh, which panics while panics are left, then fails while
    /// failures are left.
    fn attempt(&self) -> Result<()> {
        self.refreshes.fetch_add(1, Ordering::SeqCst);
        let panics = self
            .panics
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                left.checked_sub(1)
            })
            .is_ok();
        if panics {
            panic!("{REFRESH_PANIC}");
        }
        let fails = {
            let mut left = self.failures.lock().unwrap();
            let fails = *left > 0;
            if fails {
                *left -= 1;
            }
            fails
        };
        if fails {
            Err(Error::configuration(REFUSED))
        } else {
            Ok(())
        }
    }
}

impl FlowElement for FailingEngine {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
        Ok(())
    }
    fn data_key(&self) -> &str {
        "failing"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

impl AspectEngine for FailingEngine {
    fn data_source_tier(&self) -> &str {
        "Lite"
    }
    fn aspect_properties(&self) -> &[AspectPropertyMetaData] {
        &[]
    }
}

impl OnPremiseAspectEngine for FailingEngine {
    fn data_files(&self) -> &[Arc<AspectEngineDataFile>] {
        &self.data_files
    }
    fn refresh(&self, _identifier: Option<&str>) -> Result<()> {
        self.attempt()
    }
    fn refresh_from_memory(&self, _identifier: Option<&str>, _data: &[u8]) -> Result<()> {
        self.attempt()
    }
}

/// A [`FailingEngine`] for `configuration` whose first `failures` refreshes
/// fail, returned twice, once to count its refreshes and once as the service
/// takes it.
fn engine(
    configuration: DataFileConfiguration,
    failures: usize,
) -> (Arc<FailingEngine>, Arc<dyn OnPremiseAspectEngine>) {
    panicking(configuration, 0, failures)
}

/// A [`FailingEngine`] for `configuration` whose first `panics` refreshes
/// panic and whose next `failures` fail, returned as [`engine`] returns it.
fn panicking(
    configuration: DataFileConfiguration,
    panics: usize,
    failures: usize,
) -> (Arc<FailingEngine>, Arc<dyn OnPremiseAspectEngine>) {
    let engine = Arc::new(FailingEngine {
        filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        data_files: vec![Arc::new(AspectEngineDataFile::new(configuration))],
        panics: AtomicUsize::new(panics),
        failures: Mutex::new(failures),
        refreshes: AtomicUsize::new(0),
    });
    let shared: Arc<dyn OnPremiseAspectEngine> = engine.clone();
    (engine, shared)
}

/// The engine's only data file.
fn data_file(engine: &Arc<dyn OnPremiseAspectEngine>) -> Arc<AspectEngineDataFile> {
    Arc::clone(&engine.data_files()[0])
}

/// The record a refresh that failed with `error` logs.
fn applying(error: &DataUpdateError) -> String {
    format!(
        "ERROR An error occurred while applying a data file update to failing. Error detail: \
         {error}"
    )
}

/// What each report gave, as whether it named the data file expected and
/// its status.
type Reports = Arc<Mutex<Vec<(bool, AutoUpdateStatus)>>>;

/// Add a callback to `service` that keeps every report it is given.
fn reports(service: &DataUpdateService, expected: &Arc<AspectEngineDataFile>) -> Reports {
    let reports = Reports::default();
    let kept = Arc::clone(&reports);
    let expected = Arc::clone(expected);
    service.on_update_complete(move |report| {
        let named = Arc::ptr_eq(&report.data_file, &expected);
        kept.lock().unwrap().push((named, report.status));
    });
    reports
}

/// The statuses reported so far, each for the data file expected.
fn statuses(reports: &Reports) -> Vec<AutoUpdateStatus> {
    let reports = reports.lock().unwrap();
    assert!(
        reports.iter().all(|(named, _)| *named),
        "a report named another data file: {reports:?}"
    );
    reports.iter().map(|(_, status)| *status).collect()
}

/// Wait until `done` holds for the statuses reported, or [`WAIT`] passes.
fn wait_for(reports: &Reports, done: impl Fn(&[AutoUpdateStatus]) -> bool) {
    let start = Instant::now();
    while !done(&statuses(reports)) && start.elapsed() < WAIT {
        std::thread::sleep(CHECK_AGAIN);
    }
}

/// An update URL on this machine that answers every request with `body`.
fn serve(body: &'static [u8]) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/datafile", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                continue;
            };
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body);
        }
    });
    url
}

/// A programmatic update that refreshes the engine from its file.
fn a_programmatic_update(dir: &Path, refused: &str) {
    let path = dir.join("programmatic.dat");
    std::fs::write(&path, b"version 1").unwrap();
    let (engine, shared) = engine(
        DataFileConfiguration::builder(&path)
            .automatic_updates_enabled(false)
            .build(),
        1,
    );
    let service = DataUpdateService::new();
    let reports = reports(&service, &data_file(&shared));

    let error = service
        .check_for_update(&shared, None)
        .expect_err("the refresh fails");
    assert_eq!(error.status, AutoUpdateStatus::RefreshFailed);
    assert_eq!(take(), [refused], "a programmatic update");
    assert_eq!(statuses(&reports), [AutoUpdateStatus::RefreshFailed]);

    let status = service.check_for_update(&shared, None).unwrap();
    assert_eq!(status, AutoUpdateStatus::Success);
    assert_eq!(take(), Vec::<String>::new());
    assert_eq!(
        statuses(&reports),
        [AutoUpdateStatus::RefreshFailed, AutoUpdateStatus::Success]
    );
    assert_eq!(engine.refreshes.load(Ordering::SeqCst), 2);
}

/// An update from data in memory.
fn an_update_from_memory(refused: &str) {
    let (engine, shared) = engine(
        DataFileConfiguration::memory_builder()
            .automatic_updates_enabled(false)
            .build(),
        1,
    );
    let service = DataUpdateService::new();
    let reports = reports(&service, &data_file(&shared));

    let error = service
        .update_from_memory(&shared, None, b"new data")
        .expect_err("the refresh fails");
    assert_eq!(error.status, AutoUpdateStatus::RefreshFailed);
    assert_eq!(take(), [refused], "an update from memory");
    assert_eq!(statuses(&reports), [AutoUpdateStatus::RefreshFailed]);

    let status = service
        .update_from_memory(&shared, None, b"new data")
        .unwrap();
    assert_eq!(status, AutoUpdateStatus::Success);
    assert_eq!(take(), Vec::<String>::new());
    assert_eq!(
        statuses(&reports),
        [AutoUpdateStatus::RefreshFailed, AutoUpdateStatus::Success]
    );
    assert_eq!(engine.refreshes.load(Ordering::SeqCst), 2);
}

/// The update on start-up, which logs the refresh's own record and not the
/// start-up warning as well.
fn the_update_on_start_up(refused: &str) {
    let url = serve(b"new data");
    let (engine, shared) = engine(
        DataFileConfiguration::memory_builder()
            .data_update_url(url)
            .update_on_startup(true)
            .automatic_updates_enabled(false)
            .decompress_content(false)
            .build(),
        1,
    );
    let service = DataUpdateService::new();
    let reports = reports(&service, &data_file(&shared));

    let error = service
        .register(Arc::clone(&shared), data_file(&shared))
        .expect_err("the refresh fails");
    assert_eq!(error.status, AutoUpdateStatus::RefreshFailed);
    assert_eq!(take(), [refused], "the update on start-up");
    assert_eq!(statuses(&reports), [AutoUpdateStatus::RefreshFailed]);

    let status = service.check_for_update(&shared, None).unwrap();
    assert_eq!(status, AutoUpdateStatus::Success);
    assert_eq!(take(), Vec::<String>::new());
    assert_eq!(
        statuses(&reports),
        [AutoUpdateStatus::RefreshFailed, AutoUpdateStatus::Success]
    );
    assert_eq!(engine.refreshes.load(Ordering::SeqCst), 2);
}

/// A scheduled poll, retried one interval after the refresh fails, when it
/// applies the update. A callback that panics stops neither the callbacks
/// after it nor the polls that follow.
fn a_scheduled_poll(refused: &str) {
    let url = serve(b"new data");
    let (engine, shared) = engine(
        DataFileConfiguration::memory_builder()
            .data_update_url(url)
            .polling_interval_seconds(1)
            .max_randomisation_seconds(0)
            .decompress_content(false)
            .build(),
        1,
    );
    let service = DataUpdateService::new();
    service.on_update_complete(|_| panic!("{PANIC}"));
    let reports = reports(&service, &data_file(&shared));

    let status = service
        .register(Arc::clone(&shared), data_file(&shared))
        .unwrap();
    assert_eq!(status, AutoUpdateStatus::NotNeeded);
    wait_for(&reports, |statuses| statuses.len() >= 2);
    drop(service);

    let statuses = statuses(&reports);
    assert!(statuses.len() >= 2, "{statuses:?}");
    assert_eq!(
        statuses[..2],
        [AutoUpdateStatus::RefreshFailed, AutoUpdateStatus::Success]
    );
    let records = take();
    assert_eq!(
        records.iter().filter(|record| *record == refused).count(),
        1,
        "{records:?}"
    );
    assert_eq!(
        records
            .iter()
            .filter(|record| record.starts_with(PANIC_RECORD) && record.contains(PANIC))
            .count(),
        statuses.len(),
        "{records:?}"
    );
    assert_eq!(records.len(), 1 + statuses.len(), "{records:?}");
    assert!(engine.refreshes.load(Ordering::SeqCst) >= 2);
}

/// A change to a watched file. The file stays watched after its refresh
/// fails, so a file written after it is applied.
fn a_watched_file(dir: &Path, refused: &str) {
    let path = dir.join("watched.dat");
    std::fs::write(&path, b"version 1").unwrap();
    let (engine, shared) = engine(DataFileConfiguration::builder(&path).build(), 1);
    let service = DataUpdateService::new();
    let reports = reports(&service, &data_file(&shared));
    let status = service
        .register(Arc::clone(&shared), data_file(&shared))
        .unwrap();
    assert_eq!(status, AutoUpdateStatus::NotNeeded);
    std::thread::sleep(WATCHER_START);

    std::fs::write(&path, b"version 2").unwrap();
    wait_for(&reports, |statuses| !statuses.is_empty());
    assert_eq!(
        statuses(&reports).first(),
        Some(&AutoUpdateStatus::RefreshFailed),
        "the watcher did not report the refresh that failed"
    );

    std::thread::sleep(NEXT_WRITE);
    std::fs::write(&path, b"version 3").unwrap();
    wait_for(&reports, |statuses| {
        statuses.last() == Some(&AutoUpdateStatus::Success)
    });
    drop(service);

    // One write can raise several events, and each after the first finds
    // the file no newer, so it is reported as needing no update.
    let statuses: Vec<AutoUpdateStatus> = statuses(&reports)
        .into_iter()
        .filter(|status| *status != AutoUpdateStatus::NotNeeded)
        .collect();
    assert_eq!(statuses[0], AutoUpdateStatus::RefreshFailed, "{statuses:?}");
    assert!(statuses.len() >= 2, "{statuses:?}");
    assert!(
        statuses[1..]
            .iter()
            .all(|status| *status == AutoUpdateStatus::Success),
        "{statuses:?}"
    );
    assert_eq!(take(), [refused], "a watched file");
    assert_eq!(engine.refreshes.load(Ordering::SeqCst), statuses.len());
}

/// A change to a watched file that leaves it no newer than the file last
/// applied, as when an older copy is put in place, is reported as needing no
/// update, as .NET reports every event, and the engine is not refreshed.
fn a_watched_file_that_is_not_newer(dir: &Path) {
    let path = dir.join("not-newer.dat");
    std::fs::write(&path, b"version 1").unwrap();
    let (engine, shared) = engine(DataFileConfiguration::builder(&path).build(), 0);
    let service = DataUpdateService::new();
    let reports = reports(&service, &data_file(&shared));
    service
        .register(Arc::clone(&shared), data_file(&shared))
        .unwrap();
    std::thread::sleep(WATCHER_START);

    std::fs::write(&path, b"version 2").unwrap();
    wait_for(&reports, |statuses| {
        statuses.contains(&AutoUpdateStatus::Success)
    });
    let applied = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::thread::sleep(WATCHER_START);
    let before = statuses(&reports).len();

    // The older copy is written beside the file and renamed over it, so no
    // event can see a newer modified time.
    let older = dir.join("not-newer.tmp");
    std::fs::write(&older, b"version 0").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&older)
        .unwrap()
        .set_modified(applied - OLDER)
        .unwrap();
    std::fs::rename(&older, &path).unwrap();
    wait_for(&reports, |statuses| statuses.len() > before);
    drop(service);

    let statuses = statuses(&reports);
    assert!(
        statuses.len() > before,
        "the change to an older file was not reported: {statuses:?}"
    );
    assert!(
        statuses[before..]
            .iter()
            .all(|status| *status == AutoUpdateStatus::NotNeeded),
        "{statuses:?}"
    );
    assert_eq!(engine.refreshes.load(Ordering::SeqCst), 1, "{statuses:?}");
    assert_eq!(
        take(),
        Vec::<String>::new(),
        "a watched file that is not newer"
    );
}

/// A refresh that panics, in a scheduled poll and after a change to a
/// watched file, is logged and reported as a refresh that failed, and the
/// poll and the change after it are still applied.
fn a_refresh_that_panics(dir: &Path) {
    let url = serve(b"new data");
    let (engine, shared) = panicking(
        DataFileConfiguration::memory_builder()
            .data_update_url(url)
            .polling_interval_seconds(1)
            .max_randomisation_seconds(0)
            .decompress_content(false)
            .build(),
        1,
        0,
    );
    let service = DataUpdateService::new();
    let reports = reports(&service, &data_file(&shared));
    service
        .register(Arc::clone(&shared), data_file(&shared))
        .unwrap();
    wait_for(&reports, |statuses| statuses.len() >= 2);
    drop(service);
    let statuses = statuses(&reports);
    assert!(statuses.len() >= 2, "a scheduled poll: {statuses:?}");
    assert_eq!(
        statuses[..2],
        [AutoUpdateStatus::RefreshFailed, AutoUpdateStatus::Success],
        "a scheduled poll"
    );
    assert!(engine.refreshes.load(Ordering::SeqCst) >= 2);
    assert_panic_logged("a scheduled poll");

    let path = dir.join("panics.dat");
    std::fs::write(&path, b"version 1").unwrap();
    let (engine, shared) = panicking(DataFileConfiguration::builder(&path).build(), 1, 0);
    let service = DataUpdateService::new();
    let reports = reports(&service, &data_file(&shared));
    service
        .register(Arc::clone(&shared), data_file(&shared))
        .unwrap();
    std::thread::sleep(WATCHER_START);
    std::fs::write(&path, b"version 2").unwrap();
    wait_for(&reports, |statuses| !statuses.is_empty());
    std::thread::sleep(NEXT_WRITE);
    std::fs::write(&path, b"version 3").unwrap();
    wait_for(&reports, |statuses| {
        statuses.last() == Some(&AutoUpdateStatus::Success)
    });
    drop(service);
    let statuses: Vec<AutoUpdateStatus> = statuses(&reports)
        .into_iter()
        .filter(|status| *status != AutoUpdateStatus::NotNeeded)
        .collect();
    assert_eq!(
        statuses.first(),
        Some(&AutoUpdateStatus::RefreshFailed),
        "a watched file: {statuses:?}"
    );
    assert_eq!(
        statuses.last(),
        Some(&AutoUpdateStatus::Success),
        "a watched file: {statuses:?}"
    );
    assert_eq!(engine.refreshes.load(Ordering::SeqCst), statuses.len());
    assert_panic_logged("a watched file");
}

/// The records logged since the last call are one refresh that failed,
/// naming the panic.
fn assert_panic_logged(route: &str) {
    let records = take();
    assert_eq!(records.len(), 1, "{route}: {records:?}");
    assert!(
        records[0].starts_with(APPLYING) && records[0].contains(REFRESH_PANIC),
        "{route}: {records:?}"
    );
}

/// One test, so the records cannot come from another test running at the
/// same time.
#[test]
fn a_refresh_that_fails_is_logged_and_reported_as_a_failure() {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Warn);
    log::warn!("logger check");
    assert_eq!(take(), ["WARN logger check"]);

    let refused = applying(&DataUpdateError::new(
        AutoUpdateStatus::RefreshFailed,
        Error::configuration(REFUSED).to_string(),
    ));
    let dir = std::env::temp_dir().join(format!("fiftyone-refresh-failure-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    a_programmatic_update(&dir, &refused);
    an_update_from_memory(&refused);
    the_update_on_start_up(&refused);
    a_scheduled_poll(&refused);
    a_watched_file(&dir, &refused);
    a_watched_file_that_is_not_newer(&dir);
    a_refresh_that_panics(&dir);

    let _ = std::fs::remove_dir_all(&dir);
}
