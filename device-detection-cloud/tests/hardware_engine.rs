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
//! parses the multi-profile `hardware.profiles` shape.

use std::sync::Arc;

use fiftyone_pipeline_core::{
    ElementData, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    Pipeline, PropertyMetaData, PropertyValue, Result as PipelineResult,
};

use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine, HttpMethod,
};
use fiftyone_device_detection_cloud::{
    DeviceData, DeviceDataBase, HardwareProfileCloudEngine, MultiDeviceData, HARDWARE_DATA_KEY,
    HARDWARE_ELEMENT_DATA_KEY,
};
use fiftyone_device_detection_shared::ProfileReader;

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

/// The accessible-properties body advertising the hardware product.
const ACCESSIBLE_PROPERTIES: &str = r#"{
    "Products": {
        "hardware": {
            "DataTier": "CloudV4",
            "Properties": [
                { "Name": "HardwareVendor", "Type": "String", "Category": "Hardware" },
                { "Name": "HardwareName", "Type": "Array", "Category": "Hardware" },
                { "Name": "HardwareModel", "Type": "String", "Category": "Hardware" }
            ]
        }
    }
}"#;

const EVIDENCE_KEYS: &str = r#"["query.tac","query.nativemodel"]"#;

/// Build a pipeline of `CloudRequestEngine` -> `HardwareProfileCloudEngine` over
/// a fake transport returning the supplied data body.
fn fake_pipeline(data: &str) -> Arc<Pipeline> {
    fake_pipeline_after(Vec::new(), data)
}

/// Build a pipeline of `earlier`, then `CloudRequestEngine` ->
/// `HardwareProfileCloudEngine` over a fake transport returning the supplied
/// data body.
fn fake_pipeline_after(earlier: Vec<Arc<dyn FlowElement>>, data: &str) -> Arc<Pipeline> {
    let fake = Arc::new(FakeCloud {
        accessible_properties: ACCESSIBLE_PROPERTIES.to_owned(),
        evidence_keys: EVIDENCE_KEYS.to_owned(),
        data: data.to_owned(),
    });

    let request_engine = Arc::new(
        CloudRequestEngine::builder()
            .resource_key("test-resource-key")
            .http_client(fake)
            .build()
            .expect("request engine builds"),
    );

    let hardware_engine = HardwareProfileCloudEngine::builder()
        .cloud_request_engine(request_engine.clone())
        .build();

    earlier
        .into_iter()
        .fold(Pipeline::builder(), |builder, element| {
            builder.add_element(element)
        })
        .add_element(request_engine)
        .add_element(Arc::new(hardware_engine))
        .build()
        .expect("pipeline builds")
}

/// Gives each profile the hardware vendor `Earlier`, as an on-premise data
/// set gives the profile of an id.
struct EarlierReader;

impl ProfileReader for EarlierReader {
    fn read_profile(&self, _id: u32) -> DeviceDataBase {
        DeviceDataBase::new().set("HardwareVendor", "Earlier")
    }
}

/// Adds one profile by id under `hardware` before the cloud engines run, as
/// an on-premise TAC or Native engine earlier in the pipeline does.
struct EarlierProfile {
    filter: EvidenceKeyFilterWhitelist,
}

impl FlowElement for EarlierProfile {
    fn process(&self, data: &mut FlowData) -> PipelineResult<()> {
        data.get_or_add(HARDWARE_DATA_KEY, MultiDeviceData::new)?
            .push_profile_ids(Arc::new(EarlierReader), vec![12280]);
        Ok(())
    }

    fn data_key(&self) -> &str {
        HARDWARE_ELEMENT_DATA_KEY
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
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

/// The cloud's profiles follow those an earlier engine added under
/// `hardware`, rather than being dropped, and only the cloud's, which it
/// adds with their values, are written under `profiles`.
#[test]
fn profiles_follow_those_already_under_hardware() {
    let earlier = EarlierProfile {
        filter: EvidenceKeyFilterWhitelist::new(["query.tac"]),
    };
    let pipeline = fake_pipeline_after(
        vec![Arc::new(earlier) as Arc<dyn FlowElement>],
        HARDWARE_RESPONSE,
    );
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("query.tac", "35925406").build());
    data.process().expect("processing succeeds");
    assert!(data.errors().is_empty(), "{:?}", data.errors());

    let hardware = data.get(HARDWARE_DATA_KEY).expect("hardware data present");
    let models: Vec<Option<String>> = hardware
        .profiles()
        .iter()
        .map(|profile| profile.hardware_model().value().ok().cloned())
        .collect();
    assert_eq!(
        models,
        [
            None,
            Some("iPhone11,8".to_owned()),
            Some("iPhone12,3".to_owned())
        ]
    );
    assert_eq!(
        hardware.profiles()[0].hardware_vendor().value().unwrap(),
        "Earlier"
    );

    let Ok(PropertyValue::KeyValueList(records)) = hardware.get("profiles") else {
        panic!("profiles is not a list of records");
    };
    let keys: Vec<Vec<&str>> = records
        .iter()
        .map(|record| record.keys().map(String::as_str).collect())
        .collect();
    assert_eq!(
        keys,
        [
            vec!["hardwaremodel", "hardwarename", "hardwarevendor"],
            vec!["hardwaremodel", "hardwarename"],
        ]
    );
}

#[test]
fn the_json_builder_writes_each_profile_under_hardware_profiles() {
    use fiftyone_json_builder::{JsonBuilderElement, JSON_BUILDER_DATA_KEY};
    use fiftyone_pipeline_engines_fiftyone::SequenceElement;

    let fake = Arc::new(FakeCloud {
        accessible_properties: ACCESSIBLE_PROPERTIES.to_owned(),
        evidence_keys: EVIDENCE_KEYS.to_owned(),
        data: HARDWARE_RESPONSE.to_owned(),
    });
    let request_engine = Arc::new(
        CloudRequestEngine::builder()
            .resource_key("test-resource-key")
            .http_client(fake)
            .build()
            .expect("request engine builds"),
    );
    let hardware_engine = HardwareProfileCloudEngine::builder()
        .cloud_request_engine(request_engine.clone())
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
    let json: serde_json::Value = serde_json::from_str(&text).expect("the output is JSON");

    let profiles = json["hardware"]["profiles"]
        .as_array()
        .expect("hardware.profiles is a list");
    assert_eq!(profiles.len(), 2, "{text}");
    assert_eq!(profiles[0]["hardwarevendor"], "Apple");
    assert_eq!(profiles[0]["hardwaremodel"], "iPhone11,8");
    assert_eq!(
        profiles[0]["hardwarename"],
        serde_json::json!(["iPhone 11"])
    );
    assert_eq!(profiles[1]["hardwaremodel"], "iPhone12,3");
    // A property a profile holds with no value is left out of its record.
    assert!(profiles[1].get("hardwarevendor").is_none(), "{text}");
}
