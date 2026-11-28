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

//! The `derived` Properties as Element Data, read through the
//! [`DerivedCloudEngine`], against an in-process client.

use std::sync::Arc;

use fiftyone_cloud_request_engine::{
    CloudEngineState, CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine,
    DerivedCloudEngine, EvidenceKeyEntry, LicensedProducts, DERIVED_DATA_KEY, PAGE_EVIDENCE,
    SEALED_RESULT_EVIDENCE,
};
use fiftyone_pipeline_core::{ElementData, Evidence, FlowData, Pipeline};

const ENDPOINT: &str = "https://cloud.example.test/api/v4/";
const RESOURCE_KEY: &str = "test-resource-key";
const LICENSE_KEY: &str = "LK-0123456789-WXYZ";
const SEALED: &str = "c2VhbGVkLXJlc3VsdC1mb3ItdGhpcy1wYWdl";

/// A client that answers the json call with a scripted answer.
struct Cloud(Result<(u16, String), String>);

impl Cloud {
    fn answering(body: &str) -> Arc<Self> {
        Arc::new(Self(Ok((200, body.to_owned()))))
    }

    fn failing() -> Arc<Self> {
        Arc::new(Self(Err("timed out".to_owned())))
    }
}

impl CloudHttpClient for Cloud {
    fn send(&self, _: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        self.0.clone().map(|(status, body)| CloudHttpResponse {
            status,
            body,
            retry_after: None,
        })
    }
}

/// The accessible properties of a resource key whose `derived` product
/// lists these properties, or that has no `derived` product when `None`.
fn state(derived: Option<&[&str]>) -> CloudEngineState {
    let products = match derived {
        Some(names) => {
            let properties: Vec<String> = names
                .iter()
                .map(|name| format!(r#"{{"Name":"{name}","Type":"String","Category":"Derived"}}"#))
                .collect();
            format!(
                r#"{{"Products":{{"derived":{{"DataTier":"CloudV4","Properties":[{}]}}}}}}"#,
                properties.join(",")
            )
        }
        None => r#"{"Products":{"device":{"Properties":[{"Name":"IsMobile","Type":"Bool"}]}}}"#
            .to_owned(),
    };
    CloudEngineState {
        evidence_keys: vec![EvidenceKeyEntry {
            key: "header.user-agent".to_owned(),
            order: 0,
        }],
        accessible_properties: LicensedProducts::parse(&products).unwrap(),
    }
}

/// A request engine with a Resource Key whose `derived` product lists
/// `derived`, answering with `cloud`.
fn request_engine(cloud: &Arc<Cloud>, derived: Option<&[&str]>) -> Arc<CloudRequestEngine> {
    Arc::new(
        CloudRequestEngine::builder()
            .resource_key(RESOURCE_KEY)
            .endpoint(ENDPOINT)
            .http_client(cloud.clone())
            .set_state(state(derived))
            .build()
            .unwrap(),
    )
}

/// A Pipeline of the request engine and a derived engine reading it.
fn pipeline(request_engine: &Arc<CloudRequestEngine>) -> Arc<Pipeline> {
    let derived = DerivedCloudEngine::builder()
        .cloud_request_engine(request_engine.clone())
        .build()
        .unwrap();
    Pipeline::builder()
        .add_element(request_engine.clone())
        .add_element(Arc::new(derived))
        .suppress_process_exceptions(true)
        .build()
        .unwrap()
}

fn process(pipeline: &Arc<Pipeline>, evidence: &[(&str, &str)]) -> FlowData {
    let mut builder = Evidence::builder().add("header.user-agent", "Mozilla/5.0");
    for (key, value) in evidence {
        builder = builder.add(*key, *value);
    }
    let mut data = pipeline.create_flow_data_with(builder.build());
    let _ = data.process();
    data
}

/// The flow data's errors recorded for the derived engine.
fn derived_errors(data: &FlowData) -> Vec<String> {
    data.errors()
        .iter()
        .filter(|error| error.element_data_key == "derived")
        .map(|error| error.source.to_string())
        .collect()
}

/// The two Properties as the derived data gives them: the Value, or the
/// message that says why there is none.
fn read(data: &FlowData) -> [Result<String, String>; 2] {
    let derived = data.get(DERIVED_DATA_KEY).expect("the derived data");
    [
        derived.human_confidence(),
        derived.human_confidence_source(),
    ]
    .map(|value| value.into_value().map_err(|no_value| no_value.message))
}

// Required test 7: a read of HumanConfidence or HumanConfidenceSource with a
// credential that does not carry it gives no value and the message that
// says why.

#[test]
fn a_property_the_response_holds_as_null_gives_the_remote_servers_reason() {
    let cloud = Cloud::answering(
        r#"{"derived":{
            "humanconfidence":null,
            "humanconfidencenullreason":"This resource key does not include HumanConfidence.",
            "humanconfidencesource":null,
            "nullValueReasons":{"humanconfidencesource":"Not in this resource key."}
        }}"#,
    );
    let engine = request_engine(&cloud, Some(&["HumanConfidence", "HumanConfidenceSource"]));
    let data = process(&pipeline(&engine), &[]);

    assert_eq!(
        read(&data),
        [
            Err("This resource key does not include HumanConfidence.".to_owned()),
            Err("Not in this resource key.".to_owned()),
        ]
    );
    let derived = data.get(DERIVED_DATA_KEY).unwrap();
    assert_eq!(
        derived.get("HumanConfidence").unwrap_err().message,
        "This resource key does not include HumanConfidence."
    );
    assert!(derived.has_property("HumanConfidence"));
    assert!(
        !derived.keys().iter().any(|key| key.ends_with("nullreason")),
        "a reason is not a Property: {:?}",
        derived.keys()
    );
    assert!(data.errors().is_empty(), "{:?}", data.errors());
}

#[test]
fn a_null_property_with_no_reason_gives_unknown() {
    let cloud = Cloud::answering(r#"{"derived":{"humanconfidence":null}}"#);
    let engine = request_engine(&cloud, Some(&["HumanConfidence"]));
    let data = process(&pipeline(&engine), &[]);
    assert_eq!(read(&data)[0], Err("Unknown".to_owned()));
}

#[test]
fn a_property_the_response_leaves_out_gives_the_missing_properties_reason() {
    let cloud = Cloud::answering(r#"{"derived":{"other":"x"}}"#);
    let engine = request_engine(&cloud, Some(&["Other"]));
    let data = process(&pipeline(&engine), &[]);

    let [confidence, source] = read(&data);
    for (name, message) in [
        ("HumanConfidence", confidence),
        ("HumanConfidenceSource", source),
    ] {
        let message = message.expect_err(name);
        assert!(
            message.starts_with(&format!(
                "Property '{name}' not found in data for element 'derived'. This is because \
                 your resource key does not include access to this property. The properties \
                 included for your key under 'derived' are Other."
            )),
            "{message}"
        );
    }
    assert!(!data
        .get(DERIVED_DATA_KEY)
        .unwrap()
        .has_property("HumanConfidence"));
}

#[test]
fn a_resource_key_with_no_derived_product_gives_that_reason() {
    let cloud = Cloud::answering(r#"{"device":{"ismobile":true}}"#);
    let engine = request_engine(&cloud, None);
    let data = process(&pipeline(&engine), &[]);

    let message = read(&data)[0].clone().unwrap_err();
    assert!(
        message.starts_with(
            "Property 'HumanConfidence' not found in data for element 'derived'. This is \
             because your resource key does not include access to any properties under \
             'derived'."
        ),
        "{message}"
    );
}

#[test]
fn a_credential_that_carries_the_properties_gives_their_values() {
    let cloud = Cloud::answering(
        r#"{"derived":{"humanconfidence":"High","humanconfidencesource":"Passive","count":3}}"#,
    );
    let engine = request_engine(&cloud, Some(&["HumanConfidence", "HumanConfidenceSource"]));
    let data = process(&pipeline(&engine), &[]);

    assert_eq!(
        read(&data),
        [Ok("High".to_owned()), Ok("Passive".to_owned())]
    );
    let derived = data.get(DERIVED_DATA_KEY).unwrap();
    assert_eq!(derived.get("count").unwrap().as_integer(), Some(3));
    assert!(data.errors().is_empty(), "{:?}", data.errors());
}

#[test]
fn a_failed_request_gives_its_reason_and_the_engine_records_it_did_not_process() {
    let engine = request_engine(&Cloud::failing(), Some(&["HumanConfidence"]));
    let data = process(&pipeline(&engine), &[]);

    let message = read(&data)[0].clone().unwrap_err();
    assert!(
        message.ends_with("This is because an upstream cloud request failed, so no data was available to populate the property."),
        "{message}"
    );
    let errors = derived_errors(&data);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("'DerivedCloudEngine' did not process"),
        "{errors:?}"
    );
}

// A request that carries a sealed result, read through the element.

/// A request engine with a License Key, so it redeems, and a page it
/// issued.
fn licensed(cloud: &Arc<Cloud>) -> (Arc<CloudRequestEngine>, String) {
    let engine = Arc::new(
        CloudRequestEngine::builder()
            .resource_key(RESOURCE_KEY)
            .license_key(LICENSE_KEY)
            .endpoint(ENDPOINT)
            .http_client(cloud.clone())
            .set_state(state(Some(&["HumanConfidence", "HumanConfidenceSource"])))
            .build()
            .unwrap(),
    );
    let page = engine.human_confidence_page().unwrap().id().to_owned();
    (engine, page)
}

#[test]
fn a_sealed_result_that_redeems_gives_the_value_and_active() {
    for word in ["High", "Medium", "Low"] {
        let cloud = Cloud::answering(&format!(
            r#"{{"derived":{{"humanconfidence":"{word}","humanconfidencesource":"Passive"}}}}"#
        ));
        let (engine, page) = licensed(&cloud);
        let data = process(
            &pipeline(&engine),
            &[(SEALED_RESULT_EVIDENCE, SEALED), (PAGE_EVIDENCE, &page)],
        );
        assert_eq!(
            read(&data),
            [Ok(word.to_owned()), Ok("Active".to_owned())],
            "{word}"
        );
        assert!(data.errors().is_empty(), "{:?}", data.errors());
    }
}

#[test]
fn a_sealed_result_that_is_not_redeemed_gives_medium_and_active() {
    let cases = [
        (
            "a value other than High, Medium or Low",
            Cloud::answering(r#"{"derived":{"humanconfidence":"Certain"}}"#),
        ),
        (
            "a held Property with no value",
            Cloud::answering(r#"{"derived":{"humanconfidence":null}}"#),
        ),
        ("a failed request", Cloud::failing()),
    ];
    for (case, cloud) in cases {
        let (engine, page) = licensed(&cloud);
        let data = process(
            &pipeline(&engine),
            &[(SEALED_RESULT_EVIDENCE, SEALED), (PAGE_EVIDENCE, &page)],
        );
        assert_eq!(
            read(&data),
            [Ok("Medium".to_owned()), Ok("Active".to_owned())],
            "{case}"
        );
        assert!(
            derived_errors(&data).is_empty(),
            "{case}: {:?}",
            data.errors()
        );
    }

    // No transaction is kept for the page.
    let cloud = Cloud::answering(r#"{"derived":{"humanconfidence":"High"}}"#);
    let (engine, _) = licensed(&cloud);
    let data = process(
        &pipeline(&engine),
        &[(SEALED_RESULT_EVIDENCE, SEALED), (PAGE_EVIDENCE, "unknown")],
    );
    assert_eq!(
        read(&data),
        [Ok("Medium".to_owned()), Ok("Active".to_owned())],
        "no transaction"
    );
}

#[test]
fn a_cloud_request_engine_after_the_derived_engine_is_a_configuration_error() {
    let engine = request_engine(&Cloud::failing(), Some(&["HumanConfidence"]));
    let derived = DerivedCloudEngine::builder()
        .cloud_request_engine(engine.clone())
        .build()
        .unwrap();
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(derived))
        .add_element(engine)
        .suppress_process_exceptions(false)
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data_with(Evidence::builder().build());
    let error = data.process().unwrap_err().to_string();
    assert!(
        error.contains("no 'CloudRequestEngine' before the 'DerivedCloudEngine'"),
        "{error}"
    );
}

// A configuration names the engine as .NET does.

/// Answers discovery and the json call in place of the cloud service.
struct StandIn;

impl CloudHttpClient for StandIn {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        let body = if request.url.contains("evidencekeys") {
            r#"["header.user-agent"]"#
        } else if request.url.contains("accessibleproperties") {
            r#"{"Products":{"derived":{"Properties":[{"Name":"HumanConfidence","Type":"String"}]}}}"#
        } else {
            r#"{"derived":{"humanconfidence":"Low"}}"#
        };
        Ok(CloudHttpResponse {
            status: 200,
            body: body.to_owned(),
            retry_after: None,
        })
    }
}

fn from_configuration(elements: &str) -> fiftyone_pipeline_core::Result<Arc<Pipeline>> {
    use fiftyone_cloud_request_engine::configuration;
    use fiftyone_pipeline_core::configuration::{BuilderRegistry, PipelineOptions};
    use fiftyone_pipeline_core::PipelineBuilder;

    let mut registry = BuilderRegistry::new();
    configuration::add_builders(&mut registry);
    let options = PipelineOptions::from_json(&format!(
        r#"{{"PipelineOptions":{{"Elements":[{elements}]}}}}"#
    ))?;
    let client: Arc<dyn CloudHttpClient> = Arc::new(StandIn);
    PipelineBuilder::new()
        .add_resource(Arc::new(client))
        .build_from_configuration(&registry, &options)
}

#[test]
fn a_configuration_makes_the_engine_after_the_cloud_request_engine() {
    let pipeline = from_configuration(
        r#"{"BuilderName":"CloudRequestEngine","BuildParameters":{"ResourceKey":"my-resource-key"}},
           {"BuilderName":"DerivedCloudEngine"}"#,
    )
    .unwrap();
    let keys: Vec<&str> = pipeline
        .flow_elements()
        .iter()
        .map(|e| e.data_key())
        .collect();
    assert_eq!(keys, ["cloud-response", "derived"]);
    assert!(pipeline.resource::<DerivedCloudEngine>().is_some());

    let data = process(&pipeline, &[]);
    let [confidence, source] = read(&data);
    assert_eq!(confidence, Ok("Low".to_owned()));
    assert!(
        source
            .unwrap_err()
            .contains("not found in data for element 'derived'"),
        "the source is missing with its reason"
    );
}

#[test]
fn a_configuration_with_no_cloud_request_engine_before_it_is_refused() {
    let error = from_configuration(r#"{"BuilderName":"DerivedCloudEngineBuilder"}"#)
        .err()
        .expect("refused")
        .to_string();
    assert!(
        error.contains("The 'DerivedCloudEngine' requires a 'CloudRequestEngine' before it"),
        "{error}"
    );
}

#[test]
fn its_element_data_is_read_only_once_processed() {
    let engine = request_engine(
        &Cloud::answering(r#"{"derived":{"humanconfidence":"High"}}"#),
        Some(&["HumanConfidence"]),
    );
    let mut data = pipeline(&engine).create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", "Mozilla/5.0")
            .build(),
    );
    data.process().unwrap();
    assert!(
        data.get(DERIVED_DATA_KEY).is_some(),
        "no data under 'derived'"
    );
    match data.get_or_add(DERIVED_DATA_KEY, || {
        unreachable!("the element added its data")
    }) {
        Err(fiftyone_pipeline_core::Error::PipelineConfiguration { message }) => {
            assert!(message.contains("cannot be written"), "{message}")
        }
        Err(other) => panic!("not a configuration error: {other}"),
        Ok(_) => panic!("the data under 'derived' was written after processing"),
    }
}
