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

//! The processing every Cloud Aspect Engine shares, as the
//! [Cloud Aspect Engine specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/cloud-aspect-engine.md#processing)
//! gives it: the portion of the Cloud Request Engine's response for the
//! engine's Data Key, and each property's value or the reason it has none.

use std::sync::Arc;

use fiftyone_cloud_request_engine::aspect::{
    aspect_portion, aspect_response, aspect_values, AspectResponse, AspectValue,
};
use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine,
};
use fiftyone_pipeline_core::{
    Error, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, Pipeline,
    PropertyMetaData, Result,
};
use serde_json::{json, Map, Value};

/// A cloud answering discovery and every data request with `data` and
/// `status`.
struct FakeCloud {
    status: u16,
    data: &'static str,
}

impl CloudHttpClient for FakeCloud {
    fn send(&self, request: &CloudHttpRequest) -> std::result::Result<CloudHttpResponse, String> {
        let (status, body) = if request.url.contains("accessibleproperties") {
            (200, r#"{"Products":{}}"#)
        } else if request.url.contains("evidencekeys") {
            (200, r#"["header.user-agent"]"#)
        } else {
            (self.status, self.data)
        };
        Ok(CloudHttpResponse {
            status,
            body: body.to_owned(),
            retry_after: None,
        })
    }
}

/// A flow data the Cloud Request Engine processed against a cloud answering
/// the data request with `data` and `status`.
fn processed(status: u16, data: &'static str) -> FlowData {
    let engine = CloudRequestEngine::builder()
        .resource_key("test-resource-key")
        .endpoint("https://cloud.example.test/api/v4/")
        .http_client(Arc::new(FakeCloud { status, data }))
        .build()
        .expect("the engine builds");
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(engine))
        .suppress_process_exceptions(true)
        .build()
        .expect("the pipeline builds");
    let mut flow_data = pipeline.create_flow_data();
    flow_data.process().expect("errors are kept");
    flow_data
}

const RESPONSE: &str = r#"{"device":{"ismobile":true,"browsername":null,
    "browsernamenullreason":"No matching profiles"},"ip":{"registeredname":"Example"}}"#;

/// The portion `aspect_response` gives, which must be one.
fn portion(response: AspectResponse) -> Map<String, Value> {
    match response {
        AspectResponse::Portion(portion) => portion,
        AspectResponse::NoResponse => panic!("expected a portion"),
    }
}

#[test]
fn the_portion_for_the_data_key_is_given() {
    let data = processed(200, RESPONSE);

    let device = portion(aspect_response(&data, "device", "TestEngine").unwrap());
    assert_eq!(device.get("ismobile"), Some(&json!(true)));
    assert!(device.contains_key("browsername"));
    let ip = portion(aspect_response(&data, "ip", "TestEngine").unwrap());
    assert_eq!(ip.get("registeredname"), Some(&json!("Example")));
}

#[test]
fn the_portion_is_found_whatever_the_letter_case_of_the_data_key() {
    let data = processed(200, RESPONSE);

    let device = portion(aspect_response(&data, "Device", "TestEngine").unwrap());
    assert_eq!(device.get("ismobile"), Some(&json!(true)));
}

#[test]
fn a_response_with_no_portion_gives_an_empty_one_and_no_error() {
    let data = processed(200, RESPONSE);

    let hardware = portion(aspect_response(&data, "hardware", "TestEngine").unwrap());
    assert!(hardware.is_empty());
}

#[test]
fn a_failed_request_gives_no_response() {
    let data = processed(500, r#"{"errors":["The service is down."]}"#);

    assert!(matches!(
        aspect_response(&data, "device", "TestEngine"),
        Ok(AspectResponse::NoResponse)
    ));
}

/// An element that adds nothing, standing in for a pipeline with no Cloud
/// Request Engine.
struct Nothing {
    filter: EvidenceKeyFilterWhitelist,
}

impl FlowElement for Nothing {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
        Ok(())
    }
    fn data_key(&self) -> &str {
        "nothing"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

#[test]
fn no_cloud_request_engine_is_a_configuration_error_naming_the_engine() {
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(Nothing {
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        }))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();

    match aspect_response(&data, "device", "TestEngine") {
        Err(Error::PipelineConfiguration { message }) => {
            assert!(message.contains("'CloudRequestEngine'"), "{message}");
            assert!(message.contains("'TestEngine'"), "{message}");
        }
        other => panic!("expected a configuration error, got {other:?}"),
    }
}

#[test]
fn a_response_that_is_not_json_is_a_cloud_request_error() {
    match aspect_portion("not json", "device") {
        Err(Error::CloudRequest { status_code, .. }) => assert_eq!(status_code, 0),
        other => panic!("expected a cloud request error, got {other:?}"),
    }
}

#[test]
fn each_value_or_the_reason_it_has_none_is_given() {
    let portion = json!({
        "ismobile": true,
        "browsername": null,
        "BrowserNameNullReason": "No matching profiles",
        "platformname": null,
        "nullValueReasons": { "PlatformName": "No platform for this device" },
        "hardwarevendor": null,
        "orphannullreason": "No property of this name."
    });
    let portion = portion.as_object().unwrap();

    let values = aspect_values(portion);
    let value = |name: &str| {
        values
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value)
    };
    assert_eq!(values.len(), 4, "only the properties are given");
    assert!(matches!(
        value("ismobile"),
        Some(AspectValue::Value(Value::Bool(true)))
    ));
    assert!(matches!(
        value("browsername"),
        Some(AspectValue::NoValue(reason)) if reason == "No matching profiles"
    ));
    assert!(matches!(
        value("platformname"),
        Some(AspectValue::NoValue(reason)) if reason == "No platform for this device"
    ));
    assert!(matches!(
        value("hardwarevendor"),
        Some(AspectValue::NoValue(reason)) if reason == "Unknown"
    ));
}
