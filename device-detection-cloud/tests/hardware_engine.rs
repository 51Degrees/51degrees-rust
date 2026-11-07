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

//! End-to-end tests for the cloud hardware-profile lookup engine (TAC and
//! native-model lookup).
//!
//! These drive a real pipeline (cloud request engine followed by the
//! hardware-profile cloud engine) against a fake HTTP transport, so no network is
//! touched. A representative cloud `hardware.profiles` response is fed through and
//! the resulting [`MultiDeviceData`] profiles are asserted on, proving the engine
//! parses the multi-profile `hardware.profiles` shape, and the JSON builder
//! writes them back in that shape.

use std::sync::Arc;

use fiftyone_json_builder::{JsonBuilderElement, JSON_BUILDER_DATA_KEY};
use fiftyone_pipeline_core::{Evidence, FlowElement, Pipeline, PropertyValueType};
use fiftyone_pipeline_engines_fiftyone::SequenceElement;
use serde_json::{json, Value};

use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine, HttpMethod,
};
use fiftyone_device_detection_cloud::{DeviceData, HardwareProfileCloudEngine, HARDWARE_DATA_KEY};

/// A fake transport that answers the cloud endpoints from in-memory fixtures.
struct FakeCloud {
    accessible_properties: String,
    evidence_keys: String,
    data: String,
}

impl CloudHttpClient for FakeCloud {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        let body = if request.url.contains("accessibleproperties") {
            self.accessible_properties.clone()
        } else if request.url.contains("evidencekeys") {
            self.evidence_keys.clone()
        } else {
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

/// A representative hardware-profile cloud response: a `hardware.profiles` array
/// carrying two device profiles that matched the supplied TAC. One profile
/// carries a null hardware vendor paired with its `nullreason`, exactly as the
/// service sends it, so the no-value path is exercised too.
const HARDWARE_RESPONSE: &str = r#"{
    "hardware": {
        "profiles": [
            {
                "hardwarevendor": "Apple",
                "hardwarename": ["iPhone 11"],
                "hardwaremodel": "iPhone11,8"
            },
            {
                "hardwarevendor": null,
                "hardwarevendornullreason": "The value cannot be determined from a TAC alone.",
                "hardwarename": ["iPhone 11 Pro", "iPhone 11 Pro Max"],
                "hardwaremodel": "iPhone12,3"
            }
        ]
    },
    "javascriptProperties": [],
    "warnings": []
}"#;

/// A standard single-device response, as returned by a resource key that does
/// not grant the hardware-profile-lookup product. It has no `hardware` block.
const DEVICE_ONLY_RESPONSE: &str = r#"{
    "device": { "ismobile": false, "hardwarename": ["Desktop"] },
    "javascriptProperties": []
}"#;

/// A hardware-profile response that matched no profile, in the form the
/// cloud sends for a key without the paid hardware properties. Beside the
/// empty list it gives each paid property as null with a reason.
const NO_PROFILES_RESPONSE: &str = r#"{
    "hardware": {
        "profiles": [],
        "hardwarevendor": null,
        "hardwarevendornullreason": "HardwareVendor is a paid feature."
    },
    "javascriptProperties": []
}"#;

/// The accessible-properties body advertising the hardware product, in the
/// cloud's form, being one `Profiles` property whose item properties are
/// those each profile holds.
const ACCESSIBLE_PROPERTIES: &str = r#"{
    "Products": {
        "hardware": {
            "DataTier": "CloudV4",
            "Properties": [
                {
                    "Name": "Profiles",
                    "Type": "Array",
                    "Category": "",
                    "ItemProperties": [
                        { "Name": "HardwareVendor", "Type": "String", "Category": "Hardware" },
                        { "Name": "HardwareName", "Type": "Array", "Category": "Hardware" },
                        { "Name": "HardwareModel", "Type": "String", "Category": "Hardware" }
                    ]
                }
            ]
        }
    }
}"#;

const EVIDENCE_KEYS: &str = r#"["query.tac","query.nativemodel"]"#;

/// A request engine over a fake transport returning the supplied data body.
fn fake_request_engine(data: &str) -> Arc<CloudRequestEngine> {
    let fake = Arc::new(FakeCloud {
        accessible_properties: ACCESSIBLE_PROPERTIES.to_owned(),
        evidence_keys: EVIDENCE_KEYS.to_owned(),
        data: data.to_owned(),
    });

    Arc::new(
        CloudRequestEngine::builder()
            .resource_key("test-resource-key")
            .http_client(fake)
            .build()
            .expect("request engine builds"),
    )
}

/// Build a pipeline of `CloudRequestEngine` -> `HardwareProfileCloudEngine` over
/// a fake transport returning the supplied data body.
fn fake_pipeline(data: &str) -> Arc<Pipeline> {
    let request_engine = fake_request_engine(data);

    let hardware_engine = HardwareProfileCloudEngine::builder()
        .cloud_request_engine(request_engine.clone())
        .build();

    Pipeline::builder()
        .add_element(request_engine)
        .add_element(Arc::new(hardware_engine))
        .build()
        .expect("pipeline builds")
}

#[test]
fn populates_profiles_from_hardware_json() {
    let pipeline = fake_pipeline(HARDWARE_RESPONSE);
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("query.tac", "35925406").build());
    data.process().expect("processing succeeds");

    let hardware = data
        .get(HARDWARE_DATA_KEY)
        .expect("hardware data is present under the shared key");

    // Both matching profiles came through, in order.
    assert_eq!(hardware.profiles().len(), 2);

    let first = &hardware.profiles()[0];
    assert_eq!(first.hardware_vendor().value().unwrap(), "Apple");
    assert_eq!(first.hardware_model().value().unwrap(), "iPhone11,8");
    assert_eq!(
        first.hardware_name().value().unwrap(),
        &["iPhone 11".to_owned()]
    );

    // The second profile carries a multi-element name list.
    let second = &hardware.profiles()[1];
    assert_eq!(second.hardware_model().value().unwrap(), "iPhone12,3");
    assert_eq!(
        second.hardware_name().value().unwrap(),
        &["iPhone 11 Pro".to_owned(), "iPhone 11 Pro Max".to_owned()]
    );
}

#[test]
fn null_profile_property_becomes_no_value_with_reason() {
    let pipeline = fake_pipeline(HARDWARE_RESPONSE);
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("query.tac", "35925406").build());
    data.process().expect("processing succeeds");
    let hardware = data.get(HARDWARE_DATA_KEY).expect("hardware data present");

    // The second profile's vendor was null, so the typed accessor is a no-value
    // carrying the cloud's explanation.
    let second = &hardware.profiles()[1];
    assert!(
        !second.hardware_vendor().has_value(),
        "null profile property must be a no-value"
    );
    assert_eq!(
        second.hardware_vendor().no_value_message(),
        Some("The value cannot be determined from a TAC alone.")
    );

    // The nullreason entry is not a property, so it is not in the profile.
    use fiftyone_pipeline_core::ElementData;
    assert!(second.get("hardwarevendornullreason").is_err());
}

#[test]
fn device_only_response_yields_no_profiles() {
    // A resource key without the hardware-profile product returns a `device`
    // block and no `hardware.profiles`. The engine must produce an empty result
    // rather than fail.
    let pipeline = fake_pipeline(DEVICE_ONLY_RESPONSE);
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("query.tac", "00000000").build());
    data.process().expect("processing succeeds");

    let hardware = data
        .get(HARDWARE_DATA_KEY)
        .expect("hardware data is present even when empty");
    assert!(
        hardware.is_empty(),
        "a device-only response should yield no profiles"
    );
}

/// Run one TAC lookup answered by `data` through the Sequence element, the
/// cloud engines and the JSON builder, and return the JSON builder's output.
fn json_output(data: &str) -> Value {
    let request_engine = fake_request_engine(data);
    let hardware_engine = HardwareProfileCloudEngine::builder()
        .cloud_request_engine(Arc::clone(&request_engine))
        .build();
    let json_builder = JsonBuilderElement::builder()
        .exclude_element(SequenceElement::DEFAULT_ELEMENT_DATA_KEY)
        .build();
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(SequenceElement::new()))
        .add_element(request_engine)
        .add_element(Arc::new(hardware_engine))
        .add_element(Arc::new(json_builder))
        .build()
        .expect("pipeline builds");

    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("query.tac", "35925406").build());
    data.process().expect("processing succeeds");
    let text = data
        .get(JSON_BUILDER_DATA_KEY)
        .expect("json builder data present")
        .json()
        .to_owned();
    serde_json::from_str(&text).expect("the output is JSON")
}

/// The JSON builder writes each profile as an object under `profiles`, as
/// .NET's does, with a property the profile has no value for as null
/// with its reason.
#[test]
fn the_json_builder_writes_each_profile_under_hardware_profiles() {
    let json = json_output(HARDWARE_RESPONSE);
    assert_eq!(
        json["hardware"],
        json!({
            "profiles": [
                {
                    "hardwarevendor": "Apple",
                    "hardwarename": ["iPhone 11"],
                    "hardwaremodel": "iPhone11,8"
                },
                {
                    "hardwarevendor": null,
                    "hardwarevendornullreason": "The value cannot be determined from a TAC alone.",
                    "hardwarename": ["iPhone 11 Pro", "iPhone 11 Pro Max"],
                    "hardwaremodel": "iPhone12,3"
                }
            ]
        })
    );
}

/// With no profile matched, `profiles` is an empty list, and the other
/// entries of the response's `hardware` block are not written, as .NET
/// reads only `profiles` from it.
#[test]
fn the_json_builder_writes_no_profiles_as_an_empty_list() {
    let json = json_output(NO_PROFILES_RESPONSE);
    assert_eq!(json["hardware"], json!({ "profiles": [] }));
}

/// The engine lists `Profiles` as a list of element data whose item
/// properties are each profile's, as the cloud lists them.
#[test]
fn profiles_is_listed_with_its_item_properties() {
    let engine = HardwareProfileCloudEngine::builder()
        .cloud_request_engine(fake_request_engine(HARDWARE_RESPONSE))
        .build();
    let properties = engine.properties();
    assert_eq!(properties.len(), 1);
    assert_eq!(properties[0].name, "Profiles");
    assert_eq!(properties[0].value_type, PropertyValueType::ElementDataList);
    let items: Vec<(&str, PropertyValueType)> = properties[0]
        .item_properties
        .iter()
        .map(|item| (item.name.as_str(), item.value_type))
        .collect();
    assert_eq!(
        items,
        [
            ("HardwareVendor", PropertyValueType::String),
            ("HardwareName", PropertyValueType::StringList),
            ("HardwareModel", PropertyValueType::String),
        ]
    );
}
