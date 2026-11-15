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

//! The messages, levels and statuses of the specification's
//! [errors table](https://github.com/51Degrees/specifications/blob/dbaf099c712e2bd3a2a80bfb4eaf284b079f05eb/pipeline-specification/features/data-updates.md#errors),
//! which data-updates.md line 481 says "MUST also be consistent across
//! languages to make life easier for 51Degrees support". Each message names
//! the Engine by its type, a refresh from a data file is tried 10 times, a
//! failure the table has no row for is an unknown error, a watched file that
//! cannot be read in time is a timeout, and the update on start-up passes its
//! error to the caller without logging it. Kept in its own test binary
//! because a logger is installed once for the whole process.
#![cfg(feature = "data-update")]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use fiftyone_pipeline_core::{
    Error, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData,
    Result,
};
use fiftyone_pipeline_engines::{
    AspectEngine, AspectEngineDataFile, AspectPropertyMetaData, AutoUpdateStatus,
    DataFileConfiguration, DataUpdateService, OnPremiseAspectEngine,
};

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

/// An engine whose every refresh fails, counting them. Its data key, `table`,
/// is not its type name, `TableEngine`, which the messages name it by.
struct TableEngine {
    filter: EvidenceKeyFilterWhitelist,
    data_files: Vec<Arc<AspectEngineDataFile>>,
    refreshes: AtomicUsize,
}

impl FlowElement for TableEngine {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
        Ok(())
    }
    fn data_key(&self) -> &str {
        "table"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

impl AspectEngine for TableEngine {
    fn data_source_tier(&self) -> &str {
        "Lite"
    }
    fn aspect_properties(&self) -> &[AspectPropertyMetaData] {
        &[]
    }
}

impl OnPremiseAspectEngine for TableEngine {
    fn data_files(&self) -> &[Arc<AspectEngineDataFile>] {
        &self.data_files
    }
    fn refresh(&self, _identifier: Option<&str>) -> Result<()> {
        self.refreshes.fetch_add(1, Ordering::SeqCst);
        Err(Error::configuration("the new data cannot be used"))
    }
    fn refresh_from_memory(&self, _identifier: Option<&str>, _data: &[u8]) -> Result<()> {
        self.refreshes.fetch_add(1, Ordering::SeqCst);
        Err(Error::configuration("the new data cannot be used"))
    }
}

/// A [`TableEngine`] for `configuration`, returned twice, once to count its
/// refreshes and once as the service takes it.
fn engine(
    configuration: DataFileConfiguration,
) -> (Arc<TableEngine>, Arc<dyn OnPremiseAspectEngine>) {
    let engine = Arc::new(TableEngine {
        filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        data_files: vec![Arc::new(AspectEngineDataFile::new(configuration))],
        refreshes: AtomicUsize::new(0),
    });
    let shared: Arc<dyn OnPremiseAspectEngine> = engine.clone();
    (engine, shared)
}

/// An update URL on this machine that answers every request with the status
/// line `status` and `body`.
fn serve(status: &'static str, body: &'static [u8]) -> String {
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
                "{status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body);
        }
    });
    url
}

/// A refresh from memory that fails names the Engine by its type, as the
/// table's row for a refresh from data held in memory gives it, and is tried
/// once.
fn a_refresh_from_memory_names_the_engine_type() {
    let (engine, shared) = engine(
        DataFileConfiguration::memory_builder()
            .automatic_updates_enabled(false)
            .build(),
    );
    let service = DataUpdateService::new();
    let error = service
        .update_from_memory(&shared, None, b"new data")
        .expect_err("the refresh fails");
    assert_eq!(error.status, AutoUpdateStatus::RefreshFailed);
    assert!(
        error
            .message
            .starts_with("An error occurred when applying a data update to engine 'TableEngine'."),
        "{}",
        error.message
    );
    assert_eq!(engine.refreshes.load(Ordering::SeqCst), 1);
    assert_eq!(take(), Vec::<String>::new());
}

/// A refresh from a data file is tried 10 times, and the message names the
/// tries, as the table's row "Refreshing the Engine from the data file still
/// failed after 10 tries" gives it.
fn a_refresh_from_a_file_is_tried_ten_times(dir: &std::path::Path) {
    let path = dir.join("tries.dat");
    std::fs::write(&path, b"version 1").unwrap();
    let (engine, shared) = engine(
        DataFileConfiguration::builder(&path)
            .automatic_updates_enabled(false)
            .build(),
    );
    let service = DataUpdateService::new();
    let error = service
        .check_for_update(&shared, None)
        .expect_err("the refresh fails");
    assert_eq!(error.status, AutoUpdateStatus::RefreshFailed);
    assert!(
        error.message.starts_with(
            "An error occurred when applying a data update to engine 'TableEngine' after 10 \
             tries."
        ),
        "{}",
        error.message
    );
    assert_eq!(engine.refreshes.load(Ordering::SeqCst), 10);
    assert_eq!(take(), Vec::<String>::new(), "a programmatic check");
}

/// Data that cannot be decompressed is a failure the table has no row for,
/// so it is an unknown error with the table's last message.
fn a_failure_with_no_row_is_an_unknown_error() {
    let url = serve("HTTP/1.1 200 OK", b"not gzip at all");
    let (_engine, shared) = engine(
        DataFileConfiguration::memory_builder()
            .data_update_url(url)
            .verify_md5(false)
            .automatic_updates_enabled(false)
            .decompress_content(true)
            .build(),
    );
    let service = DataUpdateService::new();
    let error = service
        .check_for_update(&shared, None)
        .expect_err("the data cannot be decompressed");
    assert_eq!(error.status, AutoUpdateStatus::UnknownError);
    assert!(
        error.message.starts_with(
            "An unhandled error occurred while checking for automatic updates for engine \
             'TableEngine'"
        ),
        "{}",
        error.message
    );
    assert_eq!(take(), Vec::<String>::new());
}

/// The update on start-up passes its error to the caller and logs nothing,
/// and the messages name the Engine by its type.
fn the_update_on_start_up_passes_its_error_to_the_caller() {
    let url = serve("HTTP/1.1 403 Forbidden", b"");
    let (_engine, shared) = engine(
        DataFileConfiguration::memory_builder()
            .data_update_url(url.clone())
            .verify_md5(false)
            .update_on_startup(true)
            .automatic_updates_enabled(false)
            .decompress_content(false)
            .build(),
    );
    let service = DataUpdateService::new();
    let error = service
        .register(Arc::clone(&shared), Arc::clone(&shared.data_files()[0]))
        .expect_err("the update on start-up fails");
    assert_eq!(error.status, AutoUpdateStatus::Forbidden);
    assert_eq!(
        error.message,
        format!("Access denied to data update service at '{url}' for engine 'TableEngine'")
    );
    assert_eq!(take(), Vec::<String>::new(), "the update on start-up");
}

/// A watched file that cannot be opened for reading in time gives the
/// table's `AUTO_UPDATE_TIMEOUT` status, a failure.
fn a_timeout_has_a_status_of_its_own() {
    assert!(!AutoUpdateStatus::Timeout.is_ok());
    assert_ne!(AutoUpdateStatus::Timeout, AutoUpdateStatus::UnknownError);
}

/// One test, so the records cannot come from another test running at the
/// same time.
#[test]
fn the_messages_levels_and_statuses_are_those_of_the_errors_table() {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Warn);
    log::warn!("logger check");
    assert_eq!(take(), ["WARN logger check"]);

    let dir = std::env::temp_dir().join(format!("fiftyone-errors-table-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    a_refresh_from_memory_names_the_engine_type();
    a_refresh_from_a_file_is_tried_ten_times(&dir);
    a_failure_with_no_row_is_an_unknown_error();
    the_update_on_start_up_passes_its_error_to_the_caller();
    a_timeout_has_a_status_of_its_own();

    let _ = std::fs::remove_dir_all(&dir);
}
