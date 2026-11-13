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

//! End-to-end tests for the data update service against a local HTTP server.
//!
//! These exercise the remote-poll path (download, MD5 verify, gzip decompress,
//! write to disk and refresh the engine) without reaching the real 51Degrees
//! distributor, by spinning up a [`tiny_http`] server that serves a known
//! payload.
//!
//! Gated on the `data-update` feature, which compiles the service this test
//! drives. Without it the service (and its reqwest/notify dependencies) is not
//! built, so neither is this test.
#![cfg(feature = "data-update")]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, TimeZone, Utc};
use fiftyone_pipeline_core::{
    EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData,
    PropertyValueType, Result,
};
use fiftyone_pipeline_engines::{
    AspectDataBase, AspectEngine, AspectEngineDataFile, AspectPropertyMetaData, AutoUpdateStatus,
    DataFileConfiguration, DataUpdateService, FiftyOneUrlFormatter, OnPremiseAspectEngine,
};

/// A minimal on-premise engine that records every refresh and the bytes it was
/// last refreshed with, so a test can assert the update was applied.
struct TestEngine {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
    aspect_properties: Vec<AspectPropertyMetaData>,
    data_files: Vec<Arc<AspectEngineDataFile>>,
    refresh_count: Arc<AtomicUsize>,
    last_disk_contents: Arc<Mutex<Vec<u8>>>,
    /// The publish date a refresh records, as an engine that reads it from
    /// its data does, or none.
    publishes: Option<DateTime<Utc>>,
}

impl TestEngine {
    fn new(data_file: Arc<AspectEngineDataFile>) -> Self {
        TestEngine {
            filter: EvidenceKeyFilterWhitelist::new(["header.user-agent"]),
            properties: vec![PropertyMetaData::new(
                "IsMobile",
                "device",
                PropertyValueType::Bool,
            )],
            aspect_properties: vec![AspectPropertyMetaData::new(
                "IsMobile",
                "device",
                PropertyValueType::Bool,
            )
            .with_data_tiers(["Lite"])],
            data_files: vec![data_file],
            refresh_count: Arc::new(AtomicUsize::new(0)),
            last_disk_contents: Arc::new(Mutex::new(Vec::new())),
            publishes: None,
        }
    }

    /// Record the publish date, if the engine has one, as a refresh does.
    fn record_published(&self) {
        if let Some(published) = self.publishes {
            self.data_files[0].set_data_published(published);
        }
    }
}

impl FlowElement for TestEngine {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        let key = fiftyone_pipeline_core::TypedKey::<AspectDataBase>::new("device");
        data.get_or_add(key, || AspectDataBase::new("device").set("IsMobile", true))?;
        Ok(())
    }
    fn data_key(&self) -> &str {
        "device"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

impl AspectEngine for TestEngine {
    fn data_source_tier(&self) -> &str {
        "Lite"
    }
    fn aspect_properties(&self) -> &[AspectPropertyMetaData] {
        &self.aspect_properties
    }
}

impl OnPremiseAspectEngine for TestEngine {
    fn data_files(&self) -> &[Arc<AspectEngineDataFile>] {
        &self.data_files
    }
    fn refresh(&self, _identifier: Option<&str>) -> Result<()> {
        self.refresh_count.fetch_add(1, Ordering::SeqCst);
        if let Some(path) = self.data_files[0].data_file_path() {
            if let Ok(bytes) = std::fs::read(path) {
                *self.last_disk_contents.lock().unwrap() = bytes;
            }
        }
        self.record_published();
        Ok(())
    }
    fn refresh_from_memory(&self, _identifier: Option<&str>, data: &[u8]) -> Result<()> {
        self.refresh_count.fetch_add(1, Ordering::SeqCst);
        *self.last_disk_contents.lock().unwrap() = data.to_vec();
        self.record_published();
        Ok(())
    }
}

/// Gzip a byte slice, the way the distributor serves data files.
fn gzip(data: &[u8]) -> Vec<u8> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

/// MD5 hex of a byte slice, the way the distributor sets Content-MD5.
fn md5_hex(data: &[u8]) -> String {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(data);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Start a local server that serves `gzipped_payload` with a Content-MD5 header
/// for a single request, then returns its base URL.
fn start_server(gzipped_payload: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let url = format!("http://{}/datafile", server.server_addr());
    let md5 = md5_hex(&gzipped_payload);
    let handle = std::thread::spawn(move || {
        // Serve one request then stop.
        if let Ok(request) = server.recv() {
            let header =
                tiny_http::Header::from_bytes(&b"Content-MD5"[..], md5.as_bytes()).unwrap();
            let response = tiny_http::Response::from_data(gzipped_payload).with_header(header);
            let _ = request.respond(response);
        }
    });
    (url, handle)
}

#[test]
fn remote_update_downloads_verifies_decompresses_and_refreshes() {
    let new_contents = b"FRESH 51Degrees data file v2".to_vec();
    let gzipped = gzip(&new_contents);
    let (url, server) = start_server(gzipped);

    // A temporary data file on disk holding the "old" data.
    let dir = std::env::temp_dir().join(format!("fiftyone-dut-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("device.dat");
    std::fs::write(&path, b"OLD data v1").unwrap();

    let config = DataFileConfiguration::builder(&path)
        .data_update_url(url)
        .automatic_updates_enabled(false) // no background polling, test the trigger
        .file_system_watcher_enabled(false)
        .build();
    let data_file = Arc::new(AspectEngineDataFile::new(config));
    let engine = Arc::new(TestEngine::new(Arc::clone(&data_file)));
    let refresh_count = engine.refresh_count.clone();
    let disk = engine.last_disk_contents.clone();

    let service = DataUpdateService::new();
    let engine_dyn: Arc<dyn OnPremiseAspectEngine> = engine;
    service
        .register(Arc::clone(&engine_dyn), Arc::clone(&data_file))
        .unwrap();

    // Programmatic trigger.
    let before = Utc::now();
    let status = service.check_for_update(&engine_dyn, None).unwrap();
    assert_eq!(status, AutoUpdateStatus::Success);

    // The engine was refreshed and the file on disk now holds the new data.
    assert!(refresh_count.load(Ordering::SeqCst) >= 1);
    assert_eq!(std::fs::read(&path).unwrap(), new_contents);
    assert_eq!(*disk.lock().unwrap(), new_contents);
    // The engine records no publish date, so the time of the update is kept.
    let published = data_file.data_published().expect("a publish date");
    assert!(
        before <= published && published <= Utc::now(),
        "{published}"
    );

    server.join().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// The publish date an engine reads from the data it was refreshed with is
/// kept after a download, for a file and for a memory data source, so the
/// next request's `If-Modified-Since` carries it.
#[test]
fn a_publish_date_the_engine_records_is_kept_after_a_download() {
    let published = Utc.with_ymd_and_hms(2001, 2, 3, 0, 0, 0).unwrap();
    let dir = std::env::temp_dir().join(format!("fiftyone-dut-date-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("device.dat");
    std::fs::write(&path, b"OLD data v1").unwrap();
    for source in ["file", "memory"] {
        let (url, server) = start_server(gzip(b"FRESH data v2"));
        let config = match source {
            "file" => DataFileConfiguration::builder(&path),
            _ => DataFileConfiguration::memory_builder(),
        }
        .data_update_url(url)
        .automatic_updates_enabled(false)
        .file_system_watcher_enabled(false)
        .build();
        let data_file = Arc::new(AspectEngineDataFile::new(config));
        let mut engine = TestEngine::new(Arc::clone(&data_file));
        engine.publishes = Some(published);
        let engine: Arc<dyn OnPremiseAspectEngine> = Arc::new(engine);

        let status = DataUpdateService::new()
            .check_for_update(&engine, None)
            .unwrap();
        server.join().unwrap();
        assert_eq!(status, AutoUpdateStatus::Success, "{source}");
        assert_eq!(data_file.data_published(), Some(published), "{source}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Start a local server that answers one request with 304 and gives back
/// the request's `If-Modified-Since` header, if it had one.
fn start_not_modified_server() -> (String, std::thread::JoinHandle<Option<String>>) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let url = format!("http://{}/datafile", server.server_addr());
    let handle = std::thread::spawn(move || {
        let request = server.recv().ok()?;
        let since = request
            .headers()
            .iter()
            .find(|header| header.field.equiv("If-Modified-Since"))
            .map(|header| header.value.as_str().to_owned());
        let _ = request.respond(tiny_http::Response::empty(304));
        since
    });
    (url, handle)
}

/// `If-Modified-Since` carries the publish date the data holds, or the data
/// file's modified time when the data holds none, as the specification
/// allows. A memory data source with no publish date sends no date.
#[test]
fn if_modified_since_is_the_publish_date_or_the_file_modified_time() {
    let dir = std::env::temp_dir().join(format!("fiftyone-dut-ims-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("device.dat");
    std::fs::write(&path, b"current data").unwrap();
    let modified = DateTime::<Utc>::from(std::fs::metadata(&path).unwrap().modified().unwrap());
    let published = Utc.with_ymd_and_hms(2001, 2, 3, 0, 0, 0).unwrap();
    let cases = [
        (
            "a file with no publish date",
            Some(&path),
            None,
            Some(modified),
        ),
        (
            "a file with a publish date",
            Some(&path),
            Some(published),
            Some(published),
        ),
        ("memory with no publish date", None, None, None),
    ];
    for (case, file, recorded, expected) in cases {
        let (url, server) = start_not_modified_server();
        let config = match file {
            Some(path) => DataFileConfiguration::builder(path),
            None => DataFileConfiguration::memory_builder(),
        }
        .data_update_url(url)
        .automatic_updates_enabled(false)
        .file_system_watcher_enabled(false)
        .build();
        let data_file = Arc::new(AspectEngineDataFile::new(config));
        if let Some(recorded) = recorded {
            data_file.set_data_published(recorded);
        }
        let engine: Arc<dyn OnPremiseAspectEngine> =
            Arc::new(TestEngine::new(Arc::clone(&data_file)));

        let status = DataUpdateService::new()
            .check_for_update(&engine, None)
            .unwrap();
        let sent = server.join().unwrap();
        assert_eq!(status, AutoUpdateStatus::NotNeeded, "{case}");
        let expected = expected.map(|when| when.format("%a, %d %b %Y %H:%M:%S GMT").to_string());
        assert_eq!(sent, expected, "{case}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn not_modified_response_reports_not_needed() {
    // A server that always answers 304.
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let url = format!("http://{}/datafile", server.server_addr());
    let handle = std::thread::spawn(move || {
        if let Ok(request) = server.recv() {
            let response = tiny_http::Response::empty(304);
            let _ = request.respond(response);
        }
    });

    let dir = std::env::temp_dir().join(format!("fiftyone-dut-nm-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("device.dat");
    std::fs::write(&path, b"current data").unwrap();

    let config = DataFileConfiguration::builder(&path)
        .data_update_url(url)
        .automatic_updates_enabled(false)
        .file_system_watcher_enabled(false)
        .build();
    let data_file = Arc::new(AspectEngineDataFile::new(config));
    let engine: Arc<dyn OnPremiseAspectEngine> = Arc::new(TestEngine::new(Arc::clone(&data_file)));

    let service = DataUpdateService::new();
    service
        .register(Arc::clone(&engine), Arc::clone(&data_file))
        .unwrap();
    let status = service.check_for_update(&engine, None).unwrap();
    assert_eq!(status, AutoUpdateStatus::NotNeeded);

    handle.join().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn update_from_memory_goes_to_engine() {
    let config = DataFileConfiguration::memory_builder()
        .automatic_updates_enabled(false)
        .file_system_watcher_enabled(false)
        .build();
    let data_file = Arc::new(AspectEngineDataFile::new(config));
    let engine = Arc::new(TestEngine::new(Arc::clone(&data_file)));
    let disk = engine.last_disk_contents.clone();
    let engine_dyn: Arc<dyn OnPremiseAspectEngine> = engine;

    let service = DataUpdateService::new();
    let bytes = b"in-memory data source".to_vec();
    let status = service
        .update_from_memory(&engine_dyn, None, &bytes)
        .unwrap();
    assert_eq!(status, AutoUpdateStatus::Success);
    assert_eq!(*disk.lock().unwrap(), bytes);
}

/// The licence key the Distributor formatter writes into the update URL.
const LICENCE_KEY: &str = "LICENCE-KEY-NOT-FOR-LOGS";

/// An engine whose data file polls `url` through the Distributor formatter,
/// so each request carries [`LICENCE_KEY`] in its query.
fn engine_polling(url: &str, dir: &std::path::Path) -> Arc<dyn OnPremiseAspectEngine> {
    let path = dir.join("device.dat");
    std::fs::write(&path, b"current data").unwrap();
    let config = DataFileConfiguration::builder(&path)
        .data_update_url(url)
        .data_update_license_keys([LICENCE_KEY])
        .url_formatter(Some(Arc::new(FiftyOneUrlFormatter)))
        .automatic_updates_enabled(false)
        .file_system_watcher_enabled(false)
        .build();
    Arc::new(TestEngine::new(Arc::new(AspectEngineDataFile::new(config))))
}

/// A poll that cannot connect fails with a message that names the configured
/// URL and the cause, and not the licence key the request carried.
#[test]
fn a_failed_connection_keeps_the_licence_key_out_of_the_error() {
    // Nothing listens on a port once its listener is dropped.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/api/v2/download", listener.local_addr().unwrap());
    drop(listener);
    let dir = std::env::temp_dir().join(format!("fiftyone-dut-key-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let engine = engine_polling(&url, &dir);

    let error = DataUpdateService::new()
        .check_for_update(&engine, None)
        .expect_err("nothing is listening");
    let message = error.to_string();
    assert_eq!(error.status, AutoUpdateStatus::HttpsError, "{message}");
    assert!(!message.contains(LICENCE_KEY), "{message}");
    assert!(message.contains(&url), "{message}");
    assert!(message.contains("error sending request: "), "{message}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Serve the head of a 200 response that promises 1,000 bytes, then five of
/// them, then close the connection, so a download from the returned URL is
/// cut short.
fn serve_cut_short() -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/api/v2/download", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 4096];
        let _ = stream.read(&mut request);
        let _ = stream.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\nshort",
        );
    });
    (url, server)
}

/// Check `error` is the first row of the specification's errors table, the
/// one .NET gives for a download cut short, with the cause after it.
fn assert_access_error(error: &fiftyone_pipeline_engines::DataUpdateError, url: &str) {
    let message = &error.message;
    let first_row = format!(
        "Error accessing data update service at '{url}' for engine 'device'. Error detail: "
    );
    assert_eq!(error.status, AutoUpdateStatus::HttpsError, "{message}");
    assert!(message.starts_with(&first_row), "{message}");
    assert!(message.len() > first_row.len(), "no cause: {message}");
}

/// A download whose body ends early fails with a message that names the
/// configured URL, and not the licence key the request carried.
#[test]
fn a_failed_download_keeps_the_licence_key_out_of_the_error() {
    let (url, server) = serve_cut_short();
    let dir = std::env::temp_dir().join(format!("fiftyone-dut-body-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let engine = engine_polling(&url, &dir);

    let error = DataUpdateService::new()
        .check_for_update(&engine, None)
        .expect_err("the body ends early");
    server.join().unwrap();
    let message = error.to_string();
    assert_access_error(&error, &url);
    assert!(!message.contains(LICENCE_KEY), "{message}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A download to a data file that is cut short is reported as .NET reports
/// it, which is as a data update service that could not be reached.
#[test]
fn a_download_to_a_file_cut_short_is_the_access_error() {
    let (url, server) = serve_cut_short();
    let dir = std::env::temp_dir().join(format!("fiftyone-dut-short-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("device.dat");
    std::fs::write(&path, b"version 1").unwrap();
    let config = DataFileConfiguration::builder(&path)
        .data_update_url(url.clone())
        .automatic_updates_enabled(false)
        .file_system_watcher_enabled(false)
        .decompress_content(false)
        .build();
    let engine: Arc<dyn OnPremiseAspectEngine> =
        Arc::new(TestEngine::new(Arc::new(AspectEngineDataFile::new(config))));

    let error = DataUpdateService::new()
        .check_for_update(&engine, None)
        .expect_err("the body ends early");
    server.join().unwrap();
    assert_access_error(&error, &url);
    assert_eq!(std::fs::read(&path).unwrap(), b"version 1");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A download to a data source held in memory that is cut short is reported
/// in the same way, and the engine is not refreshed.
#[test]
fn a_download_to_memory_cut_short_is_the_access_error() {
    let (url, server) = serve_cut_short();
    let config = DataFileConfiguration::memory_builder()
        .data_update_url(url.clone())
        .automatic_updates_enabled(false)
        .file_system_watcher_enabled(false)
        .decompress_content(false)
        .build();
    let engine = Arc::new(TestEngine::new(Arc::new(AspectEngineDataFile::new(config))));
    let refreshes = Arc::clone(&engine.refresh_count);
    let engine: Arc<dyn OnPremiseAspectEngine> = engine;

    let error = DataUpdateService::new()
        .check_for_update(&engine, None)
        .expect_err("the body ends early");
    server.join().unwrap();
    assert_access_error(&error, &url);
    assert_eq!(refreshes.load(Ordering::SeqCst), 0);
}

/// A download that keeps arriving completes even when it takes longer than
/// the HTTP client's timeout, which limits each wait for more of the response
/// and not the whole download. A data file of several gigabytes takes minutes
/// to arrive.
#[test]
fn a_steady_download_longer_than_the_client_timeout_completes() {
    const CLIENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
    const PARTS: usize = 10;
    const PART_GAP: std::time::Duration = std::time::Duration::from_millis(500);
    let body: Vec<u8> = (0..50 * 1024).map(|i| (i % 251) as u8).collect();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/datafile", listener.local_addr().unwrap());
    let sent = body.clone();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 4096];
        let _ = stream.read(&mut request);
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            sent.len()
        );
        let _ = stream.write_all(head.as_bytes());
        for part in sent.chunks(sent.len().div_ceil(PARTS)) {
            std::thread::sleep(PART_GAP);
            if stream.write_all(part).and_then(|_| stream.flush()).is_err() {
                return;
            }
        }
    });

    let dir = std::env::temp_dir().join(format!("fiftyone-dut-steady-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("device.dat");
    std::fs::write(&path, b"version 1").unwrap();
    let config = DataFileConfiguration::builder(&path)
        .data_update_url(url)
        .automatic_updates_enabled(false)
        .file_system_watcher_enabled(false)
        .decompress_content(false)
        .build();
    let engine: Arc<dyn OnPremiseAspectEngine> =
        Arc::new(TestEngine::new(Arc::new(AspectEngineDataFile::new(config))));
    let client = reqwest::blocking::Client::builder()
        .timeout(CLIENT_TIMEOUT)
        .build()
        .unwrap();

    let status = DataUpdateService::with_client(client).check_for_update(&engine, None);
    server.join().unwrap();
    assert_eq!(
        status.map_err(|error| error.to_string()),
        Ok(AutoUpdateStatus::Success)
    );
    assert_eq!(std::fs::read(&path).unwrap(), body);
    let _ = std::fs::remove_dir_all(&dir);
}

/// An update whose data file cannot be replaced fails, and leaves no
/// temporary file beside the data file.
#[test]
fn a_failed_update_leaves_no_temporary_file() {
    // A folder where the data file should be, so it cannot be replaced.
    let dir = std::env::temp_dir().join(format!("fiftyone-dut-left-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("device.dat");
    std::fs::create_dir_all(&path).unwrap();
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let url = format!("http://{}/datafile", server.server_addr());
    let handle = std::thread::spawn(move || {
        if let Ok(request) = server.recv() {
            let _ = request.respond(tiny_http::Response::from_data(b"new data".to_vec()));
        }
    });
    let config = DataFileConfiguration::builder(&path)
        .data_update_url(url)
        .automatic_updates_enabled(false)
        .file_system_watcher_enabled(false)
        .decompress_content(false)
        .build();
    let engine: Arc<dyn OnPremiseAspectEngine> =
        Arc::new(TestEngine::new(Arc::new(AspectEngineDataFile::new(config))));

    let error = DataUpdateService::new()
        .check_for_update(&engine, None)
        .expect_err("a folder cannot be replaced by a file");
    handle.join().unwrap();
    let left: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        error.status,
        AutoUpdateStatus::NewFileCannotRename,
        "{error}"
    );
    assert_eq!(left, ["device.dat"]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Dropping the service ends a scheduled download in progress, rather than
/// waiting for the rest of the data file, and leaves no temporary file.
#[test]
fn dropping_the_service_stops_a_download_in_progress() {
    // A server that sends one byte every tenth of a second, so the whole of
    // its body takes 30 seconds.
    const BODY_BYTES: usize = 300;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/datafile", listener.local_addr().unwrap());
    // Not joined, so the test cannot wait on a server the scheduler never
    // reached. It ends when its next write finds the connection closed.
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 4096];
        let _ = stream.read(&mut request);
        let head =
            format!("HTTP/1.1 200 OK\r\nContent-Length: {BODY_BYTES}\r\nConnection: close\r\n\r\n");
        let _ = stream.write_all(head.as_bytes());
        for _ in 0..BODY_BYTES {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if stream.write_all(b"5").and_then(|_| stream.flush()).is_err() {
                return;
            }
        }
    });

    let dir = std::env::temp_dir().join(format!("fiftyone-dut-drop-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("device.dat");
    std::fs::write(&path, b"version 1").unwrap();
    // Polled at once, by the service's scheduler.
    let config = DataFileConfiguration::builder(&path)
        .data_update_url(url)
        .automatic_updates_enabled(true)
        .polling_interval_seconds(0)
        .max_randomisation_seconds(0)
        .file_system_watcher_enabled(false)
        .decompress_content(false)
        .build();
    let data_file = Arc::new(AspectEngineDataFile::new(config));
    let engine: Arc<dyn OnPremiseAspectEngine> = Arc::new(TestEngine::new(Arc::clone(&data_file)));
    let service = DataUpdateService::new();
    service.register(Arc::clone(&engine), data_file).unwrap();
    std::thread::sleep(std::time::Duration::from_secs(1));

    let dropping = std::time::Instant::now();
    drop(service);
    let waited = dropping.elapsed();

    let left: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        waited < std::time::Duration::from_secs(5),
        "dropping the service waited {waited:?} for the download"
    );
    assert_eq!(left, ["device.dat"]);
    assert_eq!(std::fs::read(&path).unwrap(), b"version 1");
    let _ = std::fs::remove_dir_all(&dir);
}
