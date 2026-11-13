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

//! What the JSON builder writes for Device Detection cloud data.
//!
//! A cloud response gives a property it has no value for as null, beside a
//! `<property>nullreason` entry. The data holds the property with that
//! message, and the JSON builder writes it back as the property set to null
//! followed by the same `<property>nullreason` key and text, in the place
//! the response gave the property. A null property with no
//! `<property>nullreason` entry is written with the reason `Unknown`.
//!
//! The JSON builder turns on serde_json's `preserve_order` feature, so in
//! this build the response and the output are both read in the order their
//! keys were written.

use std::sync::Arc;

use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine,
};
use fiftyone_device_detection_cloud::DeviceDetectionCloudEngine;
use fiftyone_json_builder::{JsonBuilderElement, JSON_BUILDER_DATA_KEY};
use fiftyone_pipeline_core::{Evidence, Pipeline};
use fiftyone_pipeline_engines_fiftyone::SequenceElement;
use serde_json::{json, Value};

/// The properties the resource key offers.
const ACCESSIBLE_PROPERTIES: &str = r#"{
    "Products": {
        "device": {
            "DataTier": "CloudV4",
            "Properties": [
                { "Name": "IsMobile", "Type": "Bool", "Category": "Device" },
                { "Name": "HardwareVendor", "Type": "String", "Category": "Hardware" },
                { "Name": "PlatformName", "Type": "String", "Category": "Software" }
            ]
        }
    }
}"#;

/// The evidence keys the request engine fetches.
const EVIDENCE_KEYS: &str = r#"["header.user-agent"]"#;

/// One null property with a reason and one without, each beside a property
/// with a value, in an order that is not alphabetical.
const DEVICE_RESPONSE: &str = r#"{
    "device": {
        "platformname": null,
        "ismobile": true,
        "hardwarevendor": null,
        "hardwarevendornullreason": "No matching profiles were found."
    }
}"#;

/// Answers each cloud endpoint from the fixtures above.
struct FakeCloud;

impl CloudHttpClient for FakeCloud {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        let body = if request.url.contains("accessibleproperties") {
            ACCESSIBLE_PROPERTIES
        } else if request.url.contains("evidencekeys") {
            EVIDENCE_KEYS
        } else {
            DEVICE_RESPONSE
        };
        Ok(CloudHttpResponse {
            status: 200,
            body: body.to_owned(),
            retry_after: None,
        })
    }
}

/// Run one request through the Sequence element, the cloud engines and the
/// JSON builder, and return the JSON builder's output.
fn json_output() -> Value {
    let request_engine = Arc::new(
        CloudRequestEngine::builder()
            .resource_key("test-resource-key")
            .http_client(Arc::new(FakeCloud))
            .build()
            .expect("request engine builds"),
    );
    let device_engine = DeviceDetectionCloudEngine::builder()
        .cloud_request_engine(Arc::clone(&request_engine))
        .build();
    let json_builder = JsonBuilderElement::builder()
        .exclude_element(SequenceElement::DEFAULT_ELEMENT_DATA_KEY)
        .build();
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(SequenceElement::new()))
        .add_element(request_engine)
        .add_element(Arc::new(device_engine))
        .add_element(Arc::new(json_builder))
        .build()
        .expect("pipeline builds");

    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", "Mozilla/5.0")
            .build(),
    );
    data.process().expect("processing succeeds");
    let text = data
        .get(JSON_BUILDER_DATA_KEY)
        .expect("json builder data present")
        .json()
        .to_owned();
    serde_json::from_str(&text).expect("the output is JSON")
}

#[test]
fn a_null_property_is_written_as_null_with_its_reason() {
    let json = json_output();
    assert_eq!(
        json["device"],
        json!({
            "platformname": null,
            "platformnamenullreason": "Unknown",
            "ismobile": true,
            "hardwarevendor": null,
            "hardwarevendornullreason": "No matching profiles were found."
        })
    );
    // Comparing two values ignores the order of their keys, so check it too.
    let keys: Vec<&str> = json["device"]
        .as_object()
        .expect("the device data is an object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "platformname",
            "platformnamenullreason",
            "ismobile",
            "hardwarevendor",
            "hardwarevendornullreason"
        ]
    );
}
