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

//! What the Device Detection and hardware profile cloud engines do when the
//! cloud request fails, as .NET's cloud aspect engines do. Kept in its own
//! test binary because a logger is installed once for the whole process, and
//! each test holds [`SERIAL`] so the records it reads are its own.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine,
};
use fiftyone_device_detection_cloud::{
    DeviceData, DeviceDetectionCloudEngine, HardwareProfileCloudEngine, DEVICE_DATA_KEY,
    HARDWARE_DATA_KEY,
};
use fiftyone_pipeline_core::{
    ElementData, Error, Evidence, FlowData, FlowElement, MissingPropertyReason, Pipeline,
};
use fiftyone_pipeline_engines::AspectEngine;

const DEVICE_PRODUCT: &str = r#""device":{"DataTier":"CloudV4TAC","Properties":[
    {"Name":"IsMobile","Type":"Boolean","Category":"Device"}]}"#;
const HARDWARE_PRODUCT: &str = r#""hardware":{"DataTier":"CloudV4TAC","Properties":[
    {"Name":"Profiles","Type":"Array","Category":"Device","ItemProperties":[
        {"Name":"HardwareVendor","Type":"String","Category":"Hardware"}]}]}"#;

/// The data key of the cloud request engine's own data.
const CLOUD_KEY: &str = "cloud-response";

/// .NET's description for a property of data from a failed cloud request.
const IS_MOBILE_FAILED: &str = "Property 'IsMobile' not found in data for element 'device'. \
     This is because an upstream cloud request failed, so no data was available to populate \
     the property.";

/// The record of each read of a property of data from a failed cloud request.
const IS_MOBILE_READ: &str = "INFO Property 'IsMobile' missing from aspect data \
     'DeviceDataBase' because the upstream cloud request failed for this request \
     (CloudRequestFailed). See FlowData::errors for details.";

/// Records at info level or above from the cloud crates and the aspect data,
/// as "LEVEL message".
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Held by each test for its whole run.
static SERIAL: Mutex<()> = Mutex::new(());

struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        let target = metadata.target();
        metadata.level() <= log::Level::Info
            && (target.starts_with("fiftyone_cloud_request_engine")
                || target.starts_with("fiftyone_pipeline_core")
                || target.starts_with("fiftyone_device_detection")
                || target.starts_with("fiftyone_pipeline_engines"))
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
    log::set_max_level(log::LevelFilter::Info);
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
/// `data`, or with a server error when `data_down` is set. While
/// `properties_down` is set the accessible properties request cannot reach
/// it.
struct FakeCloud {
    products: String,
    data: &'static str,
    data_down: bool,
    properties_down: Arc<AtomicBool>,
}

impl CloudHttpClient for FakeCloud {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        let (status, body) = if request.url.contains("accessibleproperties") {
            if self.properties_down.load(Ordering::Relaxed) {
                return Err("connection refused".to_owned());
            }
            (200, format!(r#"{{"Products":{{{}}}}}"#, self.products))
        } else if request.url.contains("evidencekeys") {
            (200, r#"["header.user-agent","query.tac"]"#.to_owned())
        } else if self.data_down {
            (500, r#"{"errors":["the service failed"]}"#.to_owned())
        } else {
            (200, self.data.to_owned())
        };
        Ok(CloudHttpResponse {
            status,
            body,
            retry_after: None,
        })
    }
}

fn request_engine(
    products: &[&str],
    data: &'static str,
    data_down: bool,
    properties_down: Arc<AtomicBool>,
) -> Arc<CloudRequestEngine> {
    Arc::new(
        CloudRequestEngine::builder()
            .resource_key("test-resource-key")
            .endpoint("https://cloud.example.test/api/v4/")
            .http_client(Arc::new(FakeCloud {
                products: products.join(","),
                data,
                data_down,
                properties_down,
            }))
            .build()
            .unwrap(),
    )
}

/// A pipeline of `request`, then `engine`, suppressing process errors or not.
fn pipeline(
    request: Arc<CloudRequestEngine>,
    engine: Arc<dyn FlowElement>,
    suppress: bool,
) -> Arc<Pipeline> {
    Pipeline::builder()
        .add_element(request)
        .add_element(engine)
        .suppress_process_exceptions(suppress)
        .build()
        .unwrap()
}

/// Process `key` and `value` through `pipeline`, returning the flow data and
/// the error processing returned.
fn process(pipeline: &Arc<Pipeline>, key: &str, value: &str) -> (FlowData, Option<Error>) {
    let evidence = Evidence::builder().add(key, value).build();
    let mut data = pipeline.create_flow_data_with(evidence);
    let error = data.process().err();
    (data, error)
}

/// The error .NET's cloud aspect engines record when they did not process.
fn not_processed(engine_name: &str) -> String {
    format!(
        "pipeline error: The '{engine_name}' did not process because the JSON response from \
         the CloudRequestEngine was null or empty. This indicates that the cloud request \
         failed."
    )
}

/// The info record .NET's cloud aspect engines log when they did not process.
fn not_processed_log(engine_name: &str) -> String {
    format!(
        "INFO The '{engine_name}' did not process as the JSON response from the \
         CloudRequestEngine was null or empty. Please refer to errors generated by the \
         CloudRequestEngine in the logs as this indicates an error occurred there."
    )
}

/// Check the errors of a flow data whose cloud request failed. Processing
/// returns only the request engine's error, and the flow data also holds the
/// engine's record, which does not fail processing.
fn assert_failed_errors(data: &FlowData, error: Option<Error>, key: &str, engine_name: &str) {
    match error {
        Some(Error::Aggregate(errors)) => {
            let keys: Vec<&str> = errors
                .iter()
                .map(|error| error.element_data_key.as_str())
                .collect();
            assert_eq!(keys, vec![CLOUD_KEY], "only the request engine's error");
        }
        other => panic!("expected the request engine's error, got {other:?}"),
    }
    let recorded: Vec<(&str, bool)> = data
        .errors()
        .iter()
        .map(|error| (error.element_data_key.as_str(), error.should_throw))
        .collect();
    assert_eq!(recorded, vec![(CLOUD_KEY, true), (key, false)]);
    assert!(matches!(data.errors()[1].source, Error::Pipeline { .. }));
    assert_eq!(
        data.errors()[1].source.to_string(),
        not_processed(engine_name)
    );
}

#[test]
fn a_failed_request_gives_device_data_that_says_why() {
    let _serial = start();
    let request = request_engine(&[DEVICE_PRODUCT], "{}", true, Arc::default());
    let engine = Arc::new(DeviceDetectionCloudEngine::new(Arc::clone(&request)));
    let pipeline = pipeline(request, engine, false);
    take_logged();

    let (data, error) = process(&pipeline, "header.user-agent", "A");
    assert_failed_errors(&data, error, "device", "DeviceDetectionCloudEngine");
    let logged = take_logged();
    assert!(
        logged.contains(&not_processed_log("DeviceDetectionCloudEngine")),
        "{logged:?}"
    );
    let errors: Vec<&String> = logged
        .iter()
        .filter(|record| record.starts_with("ERROR "))
        .collect();
    assert_eq!(
        errors.len(),
        1,
        "only the request engine's error: {logged:?}"
    );
    assert!(
        errors[0].starts_with(&format!(
            "ERROR An error occurred in element '{CLOUD_KEY}': "
        )),
        "{logged:?}"
    );

    let device = data.get(DEVICE_DATA_KEY).expect("the device data is there");
    assert!(device.base().cloud_request_failed());
    assert!(!device.has_property("IsMobile"));
    assert!(device.keys().is_empty());
    assert_eq!(
        device
            .base()
            .missing_property_reason("IsMobile")
            .unwrap()
            .reason,
        MissingPropertyReason::CloudRequestFailed
    );
    assert_eq!(take_logged(), Vec::<String>::new(), "asking logs nothing");

    // Each read logs at info level, not warning, as .NET does.
    let is_mobile = device.is_mobile();
    assert!(!is_mobile.has_value());
    assert_eq!(is_mobile.no_value_message(), Some(IS_MOBILE_FAILED));
    assert_eq!(
        device.get("IsMobile").unwrap_err().message,
        IS_MOBILE_FAILED
    );
    assert_eq!(take_logged(), vec![IS_MOBILE_READ, IS_MOBILE_READ]);
}

#[test]
fn a_failed_request_gives_no_hardware_profiles() {
    let _serial = start();
    let request = request_engine(&[HARDWARE_PRODUCT], "{}", true, Arc::default());
    let engine = Arc::new(
        HardwareProfileCloudEngine::builder()
            .cloud_request_engine(Arc::clone(&request))
            .build(),
    );
    let pipeline = pipeline(request, engine, false);
    take_logged();

    let (data, error) = process(&pipeline, "query.tac", "35925406");
    assert_failed_errors(&data, error, "hardware", "HardwareProfileCloudEngine");
    let logged = take_logged();
    assert!(
        logged.contains(&not_processed_log("HardwareProfileCloudEngine")),
        "{logged:?}"
    );
    let hardware = data
        .get(HARDWARE_DATA_KEY)
        .expect("the hardware data is there");
    assert!(hardware.is_empty());
}

#[test]
fn a_failed_request_fails_nothing_more_when_errors_are_suppressed() {
    let _serial = start();
    let request = request_engine(&[DEVICE_PRODUCT], "{}", true, Arc::default());
    let engine = Arc::new(DeviceDetectionCloudEngine::new(Arc::clone(&request)));
    let pipeline = pipeline(request, engine, true);

    let (data, error) = process(&pipeline, "header.user-agent", "A");
    assert!(error.is_none(), "{error:?}");
    assert_eq!(data.errors().len(), 2);
    let logged = take_logged();
    assert!(
        !logged.iter().any(|record| record.starts_with("ERROR ")),
        "{logged:?}"
    );
    assert!(data
        .get(DEVICE_DATA_KEY)
        .unwrap()
        .base()
        .cloud_request_failed());
}

#[test]
fn a_successful_request_is_unchanged() {
    let _serial = start();
    let request = request_engine(
        &[DEVICE_PRODUCT],
        r#"{"device":{"ismobile":true,"browsername":null,
            "browsernamenullreason":"No matching profiles"}}"#,
        false,
        Arc::default(),
    );
    let engine = Arc::new(DeviceDetectionCloudEngine::new(Arc::clone(&request)));
    let pipeline = pipeline(request, engine, false);
    take_logged();

    let (data, error) = process(&pipeline, "header.user-agent", "A");
    assert!(error.is_none(), "{error:?}");
    assert!(data.errors().is_empty());
    let device = data.get(DEVICE_DATA_KEY).unwrap();
    assert!(!device.base().cloud_request_failed());
    assert_eq!(device.is_mobile().value().ok(), Some(&true));
    // A property the cloud returned as null, and one it did not return, are
    // both still held, because cloud data cannot yet tell them apart.
    assert!(device.has_property("BrowserName"));
    assert!(device.has_property("PlatformName"));
    assert!(!device.browser_name().has_value());
    assert_eq!(take_logged(), Vec::<String>::new());
}

/// A cloud engine with no metadata because its property list request failed
/// gives `CloudRequestFailed`, typed accessor or not, as step 3.2 of "Deciding
/// the reason" in the properties specification says.
#[test]
fn while_the_property_list_request_has_failed_a_missing_property_says_so() {
    let _serial = start();
    let down = Arc::new(AtomicBool::new(true));
    let request = request_engine(
        &[DEVICE_PRODUCT],
        r#"{"device":{"ismobile":true}}"#,
        false,
        Arc::clone(&down),
    );
    let engine = Arc::new(DeviceDetectionCloudEngine::new(Arc::clone(&request)));
    let unprocessed = DeviceDetectionCloudEngine::new(Arc::clone(&request));
    let hardware = HardwareProfileCloudEngine::new(Arc::clone(&request));
    assert!(!engine.has_loaded_properties());
    assert!(engine.property_list_request_failed());
    assert!(hardware.property_list_request_failed());

    let is_mobile = engine.missing_property_reason("IsMobile");
    assert_eq!(is_mobile.reason, MissingPropertyReason::CloudRequestFailed);
    assert_eq!(is_mobile.description, IS_MOBILE_FAILED);
    assert_eq!(
        engine.missing_property_reason("NotAProperty").reason,
        MissingPropertyReason::CloudRequestFailed
    );
    assert_eq!(
        hardware.missing_property_reason("Profiles").reason,
        MissingPropertyReason::CloudRequestFailed
    );

    // Once discovery completes, the key's properties decide.
    down.store(false, Ordering::Relaxed);
    let pipeline = pipeline(request, engine.clone(), false);
    let (_data, error) = process(&pipeline, "header.user-agent", "A");
    assert!(error.is_none(), "{error:?}");
    assert!(engine.has_loaded_properties());
    assert!(!engine.property_list_request_failed());
    assert_eq!(
        engine.missing_property_reason("BrowserName").reason,
        MissingPropertyReason::PropertyNotAccessibleWithResourceKey
    );
    assert_eq!(
        engine.missing_property_reason("IsMobile").reason,
        MissingPropertyReason::PropertyNotAccessibleWithResourceKey,
        "a listed property the data lacks"
    );

    // Engines that have not processed since read the metadata when asked.
    assert_eq!(
        unprocessed.missing_property_reason("BrowserName").reason,
        MissingPropertyReason::PropertyNotAccessibleWithResourceKey
    );
    assert_eq!(
        hardware.missing_property_reason("Profiles").reason,
        MissingPropertyReason::ProductNotAccessibleWithResourceKey,
        "the key has no hardware product"
    );
}
