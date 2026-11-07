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

//! The dates an engine records for its data file are the ones the data file
//! header holds, as .NET's Hash engine records them, whether the engine loads
//! the file, data given in memory or a file the data update service
//! downloads.
//!
//! The tests need the Lite file `51DEGREES_DD_PATH` names and skip with a
//! note when it is missing.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, TimeZone, Timelike, Utc};
use fiftyone_device_detection_onpremise::{
    DeviceDetectionOnPremiseEngine, DeviceDetectionOnPremiseEngineBuilder,
};
use fiftyone_device_detection_shared::DEVICE_DATA_KEY;
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{ElementData, Evidence, FlowElement, Pipeline};
use fiftyone_pipeline_engines::{
    AutoUpdateStatus, DataFileConfiguration, DataUpdateService, OnPremiseAspectEngine,
};

/// A mobile Safari on iPhone User-Agent.
const MOBILE_USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
    AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";

/// Where the published date starts in a Hash data file header, after the
/// four version numbers, two 16 byte tags, the copyright offset, the age,
/// the minimum User-Agent count and the name and format offsets. The next
/// update date follows it.
const PUBLISHED_AT: usize = 66;

/// The dates written into the Lite data for these tests, far from any real
/// file's and from today.
const PUBLISHED: (i16, u8, u8) = (2001, 2, 3);
const NEXT_UPDATE: (i16, u8, u8) = (2001, 2, 4);

/// The Lite file, or [`None`] with a note when it is missing.
fn lite() -> Option<PathBuf> {
    let path = std::env::var("51DEGREES_DD_PATH")
        .ok()
        .map(PathBuf::from)
        .filter(|path| path.is_file());
    if path.is_none() {
        eprintln!("skipping the data file dates tests: set 51DEGREES_DD_PATH to the Lite file");
    }
    path
}

/// The Lite data with [`PUBLISHED`] and [`NEXT_UPDATE`] in its header.
fn dated(lite: &PathBuf) -> Vec<u8> {
    let mut bytes = std::fs::read(lite).unwrap();
    for (at, (year, month, day)) in [(PUBLISHED_AT, PUBLISHED), (PUBLISHED_AT + 4, NEXT_UPDATE)] {
        bytes[at..at + 2].copy_from_slice(&year.to_le_bytes());
        bytes[at + 2] = month;
        bytes[at + 3] = day;
    }
    bytes
}

/// Midnight UTC on `date`.
fn midnight((year, month, day): (i16, u8, u8)) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(year.into(), month.into(), day.into(), 0, 0, 0)
        .unwrap()
}

/// A folder of its own for `name`, emptied.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fiftyone-dd-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Check the engine recorded [`PUBLISHED`], and [`NEXT_UPDATE`] at noon UTC
/// and a minute of that hour.
fn assert_dates(engine: &Arc<DeviceDetectionOnPremiseEngine>, case: &str) {
    let file = &engine.data_files()[0];
    assert_eq!(file.data_published(), Some(midnight(PUBLISHED)), "{case}");
    let next = file.update_available_time().expect(case);
    assert_eq!(
        next.date_naive(),
        midnight(NEXT_UPDATE).date_naive(),
        "{case}"
    );
    assert_eq!((next.hour(), next.second()), (12, 0), "{case}");
}

/// Whether the engine answers `IsMobile` for a mobile User-Agent.
fn is_mobile(engine: &Arc<DeviceDetectionOnPremiseEngine>) -> bool {
    let element: Arc<dyn FlowElement> = engine.clone();
    let pipeline = Pipeline::builder().add_element(element).build().unwrap();
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", MOBILE_USER_AGENT)
            .build(),
    );
    data.process().unwrap();
    let device = data.get(DEVICE_DATA_KEY).unwrap();
    format!("{:?}", device.get("IsMobile").unwrap()) == "Bool(true)"
}

/// The engine records the dates in the file it loads, not the time the file
/// was written, which here is today.
#[test]
fn the_dates_recorded_are_the_ones_the_data_file_header_holds() {
    let Some(lite) = lite() else {
        return;
    };
    let dir = folder("dates");
    let path = dir.join("51Degrees.hash");
    std::fs::write(&path, dated(&lite)).unwrap();
    for profile in [PerformanceProfile::LowMemory, PerformanceProfile::InMemory] {
        let engine = DeviceDetectionOnPremiseEngineBuilder::new(&path)
            .performance_profile(profile)
            .build()
            .expect("the data file loads");
        assert_dates(&engine, profile.as_str());
        engine.refresh(None).expect("the data file loads again");
        assert_dates(&engine, profile.as_str());
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A refresh from memory records the dates the new data holds.
#[test]
fn a_refresh_from_memory_records_the_dates_of_the_new_data() {
    let Some(lite) = lite() else {
        return;
    };
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(&lite)
        .performance_profile(PerformanceProfile::LowMemory)
        .build()
        .expect("the data file loads");
    assert_ne!(
        engine.data_files()[0].data_published(),
        Some(midnight(PUBLISHED))
    );
    engine
        .refresh_from_memory(None, &dated(&lite))
        .expect("the data loads from memory");
    assert_dates(&engine, "from memory");
    assert!(is_mobile(&engine));
}

/// A data file the update service downloads keeps the publish date the
/// engine reads from it, which the next request's `If-Modified-Since`
/// carries, rather than the time of the download.
#[test]
fn a_downloaded_data_file_gives_the_date_it_holds() {
    let Some(lite) = lite() else {
        return;
    };
    let dir = folder("download-dates");
    let path = dir.join("51Degrees.hash");
    std::fs::copy(&lite, &path).unwrap();
    let body = dated(&lite);

    // Serves `body` once, uncompressed, then closes.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/download", listener.local_addr().unwrap());
    let served = body.clone();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 4096];
        let _ = stream.read(&mut request);
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            served.len()
        );
        stream.write_all(head.as_bytes()).unwrap();
        stream.write_all(&served).unwrap();
    });

    let service = Arc::new(DataUpdateService::new());
    let config = DataFileConfiguration::builder(&path)
        .data_update_url(url)
        .data_update_license_keys(["KEY"])
        .decompress_content(false)
        .verify_md5(false)
        .automatic_updates_enabled(false)
        .file_system_watcher_enabled(false)
        .build();
    // In memory, so the data file can be replaced on every platform.
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(&path)
        .performance_profile(PerformanceProfile::InMemory)
        .auto_update(config, Some(Arc::clone(&service)))
        .build()
        .expect("the data file loads");
    let engine_dyn: Arc<dyn OnPremiseAspectEngine> = engine.clone();

    let status = service.check_for_update(&engine_dyn, None).unwrap();
    server.join().unwrap();
    assert_eq!(status, AutoUpdateStatus::Success);
    assert_eq!(std::fs::read(&path).unwrap(), body);
    assert_dates(&engine, "downloaded");
    assert!(is_mobile(&engine));

    drop(engine_dyn);
    drop(engine);
    drop(service);
    let _ = std::fs::remove_dir_all(&dir);
}
