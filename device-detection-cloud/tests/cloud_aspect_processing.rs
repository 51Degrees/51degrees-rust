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

//! The Device Detection and hardware profile cloud engines process the
//! cloud response through the shared Cloud Aspect Engine logic in
//! `fiftyone_cloud_request_engine::aspect`, so they read a response as the
//! IP Intelligence cloud engine does, as the
//! [Cloud Aspect Engine specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/cloud-aspect-engine.md#processing)
//! asks.

use std::sync::Arc;

use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine,
};
use fiftyone_device_detection_cloud::{
    DeviceData, DeviceDetectionCloudEngine, HardwareProfileCloudEngine, DEVICE_DATA_KEY,
    HARDWARE_DATA_KEY,
};
use fiftyone_pipeline_core::{Error, Evidence, FlowElement, Pipeline};

/// A cloud answering every data request with `data`.
struct FakeCloud {
    data: &'static str,
}

impl CloudHttpClient for FakeCloud {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        let body = if request.url.contains("accessibleproperties") {
            r#"{"Products":{}}"#
        } else if request.url.contains("evidencekeys") {
            r#"["header.user-agent","query.tac"]"#
        } else {
            self.data
        };
        Ok(CloudHttpResponse {
            status: 200,
            body: body.to_owned(),
            retry_after: None,
        })
    }
}

fn request_engine(data: &'static str) -> Arc<CloudRequestEngine> {
    Arc::new(
        CloudRequestEngine::builder()
            .resource_key("test-resource-key")
            .endpoint("https://cloud.example.test/api/v4/")
            .http_client(Arc::new(FakeCloud { data }))
            .build()
            .unwrap(),
    )
}

#[test]
fn a_reason_in_null_value_reasons_is_the_reason_a_device_property_has_no_value() {
    let request = request_engine(
        r#"{"device":{"ismobile":true,"browsername":null,
            "nullValueReasons":{"BrowserName":"No matching profiles"}}}"#,
    );
    let engine = Arc::new(DeviceDetectionCloudEngine::new(Arc::clone(&request)));
    let pipeline = Pipeline::builder()
        .add_element(request)
        .add_element(engine)
        .build()
        .unwrap();
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("header.user-agent", "A").build());
    data.process().unwrap();

    let device = data.get(DEVICE_DATA_KEY).expect("device data");
    assert!(*device.is_mobile().value().unwrap());
    assert_eq!(
        device.browser_name().no_value_message(),
        Some("No matching profiles")
    );
}

#[test]
fn a_reason_in_null_value_reasons_is_the_reason_a_profile_property_has_no_value() {
    let request = request_engine(
        r#"{"hardware":{"profiles":[{"hardwarevendor":null,
            "nullValueReasons":{"HardwareVendor":"Not from a TAC alone."}}]}}"#,
    );
    let engine = Arc::new(HardwareProfileCloudEngine::new(Arc::clone(&request)));
    let pipeline = Pipeline::builder()
        .add_element(request)
        .add_element(engine)
        .build()
        .unwrap();
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("query.tac", "35925406").build());
    data.process().unwrap();

    let hardware = data.get(HARDWARE_DATA_KEY).expect("hardware data");
    let profile = hardware.profiles().first().expect("one profile");
    assert_eq!(
        profile.hardware_vendor().no_value_message(),
        Some("Not from a TAC alone.")
    );
}

/// The error an engine records when the pipeline has no Cloud Request
/// Engine before it.
fn error_without_a_request_engine(engine: Arc<dyn FlowElement>) -> String {
    let pipeline = Pipeline::builder().add_element(engine).build().unwrap();
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("header.user-agent", "A").build());
    match data.process() {
        Err(Error::Aggregate(errors)) => match &errors[0].source {
            Error::PipelineConfiguration { message } => message.clone(),
            other => panic!("expected a configuration error, got {other:?}"),
        },
        other => panic!("expected processing to fail, got {other:?}"),
    }
}

#[test]
fn each_engine_names_itself_when_there_is_no_request_engine_before_it() {
    let request = request_engine(r#"{}"#);
    for (engine, name) in [
        (
            Arc::new(DeviceDetectionCloudEngine::new(Arc::clone(&request))) as Arc<dyn FlowElement>,
            "DeviceDetectionCloudEngine",
        ),
        (
            Arc::new(HardwareProfileCloudEngine::new(Arc::clone(&request))),
            "HardwareProfileCloudEngine",
        ),
    ] {
        let message = error_without_a_request_engine(engine);
        assert!(
            message.starts_with("No cloud request data is present."),
            "{message}"
        );
        assert!(message.contains(&format!("'{name}'")), "{message}");
    }
}
