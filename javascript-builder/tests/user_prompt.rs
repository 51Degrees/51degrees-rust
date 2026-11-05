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

//! When the script carries the user prompt section: by .NET's rule, from the
//! elements of the pipeline, or as the builder is told.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use fiftyone_cloud_request_engine::{
    CloudEngineState, CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine,
    LicensedProducts,
};
use fiftyone_fodid_cloud::FodIdCloudEngine;
use fiftyone_javascript_builder::{
    JavaScriptBuilderElement, JavaScriptBuilderElementBuilder, JAVASCRIPT_BUILDER_DATA_KEY,
};
use fiftyone_json_builder::JsonBuilderElement;
use fiftyone_pipeline_core::{
    Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, Pipeline,
    PipelineBuilder, PropertyMetaData, PropertyValueType, Result,
};
use fiftyone_pipeline_engines_fiftyone::SequenceElement;

/// A statement found only inside the user prompt section of the template.
const USER_PROMPT_MARKER: &str = "var answerKeys = [\"id.usage\", \"tcstring\"];";

/// An element that adds no data and reports the properties it is given under
/// the data key it is given, while `offering` is set, and none otherwise.
struct Offering {
    key: &'static str,
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
    offering: Arc<AtomicBool>,
}

impl Offering {
    /// An element under `key` always reporting `properties`.
    fn new(key: &'static str, properties: Vec<PropertyMetaData>) -> Self {
        Offering::switched(key, properties, Arc::new(AtomicBool::new(true)))
    }

    /// An element under `key` reporting `properties` while `offering` is set.
    fn switched(
        key: &'static str,
        properties: Vec<PropertyMetaData>,
        offering: Arc<AtomicBool>,
    ) -> Self {
        Offering {
            key,
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            properties,
            offering,
        }
    }
}

impl FlowElement for Offering {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
        Ok(())
    }

    fn data_key(&self) -> &str {
        self.key
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        if self.offering.load(Ordering::Relaxed) {
            self.properties.as_slice()
        } else {
            &[]
        }
    }
}

/// One available property under `key`.
fn available(key: &str) -> Vec<PropertyMetaData> {
    vec![PropertyMetaData::new(
        "IdProbGlobal",
        key,
        PropertyValueType::String,
    )]
}

/// One property under `key` that is not available.
fn unavailable(key: &str) -> Vec<PropertyMetaData> {
    vec![
        PropertyMetaData::new("IdProbGlobal", key, PropertyValueType::String).with_available(false),
    ]
}

/// A JavaScript builder with minification off and the https protocol, so a
/// request with a host gets a callback URL, then whatever `configure` sets.
fn javascript_builder(
    configure: impl FnOnce(JavaScriptBuilderElementBuilder) -> JavaScriptBuilderElementBuilder,
) -> Arc<JavaScriptBuilderElement> {
    Arc::new(
        configure(
            JavaScriptBuilderElement::builder()
                .set_minify(false)
                .set_protocol("https")
                .unwrap(),
        )
        .build(),
    )
}

/// A pipeline of the Sequence element, what `add` adds, the JSON builder and
/// `element`, with process errors recorded rather than returned.
fn pipeline(
    add: impl FnOnce(PipelineBuilder) -> PipelineBuilder,
    element: &Arc<JavaScriptBuilderElement>,
) -> Arc<Pipeline> {
    add(Pipeline::builder().add_element(Arc::new(SequenceElement::new())))
        .add_element(Arc::new(JsonBuilderElement::new()))
        .add_element(Arc::clone(element) as Arc<dyn FlowElement>)
        .suppress_process_exceptions(true)
        .build()
        .expect("pipeline builds")
}

/// The script one request with the host `host` gets through `pipeline`.
fn script_for(pipeline: &Arc<Pipeline>, host: &str) -> String {
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("header.host", host).build());
    let _ = data.process();
    data.get(JAVASCRIPT_BUILDER_DATA_KEY)
        .expect("javascript builder data present")
        .javascript()
        .to_owned()
}

/// Whether one request through `pipeline` gets the user prompt section.
fn prompts(pipeline: &Arc<Pipeline>) -> bool {
    script_for(pipeline, "example.com").contains(USER_PROMPT_MARKER)
}

/// Whether a default JavaScript builder renders the section beside the one
/// element `offering`.
fn prompts_beside(offering: Offering) -> bool {
    prompts(&pipeline(
        |b| b.add_element(Arc::new(offering)),
        &javascript_builder(|b| b),
    ))
}

#[test]
fn a_51did_element_offering_an_available_property_renders_the_section() {
    // The 51Did cloud engine's key, and the key as .NET writes it.
    for key in ["fodid", "FODid"] {
        assert!(prompts_beside(Offering::new(key, available(key))), "{key}");
    }
}

#[test]
fn a_51did_element_offering_nothing_available_leaves_the_section_out() {
    assert!(!prompts_beside(Offering::new(
        "fodid",
        unavailable("fodid")
    )));
    assert!(!prompts_beside(Offering::new("fodid", Vec::new())));
}

#[test]
fn an_element_under_another_key_leaves_the_section_out() {
    for key in ["device", "fodidx", "fod"] {
        assert!(!prompts_beside(Offering::new(key, available(key))), "{key}");
    }
    assert!(!prompts(&pipeline(|b| b, &javascript_builder(|b| b))));
}

#[test]
fn a_51did_element_in_a_parallel_group_renders_the_section() {
    let element = javascript_builder(|b| b);
    let pipeline = pipeline(
        |b| {
            b.add_parallel_elements([
                Arc::new(Offering::new("device", available("device"))) as Arc<dyn FlowElement>,
                Arc::new(Offering::new("fodid", available("fodid"))) as Arc<dyn FlowElement>,
            ])
        },
        &element,
    );
    assert!(prompts(&pipeline));
}

#[test]
fn the_section_needs_a_callback_url() {
    let pipeline = pipeline(
        |b| b.add_element(Arc::new(Offering::new("fodid", available("fodid")))),
        &javascript_builder(|b| b),
    );
    let script = script_for(&pipeline, "");
    assert!(script.contains("fiftyoneDegreesManager"));
    assert!(!script.contains(USER_PROMPT_MARKER));
}

#[test]
fn the_pipeline_is_read_for_each_script() {
    // Neither answer is kept, because the 51Did cloud engine's properties
    // change when its discovery completes.
    let offering = Arc::new(AtomicBool::new(true));
    let pipeline = pipeline(
        |b| {
            b.add_element(Arc::new(Offering::switched(
                "fodid",
                available("fodid"),
                Arc::clone(&offering),
            )))
        },
        &javascript_builder(|b| b),
    );
    assert!(prompts(&pipeline));
    offering.store(false, Ordering::Relaxed);
    assert!(!prompts(&pipeline));
    offering.store(true, Ordering::Relaxed);
    assert!(prompts(&pipeline));
}

#[test]
fn one_element_in_two_pipelines_reads_the_pipeline_of_each_request() {
    let element = javascript_builder(|b| b);
    let with_51did = pipeline(
        |b| b.add_element(Arc::new(Offering::new("fodid", available("fodid")))),
        &element,
    );
    let without = pipeline(|b| b, &element);
    assert!(prompts(&with_51did));
    assert!(!prompts(&without));
    assert!(prompts(&with_51did));
}

/// A cloud request engine whose licensed products are the supplied JSON. The
/// state is injected, so the build makes no discovery request, and the
/// transport refuses every call, so the test never reaches the network.
fn request_engine(accessible_properties: &str) -> Arc<CloudRequestEngine> {
    struct NoNetwork;
    impl CloudHttpClient for NoNetwork {
        fn send(
            &self,
            _request: &CloudHttpRequest,
        ) -> std::result::Result<CloudHttpResponse, String> {
            Err("no network in tests".to_owned())
        }
    }
    Arc::new(
        CloudRequestEngine::builder()
            .resource_key("test-resource-key")
            .http_client(Arc::new(NoNetwork))
            .set_state(CloudEngineState {
                evidence_keys: Vec::new(),
                accessible_properties: LicensedProducts::parse(accessible_properties)
                    .expect("valid accessible properties"),
            })
            .build()
            .expect("request engine builds"),
    )
}

/// Licensed products that include 51Did.
const WITH_51DID: &str = r#"{"Products":{"FODid":{"Properties":[
    {"Name":"IdProbGlobal","Type":"String"}]}}}"#;

/// Licensed products that do not include 51Did.
const WITHOUT_51DID: &str = r#"{"Products":{"device":{"Properties":[
    {"Name":"IsMobile","Type":"Bool"}]}}}"#;

/// A key without the 51Did product leaves the 51Did cloud engine no
/// properties, so the section is left out.
#[test]
fn the_51did_cloud_engine_renders_the_section_with_no_setting() {
    let prompts_with = |accessible_properties| {
        let engine = request_engine(accessible_properties);
        let fodid = FodIdCloudEngine::builder()
            .cloud_request_engine(Arc::clone(&engine))
            .build()
            .expect("51Did engine builds");
        let pipeline = pipeline(
            |b| {
                b.add_element(Arc::clone(&engine) as Arc<dyn FlowElement>)
                    .add_element(Arc::new(fodid))
            },
            &javascript_builder(|b| b),
        );
        prompts(&pipeline)
    };
    assert!(prompts_with(WITH_51DID));
    assert!(!prompts_with(WITHOUT_51DID));
}

#[test]
fn set_user_prompt_decides_whatever_the_pipeline_holds() {
    let on = javascript_builder(|b| b.set_user_prompt(true));
    assert!(prompts(&pipeline(|b| b, &on)));

    let off = javascript_builder(|b| b.set_user_prompt(false));
    assert!(!prompts(&pipeline(
        |b| b.add_element(Arc::new(Offering::new("fodid", available("fodid")))),
        &off,
    )));
}

#[test]
fn the_last_user_prompt_setting_decides() {
    let with = request_engine(WITH_51DID);
    let without = request_engine(WITHOUT_51DID);
    let prompts_alone =
        |element: Arc<JavaScriptBuilderElement>| prompts(&pipeline(|b| b, &element));

    assert!(!prompts_alone(javascript_builder(|b| {
        b.set_user_prompt(true).set_cloud_request_engine(&without)
    })));
    assert!(prompts_alone(javascript_builder(|b| {
        b.set_cloud_request_engine(&without).set_user_prompt(true)
    })));
    assert!(!prompts_alone(javascript_builder(|b| {
        b.set_shared_cloud_request_engine(Arc::clone(&with))
            .set_user_prompt(false)
    })));
    assert!(prompts_alone(javascript_builder(|b| {
        b.set_user_prompt(false)
            .set_shared_cloud_request_engine(Arc::clone(&with))
    })));
}
