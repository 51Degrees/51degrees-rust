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

//! A data update asks for the product of its data file when the licence keys
//! hold several products, as the data-updates specification's "License Keys
//! with several products" section says, against a local stand-in for the
//! Distributor. The first request names no product unless one is configured.
//! A 300 (Multiple Choices) lists the products offered, and the second
//! request asks for the data file's own. Kept in its own test binary because
//! a logger is installed once for the whole process.
#![cfg(feature = "data-update")]

#[path = "support/distributor.rs"]
mod distributor;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use chrono::{TimeZone, Utc};
use distributor::{Answer, StandIn};
use fiftyone_pipeline_core::{
    EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData, Result,
};
use fiftyone_pipeline_engines::{
    product_name, AspectEngine, AspectEngineDataFile, AspectPropertyMetaData, AutoUpdateStatus,
    DataFileConfiguration, DataFileConfigurationBuilder, DataUpdateResult, DataUpdateService,
    FiftyOneUrlFormatter, OnPremiseAspectEngine,
};

/// The licence key the requests carry. The stand-in takes any key.
const KEY: &str = "ABCDEFGHIJKLMNOP";

/// The two products of the licence keys in most of these tests.
const PRODUCTS: [&str; 2] = ["IPIV4Enterprise", "V4TAC"];

/// The product of the data file in most of these tests.
const TAC: &str = "V4TAC";

/// The data the stand-in serves.
const NEW_DATA: &[u8] = b"new data";

/// The data file before an update.
const OLD_DATA: &[u8] = b"old data";

/// Every information record, as "LEVEL message".
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Keeps every information record in [`LOGGED`].
struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() == log::Level::Info
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

/// The information records that name `origin`, in order.
fn logged_for(origin: &str) -> Vec<String> {
    LOGGED
        .lock()
        .unwrap()
        .iter()
        .filter(|record| record.contains(origin))
        .cloned()
        .collect()
}

/// An engine with data loaded, which a refresh leaves as it is.
struct LoadedEngine {
    filter: EvidenceKeyFilterWhitelist,
    data_files: Vec<Arc<AspectEngineDataFile>>,
}

impl FlowElement for LoadedEngine {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
        Ok(())
    }
    fn data_key(&self) -> &str {
        "loaded"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

impl AspectEngine for LoadedEngine {
    fn data_source_tier(&self) -> &str {
        "TAC"
    }
    fn aspect_properties(&self) -> &[AspectPropertyMetaData] {
        &[]
    }
}

impl OnPremiseAspectEngine for LoadedEngine {
    fn data_files(&self) -> &[Arc<AspectEngineDataFile>] {
        &self.data_files
    }
    fn refresh(&self, _identifier: Option<&str>) -> Result<()> {
        Ok(())
    }
}

/// A data file and its engine, in a folder removed when dropped.
struct Fixture {
    folder: PathBuf,
    path: PathBuf,
    engine: Arc<dyn OnPremiseAspectEngine>,
}

impl Fixture {
    /// A data file holding [`OLD_DATA`] whose engine recorded `product`,
    /// updated from `url` with the configuration `configure` gives.
    fn new(
        test: &str,
        url: String,
        product: Option<&str>,
        configure: impl FnOnce(DataFileConfigurationBuilder) -> DataFileConfigurationBuilder,
    ) -> Fixture {
        let folder = std::env::temp_dir().join(format!(
            "fiftyone-product-selection-{test}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("data.hash");
        std::fs::write(&path, OLD_DATA).unwrap();
        let builder = DataFileConfiguration::builder(&path)
            .data_update_url(url)
            .automatic_updates_enabled(false)
            .file_system_watcher_enabled(false);
        let data_file = Arc::new(AspectEngineDataFile::new(configure(builder).build()));
        data_file.record_product(product.map(str::to_owned));
        let engine: Arc<dyn OnPremiseAspectEngine> = Arc::new(LoadedEngine {
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            data_files: vec![data_file],
        });
        Fixture {
            folder,
            path,
            engine,
        }
    }

    /// Check once for an update.
    fn check(&self) -> DataUpdateResult {
        let _ = log::set_logger(&LOGGER);
        log::set_max_level(log::LevelFilter::Info);
        DataUpdateService::new().check_for_update(&self.engine, None)
    }

    fn data(&self) -> Vec<u8> {
        std::fs::read(&self.path).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

/// The URL a test configures, with the query the formatter would add.
fn update_url(stand_in: &StandIn) -> String {
    format!(
        "{}?LicenseKeys={KEY}&Download=True&Type=HashV41",
        stand_in.url()
    )
}

/// A stand-in for licence keys holding `products` with data newer than any
/// data file here.
fn serving(products: &[&str]) -> StandIn {
    let published = Utc.with_ymd_and_hms(2100, 1, 1, 0, 0, 0).unwrap();
    StandIn::new(products, Some((NEW_DATA, published)))
}

/// A stand-in for licence keys holding `products` with data no newer than
/// any data file here.
fn not_newer(products: &[&str]) -> StandIn {
    let published = Utc.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap();
    StandIn::new(products, Some((NEW_DATA, published)))
}

/// A 300 whose JSON body lists `uris` in `AvailableUris`.
fn choices(uris: &[&str]) -> Answer {
    let quoted: Vec<String> = uris.iter().map(|uri| format!("\"{uri}\"")).collect();
    Answer::Status(
        300,
        Vec::new(),
        format!("{{\"AvailableUris\":[{}]}}", quoted.join(",")),
    )
}

/// The path of a file at `path` with a query, for the URIs of a 300.
fn uri(stand_in: &StandIn, query: &str) -> String {
    format!("{}?{query}", stand_in.url())
}

/// The status of a failed check, and its message.
fn failure(result: DataUpdateResult) -> (AutoUpdateStatus, String) {
    let error = result.expect_err("the check fails");
    (error.status, error.message)
}

fn path_and_query(url: &str) -> String {
    let rest = url.split_once("://").unwrap().1;
    rest[rest.find('/').unwrap()..].to_owned()
}

/// With no product configured, the first request names none and gets the
/// 300, and the second asks for the data file's product as the last
/// parameter of the first request's query, with the rest unchanged. The
/// update is applied, and `Requesting product` is logged before the second
/// `Checking for update from`.
#[test]
fn with_no_product_the_second_request_asks_for_the_data_files_product() {
    let stand_in = serving(&PRODUCTS);
    let url = update_url(&stand_in);
    let fixture = Fixture::new("no-product", url.clone(), Some(TAC), |b| b);

    assert_eq!(fixture.check().unwrap(), AutoUpdateStatus::Success);

    let requests = stand_in.requests();
    let asked: Vec<_> = requests
        .iter()
        .map(|r| (r.product.clone(), r.status))
        .collect();
    assert_eq!(asked, [(None, 300), (Some(TAC.to_owned()), 200)]);
    let first = path_and_query(&url);
    assert_eq!(requests[0].target, first);
    assert_eq!(requests[1].target, format!("{first}&Product={TAC}"));
    assert_eq!(fixture.data(), NEW_DATA);

    let logged = logged_for(stand_in.url().as_str());
    let masked = url.replace(KEY, "************MNOP");
    let requesting =
        format!("INFO Requesting product '{TAC}' from '{masked}' for Engine 'LoadedEngine'");
    let second =
        format!("INFO Checking for update from '{masked}&Product={TAC}' for Engine 'LoadedEngine'");
    let at = |message: &str| logged.iter().position(|r| r == message);
    let (Some(requesting_at), Some(second_at)) = (at(&requesting), at(&second)) else {
        panic!("'{requesting}' and '{second}' are not both logged: {logged:?}");
    };
    assert!(requesting_at < second_at, "{logged:?}");
}

/// A 300 is answered with the product as the 300 spells it, compared with
/// the data file's ignoring case.
#[test]
fn the_product_is_sent_as_the_300_spells_it() {
    let stand_in = serving(&PRODUCTS);
    stand_in.then(choices(&[&uri(&stand_in, "product=v4tac")]));
    let fixture = Fixture::new("spelling", update_url(&stand_in), Some(TAC), |b| b);

    assert_eq!(fixture.check().unwrap(), AutoUpdateStatus::Success);

    let requests = stand_in.requests();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1].target.ends_with("&Product=v4tac"),
        "{requests:?}"
    );
}

/// The second request carries the first request's `If-Modified-Since`, and
/// a 304 to it is no update needed.
#[test]
fn a_304_to_the_second_request_needs_no_update() {
    let stand_in = not_newer(&PRODUCTS);
    let fixture = Fixture::new("not-modified", update_url(&stand_in), Some(TAC), |b| b);

    assert_eq!(fixture.check().unwrap(), AutoUpdateStatus::NotNeeded);

    let requests = stand_in.requests();
    let statuses: Vec<u16> = requests.iter().map(|r| r.status).collect();
    assert_eq!(statuses, [300, 304]);
    assert!(requests[0].if_modified_since.is_some());
    assert_eq!(requests[0].if_modified_since, requests[1].if_modified_since);
    assert_eq!(fixture.data(), OLD_DATA);
}

/// With a product configured, the first request asks for it, and the
/// Distributor formatter's parameters come before it.
#[test]
fn a_configured_product_is_asked_for_in_the_first_request() {
    let stand_in = serving(&PRODUCTS);
    let fixture = Fixture::new("configured", stand_in.url(), None, |b| {
        b.data_update_product(TAC)
            .data_update_license_keys([KEY])
            .data_download_type("HashV41")
            .url_formatter(Some(Arc::new(FiftyOneUrlFormatter)))
    });

    assert_eq!(fixture.check().unwrap(), AutoUpdateStatus::Success);

    let requests = stand_in.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].target,
        format!("/api/v2/download?LicenseKeys={KEY}&Download=True&Type=HashV41&Product={TAC}")
    );
    assert_eq!(fixture.data(), NEW_DATA);
}

/// A configured product the licence keys do not hold gets a 403, which is
/// the forbidden status with the `Access denied` message.
#[test]
fn a_configured_product_not_offered_is_forbidden() {
    let stand_in = serving(&PRODUCTS);
    let url = update_url(&stand_in);
    let fixture = Fixture::new("forbidden", url, Some(TAC), |b| {
        b.data_update_product("V4Enterprise")
    });

    let (status, message) = failure(fixture.check());

    assert_eq!(status, AutoUpdateStatus::Forbidden);
    assert!(
        message.starts_with("Access denied to data update service at"),
        "{message}"
    );
    let asked: Vec<_> = stand_in.requests().into_iter().map(|r| r.product).collect();
    assert_eq!(asked, [Some("V4Enterprise".to_owned())]);
    assert_eq!(fixture.data(), OLD_DATA);
}

/// With a product configured, a 300 is an error as any other status is, and
/// no second request is made.
#[test]
fn with_a_configured_product_a_300_is_an_error() {
    let stand_in = serving(&PRODUCTS);
    stand_in.then(choices(&[&uri(&stand_in, "Product=V4TAC")]));
    let fixture = Fixture::new("configured-300", update_url(&stand_in), None, |b| {
        b.data_update_product(TAC)
    });

    let (status, message) = failure(fixture.check());

    assert_eq!(status, AutoUpdateStatus::HttpsError);
    assert!(message.starts_with("HTTP status code '300'"), "{message}");
    assert_eq!(stand_in.requests().len(), 1);
}

/// A `Product` with a value in the configured URL is the configured
/// product. It is sent as it is, and no other is added, whatever the
/// configuration names.
#[test]
fn a_product_in_the_configured_url_is_sent_as_it_is() {
    let stand_in = serving(&PRODUCTS);
    let url = format!("{}&product=V4TAC", update_url(&stand_in));
    let fixture = Fixture::new("url-product", url.clone(), None, |b| {
        b.data_update_product("V4Enterprise")
    });

    assert_eq!(fixture.check().unwrap(), AutoUpdateStatus::Success);

    let requests = stand_in.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].target, path_and_query(&url));
}

/// A `Product` with an empty value in the configured URL is no product, and
/// the second request carries `Product` once, as its last parameter.
#[test]
fn an_empty_product_in_the_configured_url_is_replaced() {
    let stand_in = serving(&PRODUCTS);
    let url = format!(
        "{}?Product=&LicenseKeys={KEY}&Download=True",
        stand_in.url()
    );
    let fixture = Fixture::new("empty-product", url, Some(TAC), |b| b);

    assert_eq!(fixture.check().unwrap(), AutoUpdateStatus::Success);

    let requests = stand_in.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1].target,
        format!("/api/v2/download?LicenseKeys={KEY}&Download=True&Product={TAC}")
    );
}

/// A 300 that does not offer the data file's product fails after one
/// request, naming the product and the products offered.
#[test]
fn a_product_not_offered_fails_naming_the_products_offered() {
    let stand_in = serving(&["IPIV4Enterprise"]);
    let fixture = Fixture::new("not-offered", update_url(&stand_in), Some(TAC), |b| b);

    let (status, message) = failure(fixture.check());

    assert_eq!(status, AutoUpdateStatus::HttpsError);
    assert!(
        message.starts_with("Product 'V4TAC' is not offered by data update service at"),
        "{message}"
    );
    assert!(
        message.ends_with("Products offered: 'IPIV4Enterprise'"),
        "{message}"
    );
    assert_eq!(stand_in.requests().len(), 1);
}

/// With no product known for the data file, a 300 fails after one request
/// naming an empty product.
#[test]
fn with_no_product_known_a_300_fails() {
    let stand_in = serving(&[TAC]);
    let fixture = Fixture::new("unknown", update_url(&stand_in), None, |b| b);

    let (status, message) = failure(fixture.check());

    assert_eq!(status, AutoUpdateStatus::HttpsError);
    assert!(
        message.starts_with("Product '' is not offered"),
        "{message}"
    );
    assert!(message.ends_with("Products offered: 'V4TAC'"), "{message}");
    assert_eq!(stand_in.requests().len(), 1);
}

/// A 300 in answer to the second request is an error naming the second
/// request's URL, and no third request is made.
#[test]
fn a_300_to_the_second_request_is_an_error() {
    let stand_in = serving(&PRODUCTS);
    let listed = uri(&stand_in, "Product=V4TAC");
    stand_in.then(choices(&[&listed])).then(choices(&[&listed]));
    let url = update_url(&stand_in);
    let fixture = Fixture::new("second-300", url.clone(), Some(TAC), |b| b);

    let (status, message) = failure(fixture.check());

    assert_eq!(status, AutoUpdateStatus::HttpsError);
    assert_eq!(
        message,
        format!(
            "HTTP status code '300' from data update service at '{}&Product={TAC}' for engine \
             'LoadedEngine'",
            url.replace(KEY, "************MNOP")
        )
    );
    assert_eq!(stand_in.requests().len(), 2);
}

/// A 300 whose body is not JSON, has no `AvailableUris` list, has an empty
/// list, or a list that gives no `Product` value, is the error for its
/// status after one request.
#[test]
fn a_300_that_lists_no_product_is_an_error() {
    let bodies = [
        "not JSON".to_owned(),
        "{\"Error\":\"several\"}".to_owned(),
        "{\"availableuris\":[\"http://h/d?Product=V4TAC\"]}".to_owned(),
        "{\"AvailableUris\":[]}".to_owned(),
        "{\"AvailableUris\":[1,\"http://h/d?Type=HashV41\",\"http://h/d?Product=\"]}".to_owned(),
    ];
    for (index, body) in bodies.into_iter().enumerate() {
        let stand_in = serving(&PRODUCTS);
        stand_in.then(Answer::Status(300, Vec::new(), body.clone()));
        let fixture = Fixture::new(
            &format!("no-list-{index}"),
            update_url(&stand_in),
            Some(TAC),
            |b| b,
        );

        let (status, message) = failure(fixture.check());

        assert_eq!(status, AutoUpdateStatus::HttpsError, "{body}");
        assert!(
            message.starts_with("HTTP status code '300'"),
            "{body}: {message}"
        );
        assert_eq!(stand_in.requests().len(), 1, "{body}");
    }
}

/// The `Location` of a 300 is not followed, and the second request goes to
/// the server that answered the first, not to a URI from the list.
#[test]
fn the_location_of_a_300_is_not_followed() {
    let other = serving(&PRODUCTS);
    let stand_in = serving(&PRODUCTS);
    let listed = uri(&other, "Product=V4TAC");
    stand_in.then(Answer::Status(
        300,
        vec![("Location".to_owned(), listed.clone())],
        format!("{{\"AvailableUris\":[\"{listed}\"]}}"),
    ));
    let fixture = Fixture::new("location", update_url(&stand_in), Some(TAC), |b| b);

    assert_eq!(fixture.check().unwrap(), AutoUpdateStatus::Success);

    assert_eq!(stand_in.requests().len(), 2);
    assert!(other.requests().is_empty());
}

/// A redirect from the first server to a second, which answers 300, is
/// followed, and the second request goes to the server that answered 300.
#[test]
fn a_redirect_before_the_300_is_followed() {
    let second = serving(&PRODUCTS);
    let first = serving(&PRODUCTS);
    let moved = update_url(&second);
    first.then(Answer::Status(
        307,
        vec![("Location".to_owned(), moved.clone())],
        String::new(),
    ));
    let fixture = Fixture::new("redirect", update_url(&first), Some(TAC), |b| b);

    assert_eq!(fixture.check().unwrap(), AutoUpdateStatus::Success);

    assert_eq!(first.requests().len(), 1);
    let asked: Vec<_> = second
        .requests()
        .into_iter()
        .map(|r| (r.product, r.status))
        .collect();
    assert_eq!(asked, [(None, 300), (Some(TAC.to_owned()), 200)]);
    let requesting = format!(
        "INFO Requesting product '{TAC}' from '{}' for Engine 'LoadedEngine'",
        moved.replace(KEY, "************MNOP")
    );
    assert!(
        logged_for(&second.url()).contains(&requesting),
        "{requesting}"
    );
}

/// A 429 is the too many requests status, and the data file is unchanged.
#[test]
fn a_429_is_too_many_requests() {
    let stand_in = serving(&PRODUCTS);
    stand_in.then(Answer::Status(
        429,
        vec![("Retry-After".to_owned(), "60".to_owned())],
        String::new(),
    ));
    let fixture = Fixture::new("429", update_url(&stand_in), None, |b| {
        b.data_update_product(TAC)
    });

    let (status, message) = failure(fixture.check());

    assert_eq!(status, AutoUpdateStatus::TooManyRequests);
    assert!(message.starts_with("Too many requests to"), "{message}");
    assert_eq!(fixture.data(), OLD_DATA);
}

/// A download whose MD5 is not the one the server sent fails its integrity
/// check, after the product is chosen, and the data file is unchanged.
#[test]
fn a_download_with_the_wrong_md5_fails_its_integrity_check() {
    let stand_in = serving(&PRODUCTS);
    stand_in
        .then(choices(&[&uri(&stand_in, "Product=V4TAC")]))
        .then(Answer::FileWithMd5(
            "0123456789abcdef0123456789abcdef".to_owned(),
        ));
    let fixture = Fixture::new("md5", update_url(&stand_in), Some(TAC), |b| b);

    let (status, message) = failure(fixture.check());

    assert_eq!(status, AutoUpdateStatus::Md5ValidationFailed);
    assert!(message.starts_with("Integrity check failed."), "{message}");
    assert_eq!(fixture.data(), OLD_DATA);
}

/// A download cut short is the access error, and the data file is
/// unchanged.
#[test]
fn a_download_cut_short_is_an_access_error() {
    let stand_in = serving(&PRODUCTS);
    stand_in.then(Answer::TruncatedFile);
    let fixture = Fixture::new("truncated", update_url(&stand_in), None, |b| {
        b.data_update_product(TAC)
    });

    let (status, message) = failure(fixture.check());

    assert_eq!(status, AutoUpdateStatus::HttpsError);
    assert!(
        message.starts_with("Error accessing data update service at"),
        "{message}"
    );
    assert_eq!(fixture.data(), OLD_DATA);
    let left: Vec<_> = std::fs::read_dir(&fixture.folder).unwrap().collect();
    assert_eq!(left.len(), 1, "only the data file is left");
}

/// The product name of a data file is its kind's prefix and its tier, or
/// the tier alone when it starts with the prefix in any case, and there is
/// none for an empty tier or prefix.
#[test]
fn product_names_are_made_from_the_prefix_and_the_tier() {
    assert_eq!(product_name("V4", "TAC").as_deref(), Some("V4TAC"));
    assert_eq!(
        product_name("V4", "Enterprise").as_deref(),
        Some("V4Enterprise")
    );
    assert_eq!(
        product_name("IPIV4", "IPIV4Enterprise").as_deref(),
        Some("IPIV4Enterprise")
    );
    assert_eq!(
        product_name("IPIV4", "ipiv4Lite").as_deref(),
        Some("ipiv4Lite")
    );
    assert_eq!(product_name("V4", ""), None);
    assert_eq!(product_name("", "TAC"), None);
}
