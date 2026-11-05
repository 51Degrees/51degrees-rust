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

//! Integration tests for the JavaScript builder element.
//!
//! These build small pipelines, process a flow data and assert on the generated
//! JavaScript.

use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use fiftyone_cloud_request_engine::{
    CloudEngineState, CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine,
    LicensedProducts,
};
use fiftyone_javascript_builder::{JavaScriptBuilderElement, JAVASCRIPT_BUILDER_DATA_KEY};
use fiftyone_json_builder::JsonBuilderElement;
use fiftyone_pipeline_core::{
    ElementData, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    NoValueError, Pipeline, PropertyMetaData, PropertyValue, PropertyValueType, Result, TypedKey,
};
use fiftyone_pipeline_engines_fiftyone::SequenceElement;

// ---------------------------------------------------------------------------
// A minimal device-like element that exposes the Promise and Fetch properties
// the JavaScript builder consults, so its promise and fetch decisions can be
// driven through a real pipeline.
// ---------------------------------------------------------------------------

struct DeviceData {
    promise: Option<String>,
    fetch: Option<bool>,
}

impl ElementData for DeviceData {
    fn get(&self, name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        if name.eq_ignore_ascii_case("Promise") {
            if let Some(promise) = &self.promise {
                return Ok(PropertyValue::String(promise.clone()));
            }
        } else if name.eq_ignore_ascii_case("Fetch") {
            if let Some(fetch) = self.fetch {
                return Ok(PropertyValue::Bool(fetch));
            }
        }
        Err(NoValueError::new(format!(
            "No value for property '{name}'."
        )))
    }

    fn keys(&self) -> Vec<String> {
        let mut keys = Vec::new();
        if self.promise.is_some() {
            keys.push("Promise".to_owned());
        }
        if self.fetch.is_some() {
            keys.push("Fetch".to_owned());
        }
        keys
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

struct DeviceElement {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
    promise: Option<String>,
    fetch: Option<bool>,
}

impl DeviceElement {
    const KEY: TypedKey<DeviceData> = TypedKey::new("device");

    fn new(promise: Option<&str>, fetch: Option<bool>) -> Self {
        DeviceElement {
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            properties: vec![
                PropertyMetaData::new("Promise", "device", PropertyValueType::String),
                PropertyMetaData::new("Fetch", "device", PropertyValueType::Bool),
            ],
            promise: promise.map(|s| s.to_owned()),
            fetch,
        }
    }
}

impl FlowElement for DeviceElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        let promise = self.promise.clone();
        let fetch = self.fetch;
        data.get_or_add(Self::KEY, || DeviceData { promise, fetch })?;
        Ok(())
    }

    fn data_key(&self) -> &str {
        "device"
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

/// Build a pipeline: optional device element, then JSON builder, then the
/// JavaScript builder (configured via `configure`). Returns the generated
/// JavaScript for the supplied evidence.
fn run(
    device: Option<DeviceElement>,
    configure: impl FnOnce(
        fiftyone_javascript_builder::JavaScriptBuilderElementBuilder,
    ) -> JavaScriptBuilderElement,
    evidence: &[(&str, &str)],
) -> String {
    let mut builder = Pipeline::builder();
    if let Some(device) = device {
        builder = builder.add_element(Arc::new(device));
    }
    let js_element = configure(JavaScriptBuilderElement::builder());
    let pipeline = builder
        .add_element(Arc::new(JsonBuilderElement::new()))
        .add_element(Arc::new(js_element))
        .build()
        .expect("pipeline builds");

    let mut ev = Evidence::builder();
    for (key, value) in evidence {
        ev = ev.add(*key, *value);
    }
    let mut data = pipeline.create_flow_data_with(ev.build());
    data.process().expect("processing succeeds");

    data.get(JAVASCRIPT_BUILDER_DATA_KEY)
        .expect("javascript builder data present")
        .javascript()
        .to_owned()
}

// ---------------------------------------------------------------------------
// Rendered-output behavior.
// ---------------------------------------------------------------------------

#[test]
fn generates_manager_object_with_default_name() {
    let js = run(None, |b| b.build(), &[("header.host", "localhost")]);
    assert!(js.contains("fiftyoneDegreesManager"));
    // The default object name is `fod`.
    assert!(js.contains("fod"));
}

#[test]
fn object_name_can_be_overridden_by_evidence() {
    let js = run(
        None,
        |b| b.set_minify(false).build(),
        &[
            ("header.host", "localhost"),
            ("query.fod-js-object-name", "myCustomObj"),
        ],
    );
    assert!(js.contains("var myCustomObj = new fiftyoneDegreesManager();"));
}

#[test]
fn url_appears_when_host_present_and_update_enabled() {
    // With minify off the URL is easy to find verbatim.
    let js = run(
        None,
        |b| b.set_minify(false).set_protocol("https").unwrap().build(),
        &[("header.host", "example.com")],
    );
    assert!(js.contains("https://example.com/51dpipeline/json"));
    // The update-enabled section (processRequest) is present.
    assert!(js.contains("processRequest"));
}

#[test]
fn no_url_means_no_update_section() {
    // No host anywhere, so no callback URL and the update mechanism is absent.
    let js = run(None, |b| b.set_minify(false).set_host("").build(), &[]);
    assert!(!js.contains("processRequest"));
}

#[test]
fn promise_full_enables_promise_path() {
    let device = DeviceElement::new(Some("Full"), Some(false));
    let js = run(
        Some(device),
        |b| b.set_minify(false).build(),
        &[("header.host", "localhost")],
    );
    // The promise section creates `this.promise = new Promise(...)`.
    assert!(js.contains("this.promise = new Promise"));
}

#[test]
fn promise_not_full_disables_promise_path() {
    let device = DeviceElement::new(Some("Partial"), Some(false));
    let js = run(
        Some(device),
        |b| b.set_minify(false).build(),
        &[("header.host", "localhost")],
    );
    assert!(!js.contains("this.promise = new Promise"));
    // The non-promise fallback calls process(...) directly.
    assert!(js.contains("process(function(json) {}, catchError);"));
}

#[test]
fn fetch_true_uses_fetch_api() {
    let device = DeviceElement::new(Some("Full"), Some(true));
    let js = run(
        Some(device),
        |b| b.set_minify(false).set_protocol("https").unwrap().build(),
        &[("header.host", "example.com")],
    );
    // The fetch path calls fetch(...) rather than building an XHR.
    assert!(js.contains("fetch('https://example.com/51dpipeline/json'"));
    assert!(!js.contains("createCORSRequest"));
}

#[test]
fn fetch_false_uses_xml_http_request() {
    let device = DeviceElement::new(Some("Full"), Some(false));
    let js = run(
        Some(device),
        |b| b.set_minify(false).set_protocol("https").unwrap().build(),
        &[("header.host", "example.com")],
    );
    assert!(js.contains("createCORSRequest"));
}

#[test]
fn minification_is_robust_on_the_real_template() {
    // The bundled minifier (minify-js 0.6.0) panics on some constructs in the
    // full template. The element must catch that and fall back to the
    // unminified content rather than aborting the request. So the minified-on
    // run must still produce usable output
    // (and never crash). It is no larger than the unminified output.
    let minified = run(
        None,
        |b| b.set_minify(true).build(),
        &[("header.host", "h")],
    );
    let plain = run(
        None,
        |b| b.set_minify(false).build(),
        &[("header.host", "h")],
    );
    assert!(minified.contains("fiftyoneDegreesManager"));
    assert!(minified.len() <= plain.len());
}

#[test]
fn full_template_renders_through_pipeline() {
    // Render the complete embedded template through a real pipeline, the same
    // path the element.rs doctest exercises. This is the render that previously
    // crashed under the third-party engine; it must now produce stable, complete
    // output every time.
    let js = run(
        Some(DeviceElement::new(Some("Full"), Some(true))),
        |b| b.set_minify(false).set_protocol("https").unwrap().build(),
        &[
            ("header.host", "example.com"),
            ("query.sequence", "1"),
            ("query.session-id", "abc"),
            ("query.user-agent", "test agent"),
        ],
    );

    // The manager object, its construction and the fetch-based callback path are
    // all present, proving the variables and the nested update/fetch sections
    // rendered.
    assert!(js.contains("fiftyoneDegreesManager = function()"));
    assert!(js.contains("var fod = new fiftyoneDegreesManager();"));
    assert!(js.contains("fetch('https://example.com/51dpipeline/json'"));
    assert!(js.contains("var sequence = 1;"));
    // No unrendered Mustache tags survive in the output.
    assert!(!js.contains("{{"));
    assert!(!js.contains("}}"));
    // The output is trimmed at the end, matching the previous engine.
    assert_eq!(js, js.trim_end());
    assert!(js.ends_with("var fod = new fiftyoneDegreesManager();"));
}

/// A cloud request engine whose accessible products are the supplied JSON. The
/// state is injected, so the build makes no discovery request, and the stub
/// transport refuses every call, so the test never reaches the network.
fn request_engine(accessible_properties: &str) -> CloudRequestEngine {
    struct NoNetwork;
    impl CloudHttpClient for NoNetwork {
        fn send(
            &self,
            _request: &CloudHttpRequest,
        ) -> std::result::Result<CloudHttpResponse, String> {
            Err("no network in tests".to_owned())
        }
    }
    CloudRequestEngine::builder()
        .resource_key("test-resource-key")
        .http_client(Arc::new(NoNetwork))
        .set_state(CloudEngineState {
            evidence_keys: Vec::new(),
            accessible_properties: LicensedProducts::parse(accessible_properties)
                .expect("valid accessible properties"),
        })
        .build()
        .expect("request engine builds")
}

/// Render the full template for a JavaScript builder configured from the
/// supplied request engine, with a host so the update section is present.
fn render_for_key(engine: &CloudRequestEngine) -> String {
    run(
        None,
        |b| {
            b.set_minify(false)
                .set_protocol("https")
                .unwrap()
                .set_cloud_request_engine(engine)
                .build()
        },
        &[("header.host", "example.com")],
    )
}

/// A statement found only inside the user prompt section of the template.
const USER_PROMPT_MARKER: &str = "var answerKeys = [\"id.usage\", \"tcstring\"];";

#[test]
fn user_prompt_section_follows_the_keys_licensed_products() {
    // The product name is written the way the cloud service reports it,
    // "FODid", which differs in case from the "fodid" element data key.
    let entitled = request_engine(
        r#"{"Products":{
            "device":{"Properties":[{"Name":"IsMobile","Type":"Bool"}]},
            "FODid":{"Properties":[{"Name":"IdProbGlobal","Type":"String"}]}
        }}"#,
    );
    let js = render_for_key(&entitled);
    assert!(
        js.contains(USER_PROMPT_MARKER),
        "a key licensed for 51Did renders the user prompt section"
    );
    assert!(!js.contains("{{"));
    assert!(!js.contains("}}"));
    assert_eq!(js, js.trim_end());
    assert!(js.ends_with("var fod = new fiftyoneDegreesManager();"));

    // A key with no 51Did product, and a key whose 51Did product grants no
    // property, both leave the section out.
    for unentitled in [
        r#"{"Products":{"device":{"Properties":[{"Name":"IsMobile","Type":"Bool"}]}}}"#,
        r#"{"Products":{"FODid":{"Properties":[]}}}"#,
    ] {
        let js = render_for_key(&request_engine(unentitled));
        assert!(
            !js.contains(USER_PROMPT_MARKER),
            "a key without 51Did leaves the user prompt section out: {unentitled}"
        );
        assert!(!js.contains("{{"));
        assert!(!js.contains("}}"));
        assert!(js.ends_with("var fod = new fiftyoneDegreesManager();"));
    }

    // A builder given no request engine has no licensed products to read, so
    // the section is left out as well.
    let js = run(
        None,
        |b| b.set_minify(false).build(),
        &[("header.host", "example.com")],
    );
    assert!(!js.contains(USER_PROMPT_MARKER));
}

/// A cloud that refuses every request while `down` is set, and once it
/// answers grants 51Did and answers each data request with an empty device.
struct RecoveringCloud {
    down: Arc<AtomicBool>,
}

impl CloudHttpClient for RecoveringCloud {
    fn send(&self, request: &CloudHttpRequest) -> std::result::Result<CloudHttpResponse, String> {
        if self.down.load(Ordering::Relaxed) {
            return Err("connection refused".to_owned());
        }
        let body = if request.url.contains("accessibleproperties") {
            r#"{"Products":{"FODid":{"Properties":[{"Name":"IdProbGlobal","Type":"String"}]}}}"#
        } else if request.url.contains("evidencekeys") {
            r#"["header.user-agent"]"#
        } else {
            r#"{"device":{}}"#
        };
        Ok(CloudHttpResponse {
            status: 200,
            body: body.to_owned(),
            retry_after: None,
        })
    }
}

/// A pipeline of the request engine, the JSON builder and `element`.
fn cloud_pipeline(
    engine: &Arc<CloudRequestEngine>,
    element: JavaScriptBuilderElement,
) -> Arc<Pipeline> {
    Pipeline::builder()
        .add_element(Arc::clone(engine) as Arc<dyn FlowElement>)
        .add_element(Arc::new(JsonBuilderElement::new()))
        .add_element(Arc::new(element))
        .suppress_process_exceptions(true)
        .build()
        .expect("pipeline builds")
}

/// The script one request through `pipeline` gets, whatever the request
/// engine made of it.
fn render_through(pipeline: &Arc<Pipeline>) -> String {
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.host", "example.com")
            .build(),
    );
    let _ = data.process();
    data.get(JAVASCRIPT_BUILDER_DATA_KEY)
        .expect("javascript builder data present")
        .javascript()
        .to_owned()
}

#[test]
fn user_prompt_section_waits_for_discovery_on_a_shared_engine() {
    let down = Arc::new(AtomicBool::new(true));
    let engine = Arc::new(
        CloudRequestEngine::builder()
            .resource_key("test-resource-key")
            .http_client(Arc::new(RecoveringCloud {
                down: Arc::clone(&down),
            }))
            .recovery_seconds(0.0)
            .build()
            .expect("a transient failure does not fail the build"),
    );
    let configure = || {
        JavaScriptBuilderElement::builder()
            .set_minify(false)
            .set_protocol("https")
            .unwrap()
    };
    let shared = cloud_pipeline(
        &engine,
        configure()
            .set_shared_cloud_request_engine(Arc::clone(&engine))
            .build(),
    );
    let decided = cloud_pipeline(
        &engine,
        configure().set_cloud_request_engine(&engine).build(),
    );

    for pipeline in [&shared, &decided] {
        assert!(
            !render_through(pipeline).contains(USER_PROMPT_MARKER),
            "left out while discovery is pending"
        );
    }

    down.store(false, Ordering::Relaxed);
    assert!(
        render_through(&shared).contains(USER_PROMPT_MARKER),
        "the shared engine's element renders it once discovery completes"
    );
    assert!(engine.has_loaded_metadata());
    assert!(
        !render_through(&decided).contains(USER_PROMPT_MARKER),
        "the reference form keeps the answer it was built with"
    );
    down.store(true, Ordering::Relaxed);
    assert!(
        render_through(&shared).contains(USER_PROMPT_MARKER),
        "and keeps rendering it once it is settled"
    );
}

#[test]
fn fodid_product_name_matches_the_51did_engine_data_key() {
    assert!(fiftyone_javascript_builder::FODID_PRODUCT_NAME
        .eq_ignore_ascii_case(fiftyone_fodid_cloud::FODID_ELEMENT_DATA_KEY));
}

#[test]
fn full_template_render_is_deterministic_under_repetition() {
    // Rendering the full template repeatedly must always yield identical bytes.
    // This guards against any nondeterminism in the renderer.
    let render = || {
        run(
            None,
            |b| b.set_minify(false).build(),
            &[("header.host", "localhost"), ("query.sequence", "1")],
        )
    };
    let first = render();
    for _ in 0..200 {
        assert_eq!(render(), first);
    }
}

#[test]
fn builder_rejects_invalid_protocol() {
    let result = JavaScriptBuilderElement::builder().set_protocol("ftp");
    assert!(result.is_err());
}

#[test]
fn builder_rejects_invalid_object_name() {
    assert!(JavaScriptBuilderElement::builder()
        .set_object_name("1bad")
        .is_err());
    assert!(JavaScriptBuilderElement::builder()
        .set_object_name("has space")
        .is_err());
    // A valid identifier is accepted.
    assert!(JavaScriptBuilderElement::builder()
        .set_object_name("_ok$Name1")
        .is_ok());
}

// ---------------------------------------------------------------------------
// Session id and sequence, which the specification takes from the sequence
// element's data rather than from evidence.
// ---------------------------------------------------------------------------

/// Run a pipeline that starts with the sequence element, as the web pipeline
/// does, and return the generated JavaScript.
fn run_with_sequence(evidence: &[(&str, &str)]) -> String {
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(SequenceElement::new()))
        .add_element(Arc::new(JsonBuilderElement::new()))
        .add_element(Arc::new(
            JavaScriptBuilderElement::builder()
                .set_minify(false)
                .build(),
        ))
        .build()
        .expect("pipeline builds");

    let mut ev = Evidence::builder();
    for (key, value) in evidence {
        ev = ev.add(*key, *value);
    }
    let mut data = pipeline.create_flow_data_with(ev.build());
    data.process().expect("processing succeeds");

    data.get(JAVASCRIPT_BUILDER_DATA_KEY)
        .expect("javascript builder data present")
        .javascript()
        .to_owned()
}

/// The value the template assigned to `var sessionId`.
fn rendered_session_id(js: &str) -> String {
    let start = js
        .find("var sessionId = \"")
        .expect("the template declares sessionId")
        + "var sessionId = \"".len();
    let end = js[start..].find('"').expect("sessionId is a closed string");
    js[start..start + end].to_owned()
}

#[test]
fn first_request_carries_the_session_id_the_sequence_element_created() {
    // A first page load has no session-id evidence. The sequence element
    // creates one, and the include must carry it, or fod.sessionId is empty
    // and the browser cannot tell one include from the next.
    let first = rendered_session_id(&run_with_sequence(&[("header.host", "localhost")]));
    let second = rendered_session_id(&run_with_sequence(&[("header.host", "localhost")]));
    assert_eq!(first.len(), 36, "a GUID session id, got '{first}'");
    assert_ne!(first, second, "each first request gets its own session id");
}

#[test]
fn supplied_session_id_is_kept_and_the_sequence_moves_on() {
    // With session-id and sequence evidence the sequence element keeps the id
    // and adds one to the sequence, and the include shows both.
    let js = run_with_sequence(&[
        ("header.host", "localhost"),
        ("query.session-id", "abc"),
        ("query.sequence", "1"),
    ]);
    assert_eq!(rendered_session_id(&js), "abc");
    assert!(js.contains("var sequence = 2;"), "the sequence is one more");
}

#[test]
fn without_a_sequence_element_the_evidence_is_used() {
    // A pipeline with no sequence element still reads the evidence directly.
    let js = run(
        None,
        |b| b.set_minify(false).build(),
        &[
            ("header.host", "localhost"),
            ("query.session-id", "abc"),
            ("query.sequence", "4"),
        ],
    );
    assert_eq!(rendered_session_id(&js), "abc");
    assert!(js.contains("var sequence = 4;"));
}

#[test]
fn the_json_builder_leaves_out_this_builders_data() {
    // The JSON builder names this builder's data key in its exclusion list
    // without depending on this crate, so the two keys are checked here.
    assert!(fiftyone_json_builder::DEFAULT_ELEMENT_EXCLUSION_LIST
        .contains(&fiftyone_javascript_builder::JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY));
}

/// The user agent that makes [`CloudLikeDevice`] fail, as a device engine
/// does when its cloud request failed.
const FAILING_REQUEST: &str = "the cloud request for this one failed";

/// A device element that behaves like the cloud device engine: it adds device
/// data carrying full promise and fetch support, except on a request whose
/// cloud call failed, where it records the failure and adds no device data.
struct CloudLikeDevice {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

impl CloudLikeDevice {
    fn new() -> Self {
        CloudLikeDevice {
            filter: EvidenceKeyFilterWhitelist::new(["header.user-agent"]),
            properties: vec![
                PropertyMetaData::new("Promise", "device", PropertyValueType::String),
                PropertyMetaData::new("Fetch", "device", PropertyValueType::Bool),
            ],
        }
    }
}

impl FlowElement for CloudLikeDevice {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        if data.evidence().get("header.user-agent") == Some(FAILING_REQUEST) {
            return Err(fiftyone_pipeline_core::Error::CloudRequest {
                status_code: 0,
                retry_after_seconds: None,
                message: "the request timed out".to_owned(),
            });
        }
        data.get_or_add(DeviceElement::KEY, || DeviceData {
            promise: Some("Full".to_owned()),
            fetch: Some(true),
        })?;
        Ok(())
    }

    fn data_key(&self) -> &str {
        "device"
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

#[test]
fn a_request_without_device_data_leaves_later_visitors_unaffected() {
    // One builder serves every visitor. Whether a browser supports promises
    // and fetch belongs to the request, so a request that got no device answer
    // must not change the script the next visitor is given.
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(CloudLikeDevice::new()))
        .add_element(Arc::new(JsonBuilderElement::new()))
        .add_element(Arc::new(
            JavaScriptBuilderElement::builder()
                .set_minify(false)
                .set_protocol("https")
                .unwrap()
                .build(),
        ))
        .suppress_process_exceptions(true)
        .build()
        .expect("pipeline builds");
    let script = |user_agent: &str| {
        let mut data = pipeline.create_flow_data_with(
            Evidence::builder()
                .add("header.host", "example.com")
                .add("header.user-agent", user_agent)
                .build(),
        );
        data.process().expect("errors are suppressed");
        data.get(JAVASCRIPT_BUILDER_DATA_KEY)
            .expect("javascript builder data present")
            .javascript()
            .to_owned()
    };

    let failed = script(FAILING_REQUEST);
    assert!(
        !failed.contains("this.promise = new Promise"),
        "with no device answer the script claims no promise support"
    );

    let next = script("Mozilla/5.0");
    assert!(
        next.contains("this.promise = new Promise"),
        "the next visitor's browser supports promises, but the script was \
         rendered without them"
    );
    assert!(
        next.contains("fetch('https://example.com/51dpipeline/json'"),
        "the next visitor's browser supports fetch, but the script was \
         rendered without it"
    );
}
