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

//! A data update of a Hash engine asks for the product of the data file it
//! loaded, or, when no product was recorded for the data loaded, the product
//! the header of the data file names, against a local stand-in for the
//! Distributor, and applies the file it gets. The tests need the Lite file `_51DEGREES_DD_PATH` names, or
//! the one under `cxx/data`, and fail without it.

#[path = "../../pipeline-engines/tests/support/distributor.rs"]
mod distributor;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Days, NaiveDate, Utc};
use distributor::{Answer, StandIn};
use fiftyone_device_detection_onpremise::{
    DeviceDetectionOnPremiseEngine, DeviceDetectionOnPremiseEngineBuilder,
};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{
    EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData,
};
use fiftyone_pipeline_engines::{
    AspectEngine, AspectEngineDataFile, AspectPropertyMetaData, AutoUpdateStatus,
    DataFileConfiguration, DataUpdateResult, DataUpdateService, OnPremiseAspectEngine,
};

/// The product of the Lite data file.
const LITE: &str = "V4Lite";

/// Where the published date starts in a Hash data file header, as a 16 bit
/// year, a month and a day.
const PUBLISHED_AT: usize = 66;

/// The licence key the requests carry. The stand-in takes any key.
const KEY: &str = "ABCDEFGHIJKLMNOP";

/// The Lite file the tests read.
fn lite() -> PathBuf {
    std::env::var_os("_51DEGREES_DD_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../cxx/data/device-detection/51Degrees-LiteV4.1.hash")
        })
}

/// The bytes of the Lite file, failing the test when it is missing.
fn lite_bytes() -> Vec<u8> {
    let path = lite();
    std::fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "the Lite file at {} cannot be read ({error}), set _51DEGREES_DD_PATH",
            path.display()
        )
    })
}

/// The published date in the header of `data`, at midnight UTC.
fn published(data: &[u8]) -> DateTime<Utc> {
    let year = i16::from_le_bytes([data[PUBLISHED_AT], data[PUBLISHED_AT + 1]]);
    NaiveDate::from_ymd_opt(
        year.into(),
        data[PUBLISHED_AT + 2].into(),
        data[PUBLISHED_AT + 3].into(),
    )
    .unwrap()
    .and_hms_opt(0, 0, 0)
    .unwrap()
    .and_utc()
}

/// `data` with its published date moved on by a day.
fn published_a_day_later(data: &[u8]) -> Vec<u8> {
    let later = published(data)
        .date_naive()
        .checked_add_days(Days::new(1))
        .unwrap();
    let mut moved = data.to_vec();
    let year = i16::try_from(chrono::Datelike::year(&later)).unwrap();
    moved[PUBLISHED_AT..PUBLISHED_AT + 2].copy_from_slice(&year.to_le_bytes());
    moved[PUBLISHED_AT + 2] = u8::try_from(chrono::Datelike::month(&later)).unwrap();
    moved[PUBLISHED_AT + 3] = u8::try_from(chrono::Datelike::day(&later)).unwrap();
    moved
}

/// A folder of its own for `name`, emptied, holding a copy of the Lite
/// file, whose path is returned with the folder.
fn folder_with_lite(name: &str, lite: &[u8]) -> (PathBuf, PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("fiftyone-dd-product-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("51Degrees.hash");
    std::fs::write(&path, lite).unwrap();
    (dir, path)
}

/// An engine for the data file at `path` updated from `url` through
/// `service`, with the Distributor's URL formatter, its configuration
/// changed by `configure`.
fn engine(
    path: &Path,
    url: String,
    service: &Arc<DataUpdateService>,
    configure: impl FnOnce(
        fiftyone_pipeline_engines::DataFileConfigurationBuilder,
    ) -> fiftyone_pipeline_engines::DataFileConfigurationBuilder,
) -> Arc<DeviceDetectionOnPremiseEngine> {
    let config = DataFileConfiguration::builder(path)
        .data_update_url(url)
        .data_update_license_keys([KEY])
        .automatic_updates_enabled(false)
        .file_system_watcher_enabled(false);
    DeviceDetectionOnPremiseEngineBuilder::new(path)
        .performance_profile(PerformanceProfile::InMemory)
        .auto_update(configure(config).build(), Some(Arc::clone(service)))
        .build()
        .unwrap()
}

/// The status of each request, with the product it named.
fn asked(stand_in: &StandIn) -> Vec<(Option<String>, u16)> {
    stand_in
        .requests()
        .into_iter()
        .map(|request| (request.product, request.status))
        .collect()
}

/// Check once for an update of `engine`'s data file.
fn check(
    service: &DataUpdateService,
    engine: &Arc<DeviceDetectionOnPremiseEngine>,
) -> AutoUpdateStatus {
    let engine: Arc<dyn OnPremiseAspectEngine> = engine.clone();
    match service.check_for_update(&engine, None) {
        Ok(status) => status,
        Err(error) => error.status,
    }
}

/// With no product configured, the update on start-up asks for no product,
/// gets the 300, and asks for the product of the Lite file the engine
/// loaded. The file served, the Lite file with its published date moved on
/// a day, is written over the data file and loaded.
#[test]
fn the_update_on_start_up_asks_for_the_product_of_the_data_file() {
    let lite = lite_bytes();
    let newer = published_a_day_later(&lite);
    let stand_in = StandIn::new(
        &["IPIV4Enterprise", LITE],
        Some((&newer, published(&newer))),
    );
    let (dir, path) = folder_with_lite("start-up", &lite);
    let service = Arc::new(DataUpdateService::new());

    let engine = engine(&path, stand_in.url(), &service, |c| {
        c.update_on_startup(true)
    });

    assert_eq!(
        asked(&stand_in),
        [(None, 300), (Some(LITE.to_owned()), 200)]
    );
    let requests = stand_in.requests();
    assert_eq!(
        requests[0].target,
        format!("/api/v2/download?LicenseKeys={KEY}&Download=True&Type=HashV41")
    );
    assert_eq!(
        requests[1].target,
        format!("{}&Product={LITE}", requests[0].target)
    );
    let on_disk = std::fs::read(&path).unwrap();
    assert_eq!(published(&on_disk), published(&newer));
    assert_eq!(
        engine.data_files()[0].data_published(),
        Some(published(&newer))
    );
    assert_eq!(engine.data_files()[0].product().as_deref(), Some(LITE));

    drop(engine);
    drop(service);
    let _ = std::fs::remove_dir_all(&dir);
}

/// With a product configured, the first request asks for it.
#[test]
fn a_configured_product_is_asked_for_in_the_first_request() {
    let lite = lite_bytes();
    let newer = published_a_day_later(&lite);
    let stand_in = StandIn::new(&["V4TAC"], Some((&newer, published(&newer))));
    let (dir, path) = folder_with_lite("configured", &lite);
    let service = Arc::new(DataUpdateService::new());
    let engine = engine(&path, stand_in.url(), &service, |c| {
        c.data_update_product("V4TAC")
    });

    assert_eq!(check(&service, &engine), AutoUpdateStatus::Success);

    assert_eq!(asked(&stand_in), [(Some("V4TAC".to_owned()), 200)]);
    drop(engine);
    drop(service);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A download cut short, one whose MD5 is not the one sent, and a 429 each
/// fail with the status the specification names, after the product is
/// chosen, and leave the data file and the engine's data as they were.
#[test]
fn a_failed_download_leaves_the_data_file_as_it_was() {
    let lite = lite_bytes();
    let newer = published_a_day_later(&lite);
    let cases = [
        (Answer::TruncatedFile, AutoUpdateStatus::HttpsError),
        (
            Answer::FileWithMd5("0123456789abcdef0123456789abcdef".to_owned()),
            AutoUpdateStatus::Md5ValidationFailed,
        ),
        (
            Answer::Status(429, Vec::new(), String::new()),
            AutoUpdateStatus::TooManyRequests,
        ),
    ];
    for (index, (answer, wanted)) in cases.into_iter().enumerate() {
        let stand_in = StandIn::new(
            &["IPIV4Enterprise", LITE],
            Some((&newer, published(&newer))),
        );
        let (dir, path) = folder_with_lite(&format!("failed-{index}"), &lite);
        let service = Arc::new(DataUpdateService::new());
        let engine = engine(&path, stand_in.url(), &service, |c| c);
        let target = format!("/api/v2/download?LicenseKeys={KEY}&Download=True&Type=HashV41");
        stand_in.then(Answer::Status(
            300,
            Vec::new(),
            format!(
                "{{\"AvailableUris\":[\"{}?Product={LITE}\"]}}",
                stand_in.url()
            ),
        ));
        stand_in.then(answer);

        assert_eq!(check(&service, &engine), wanted, "case {index}");

        let requests = stand_in.requests();
        assert_eq!(requests.len(), 2, "case {index}");
        assert_eq!(requests[0].target, target, "case {index}");
        assert!(requests[1].if_modified_since.is_some(), "case {index}");
        assert_eq!(std::fs::read(&path).unwrap(), lite, "case {index}");
        assert_eq!(
            engine.data_files()[0].data_published(),
            Some(published(&lite)),
            "case {index}"
        );
        drop(engine);
        drop(service);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// An engine that has recorded no product for its data file, as before it
/// has loaded one, and whose refresh does nothing.
struct NotLoaded {
    filter: EvidenceKeyFilterWhitelist,
    data_files: Vec<Arc<AspectEngineDataFile>>,
}

impl FlowElement for NotLoaded {
    fn process(&self, _data: &mut FlowData) -> fiftyone_pipeline_core::Result<()> {
        Ok(())
    }
    fn data_key(&self) -> &str {
        "not-loaded"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

impl AspectEngine for NotLoaded {
    fn data_source_tier(&self) -> &str {
        ""
    }
    fn aspect_properties(&self) -> &[AspectPropertyMetaData] {
        &[]
    }
}

impl OnPremiseAspectEngine for NotLoaded {
    fn data_files(&self) -> &[Arc<AspectEngineDataFile>] {
        &self.data_files
    }
    fn refresh(&self, _identifier: Option<&str>) -> fiftyone_pipeline_core::Result<()> {
        Ok(())
    }
}

/// Register, with update on start-up, an engine that recorded `recorded`
/// for the data file at `path`, whose header the Hash engine's reader reads,
/// against a stand-in offering `V4TAC` and the Lite product, and give the
/// start-up result with the stand-in.
fn start_up(path: &Path, recorded: Option<&str>) -> (DataUpdateResult, StandIn) {
    let lite = lite_bytes();
    let stand_in = StandIn::new(&["V4TAC", LITE], Some((&lite, published(&lite))));
    let data_file = Arc::new(AspectEngineDataFile::new(
        DataFileConfiguration::builder(path)
            .data_update_url(stand_in.url())
            .automatic_updates_enabled(false)
            .file_system_watcher_enabled(false)
            .update_on_startup(true)
            .product_from_file(DeviceDetectionOnPremiseEngine::product_from_file)
            .build(),
    ));
    data_file.record_product(recorded.map(str::to_owned));
    let engine: Arc<dyn OnPremiseAspectEngine> = Arc::new(NotLoaded {
        filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        data_files: vec![Arc::clone(&data_file)],
    });
    let service = DataUpdateService::new();
    let result = service.register(Arc::clone(&engine), data_file);
    drop(service);
    drop(engine);
    (result, stand_in)
}

/// The products the requests named, in order.
fn products(stand_in: &StandIn) -> Vec<Option<String>> {
    stand_in.requests().into_iter().map(|r| r.product).collect()
}

/// With no product recorded by the engine, the update on start-up asks for
/// the product the header of the data file names.
#[test]
fn with_no_engine_product_the_header_names_the_product() {
    let (dir, path) = folder_with_lite("header", &lite_bytes());

    let (result, stand_in) = start_up(&path, None);

    assert_eq!(result.unwrap(), AutoUpdateStatus::NotNeeded);
    assert_eq!(products(&stand_in), [None, Some(LITE.to_owned())]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A product the engine recorded for the data it loaded is asked for in
/// place of the one the header names.
#[test]
fn the_engine_product_wins_over_the_header() {
    let (dir, path) = folder_with_lite("engine-wins", &lite_bytes());

    let (result, stand_in) = start_up(&path, Some("V4TAC"));

    assert_eq!(result.unwrap(), AutoUpdateStatus::NotNeeded);
    assert_eq!(products(&stand_in), [None, Some("V4TAC".to_owned())]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// With no data file at the configured location, a data file cut short, or
/// a file of another kind, no product is known, so the 300 fails the check
/// naming an empty product, after one request.
#[test]
fn with_no_readable_header_no_product_is_known() {
    let lite = lite_bytes();
    let cases: [(&str, Option<Vec<u8>>); 3] = [
        ("missing", None),
        ("truncated", Some(lite[..PUBLISHED_AT].to_vec())),
        ("foreign", Some(b"not a data file".repeat(100))),
    ];
    for (name, bytes) in cases {
        let (dir, path) = folder_with_lite(name, &lite);
        match bytes {
            Some(bytes) => std::fs::write(&path, bytes).unwrap(),
            None => std::fs::remove_file(&path).unwrap(),
        }

        let (result, stand_in) = start_up(&path, None);

        let error = result.expect_err(name);
        assert_eq!(error.status, AutoUpdateStatus::HttpsError, "{name}");
        assert!(
            error.message.starts_with("Product '' is not offered"),
            "{name}: {}",
            error.message
        );
        assert_eq!(products(&stand_in), [None], "{name}");
        assert_eq!(
            DeviceDetectionOnPremiseEngine::product_from_file(&path),
            None,
            "{name}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// The engine's reader gives the product of the Lite file.
#[test]
fn the_engine_reads_the_product_of_the_lite_file() {
    assert_eq!(
        DeviceDetectionOnPremiseEngine::product_from_file(&lite()).as_deref(),
        Some(LITE)
    );
}
