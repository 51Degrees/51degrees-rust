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

//! End-to-end tests for the cloud device-detection engine.
//!
//! The offline tests drive a real pipeline (cloud request engine followed by the
//! device-detection cloud engine) against a fake HTTP transport, so no network is
//! touched. A representative cloud `device` response, including a
//! `null` property paired with its `nullreason` sibling, is fed through and the
//! resulting [`DeviceDataBase`] is asserted on.
//!
//! The live test at the end is `#[ignore]` and runs only when a real resource key
//! is supplied through `51DEGREES_RESOURCE_KEY`.

use std::sync::Arc;

use fiftyone_pipeline_core::{Evidence, Pipeline};

use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine, HttpMethod,
};
use fiftyone_device_detection_cloud::{
    DeviceData, DeviceDetectionCloudEngine, HardwareProfileCloudEngine, DEVICE_DATA_KEY,
    HARDWARE_DATA_KEY,
};

/// A fake transport that answers the three cloud endpoints from in-memory
/// fixtures, so the whole pipeline runs without a network. Requests are routed
/// by the URL suffix the cloud request engine builds.
struct FakeCloud {
    /// The `accessibleproperties` body returned for property discovery.
    accessible_properties: String,
    /// The `evidencekeys` body returned for evidence-key discovery.
    evidence_keys: String,
    /// The `json` data body returned for each flow data.
    data: String,
}

impl CloudHttpClient for FakeCloud {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        let body = if request.url.contains("accessibleproperties") {
            self.accessible_properties.clone()
        } else if request.url.contains("evidencekeys") {
            self.evidence_keys.clone()
        } else {
            // The data endpoint is the POST target.
            assert_eq!(request.method, HttpMethod::Post);
            self.data.clone()
        };
        Ok(CloudHttpResponse {
            status: 200,
            body,
            retry_after: None,
        })
    }
}

/// A representative cloud response document. The `device` member holds the
/// detection result, with `hardwarevendor` deliberately null and paired with a
/// `hardwarevendornullreason` explanation, exactly as the service sends it.
const DEVICE_RESPONSE: &str = r#"{
    "device": {
        "ismobile": true,
        "hardwarevendor": null,
        "hardwarevendornullreason": "The value cannot be determined until more evidence is provided.",
        "hardwaremodel": "SM-G960U",
        "hardwarename": ["Galaxy S9"],
        "platformname": "Android",
        "platformversion": "8.0",
        "browsername": "Chrome Mobile",
        "browserversion": "67.0",
        "screenpixelswidth": 1440,
        "screenpixelsheight": 2960,
        "iscrawler": false
    },
    "javascriptProperties": [],
    "warnings": []
}"#;

/// The accessible-properties body advertising the device product and a few of
/// its properties, so the engine can derive its metadata.
const ACCESSIBLE_PROPERTIES: &str = r#"{
    "Products": {
        "device": {
            "DataTier": "CloudV4",
            "Properties": [
                { "Name": "IsMobile", "Type": "Bool", "Category": "Device" },
                { "Name": "HardwareVendor", "Type": "String", "Category": "Hardware" },
                { "Name": "HardwareName", "Type": "Array", "Category": "Hardware" },
                { "Name": "PlatformName", "Type": "String", "Category": "Software" },
                { "Name": "ScreenPixelsWidth", "Type": "Int32", "Category": "Screen" }
            ]
        }
    }
}"#;

/// The evidence-keys body the request engine fetches.
const EVIDENCE_KEYS: &str = r#"["query.user-agent","header.user-agent"]"#;

/// Build a pipeline of `CloudRequestEngine` -> `DeviceDetectionCloudEngine` over
/// the fake transport.
fn fake_pipeline() -> (Arc<Pipeline>, Arc<CloudRequestEngine>) {
    let fake = Arc::new(FakeCloud {
        accessible_properties: ACCESSIBLE_PROPERTIES.to_owned(),
        evidence_keys: EVIDENCE_KEYS.to_owned(),
        data: DEVICE_RESPONSE.to_owned(),
    });

    let request_engine = Arc::new(
        CloudRequestEngine::builder()
            .resource_key("test-resource-key")
            .http_client(fake)
            .build()
            .expect("request engine builds"),
    );

    let device_engine = DeviceDetectionCloudEngine::builder()
        .cloud_request_engine(request_engine.clone())
        .build();

    let pipeline = Pipeline::builder()
        .add_element(request_engine.clone())
        .add_element(Arc::new(device_engine))
        .build()
        .expect("pipeline builds");

    (pipeline, request_engine)
}

/// A transport whose data endpoint fails, as in a cloud outage.
struct DataDownCloud;

impl CloudHttpClient for DataDownCloud {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        let (status, body) = if request.url.contains("accessibleproperties") {
            (200, ACCESSIBLE_PROPERTIES.to_owned())
        } else if request.url.contains("evidencekeys") {
            (200, EVIDENCE_KEYS.to_owned())
        } else {
            (500, r#"{"errors":["the service failed"]}"#.to_owned())
        };
        Ok(CloudHttpResponse {
            status,
            body,
            retry_after: None,
        })
    }
}

/// When the cloud request fails, both engines still store their data, empty,
/// as .NET's engines store theirs before processing. An element after them
/// that reads `device` or `hardware`, such as a derived property element,
/// then finds the data, so it cannot take the failure for a pipeline in the
/// wrong order.
#[test]
fn a_failed_request_stores_empty_device_and_hardware_data() {
    let request_engine = Arc::new(
        CloudRequestEngine::builder()
            .resource_key("test-resource-key")
            .http_client(Arc::new(DataDownCloud))
            .build()
            .expect("request engine builds"),
    );
    let device_engine = DeviceDetectionCloudEngine::builder()
        .cloud_request_engine(request_engine.clone())
        .build();
    let hardware_engine = HardwareProfileCloudEngine::builder()
        .cloud_request_engine(request_engine.clone())
        .build();
    let pipeline = Pipeline::builder()
        .add_element(request_engine)
        .add_element(Arc::new(device_engine))
        .add_element(Arc::new(hardware_engine))
        .suppress_process_exceptions(true)
        .build()
        .expect("pipeline builds");
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("header.user-agent", "A").build());
    data.process().expect("errors are suppressed");

    let keys: Vec<&str> = data
        .errors()
        .iter()
        .map(|error| error.element_data_key.as_str())
        .collect();
    assert_eq!(
        keys,
        vec!["cloud-response", "device", "hardware"],
        "the request engine's error, then one from each engine that did not process"
    );
    let device = data
        .get(DEVICE_DATA_KEY)
        .expect("the device data is stored");
    assert!(!device.is_mobile().has_value());
    let hardware = data
        .get(HARDWARE_DATA_KEY)
        .expect("the hardware data is stored");
    assert!(hardware.is_empty());
}

#[test]
fn populates_device_data_from_cloud_json() {
    let (pipeline, _request_engine) = fake_pipeline();
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", "Mozilla/5.0 (Linux; Android 8.0)")
            .build(),
    );
    data.process().expect("processing succeeds");

    let device = data
        .get(DEVICE_DATA_KEY)
        .expect("device data is present under the shared key");

    // Scalar values come through typed.
    assert!(*device.is_mobile().value().unwrap());
    assert_eq!(device.hardware_model().value().unwrap(), "SM-G960U");
    assert_eq!(device.platform_name().value().unwrap(), "Android");
    assert_eq!(device.browser_name().value().unwrap(), "Chrome Mobile");
    assert_eq!(*device.screen_pixels_width().value().unwrap(), 1440);
    assert_eq!(*device.screen_pixels_height().value().unwrap(), 2960);
    assert!(!*device.is_crawler().value().unwrap());

    // A list property is returned as a list.
    assert_eq!(
        device.hardware_name().value().unwrap(),
        &["Galaxy S9".to_owned()]
    );

    // Every generated property the cloud response carried must unpack through
    // the by-name getter the strongly-typed accessor uses, as the type the
    // generated metadata declares, so a consumer never gets a wrong-type
    // no-value for a value that is present.
    use fiftyone_device_detection_shared::GENERATED_PROPERTY_TYPES;
    use fiftyone_pipeline_core::{ElementData, PropertyValueType};
    let mut offenders: Vec<String> = Vec::new();
    for (name, declared) in GENERATED_PROPERTY_TYPES {
        if device.get(name).is_err() {
            continue;
        }
        let unpacks = match declared {
            PropertyValueType::Bool => device.bool_property(name).value().is_ok(),
            PropertyValueType::Integer => device.integer_property(name).value().is_ok(),
            PropertyValueType::Double => device.double_property(name).value().is_ok(),
            PropertyValueType::StringList => device.string_list_property(name).value().is_ok(),
            _ => device.string_property(name).value().is_ok(),
        };
        if !unpacks {
            offenders.push(format!(
                "{name} [{declared:?}] raw={:?}",
                device.get(name).ok()
            ));
        }
    }
    assert!(
        offenders.is_empty(),
        "cloud values not unpackable as their declared type:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn null_property_becomes_no_value_with_reason() {
    let (pipeline, _request_engine) = fake_pipeline();
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", "Mozilla/5.0")
            .build(),
    );
    data.process().expect("processing succeeds");
    let device = data.get(DEVICE_DATA_KEY).expect("device data present");

    // hardwarevendor was null in the response, so the typed accessor is a
    // no-value carrying the cloud's explanation.
    let vendor = device.hardware_vendor();
    assert!(!vendor.has_value(), "null property must be a no-value");
    assert_eq!(
        vendor.no_value_message(),
        Some("The value cannot be determined until more evidence is provided.")
    );

    // The nullreason entry is not a property, so it is not in the data.
    use fiftyone_pipeline_core::ElementData;
    assert!(device.get("hardwarevendornullreason").is_err());
    assert!(!device
        .keys()
        .iter()
        .any(|name| name.ends_with("nullreason")));
}

#[test]
fn derives_aspect_metadata_from_request_engine() {
    let (_pipeline, request_engine) = fake_pipeline();

    // The engine reads the device product's metadata when it is built, from
    // the accessible properties the request engine fetched when it was built.
    let mut engine = DeviceDetectionCloudEngine::builder()
        .cloud_request_engine(request_engine)
        .eager_properties(true)
        .try_build()
        .expect("eager build succeeds against the fake cloud");

    let count = engine.refresh_properties().expect("refresh succeeds");
    assert_eq!(count, 5, "five device properties were advertised");

    use fiftyone_pipeline_core::FlowElement;
    let names: Vec<&str> = engine
        .properties()
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert!(names.contains(&"IsMobile"));
    assert!(names.contains(&"HardwareName"));

    use fiftyone_pipeline_engines::AspectEngine;
    assert_eq!(engine.data_source_tier(), "CloudV4");
    assert_eq!(engine.aspect_properties().len(), 5);
}

#[test]
fn properties_are_read_when_the_engine_is_built() {
    // The engine exposes its metadata as soon as it is built, so the
    // set-headers element sees the device SetHeader* properties before the
    // first request, which is what makes the cloud Accept-CH headers work.
    use fiftyone_pipeline_core::FlowElement;

    let fake = Arc::new(FakeCloud {
        accessible_properties: ACCESSIBLE_PROPERTIES.to_owned(),
        evidence_keys: EVIDENCE_KEYS.to_owned(),
        data: DEVICE_RESPONSE.to_owned(),
    });
    let request_engine = Arc::new(
        CloudRequestEngine::builder()
            .resource_key("test-resource-key")
            .http_client(fake)
            .build()
            .expect("request engine builds"),
    );
    let device_engine = Arc::new(
        DeviceDetectionCloudEngine::builder()
            .cloud_request_engine(request_engine.clone())
            .build(),
    );

    let names: Vec<String> = device_engine
        .properties()
        .iter()
        .map(|p| p.name.to_lowercase())
        .collect();
    assert!(
        names.iter().any(|n| n == "screenpixelswidth"),
        "metadata is read when the engine is built, names were: {names:?}"
    );

    let pipeline = Pipeline::builder()
        .add_element(request_engine)
        .add_element(device_engine.clone() as Arc<dyn FlowElement>)
        .build()
        .expect("pipeline builds");
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", "Mozilla/5.0")
            .build(),
    );
    data.process().expect("processing succeeds");

    // Processing keeps the same metadata.
    assert_eq!(device_engine.properties().len(), names.len());
}

#[test]
fn missing_cloud_request_engine_is_a_configuration_error() {
    // A device cloud engine run without a cloud request engine before it must
    // fail with a configuration error rather than silently producing nothing.
    let fake = Arc::new(FakeCloud {
        accessible_properties: ACCESSIBLE_PROPERTIES.to_owned(),
        evidence_keys: EVIDENCE_KEYS.to_owned(),
        data: DEVICE_RESPONSE.to_owned(),
    });
    let request_engine = Arc::new(
        CloudRequestEngine::builder()
            .resource_key("rk")
            .http_client(fake)
            .build()
            .unwrap(),
    );
    let device_engine = DeviceDetectionCloudEngine::builder()
        .cloud_request_engine(request_engine)
        .build();

    // Pipeline with only the device engine, no request engine before it. Do not
    // suppress exceptions so the error surfaces.
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(device_engine))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", "Mozilla/5.0")
            .build(),
    );
    let result = data.process();
    assert!(
        result.is_err(),
        "processing without a cloud request engine must error"
    );
}

/// Resolve a cloud resource key from the environment for the live test.
///
/// The aligned `51DEGREES_RESOURCE_KEY` is checked first, then the CI-exported
/// bespoke and basic tiered names, mirroring the order in
/// `examples-shared::keys::resource_key_from_env`, so the live test runs from an
/// explicit key or the keys CI exports. Returns `None` when none is set, so the
/// test skips cleanly.
///
/// Gated on `reqwest-client` to match its only caller, the live test below, so
/// the helper is not flagged as dead code when the feature is off (for example
/// the no-default-features wasm32-wasip1 build).
#[cfg(feature = "reqwest-client")]
fn live_resource_key() -> Option<String> {
    [
        "51DEGREES_RESOURCE_KEY",
        "_51DEGREES_RESOURCE_KEY_BESPOKE",
        "_51DEGREES_RESOURCE_KEY_BASIC",
    ]
    .into_iter()
    .find_map(|name| match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => Some(value.trim().to_owned()),
        _ => None,
    })
}

/// A live test that hits the real 51Degrees cloud. It is ignored by default and
/// runs only when a resource key is present in the environment, for example:
/// `51DEGREES_RESOURCE_KEY=... cargo test -p fiftyone-device-detection-cloud -- --ignored`.
// Builds a CloudRequestEngine relying on the built-in reqwest client, so it only
// compiles with the reqwest-client feature. The workspace build unifies that
// feature on, and a standalone run needs `--features reqwest-client`.
#[cfg(feature = "reqwest-client")]
#[test]
#[ignore = "requires a network and a real resource key (51DEGREES_RESOURCE_KEY or the _51DEGREES_RESOURCE_KEY_BESPOKE/_BASIC tiered names)"]
fn live_cloud_detection() {
    let Some(resource_key) = live_resource_key() else {
        eprintln!("no resource key in the environment; skipping live cloud test");
        return;
    };

    let request_engine = Arc::new(
        CloudRequestEngine::builder()
            .resource_key(resource_key)
            .build()
            .expect("request engine builds"),
    );
    let device_engine = DeviceDetectionCloudEngine::builder()
        .cloud_request_engine(request_engine.clone())
        .build();

    let pipeline = Pipeline::builder()
        .add_element(request_engine)
        .add_element(Arc::new(device_engine))
        .build()
        .expect("pipeline builds");

    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add(
                "header.user-agent",
                "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
                 AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1",
            )
            .build(),
    );
    data.process().expect("live processing succeeds");

    let device = data.get(DEVICE_DATA_KEY).expect("live device data present");
    // The real service returns IsMobile for an iPhone user agent.
    assert!(
        device.is_mobile().has_value(),
        "live result should carry IsMobile"
    );
}
