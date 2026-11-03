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

//! What the Device Detection and hardware profile cloud engines log at
//! warning level or above, for the cases where .NET's engines log. Kept in
//! its own test binary because a logger is installed once for the whole
//! process, and each test holds [`SERIAL`] so the records it reads are its
//! own.

use std::sync::{Arc, Mutex, MutexGuard};

use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine,
};
use fiftyone_device_detection_cloud::{DeviceDetectionCloudEngine, HardwareProfileCloudEngine};
use fiftyone_pipeline_core::{Evidence, FlowElement, Pipeline};

const DEVICE_PRODUCT: &str = r#""device":{"DataTier":"CloudV4TAC","Properties":[
    {"Name":"IsMobile","Type":"Boolean","Category":"Device"}]}"#;
const HARDWARE_PRODUCT: &str = r#""hardware":{"DataTier":"CloudV4TAC","Properties":[
    {"Name":"Profiles","Type":"Array","Category":"Device","ItemProperties":[
        {"Name":"HardwareVendor","Type":"String","Category":"Hardware"}]}]}"#;
const IP_PRODUCT: &str = r#""ip":{"DataTier":"CloudV4","Properties":[
    {"Name":"RegisteredName","Type":"String","Category":"Network"}]}"#;

const NOT_GRANTED_DEVICE: &str =
    "ERROR Aspect properties could not be loaded for DeviceDetectionCloudEngine - 'device'";
const NOT_GRANTED_HARDWARE: &str =
    "ERROR Aspect properties could not be loaded for HardwareProfileCloudEngine - 'hardware'";

/// Records at warning level or above, as "LEVEL message".
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Held by each test for its whole run.
static SERIAL: Mutex<()> = Mutex::new(());

/// Keeps the cloud engines' records at warning level or above in [`LOGGED`].
/// The checks are made in the cloud request engine crate, so its records are
/// the ones kept. The pipeline's own record of an element's error is left
/// out.
struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
            && metadata
                .target()
                .starts_with("fiftyone_cloud_request_engine")
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

/// Install the logger, wait for any other test to finish and clear the
/// records it left.
fn start() -> MutexGuard<'static, ()> {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Warn);
    let guard = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    LOGGED.lock().unwrap().clear();
    guard
}

/// Take the records logged since [`start`] or the last call.
fn take_logged() -> Vec<String> {
    std::mem::take(&mut *LOGGED.lock().unwrap())
}

/// A fake cloud granting `products` and answering every data request with
/// `data`.
struct FakeCloud {
    products: String,
    data: &'static str,
}

impl CloudHttpClient for FakeCloud {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        let body = if request.url.contains("accessibleproperties") {
            format!(r#"{{"Products":{{{}}}}}"#, self.products)
        } else if request.url.contains("evidencekeys") {
            r#"["header.user-agent","query.tac"]"#.to_owned()
        } else {
            self.data.to_owned()
        };
        Ok(CloudHttpResponse {
            status: 200,
            body,
            retry_after: None,
        })
    }
}

/// A request engine with a resource key, on a cloud granting `products`.
fn request_engine(products: &[&str], data: &'static str) -> Arc<CloudRequestEngine> {
    Arc::new(
        CloudRequestEngine::builder()
            .resource_key("test-resource-key")
            .endpoint("https://cloud.example.test/api/v4/")
            .http_client(Arc::new(FakeCloud {
                products: products.join(","),
                data,
            }))
            .build()
            .unwrap(),
    )
}

fn pipeline(
    request_engine: Arc<CloudRequestEngine>,
    engine: Arc<dyn FlowElement>,
) -> Arc<Pipeline> {
    Pipeline::builder()
        .add_element(request_engine)
        .add_element(engine)
        .build()
        .unwrap()
}

fn process(pipeline: &Arc<Pipeline>, key: &str, value: &str) {
    let evidence = Evidence::builder().add(key, value).build();
    pipeline.create_flow_data_with(evidence).process().unwrap();
}

#[test]
fn a_device_product_that_is_not_granted_logs_one_error_when_built() {
    let _serial = start();
    let request = request_engine(&[IP_PRODUCT], r#"{"device":{"ismobile":true}}"#);
    let engine = DeviceDetectionCloudEngine::builder()
        .cloud_request_engine(Arc::clone(&request))
        .build();
    assert_eq!(take_logged(), vec![NOT_GRANTED_DEVICE]);

    let pipeline = pipeline(request, Arc::new(engine));
    for request in 1..=2 {
        process(&pipeline, "header.user-agent", "A");
        assert_eq!(take_logged(), Vec::<String>::new(), "request {request}");
    }
}

#[test]
fn device_properties_with_no_metadata_log_a_warning_on_every_request() {
    let _serial = start();
    // IsMobile is listed, and matches whatever its case.
    let request = request_engine(
        &[DEVICE_PRODUCT],
        r#"{"device":{"IsMobile":true,"unlisted":"x","unlistedtoo":null,
            "unlistedtoonullreason":"why"}}"#,
    );
    let engine = DeviceDetectionCloudEngine::builder()
        .cloud_request_engine(Arc::clone(&request))
        .build();
    let pipeline = pipeline(request, Arc::new(engine));
    assert_eq!(take_logged(), Vec::<String>::new());
    for request in 1..=2 {
        process(&pipeline, "header.user-agent", "A");
        assert_eq!(
            take_logged(),
            vec![
                "WARN No meta-data entry for property 'unlisted' in \
                 'DeviceDetectionCloudEngine'",
                "WARN No meta-data entry for property 'unlistedtoo' in \
                 'DeviceDetectionCloudEngine'",
            ],
            "request {request}"
        );
    }
}

#[test]
fn a_license_key_alone_has_nothing_to_check_and_logs_nothing() {
    let _serial = start();
    let request = Arc::new(
        CloudRequestEngine::builder()
            .license_key("test-license-key")
            .values(["IsMobile"])
            .endpoint("https://cloud.example.test/api/v4/")
            .http_client(Arc::new(FakeCloud {
                products: String::new(),
                data: r#"{"device":{"ismobile":true,"unlisted":"x"}}"#,
            }))
            .build()
            .unwrap(),
    );
    let engine = DeviceDetectionCloudEngine::builder()
        .cloud_request_engine(Arc::clone(&request))
        .build();
    let pipeline = pipeline(request, Arc::new(engine));
    process(&pipeline, "header.user-agent", "A");
    assert_eq!(take_logged(), Vec::<String>::new());
}

#[test]
fn a_hardware_product_that_is_not_granted_logs_one_error_when_built() {
    let _serial = start();
    let request = request_engine(&[DEVICE_PRODUCT], r#"{"hardware":{"profiles":[]}}"#);
    let engine = HardwareProfileCloudEngine::builder()
        .cloud_request_engine(Arc::clone(&request))
        .build();
    assert_eq!(take_logged(), vec![NOT_GRANTED_HARDWARE]);

    let pipeline = pipeline(request, Arc::new(engine));
    process(&pipeline, "query.tac", "35925406");
    assert_eq!(take_logged(), Vec::<String>::new());
}

#[test]
fn profile_properties_with_no_metadata_log_a_warning_for_each_profile() {
    let _serial = start();
    let request = request_engine(
        &[HARDWARE_PRODUCT],
        r#"{"hardware":{"profiles":[
            {"hardwarevendor":"A","unlisted":"x"},
            {"hardwarevendor":"B","unlisted":"y"}]}}"#,
    );
    let engine = HardwareProfileCloudEngine::builder()
        .cloud_request_engine(Arc::clone(&request))
        .build();
    let pipeline = pipeline(request, Arc::new(engine));
    assert_eq!(take_logged(), Vec::<String>::new());
    let unlisted = "WARN No meta-data entry for property 'unlisted' in \
         'HardwareProfileCloudEngine'";
    for request in 1..=2 {
        process(&pipeline, "query.tac", "35925406");
        assert_eq!(take_logged(), vec![unlisted, unlisted], "request {request}");
    }
}
