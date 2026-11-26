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

//! A data update of an IP Intelligence engine asks for the product of the
//! data file it loaded, against a local stand-in for the Distributor. The
//! tests need the ASN file `_51DEGREES_IPI_PATH` names, or the one under
//! `cxx/data`, and fail without it.

#[path = "../../pipeline-engines/tests/support/distributor.rs"]
mod distributor;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{TimeZone, Utc};
use distributor::{Answer, StandIn};
use fiftyone_ip_intelligence_onpremise::IpIntelligenceOnPremiseEngine;
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{
    EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData,
};
use fiftyone_pipeline_engines::{
    AspectEngine, AspectEngineDataFile, AspectPropertyMetaData, AutoUpdateStatus,
    DataFileConfiguration, DataUpdateService, OnPremiseAspectEngine,
};

/// The product of the ASN data file.
const ASN: &str = "IPIV4Asn";

/// The licence key the requests carry. The stand-in takes any key.
const KEY: &str = "ABCDEFGHIJKLMNOP";

/// The ASN file the tests read.
fn asn() -> PathBuf {
    std::env::var_os("_51DEGREES_IPI_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../cxx/data/ip-intelligence/51Degrees-IPIV4AsnIpiV41.ipi")
        })
}

/// The bytes of the ASN file, failing the test when it is missing.
fn asn_bytes() -> Vec<u8> {
    let path = asn();
    std::fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "the ASN file at {} cannot be read ({error}), set _51DEGREES_IPI_PATH",
            path.display()
        )
    })
}

/// A stand-in for licence keys holding the Device Detection product and
/// `product`, serving `data` as published long before any real file, so
/// that it answers 304 to a request for it.
fn stand_in(product: &str, data: &[u8]) -> StandIn {
    let long_ago = Utc.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap();
    StandIn::new(&["V4TAC", product], Some((data, long_ago)))
}

/// A folder of its own for `name`, emptied, holding a copy of the ASN file,
/// whose path is returned with the folder.
fn folder_with_asn(name: &str, asn: &[u8]) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "fiftyone-ipi-product-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("51Degrees.ipi");
    std::fs::write(&path, asn).unwrap();
    (dir, path)
}

/// An engine for the data file at `path` updated from `url` through
/// `service`, polling on start-up when `on_start_up` is set.
fn engine(
    path: &Path,
    url: String,
    service: &Arc<DataUpdateService>,
    on_start_up: bool,
) -> Arc<IpIntelligenceOnPremiseEngine> {
    IpIntelligenceOnPremiseEngine::builder(path)
        .performance_profile(PerformanceProfile::InMemory)
        .data_update_url(url)
        .data_update_license_keys([KEY])
        .auto_update(false)
        .file_system_watcher(false)
        .update_on_startup(on_start_up)
        .data_update_service(Arc::clone(service))
        .build_shared()
        .unwrap()
}

/// With no product configured, the update on start-up asks for no product,
/// gets the 300, and asks for the product of the ASN file the engine
/// loaded, which is answered 304, as the file is no newer.
#[test]
fn the_update_on_start_up_asks_for_the_product_of_the_data_file() {
    let asn = asn_bytes();
    let stand_in = stand_in(ASN, &asn);
    let (dir, path) = folder_with_asn("start-up", &asn);
    let service = Arc::new(DataUpdateService::new());

    let engine = engine(&path, stand_in.url(), &service, true);

    let requests = stand_in.requests();
    let asked: Vec<_> = requests
        .iter()
        .map(|r| (r.product.clone(), r.status))
        .collect();
    assert_eq!(asked, [(None, 300), (Some(ASN.to_owned()), 304)]);
    assert_eq!(
        requests[1].target,
        format!("{}&Product={ASN}", requests[0].target)
    );
    assert_eq!(requests[0].if_modified_since, requests[1].if_modified_since);
    assert_eq!(engine.data_files()[0].product().as_deref(), Some(ASN));
    assert_eq!(std::fs::read(&path).unwrap(), asn);

    drop(engine);
    drop(service);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A download cut short, one whose MD5 is not the one sent, and a 429 each
/// fail with the status the specification names, after the product is
/// chosen, and leave the data file as it was.
#[test]
fn a_failed_download_leaves_the_data_file_as_it_was() {
    let asn = asn_bytes();
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
        let stand_in = stand_in(ASN, &asn);
        let (dir, path) = folder_with_asn(&format!("failed-{index}"), &asn);
        let service = Arc::new(DataUpdateService::new());
        let engine = engine(&path, stand_in.url(), &service, false);
        stand_in.then(Answer::Status(
            300,
            Vec::new(),
            format!(
                "{{\"AvailableUris\":[\"{}?Product={ASN}\"]}}",
                stand_in.url()
            ),
        ));
        stand_in.then(answer);
        let as_engine: Arc<dyn OnPremiseAspectEngine> = engine.clone();

        let status = match service.check_for_update(&as_engine, None) {
            Ok(status) => status,
            Err(error) => error.status,
        };

        assert_eq!(status, wanted, "case {index}");
        let requests = stand_in.requests();
        assert_eq!(requests.len(), 2, "case {index}");
        assert!(requests[0].target.contains("Type=IPIV41"), "case {index}");
        assert_eq!(std::fs::read(&path).unwrap(), asn, "case {index}");
        drop(as_engine);
        drop(engine);
        drop(service);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// With a product configured on the builder, the first request asks for it.
#[test]
fn a_configured_product_is_asked_for_in_the_first_request() {
    let asn = asn_bytes();
    let stand_in = stand_in(ASN, &asn);
    let (dir, path) = folder_with_asn("configured", &asn);
    let service = Arc::new(DataUpdateService::new());

    let engine = IpIntelligenceOnPremiseEngine::builder(&path)
        .performance_profile(PerformanceProfile::InMemory)
        .data_update_url(stand_in.url())
        .data_update_license_keys([KEY])
        .data_update_product(ASN)
        .auto_update(false)
        .file_system_watcher(false)
        .update_on_startup(true)
        .data_update_service(Arc::clone(&service))
        .build_shared()
        .unwrap();

    let requests = stand_in.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].product.as_deref(), Some(ASN));
    assert_eq!(requests[0].status, 304);
    assert!(requests[0].target.ends_with(&format!("&Product={ASN}")));
    drop(engine);
    drop(service);
    let _ = std::fs::remove_dir_all(&dir);
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

/// With no product recorded by the engine, the update on start-up asks for
/// the product the header of the IP Intelligence data file names, and the
/// engine's reader gives it, and none for a missing file.
#[test]
fn with_no_engine_product_the_header_names_the_product() {
    let data = asn_bytes();
    let stand_in = stand_in(ASN, &data);
    let (dir, path) = folder_with_asn("header", &data);
    let data_file = Arc::new(AspectEngineDataFile::new(
        DataFileConfiguration::builder(&path)
            .data_update_url(stand_in.url())
            .automatic_updates_enabled(false)
            .file_system_watcher_enabled(false)
            .update_on_startup(true)
            .product_from_file(IpIntelligenceOnPremiseEngine::product_from_file)
            .build(),
    ));
    let engine: Arc<dyn OnPremiseAspectEngine> = Arc::new(NotLoaded {
        filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        data_files: vec![Arc::clone(&data_file)],
    });
    let service = DataUpdateService::new();

    let result = service.register(Arc::clone(&engine), data_file);

    assert_eq!(result.unwrap(), AutoUpdateStatus::NotNeeded);
    let asked: Vec<_> = stand_in.requests().into_iter().map(|r| r.product).collect();
    assert_eq!(asked, [None, Some(ASN.to_owned())]);
    assert_eq!(
        IpIntelligenceOnPremiseEngine::product_from_file(&asn()).as_deref(),
        Some(ASN)
    );
    assert_eq!(
        IpIntelligenceOnPremiseEngine::product_from_file(&dir.join("missing.ipi")),
        None
    );
    drop(service);
    drop(engine);
    let _ = std::fs::remove_dir_all(&dir);
}
