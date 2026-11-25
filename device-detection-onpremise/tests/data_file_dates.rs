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
//! downloads. The engine reads a file's published date without loading it,
//! and the data update service loads a downloaded file only when that date
//! is later than the data's in use.
//!
//! The tests need the Lite file `_51DEGREES_DD_PATH` names and skip with a
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

/// A published date before [`PUBLISHED`].
const EARLIER: (i16, u8, u8) = (2001, 2, 2);

/// The Lite file, or [`None`] with a note when it is missing.
fn lite() -> Option<PathBuf> {
    let path = std::env::var("_51DEGREES_DD_PATH")
        .ok()
        .map(PathBuf::from)
        .filter(|path| path.is_file());
    if path.is_none() {
        eprintln!("skipping the data file dates tests: set _51DEGREES_DD_PATH to the Lite file");
    }
    path
}

/// The Lite data with [`PUBLISHED`] and [`NEXT_UPDATE`] in its header.
fn dated(lite: &PathBuf) -> Vec<u8> {
    dated_as(lite, PUBLISHED)
}

/// The Lite data with `published` and [`NEXT_UPDATE`] in its header.
fn dated_as(lite: &PathBuf, published: (i16, u8, u8)) -> Vec<u8> {
    let mut bytes = std::fs::read(lite).unwrap();
    for (at, (year, month, day)) in [(PUBLISHED_AT, published), (PUBLISHED_AT + 4, NEXT_UPDATE)] {
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

/// Data whose header dates are not valid loads, and records no dates, where
/// .NET refuses to build on it and throws from a refresh after loading it.
/// A refresh to such data clears the dates of the data it replaces, and a
/// refresh to data whose next update date alone is not valid clears that
/// date alone.
#[test]
fn data_whose_header_dates_are_not_valid_loads_and_records_no_dates() {
    let Some(lite) = lite() else {
        return;
    };
    let mut undated = dated(&lite);
    undated[PUBLISHED_AT..PUBLISHED_AT + 8].fill(0);
    let mut no_next_update = dated(&lite);
    no_next_update[PUBLISHED_AT + 4..PUBLISHED_AT + 8].fill(0);
    let dir = folder("undated");
    let path = dir.join("51Degrees.hash");
    std::fs::write(&path, &undated).unwrap();
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(&path)
        .performance_profile(PerformanceProfile::LowMemory)
        .build()
        .expect("the data file loads");
    let file = &engine.data_files()[0];
    assert_eq!(file.data_published(), None);
    assert_eq!(file.update_available_time(), None);
    assert!(is_mobile(&engine));

    engine
        .refresh_from_memory(None, &dated(&lite))
        .expect("the data loads from memory");
    assert_dates(&engine, "dated");
    engine
        .refresh_from_memory(None, &undated)
        .expect("the data loads from memory");
    assert_eq!(file.data_published(), None, "undated after dated");
    assert_eq!(file.update_available_time(), None, "undated after dated");
    assert!(is_mobile(&engine));

    engine
        .refresh_from_memory(None, &dated(&lite))
        .expect("the data loads from memory");
    assert_dates(&engine, "dated again");
    engine
        .refresh_from_memory(None, &no_next_update)
        .expect("the data loads from memory");
    assert_eq!(
        file.data_published(),
        Some(midnight(PUBLISHED)),
        "no next update after dated"
    );
    assert_eq!(
        file.update_available_time(),
        None,
        "no next update after dated"
    );
    assert!(is_mobile(&engine));
    drop(engine);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A refresh from the file after a refresh from memory loads the file the
/// engine was built from, with its dates. .NET's fails, as its reload from
/// the original file then has no file name.
#[test]
fn a_refresh_from_the_file_after_one_from_memory_loads_the_file() {
    let Some(lite) = lite() else {
        return;
    };
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(&lite)
        .performance_profile(PerformanceProfile::LowMemory)
        .build()
        .expect("the data file loads");
    let built = engine.data_files()[0].data_published();
    assert_ne!(built, Some(midnight(PUBLISHED)));
    engine
        .refresh_from_memory(None, &dated(&lite))
        .expect("the data loads from memory");
    assert_dates(&engine, "from memory");
    engine.refresh(None).expect("the data file loads again");
    assert_eq!(engine.data_files()[0].data_published(), built);
    assert!(is_mobile(&engine));
}

/// Serve `body` once, uncompressed, then close, and give the URL.
fn serve_once(body: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/download", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 4096];
        let _ = stream.read(&mut request);
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes()).unwrap();
        stream.write_all(&body).unwrap();
    });
    (url, server)
}

/// An engine on the data file at `path` that `service` updates from `url`
/// when asked.
fn downloading_engine(
    path: &PathBuf,
    url: String,
    service: &Arc<DataUpdateService>,
) -> Arc<DeviceDetectionOnPremiseEngine> {
    let config = DataFileConfiguration::builder(path)
        .data_update_url(url)
        .data_update_license_keys(["KEY"])
        .decompress_content(false)
        .verify_md5(false)
        .automatic_updates_enabled(false)
        .file_system_watcher_enabled(false)
        .build();
    // In memory, so the data file can be replaced on every platform.
    DeviceDetectionOnPremiseEngineBuilder::new(path)
        .performance_profile(PerformanceProfile::InMemory)
        .auto_update(config, Some(Arc::clone(service)))
        .build()
        .expect("the data file loads")
}

/// A data file the update service downloads keeps the publish date the
/// engine reads from it, which the next request's `If-Modified-Since`
/// carries, rather than the time of the download. The data in use was
/// published earlier, so the download is loaded.
#[test]
fn a_downloaded_data_file_gives_the_date_it_holds() {
    let Some(lite) = lite() else {
        return;
    };
    let dir = folder("download-dates");
    let path = dir.join("51Degrees.hash");
    std::fs::write(&path, dated_as(&lite, EARLIER)).unwrap();
    let body = dated(&lite);
    let (url, server) = serve_once(body.clone());

    let service = Arc::new(DataUpdateService::new());
    let engine = downloading_engine(&path, url, &service);
    let engine_dyn: Arc<dyn OnPremiseAspectEngine> = engine.clone();
    assert_eq!(
        engine.data_files()[0].data_published(),
        Some(midnight(EARLIER))
    );

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

/// A downloaded data file published on the same day as the data in use, or
/// earlier, is not loaded, and the check is reported as not needed. The data
/// file and the dates recorded stay as they were.
#[test]
fn a_downloaded_data_file_published_no_later_is_not_loaded() {
    let Some(lite) = lite() else {
        return;
    };
    for (name, served) in [("same", PUBLISHED), ("earlier", EARLIER)] {
        let dir = folder(&format!("download-{name}"));
        let path = dir.join("51Degrees.hash");
        let loaded = dated(&lite);
        std::fs::write(&path, &loaded).unwrap();
        let (url, server) = serve_once(dated_as(&lite, served));

        let service = Arc::new(DataUpdateService::new());
        let engine = downloading_engine(&path, url, &service);
        let engine_dyn: Arc<dyn OnPremiseAspectEngine> = engine.clone();

        let status = service.check_for_update(&engine_dyn, None).unwrap();
        server.join().unwrap();
        assert_eq!(status, AutoUpdateStatus::NotNeeded, "{name}");
        assert!(std::fs::read(&path).unwrap() == loaded, "{name}");
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            1,
            "{name}: the download was not removed"
        );
        assert_dates(&engine, name);
        assert!(is_mobile(&engine), "{name}");

        drop(engine_dyn);
        drop(engine);
        drop(service);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// The engine reads the date a data file was published from its header
/// without loading it, and reads none from a file that is not a data file.
#[test]
fn the_published_date_of_a_data_file_is_read_without_loading_it() {
    let Some(lite) = lite() else {
        return;
    };
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(&lite)
        .performance_profile(PerformanceProfile::LowMemory)
        .build()
        .expect("the data file loads");
    let built = engine.data_files()[0].data_published();
    let dir = folder("published-from-file");
    let path = dir.join("51Degrees.hash");
    std::fs::write(&path, dated(&lite)).unwrap();
    let text = dir.join("notes.txt");
    std::fs::write(&text, "not a data file. ".repeat(20)).unwrap();

    assert_eq!(
        engine.published_from_file(None, &path),
        Some(midnight(PUBLISHED))
    );
    assert_eq!(engine.published_from_file(None, &lite), built);
    assert_eq!(engine.published_from_file(None, &text), None);
    assert_eq!(engine.data_files()[0].data_published(), built);
    let _ = std::fs::remove_dir_all(&dir);
}
