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

//! A data file update is written to disk as it arrives, so the memory a
//! download needs does not grow with the size of the file.
//!
//! The test counts every allocation through its own global allocator, which
//! the other tests in the same binary would share, so it is the only test in
//! this file.
#![cfg(feature = "data-update")]

use std::alloc::{GlobalAlloc, Layout, System};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use fiftyone_pipeline_core::{
    EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData, Result,
};
use fiftyone_pipeline_engines::{
    AspectEngine, AspectEngineDataFile, AspectPropertyMetaData, AutoUpdateStatus,
    DataFileConfiguration, DataUpdateService, OnPremiseAspectEngine,
};

/// The system allocator, counting the bytes allocated now and the most
/// allocated at once.
struct Counting;

static ALLOCATED: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = System.alloc(layout);
        if !pointer.is_null() {
            let now = ALLOCATED.fetch_add(layout.size(), Ordering::SeqCst) + layout.size();
            PEAK.fetch_max(now, Ordering::SeqCst);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        System.dealloc(pointer, layout);
        ALLOCATED.fetch_sub(layout.size(), Ordering::SeqCst);
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// The size of the data file the server sends.
const FILE_BYTES: usize = 32 * 1024 * 1024;

/// The most a download may add to the memory in use. Well under the size of
/// the file, so holding the file in memory fails.
const MOST_ADDED_BYTES: usize = 8 * 1024 * 1024;

/// An engine that reads nothing when it refreshes.
struct QuietEngine {
    filter: EvidenceKeyFilterWhitelist,
    data_files: Vec<Arc<AspectEngineDataFile>>,
}

impl FlowElement for QuietEngine {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
        Ok(())
    }
    fn data_key(&self) -> &str {
        "quiet"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

impl AspectEngine for QuietEngine {
    fn data_source_tier(&self) -> &str {
        "Lite"
    }
    fn aspect_properties(&self) -> &[AspectPropertyMetaData] {
        &[]
    }
}

impl OnPremiseAspectEngine for QuietEngine {
    fn data_files(&self) -> &[Arc<AspectEngineDataFile>] {
        &self.data_files
    }
    fn refresh(&self, _identifier: Option<&str>) -> Result<()> {
        Ok(())
    }
}

/// Serve `body` once, as gzip when `gzip` is set, and return the URL.
fn serve_once(body: Vec<u8>, gzip: bool) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/datafile", listener.local_addr().unwrap());
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 4096];
        let _ = stream.read(&mut request);
        let encoding = if gzip {
            "Content-Type: application/gzip\r\n"
        } else {
            ""
        };
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{encoding}Connection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes()).unwrap();
        stream.write_all(&body).unwrap();
    });
    (url, handle)
}

/// The most memory a check for an update added while downloading `body`.
fn memory_added_by_update(body: Vec<u8>, gzip: bool, test: &str) -> usize {
    let dir = std::env::temp_dir().join(format!(
        "fiftyone-download-memory-{test}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("device.dat");
    std::fs::write(&path, b"version 1").unwrap();

    let (url, server) = serve_once(body, gzip);
    let config = DataFileConfiguration::builder(&path)
        .data_update_url(url)
        .automatic_updates_enabled(false)
        .file_system_watcher_enabled(false)
        .decompress_content(gzip)
        .build();
    let engine: Arc<dyn OnPremiseAspectEngine> = Arc::new(QuietEngine {
        filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        data_files: vec![Arc::new(AspectEngineDataFile::new(config))],
    });
    let service = DataUpdateService::new();

    let before = ALLOCATED.load(Ordering::SeqCst);
    PEAK.store(before, Ordering::SeqCst);
    let status = service.check_for_update(&engine, None);
    let added = PEAK.load(Ordering::SeqCst) - before;
    eprintln!("{test}: a {FILE_BYTES} byte update added {added} bytes at most");

    server.join().unwrap();
    assert_eq!(
        status.map_err(|error| error.to_string()),
        Ok(AutoUpdateStatus::Success)
    );
    assert_eq!(std::fs::metadata(&path).unwrap().len(), FILE_BYTES as u64);
    let _ = std::fs::remove_dir_all(&dir);
    added
}

#[test]
fn downloading_an_update_holds_little_of_the_file_in_memory() {
    // Bytes that do not repeat, so the plain file is as large to send as it
    // is to keep.
    let plain: Vec<u8> = (0..FILE_BYTES)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    let added = memory_added_by_update(plain, false, "plain");
    assert!(
        added < MOST_ADDED_BYTES,
        "a {FILE_BYTES} byte download added {added} bytes"
    );

    // A file that compresses well, so most of its size appears only once it
    // is decompressed.
    let compressed = {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&vec![51u8; FILE_BYTES]).unwrap();
        encoder.finish().unwrap()
    };
    let added = memory_added_by_update(compressed, true, "gzip");
    assert!(
        added < MOST_ADDED_BYTES,
        "a {FILE_BYTES} byte file decompressed while adding {added} bytes"
    );
}
