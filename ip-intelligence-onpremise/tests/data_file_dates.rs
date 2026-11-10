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
//! header holds, as .NET's IP Intelligence engine records them, whether the
//! engine loads the file, data given in memory or a file the data update
//! service downloads.
//!
//! Each test holds several copies of the data at once, so the tests use the
//! ASN file in the data repository inside the `cxx` submodule whatever
//! `51DEGREES_IPI_PATH` names, and skip with a note when it is missing.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, TimeZone, Timelike, Utc};
use fiftyone_ip_intelligence_onpremise::{IpIntelligenceOnPremiseEngine, IP_DATA_KEY};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{Evidence, FlowElement, Pipeline};
use fiftyone_pipeline_engines::{AutoUpdateStatus, DataUpdateService, OnPremiseAspectEngine};

/// Where the published date starts in an IP Intelligence data file header,
/// after the four version numbers, two 16 byte tags, the copyright offset,
/// the age, the profile offset shift and the name and format offsets. The
/// next update date follows it.
const PUBLISHED_AT: usize = 66;

/// The dates written into the ASN data for these tests, far from any real
/// file's and from today.
const PUBLISHED: (i16, u8, u8) = (2001, 2, 3);
const NEXT_UPDATE: (i16, u8, u8) = (2001, 2, 4);

/// The name of the ASN data file in the data repository.
const ASN_FILE_NAME: &str = "51Degrees-IPIV4AsnIpiV41.ipi";

/// The ASN file in the data repository inside the `cxx` submodule, or
/// [`None`] with a note when it is missing.
fn asn_file() -> Option<PathBuf> {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        let candidate = dir.join("cxx/data/ip-intelligence").join(ASN_FILE_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            eprintln!(
                "skipping the data file dates tests: {ASN_FILE_NAME} is not in \
                 cxx/data/ip-intelligence"
            );
            return None;
        }
    }
}

/// The ASN data with [`PUBLISHED`] and [`NEXT_UPDATE`] in its header.
fn dated(asn: &Path) -> Vec<u8> {
    let mut bytes = std::fs::read(asn).unwrap();
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
    let dir = std::env::temp_dir().join(format!("fiftyone-ipi-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// An engine on the data file at `path` under `profile`, with automatic
/// updates off so the build needs no licence key.
fn built_on(path: &Path, profile: PerformanceProfile) -> Arc<IpIntelligenceOnPremiseEngine> {
    IpIntelligenceOnPremiseEngine::builder(path)
        .performance_profile(profile)
        .properties(["Asn"])
        .auto_update(false)
        .build_shared()
        .expect("the data file loads")
}

/// Check the engine recorded [`PUBLISHED`], and [`NEXT_UPDATE`] at noon UTC
/// and a minute of that hour.
fn assert_dates(engine: &Arc<IpIntelligenceOnPremiseEngine>, case: &str) {
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

/// Whether the engine answers Cloudflare's autonomous system for 1.1.1.1.
fn answers(engine: &Arc<IpIntelligenceOnPremiseEngine>) -> bool {
    let element: Arc<dyn FlowElement> = engine.clone();
    let pipeline = Pipeline::builder().add_element(element).build().unwrap();
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("server.client-ip", "1.1.1.1")
            .build(),
    );
    data.process().unwrap();
    let ip = data.get(IP_DATA_KEY).unwrap();
    let asn = ip.string("Asn");
    asn.value().is_ok_and(|value| value.contains("AS13335"))
}

/// The engine records the dates in the file it loads, not the time the file
/// was written, which here is today.
#[test]
fn the_dates_recorded_are_the_ones_the_data_file_header_holds() {
    let Some(asn) = asn_file() else {
        return;
    };
    let dir = folder("dates");
    let path = dir.join("51Degrees.ipi");
    std::fs::write(&path, dated(&asn)).unwrap();
    for profile in [PerformanceProfile::LowMemory, PerformanceProfile::InMemory] {
        let engine = built_on(&path, profile);
        assert_dates(&engine, profile.as_str());
        engine.refresh(None).expect("the data file loads again");
        assert_dates(&engine, profile.as_str());
        assert!(answers(&engine), "{}", profile.as_str());
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A refresh from memory records the dates the new data holds, keeps the
/// properties the engine was built with, and the engine answers from the
/// new data.
#[test]
fn a_refresh_from_memory_records_the_dates_of_the_new_data() {
    let Some(asn) = asn_file() else {
        return;
    };
    for profile in [PerformanceProfile::LowMemory, PerformanceProfile::InMemory] {
        let engine = built_on(&asn, profile);
        assert_ne!(
            engine.data_files()[0].data_published(),
            Some(midnight(PUBLISHED))
        );
        engine
            .refresh_from_memory(None, &dated(&asn))
            .expect("the data loads from memory");
        assert_dates(&engine, profile.as_str());
        assert_eq!(
            engine.available_property_names(),
            ["Asn"],
            "{}",
            profile.as_str()
        );
        assert!(answers(&engine), "{}", profile.as_str());
    }
}

/// A refresh from data that is not a whole data file fails, and the engine
/// keeps the data and the dates it had.
#[test]
fn a_failed_refresh_from_memory_keeps_the_data_and_dates() {
    let Some(asn) = asn_file() else {
        return;
    };
    let dir = folder("failed");
    let path = dir.join("51Degrees.ipi");
    let data = dated(&asn);
    std::fs::write(&path, &data).unwrap();
    let engine = built_on(&path, PerformanceProfile::LowMemory);
    for (case, refused) in [
        ("no data", &[][..]),
        ("half the data", &data[..data.len() / 2]),
    ] {
        engine.refresh_from_memory(None, refused).expect_err(case);
        assert_dates(&engine, case);
        assert!(answers(&engine), "{case}");
    }
    drop(engine);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A refresh from a file that is not a whole data file, or from no file,
/// fails, and the engine keeps the data and the dates it had. The engine
/// holds its data in memory, so the file can be replaced on every platform.
#[test]
fn a_failed_refresh_from_the_file_keeps_the_data_and_dates() {
    let Some(asn) = asn_file() else {
        return;
    };
    let dir = folder("failed-file");
    let path = dir.join("51Degrees.ipi");
    let data = dated(&asn);
    std::fs::write(&path, &data).unwrap();
    let engine = built_on(&path, PerformanceProfile::InMemory);
    for (case, refused) in [
        ("a file that is not a data file", Some(vec![0x5a_u8; 4096])),
        ("half a data file", Some(data[..data.len() / 2].to_vec())),
        ("no file", None),
    ] {
        match refused {
            Some(refused) => std::fs::write(&path, refused).unwrap(),
            None => std::fs::remove_file(&path).unwrap(),
        }
        engine.refresh(None).expect_err(case);
        assert_dates(&engine, case);
        assert!(answers(&engine), "{case}");
    }
    drop(engine);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Data whose header dates are not valid loads, and records no dates, where
/// .NET throws from building each date. A refresh to such data clears the
/// dates of the data it replaces, and a refresh to data whose next update
/// date alone is not valid clears that date alone.
#[test]
fn data_whose_header_dates_are_not_valid_loads_and_records_no_dates() {
    let Some(asn) = asn_file() else {
        return;
    };
    let mut undated = dated(&asn);
    undated[PUBLISHED_AT..PUBLISHED_AT + 8].fill(0);
    let mut no_next_update = dated(&asn);
    no_next_update[PUBLISHED_AT + 4..PUBLISHED_AT + 8].fill(0);
    let dir = folder("undated");
    let path = dir.join("51Degrees.ipi");
    std::fs::write(&path, &undated).unwrap();
    let engine = built_on(&path, PerformanceProfile::LowMemory);
    let file = &engine.data_files()[0];
    assert_eq!(file.data_published(), None);
    assert_eq!(file.update_available_time(), None);
    assert!(answers(&engine));

    engine
        .refresh_from_memory(None, &dated(&asn))
        .expect("the data loads from memory");
    assert_dates(&engine, "dated");
    engine
        .refresh_from_memory(None, &undated)
        .expect("the data loads from memory");
    assert_eq!(file.data_published(), None, "undated after dated");
    assert_eq!(file.update_available_time(), None, "undated after dated");
    assert!(answers(&engine));

    engine
        .refresh_from_memory(None, &dated(&asn))
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
    assert!(answers(&engine));
    drop(engine);

    // The same for a refresh from the file, with the data held in memory so
    // the file can be replaced on every platform.
    std::fs::write(&path, dated(&asn)).unwrap();
    let engine = built_on(&path, PerformanceProfile::InMemory);
    assert_dates(&engine, "dated file");
    std::fs::write(&path, undated).unwrap();
    engine.refresh(None).expect("the data file loads again");
    let file = &engine.data_files()[0];
    assert_eq!(file.data_published(), None, "undated file after dated");
    assert_eq!(
        file.update_available_time(),
        None,
        "undated file after dated"
    );
    assert!(answers(&engine));
    drop(engine);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A refresh from the file after a refresh from memory loads the file the
/// engine was built from, with its dates.
#[test]
fn a_refresh_from_the_file_after_one_from_memory_loads_the_file() {
    let Some(asn) = asn_file() else {
        return;
    };
    let engine = built_on(&asn, PerformanceProfile::LowMemory);
    let built = engine.data_files()[0].data_published();
    assert_ne!(built, Some(midnight(PUBLISHED)));
    engine
        .refresh_from_memory(None, &dated(&asn))
        .expect("the data loads from memory");
    assert_dates(&engine, "from memory");
    engine.refresh(None).expect("the data file loads again");
    assert_eq!(engine.data_files()[0].data_published(), built);
    assert!(answers(&engine));
}

/// A data file the update service downloads keeps the dates the engine
/// reads from it, rather than the time of the download.
#[test]
fn a_downloaded_data_file_gives_the_dates_it_holds() {
    let Some(asn) = asn_file() else {
        return;
    };
    let dir = folder("download-dates");
    let path = dir.join("51Degrees.ipi");
    std::fs::copy(&asn, &path).unwrap();
    let body = dated(&asn);

    // Serves `body` once, compressed as the Distributor serves a data file,
    // then closes.
    let compressed = {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(&body).unwrap();
        encoder.finish().unwrap()
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/download", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 4096];
        let _ = stream.read(&mut request);
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            compressed.len()
        );
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(&compressed);
    });

    let service = Arc::new(DataUpdateService::new());
    // In memory, so the data file can be replaced on every platform.
    let engine = IpIntelligenceOnPremiseEngine::builder(&path)
        .performance_profile(PerformanceProfile::InMemory)
        .properties(["Asn"])
        .auto_update(true)
        .file_system_watcher(false)
        .polling_interval_seconds(24 * 60 * 60)
        .max_randomisation_seconds(0)
        .data_update_url(url)
        .data_update_license_keys(["KEY"])
        .data_update_service(Arc::clone(&service))
        .build_shared()
        .expect("the data file loads");
    let engine_dyn: Arc<dyn OnPremiseAspectEngine> = engine.clone();

    let status = service.check_for_update(&engine_dyn, None);
    server.join().unwrap();
    assert_eq!(
        status.map_err(|error| error.to_string()),
        Ok(AutoUpdateStatus::Success)
    );
    assert_eq!(std::fs::read(&path).unwrap(), body);
    assert_dates(&engine, "downloaded");
    assert!(answers(&engine));

    drop(engine_dyn);
    drop(engine);
    drop(service);
    let _ = std::fs::remove_dir_all(&dir);
}
