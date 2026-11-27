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

//! The data updates specification says of the data update service's
//! messages "These messages MUST also be consistent across languages to make
//! life easier for 51Degrees support." Its tables name the engine by its
//! type, as '<Engine type>' and '<Engine type name>', and give an error of
//! any kind outside the table, such as a download that cannot be
//! decompressed or written, the status `AUTO_UPDATE_UNKNOWN_ERROR` and the
//! message "An unhandled error occurred while checking for automatic updates
//! for engine '<Engine type name>'". A refresh from the data file is tried
//! 10 times before it gives `AUTO_UPDATE_REFRESH_FAILED` and "An error
//! occurred when applying a data update to engine '<Engine type name>' after
//! <tries> tries.", and one from memory once. Kept in its own test binary
//! because a logger is installed once for the whole process.
#![cfg(feature = "data-update")]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use fiftyone_pipeline_core::{
    EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData, Result,
};
use fiftyone_pipeline_engines::{
    AspectEngine, AspectEngineDataFile, AspectPropertyMetaData, AutoUpdateStatus,
    DataFileConfiguration, DataUpdateService, OnPremiseAspectEngine,
};

/// The engine's type name, which the messages name it by.
const TYPE_NAME: &str = "NamedEngine";

/// The engine's data key, which no message names it by.
const DATA_KEY: &str = "named-key";

/// The message the errors table gives an error of any other kind.
const UNHANDLED: &str =
    "An unhandled error occurred while checking for automatic updates for engine 'NamedEngine'";

/// Every record, as "LEVEL message".
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The tests share [`LOGGED`], so they run one at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// Keeps every record in [`LOGGED`].
struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        if record.target().starts_with("fiftyone") {
            LOGGED
                .lock()
                .unwrap()
                .push(format!("{} {}", record.level(), record.args()));
        }
    }

    fn flush(&self) {}
}

static LOGGER: TestLogger = TestLogger;

/// Install the logger, clear what it holds, and keep the other tests out.
fn start() -> std::sync::MutexGuard<'static, ()> {
    let guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Info);
    LOGGED.lock().unwrap().clear();
    guard
}

fn logged() -> Vec<String> {
    std::mem::take(&mut *LOGGED.lock().unwrap())
}

/// An engine whose type name and data key differ, whose first refreshes
/// fail, while failures are left, and whose later ones take any data.
struct NamedEngine {
    filter: EvidenceKeyFilterWhitelist,
    data_files: Vec<Arc<AspectEngineDataFile>>,
    failures: Mutex<usize>,
    refreshes: AtomicUsize,
}

impl NamedEngine {
    /// One refresh, which fails while failures are left.
    fn attempt(&self) -> Result<()> {
        self.refreshes.fetch_add(1, Ordering::SeqCst);
        let failed = {
            let mut left = self.failures.lock().unwrap();
            let failed = *left > 0;
            if failed {
                *left -= 1;
            }
            failed
        };
        if failed {
            Err(fiftyone_pipeline_core::Error::configuration(REFUSED))
        } else {
            Ok(())
        }
    }
}

/// What a refresh of [`NamedEngine`] that fails says.
const REFUSED: &str = "the refresh is refused";

/// The tries the errors table gives a refresh from the data file.
const TRIES: usize = 10;

impl FlowElement for NamedEngine {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
        Ok(())
    }
    fn data_key(&self) -> &str {
        DATA_KEY
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

impl AspectEngine for NamedEngine {
    fn data_source_tier(&self) -> &str {
        "Lite"
    }
    fn aspect_properties(&self) -> &[AspectPropertyMetaData] {
        &[]
    }
}

impl OnPremiseAspectEngine for NamedEngine {
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

fn engine(config: DataFileConfiguration) -> Arc<dyn OnPremiseAspectEngine> {
    failing(config, 0)
}

/// A [`NamedEngine`] for `config` whose first `failures` refreshes fail.
fn failing(config: DataFileConfiguration, failures: usize) -> Arc<NamedEngine> {
    Arc::new(NamedEngine {
        filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        data_files: vec![Arc::new(AspectEngineDataFile::new(config))],
        failures: Mutex::new(failures),
        refreshes: AtomicUsize::new(0),
    })
}

/// Serve `body` to one request, and return the URL.
fn serve_once(body: &'static [u8]) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/datafile", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 4096];
        let _ = stream.read(&mut request);
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(body);
    });
    url
}

/// A URL nothing listens at.
fn dead_url() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/datafile", listener.local_addr().unwrap());
    drop(listener);
    url
}

/// A new, empty folder for one test.
fn folder(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fiftyone-consistent-messages-{test}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The configuration of a file data source at `path` updated from `url`.
fn file_config(path: &PathBuf, url: String, decompress: bool) -> DataFileConfiguration {
    DataFileConfiguration::builder(path)
        .data_update_url(url)
        // On, so a programmatic check polls the URL. Nothing registers the
        // engine, so nothing polls in the background.
        .automatic_updates_enabled(true)
        .file_system_watcher_enabled(false)
        .verify_md5(false)
        .verify_modified_since(false)
        .decompress_content(decompress)
        .build()
}

/// The configuration of a memory data source updated from `url`.
fn memory_config(url: String, decompress: bool) -> DataFileConfiguration {
    DataFileConfiguration::memory_builder()
        .data_update_url(url)
        // On, so a programmatic check polls the URL. Nothing registers the
        // engine, so nothing polls in the background.
        .automatic_updates_enabled(true)
        .verify_md5(false)
        .verify_modified_since(false)
        .decompress_content(decompress)
        .build()
}

/// The check's request and its error name the engine by its type.
#[test]
fn a_failed_request_names_the_engine_by_its_type() {
    let _guard = start();
    let engine = engine(memory_config(dead_url(), false));

    let error = DataUpdateService::new()
        .check_for_update(&engine, None)
        .expect_err("nothing listens at the URL");

    assert_eq!(error.status, AutoUpdateStatus::HttpsError);
    assert!(
        error.message.contains(&format!("for engine '{TYPE_NAME}'")),
        "{}",
        error.message
    );
    let logged = logged();
    assert!(
        logged.iter().any(
            |record| record.starts_with("INFO Checking for update from '")
                && record.ends_with(&format!("' for Engine '{TYPE_NAME}'"))
        ),
        "{logged:?}"
    );
    for record in &logged {
        assert!(!record.contains(DATA_KEY), "{record}");
    }
}

/// An update that is applied logs the download and the refresh naming the
/// engine by its type.
#[test]
fn an_applied_update_names_the_engine_by_its_type() {
    let _guard = start();
    let dir = folder("applied");
    let path = dir.join("data.hash");
    std::fs::write(&path, b"old data").unwrap();
    let engine = engine(file_config(&path, serve_once(b"new data"), false));

    let status = DataUpdateService::new().check_for_update(&engine, None);

    assert_eq!(
        status.map_err(|e| e.to_string()),
        Ok(AutoUpdateStatus::Success)
    );
    let logged = logged();
    assert!(
        logged.contains(&format!(
            "INFO Attempting to refresh Engine '{TYPE_NAME}' with new data"
        )),
        "{logged:?}"
    );
    for record in &logged {
        assert!(!record.contains(DATA_KEY), "{record}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A download to a memory data source that cannot be decompressed is an
/// error outside the table.
#[test]
fn a_download_to_memory_that_cannot_be_decompressed_is_an_unknown_error() {
    let _guard = start();
    let engine = engine(memory_config(serve_once(b"not gzip"), true));

    let error = DataUpdateService::new()
        .check_for_update(&engine, None)
        .expect_err("the download is not gzip");

    assert_eq!(error.status, AutoUpdateStatus::UnknownError);
    assert!(error.message.starts_with(UNHANDLED), "{}", error.message);
}

/// A download to a file data source that cannot be decompressed is an error
/// outside the table.
#[test]
fn a_download_to_a_file_that_cannot_be_decompressed_is_an_unknown_error() {
    let _guard = start();
    let dir = folder("decompress");
    let path = dir.join("data.hash");
    std::fs::write(&path, b"old data").unwrap();
    let engine = engine(file_config(&path, serve_once(b"not gzip"), true));

    let error = DataUpdateService::new()
        .check_for_update(&engine, None)
        .expect_err("the download is not gzip");

    assert_eq!(error.status, AutoUpdateStatus::UnknownError);
    assert!(error.message.starts_with(UNHANDLED), "{}", error.message);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A download that cannot be written beside the data file is an error
/// outside the table.
#[test]
fn a_download_that_cannot_be_written_is_an_unknown_error() {
    let _guard = start();
    let dir = folder("write");
    // The data file's folder does not exist, so nothing can be written
    // beside it.
    let path = dir.join("missing").join("data.hash");
    let engine = engine(file_config(&path, serve_once(b"new data"), false));

    let error = DataUpdateService::new()
        .check_for_update(&engine, None)
        .expect_err("the download cannot be written");

    assert_eq!(error.status, AutoUpdateStatus::UnknownError);
    assert!(error.message.starts_with(UNHANDLED), "{}", error.message);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The configuration of a file data source at `path` with no update URL,
/// which a programmatic check refreshes the engine from.
fn local_file_config(path: &PathBuf) -> DataFileConfiguration {
    DataFileConfiguration::builder(path)
        .automatic_updates_enabled(false)
        .file_system_watcher_enabled(false)
        .build()
}

/// A refresh from the data file that fails fewer than 10 times is tried
/// until it works.
#[test]
fn a_refresh_from_the_file_is_tried_again_until_it_works() {
    let _guard = start();
    let dir = folder("tried-again");
    let path = dir.join("data.hash");
    std::fs::write(&path, b"new data").unwrap();
    let engine = failing(local_file_config(&path), TRIES - 1);
    let shared: Arc<dyn OnPremiseAspectEngine> = engine.clone();

    let status = DataUpdateService::new().check_for_update(&shared, None);

    assert_eq!(
        status.map_err(|e| e.to_string()),
        Ok(AutoUpdateStatus::Success)
    );
    assert_eq!(engine.refreshes.load(Ordering::SeqCst), TRIES);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A refresh from the data file that fails 10 times gives the table's status
/// and message, which names the tries.
#[test]
fn a_refresh_from_the_file_that_fails_10_times_names_the_tries() {
    let _guard = start();
    let dir = folder("ten-tries");
    let path = dir.join("data.hash");
    std::fs::write(&path, b"new data").unwrap();
    let engine = failing(local_file_config(&path), usize::MAX);
    let shared: Arc<dyn OnPremiseAspectEngine> = engine.clone();

    let error = DataUpdateService::new()
        .check_for_update(&shared, None)
        .expect_err("every try fails");

    assert_eq!(error.status, AutoUpdateStatus::RefreshFailed);
    let expected = format!(
        "An error occurred when applying a data update to engine '{TYPE_NAME}' after \
         {TRIES} tries."
    );
    assert!(error.message.starts_with(&expected), "{}", error.message);
    assert!(error.message.contains(REFUSED), "{}", error.message);
    assert_eq!(engine.refreshes.load(Ordering::SeqCst), TRIES);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A refresh from data held in memory is tried once, and its message names
/// no tries.
#[test]
fn a_refresh_from_memory_is_tried_once() {
    let _guard = start();
    let engine = failing(
        DataFileConfiguration::memory_builder()
            .automatic_updates_enabled(false)
            .build(),
        1,
    );
    let shared: Arc<dyn OnPremiseAspectEngine> = engine.clone();

    let error = DataUpdateService::new()
        .update_from_memory(&shared, None, b"new data")
        .expect_err("the refresh fails");

    assert_eq!(error.status, AutoUpdateStatus::RefreshFailed);
    let expected =
        format!("An error occurred when applying a data update to engine '{TYPE_NAME}'.");
    assert!(error.message.starts_with(&expected), "{}", error.message);
    assert_eq!(engine.refreshes.load(Ordering::SeqCst), 1);
}
