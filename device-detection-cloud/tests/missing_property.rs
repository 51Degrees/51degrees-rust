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

//! What Device Detection data and hardware profiles from a successful cloud
//! request give and log for a property the response did not return, as
//! .NET's cloud data does, and what Set Headers logs for one. A read of the
//! device data logs a warning. A read of a profile logs nothing, because
//! .NET builds each profile with no logger. Kept in its own test binary
//! because a logger is installed once for the whole process, and each test
//! holds [`SERIAL`] so the records it reads are its own.

use std::sync::{Arc, Mutex, MutexGuard};

use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine,
};
use fiftyone_device_detection_cloud::{
    DeviceData, DeviceDetectionCloudEngine, HardwareProfileCloudEngine, DEVICE_DATA_KEY,
    HARDWARE_DATA_KEY,
};
use fiftyone_pipeline_core::{
    ElementData, Evidence, FlowData, FlowElement, MissingPropertyReason, Pipeline,
};
use fiftyone_pipeline_engines::AspectEngine;
use fiftyone_pipeline_engines_fiftyone::SetHeadersElement;

const DEVICE_PRODUCT: &str = r#""device":{"DataTier":"CloudV4","Properties":[
    {"Name":"IsMobile","Type":"Boolean","Category":"Device"},
    {"Name":"BrowserName","Type":"String","Category":"Browser"}]}"#;
const HARDWARE_PRODUCT: &str = r#""hardware":{"DataTier":"CloudV4TAC","Properties":[
    {"Name":"Profiles","Type":"Array","Category":"Device","ItemProperties":[
        {"Name":"HardwareVendor","Type":"String","Category":"Hardware"}]}]}"#;
const IP_PRODUCT: &str = r#""ip":{"DataTier":"CloudV4","Properties":[
    {"Name":"RegisteredName","Type":"String","Category":"Network"}]}"#;

/// Gives IsMobile, gives BrowserName as null with the cloud's reason, and
/// does not return HardwareVendor.
const DEVICE_RESPONSE: &str = r#"{"device":{"ismobile":true,"browsername":null,
    "browsernamenullreason":"No matching profiles"}}"#;

/// Two profiles, neither giving HardwareModel, the second giving
/// HardwareVendor as null with the cloud's reason.
const HARDWARE_RESPONSE: &str = r#"{"hardware":{"profiles":[
    {"hardwarevendor":"Apple"},
    {"hardwarevendor":null,
     "hardwarevendornullreason":"The value cannot be determined from a TAC alone."}]}}"#;

/// Grants IsMobile and three properties Set Headers reads, all for the
/// Accept-CH header.
const SET_HEADER_PRODUCT: &str = r#""device":{"DataTier":"CloudV4","Properties":[
    {"Name":"IsMobile","Type":"Boolean","Category":"Device"},
    {"Name":"SetHeaderBrowserAccept-CH","Type":"String","Category":"Browser"},
    {"Name":"SetHeaderHardwareAccept-CH","Type":"String","Category":"Hardware"},
    {"Name":"SetHeaderPlatformAccept-CH","Type":"String","Category":"Platform"}]}"#;

/// Gives SetHeaderBrowserAccept-CH, gives SetHeaderHardwareAccept-CH as null
/// with the cloud's reason, and does not return SetHeaderPlatformAccept-CH.
const SET_HEADER_RESPONSE: &str = r#"{"device":{"ismobile":true,
    "setheaderbrowseraccept-ch":"Sec-CH-UA,Sec-CH-UA-Full-Version-List",
    "setheaderhardwareaccept-ch":null,
    "setheaderhardwareaccept-chnullreason":"No matching profiles"}}"#;

const NOT_GRANTED_DEVICE: &str =
    "ERROR Aspect properties could not be loaded for DeviceDetectionCloudEngine - 'device'";

/// The record of each read of a property the data does not hold.
fn missing_read(property: &str, reason: &str) -> String {
    format!("WARN Property '{property}' missing from aspect data 'DeviceDataBase'. {reason}")
}

/// Records at warning level or above from the cloud crates, the aspect data
/// and Set Headers, as "LEVEL message".
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Held by each test for its whole run.
static SERIAL: Mutex<()> = Mutex::new(());

struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        let target = metadata.target();
        metadata.level() <= log::Level::Warn
            && (target.starts_with("fiftyone_cloud_request_engine")
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

/// A request engine with a resource key granting `products`.
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

fn pipeline(request: Arc<CloudRequestEngine>, engine: Arc<dyn FlowElement>) -> Arc<Pipeline> {
    Pipeline::builder()
        .add_element(request)
        .add_element(engine)
        .build()
        .unwrap()
}

/// Process `key` and `value` through `pipeline`, which must succeed.
fn process(pipeline: &Arc<Pipeline>, key: &str, value: &str) -> FlowData {
    let evidence = Evidence::builder().add(key, value).build();
    let mut data = pipeline.create_flow_data_with(evidence);
    data.process().expect("processing succeeds");
    assert!(data.errors().is_empty(), "{:?}", data.errors());
    data
}

#[test]
fn a_property_the_response_did_not_return_gives_the_resource_key_reason() {
    let _serial = start();
    let request = request_engine(&[DEVICE_PRODUCT], DEVICE_RESPONSE);
    let engine = Arc::new(DeviceDetectionCloudEngine::new(Arc::clone(&request)));
    let pipeline = pipeline(request, engine.clone());
    let data = process(&pipeline, "header.user-agent", "A");
    let device = data.get(DEVICE_DATA_KEY).expect("the device data is there");
    assert!(!device.base().cloud_request_failed());
    assert_eq!(
        take_logged(),
        Vec::<String>::new(),
        "processing logs nothing"
    );

    // The data knows which properties it holds, and asking logs nothing.
    assert!(device.has_property("IsMobile"));
    assert!(device.has_property("BrowserName"));
    assert!(!device.has_property("HardwareVendor"));
    assert_eq!(take_logged(), Vec::<String>::new());

    // The reason is the engine's own, naming the properties the key grants.
    let expected = engine.missing_property_reason("HardwareVendor");
    assert_eq!(
        expected.reason,
        MissingPropertyReason::PropertyNotAccessibleWithResourceKey
    );
    assert!(
        expected.description.starts_with(
            "Property 'HardwareVendor' not found in data for element 'device'. This is \
             because your resource key does not include access to this property. The \
             properties included for your key under 'device' are IsMobile,BrowserName. "
        ),
        "{}",
        expected.description
    );
    assert_eq!(
        device.base().missing_property_reason("HardwareVendor"),
        Some(expected.clone())
    );
    assert_eq!(take_logged(), Vec::<String>::new(), "asking logs nothing");

    // Each read gives the reason and logs a warning.
    let vendor = device.hardware_vendor();
    assert!(!vendor.has_value());
    assert_eq!(
        vendor.no_value_message(),
        Some(expected.description.as_str())
    );
    assert_eq!(
        device.get("HardwareVendor").unwrap_err().message,
        expected.description
    );
    let read = missing_read("HardwareVendor", "PropertyNotAccessibleWithResourceKey");
    assert_eq!(take_logged(), vec![read.clone(), read]);

    // A property the response gave as null has the cloud's reason, and a
    // property it gave has its value. Reading either logs nothing.
    let browser = device.browser_name();
    assert!(!browser.has_value());
    assert_eq!(browser.no_value_message(), Some("No matching profiles"));
    assert_eq!(device.is_mobile().value().ok(), Some(&true));
    assert_eq!(take_logged(), Vec::<String>::new());
}

#[test]
fn a_key_without_the_device_product_gives_the_product_reason() {
    let _serial = start();
    let request = request_engine(&[IP_PRODUCT], r#"{"ip":{"registeredname":"A"}}"#);
    let engine = Arc::new(DeviceDetectionCloudEngine::new(Arc::clone(&request)));
    assert_eq!(take_logged(), vec![NOT_GRANTED_DEVICE]);
    let pipeline = pipeline(request, engine.clone());
    let data = process(&pipeline, "header.user-agent", "A");
    let device = data.get(DEVICE_DATA_KEY).expect("the device data is there");
    assert!(device.keys().is_empty());
    assert!(!device.has_property("IsMobile"));
    assert_eq!(take_logged(), Vec::<String>::new());

    let expected = engine.missing_property_reason("IsMobile");
    assert_eq!(
        expected.reason,
        MissingPropertyReason::ProductNotAccessibleWithResourceKey
    );
    assert!(
        expected.description.starts_with(
            "Property 'IsMobile' not found in data for element 'device'. This is because \
             your resource key does not include access to any properties under 'device'. "
        ),
        "{}",
        expected.description
    );
    assert_eq!(
        device.is_mobile().no_value_message(),
        Some(expected.description.as_str())
    );
    assert_eq!(
        take_logged(),
        vec![missing_read(
            "IsMobile",
            "ProductNotAccessibleWithResourceKey"
        )]
    );
}

#[test]
fn each_hardware_profile_gives_the_reason_for_a_property_it_does_not_hold() {
    let _serial = start();
    let request = request_engine(&[HARDWARE_PRODUCT], HARDWARE_RESPONSE);
    let engine = Arc::new(
        HardwareProfileCloudEngine::builder()
            .cloud_request_engine(Arc::clone(&request))
            .build(),
    );
    let pipeline = pipeline(request, engine.clone());
    let data = process(&pipeline, "query.tac", "35925406");
    let profiles = data
        .get(HARDWARE_DATA_KEY)
        .expect("the hardware data is there")
        .profiles();
    assert_eq!(profiles.len(), 2);
    assert_eq!(
        take_logged(),
        Vec::<String>::new(),
        "processing logs nothing"
    );

    // As in .NET, a profile's reason is the hardware engine's, which lists
    // the hardware product's own properties.
    let expected = engine.missing_property_reason("HardwareModel");
    assert_eq!(
        expected.reason,
        MissingPropertyReason::PropertyNotAccessibleWithResourceKey
    );
    assert!(
        expected.description.starts_with(
            "Property 'HardwareModel' not found in data for element 'hardware'. This is \
             because your resource key does not include access to this property. The \
             properties included for your key under 'hardware' are Profiles. "
        ),
        "{}",
        expected.description
    );
    for profile in profiles {
        assert!(!profile.has_property("HardwareModel"));
        assert_eq!(
            profile.hardware_model().no_value_message(),
            Some(expected.description.as_str())
        );
        assert_eq!(
            profile.get("HardwareModel").unwrap_err().message,
            expected.description
        );
    }
    // .NET builds each profile with no logger, so reading a property a
    // profile does not hold logs nothing.
    assert_eq!(
        take_logged(),
        Vec::<String>::new(),
        "reading a profile logs nothing"
    );

    // A vendor the response gave, and one it gave as null with a reason.
    assert_eq!(profiles[0].hardware_vendor().value().unwrap(), "Apple");
    assert!(profiles[1].has_property("HardwareVendor"));
    assert_eq!(
        profiles[1].hardware_vendor().no_value_message(),
        Some("The value cannot be determined from a TAC alone.")
    );
    assert_eq!(take_logged(), Vec::<String>::new());
}

#[test]
fn with_a_license_key_alone_every_property_counts_as_held() {
    let _serial = start();
    let request = Arc::new(
        CloudRequestEngine::builder()
            .license_key("test-license-key")
            .values(["IsMobile"])
            .endpoint("https://cloud.example.test/api/v4/")
            .http_client(Arc::new(FakeCloud {
                products: String::new(),
                data: r#"{"device":{"ismobile":true}}"#,
            }))
            .build()
            .unwrap(),
    );
    let engine = Arc::new(DeviceDetectionCloudEngine::new(Arc::clone(&request)));
    let pipeline = pipeline(request, engine);
    let data = process(&pipeline, "header.user-agent", "A");
    let device = data.get(DEVICE_DATA_KEY).expect("the device data is there");

    // The properties a license key grants are not listed, so the data
    // cannot say why one is missing.
    assert!(device.has_property("HardwareVendor"));
    assert_eq!(
        device.base().missing_property_reason("HardwareVendor"),
        None
    );
    assert!(!device.hardware_vendor().has_value());
    assert_eq!(device.is_mobile().value().ok(), Some(&true));
    assert_eq!(take_logged(), Vec::<String>::new());
}

#[test]
fn set_headers_warns_only_for_a_property_the_response_did_not_return() {
    let _serial = start();
    let request = request_engine(&[SET_HEADER_PRODUCT], SET_HEADER_RESPONSE);
    let engine = Arc::new(DeviceDetectionCloudEngine::new(Arc::clone(&request)));
    let pipeline = Pipeline::builder()
        .add_element(request)
        .add_element(engine)
        .add_element(Arc::new(SetHeadersElement::new()))
        .build()
        .unwrap();
    let data = process(&pipeline, "header.user-agent", "A");

    // The header carries the one value the response gave.
    let headers = data
        .get(SetHeadersElement::KEY)
        .expect("the Set Headers data is there")
        .response_headers();
    assert_eq!(headers.len(), 1, "{headers:?}");
    assert_eq!(
        headers.get("Accept-CH").map(String::as_str),
        Some("Sec-CH-UA,Sec-CH-UA-Full-Version-List")
    );

    // Set Headers warns for the property the response did not return, as
    // .NET's SetHeadersElement does when TryGet fails, and the data's own
    // warning for a read is not given, because Set Headers asks first. The
    // property the response gave as null logs nothing.
    assert_eq!(
        take_logged(),
        vec![
            "WARN Property 'SetHeaderPlatformAccept-CH' is missing from 'device', \
             so it adds no value to the Accept-CH header."
        ]
    );
}
