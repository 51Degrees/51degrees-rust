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

//! The Device Detection and hardware profile cloud engines made from a
//! configuration, compared with what .NET's FiftyOne.DeviceDetection.Cloud
//! 4.5.180 on pipeline 4.5.129 made from the same configuration against a
//! local stand-in for the cloud service. Each expected answer is the one
//! .NET gave, and each message is the one .NET gave, apart from the
//! differences the tests name.

use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, ThreadId};

use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine, HttpMethod,
};
use fiftyone_device_detection_cloud::{
    configuration, DeviceData, DeviceDetectionCloudEngine, HardwareProfileCloudEngine,
    DEVICE_DATA_KEY, HARDWARE_DATA_KEY,
};
use fiftyone_pipeline_core::configuration::{BuilderRegistry, ElementOptions, PipelineOptions};
use fiftyone_pipeline_core::{Error, Evidence, FlowData, Pipeline, PipelineBuilder, Result};
use fiftyone_pipeline_engines::{AspectData, AspectEngine};

const REQUEST_ENGINE: &str = "CloudRequestEngineBuilder";
const RESOURCE_KEY: &str = "AQkey";
/// The resource key the stand-in grants no product.
const KEY_WITHOUT_PRODUCTS: &str = "nodevice";
const NO_SUB_ELEMENTS: &str = "No SubElements";
const NO_INNER: &str = "No Inner Exception.";
const DESKTOP: &[(&str, &str)] = &[("header.user-agent", "Desktop Chrome")];
const MOBILE: &[(&str, &str)] = &[("header.user-agent", "iPhone Mobile")];
const TAC: &[(&str, &str)] = &[("query.tac", "35925406")];

/// The products the stand-in grants, as .NET's run had them.
const PROPERTIES: &str = r#"{"Products":{
    "device":{"DataTier":"CloudV4TAC","Properties":[
        {"Name":"IsMobile","Type":"Boolean","Category":"Device"},
        {"Name":"BrowserName","Type":"String","Category":"Browser"}]},
    "hardware":{"DataTier":"CloudV4TAC","Properties":[
        {"Name":"Profiles","Type":"Array","Category":"Unspecified","ItemProperties":[
            {"Name":"HardwareVendor","Type":"String","Category":"Device"},
            {"Name":"IsMobile","Type":"Boolean","Category":"Device"}]}]}}}"#;

/// One of the two engines, with the names .NET gives it.
struct Engine {
    builder: &'static str,
    name: &'static str,
    builder_type: &'static str,
    data_key: &'static str,
}

const DEVICE: Engine = Engine {
    builder: "DeviceDetectionCloudEngineBuilder",
    name: "DeviceDetectionCloudEngine",
    builder_type: "fiftyone_device_detection_cloud::engine::DeviceDetectionCloudEngineBuilder",
    data_key: "device",
};

const HARDWARE: Engine = Engine {
    builder: "HardwareProfileCloudEngineBuilder",
    name: "HardwareProfileCloudEngine",
    builder_type:
        "fiftyone_device_detection_cloud::hardware_engine::HardwareProfileCloudEngineBuilder",
    data_key: "hardware",
};

/// Answers for the cloud service, as .NET's run had them. A user agent
/// holding "Mobile" is a mobile Safari, and any other a desktop Chrome.
#[derive(Default)]
struct StandIn {
    requests: Mutex<Vec<CloudHttpRequest>>,
}

impl CloudHttpClient for StandIn {
    fn send(&self, request: &CloudHttpRequest) -> std::result::Result<CloudHttpResponse, String> {
        self.requests.lock().unwrap().push(request.clone());
        let body = if request.url.contains("evidencekeys") {
            r#"["header.user-agent","query.user-agent","query.tac"]"#.to_string()
        } else if request.url.contains("accessibleproperties") {
            if request
                .url
                .contains(&format!("resource={KEY_WITHOUT_PRODUCTS}"))
            {
                r#"{"Products":{}}"#.to_string()
            } else {
                PROPERTIES.to_string()
            }
        } else {
            let mobile = request.form.iter().any(|(_, v)| v.contains("Mobile"));
            let browser = if mobile { "Mobile Safari" } else { "Chrome" };
            format!(
                r#"{{"device":{{"ismobile":{mobile},"browsername":"{browser}"}},
                "hardware":{{"profiles":[{{"hardwarevendor":"Apple","ismobile":true}},
                {{"hardwarevendor":"Samsung","ismobile":{mobile}}}]}}}}"#
            )
        };
        Ok(CloudHttpResponse {
            status: 200,
            body,
            retry_after: None,
        })
    }
}

impl StandIn {
    /// The data requests, being each POST.
    fn data_requests(&self) -> usize {
        let requests = self.requests.lock().unwrap();
        requests
            .iter()
            .filter(|r| r.method == HttpMethod::Post)
            .count()
    }
}

fn registry() -> BuilderRegistry {
    let mut registry = BuilderRegistry::new();
    fiftyone_cloud_request_engine::configuration::add_builders(&mut registry);
    configuration::add_builders(&mut registry);
    registry
}

fn request_engine() -> ElementOptions {
    ElementOptions::new(REQUEST_ENGINE).parameter("ResourceKey", RESOURCE_KEY)
}

/// The pipeline `elements` make, with the stand-in given as the client.
fn build(elements: Vec<ElementOptions>) -> (Result<Arc<Pipeline>>, Arc<StandIn>) {
    let options = elements
        .into_iter()
        .fold(PipelineOptions::new(), PipelineOptions::element);
    build_options(&options)
}

fn build_options(options: &PipelineOptions) -> (Result<Arc<Pipeline>>, Arc<StandIn>) {
    let stand_in = Arc::new(StandIn::default());
    let client: Arc<dyn CloudHttpClient> = stand_in.clone();
    let result = PipelineBuilder::new()
        .add_resource(Arc::new(client))
        .build_from_configuration(&registry(), options);
    (result, stand_in)
}

/// The cloud request engine, then `engine` with `parameters`.
fn after_request_engine(engine: &Engine, parameters: &[(&str, &str)]) -> Result<Arc<Pipeline>> {
    let element = parameters
        .iter()
        .fold(ElementOptions::new(engine.builder), |element, (n, v)| {
            element.parameter(*n, *v)
        });
    build(vec![request_engine(), element]).0
}

fn message(result: Result<Arc<Pipeline>>) -> String {
    match result {
        Err(Error::PipelineConfiguration { message }) => message,
        Err(other) => panic!("not a configuration error: {other}"),
        Ok(_) => panic!("the pipeline was built"),
    }
}

/// The message .NET gives for the element `builder` that cannot be made.
fn failed(builder: &str, message: &str) -> String {
    format!(
        "Configuration failed to build.\nBuilderName: {builder}\nSubElement Builder Names: \
         {NO_SUB_ELEMENTS}\n{message}\n{NO_INNER}\n"
    )
}

fn process(pipeline: &Arc<Pipeline>, evidence: &[(&str, &str)]) -> FlowData {
    let evidence = evidence
        .iter()
        .fold(Evidence::builder(), |builder, (k, v)| builder.add(*k, *v))
        .build();
    let mut data = pipeline.create_flow_data_with(evidence);
    data.process().expect("processing");
    data
}

/// IsMobile and BrowserName, and whether the device data came from a
/// cache.
fn device(data: &FlowData) -> (bool, String, bool) {
    let device = data.get(DEVICE_DATA_KEY).expect("the device data");
    (
        *device.is_mobile().value().expect("IsMobile"),
        device.browser_name().value().expect("BrowserName").clone(),
        device.cache_hit(),
    )
}

/// Each profile's HardwareVendor and IsMobile.
fn hardware(data: &FlowData) -> Vec<(String, bool)> {
    data.get(HARDWARE_DATA_KEY)
        .expect("the hardware data")
        .profiles()
        .iter()
        .map(|profile| {
            (
                profile
                    .hardware_vendor()
                    .value()
                    .expect("HardwareVendor")
                    .clone(),
                *profile.is_mobile().value().expect("IsMobile"),
            )
        })
        .collect()
}

fn keys(pipeline: &Pipeline) -> Vec<&str> {
    pipeline
        .flow_elements()
        .iter()
        .map(|e| e.data_key())
        .collect()
}

/// The engine of `engine`'s kind that `pipeline` holds.
fn made(pipeline: &Pipeline, engine: &Engine) -> Arc<dyn AspectEngine> {
    if engine.data_key == "device" {
        pipeline
            .resource::<DeviceDetectionCloudEngine>()
            .expect("the Device Detection cloud engine")
    } else {
        pipeline
            .resource::<HardwareProfileCloudEngine>()
            .expect("the hardware profile cloud engine")
    }
}

fn property_names(engine: &dyn AspectEngine) -> Vec<&str> {
    engine
        .aspect_properties()
        .iter()
        .map(|p| p.name())
        .collect()
}

struct CapturingLogger {
    entries: Mutex<Vec<(ThreadId, log::Level, String)>>,
}

impl log::Log for CapturingLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            self.entries.lock().unwrap().push((
                thread::current().id(),
                record.level(),
                record.args().to_string(),
            ));
        }
    }

    fn flush(&self) {}
}

fn logger() -> &'static CapturingLogger {
    static LOGGER: OnceLock<&'static CapturingLogger> = OnceLock::new();
    LOGGER.get_or_init(|| {
        let logger: &'static CapturingLogger = Box::leak(Box::new(CapturingLogger {
            entries: Mutex::new(Vec::new()),
        }));
        log::set_logger(logger).expect("no other logger is installed in this binary");
        log::set_max_level(log::LevelFilter::Warn);
        logger
    })
}

/// The warnings and errors logged on the current thread while `run` runs.
/// The tests in this binary run in parallel, so each reads only its own.
fn logged_by<T>(run: impl FnOnce() -> T) -> (T, Vec<(log::Level, String)>) {
    let me = thread::current().id();
    let mine = || {
        logger()
            .entries
            .lock()
            .unwrap()
            .iter()
            .filter(|(id, _, _)| *id == me)
            .map(|(_, level, message)| (*level, message.clone()))
            .collect::<Vec<_>>()
    };
    let before = mine().len();
    let result = run();
    (result, mine().split_off(before))
}

fn ignored(engine: &Engine, setting: &str) -> (log::Level, String) {
    (
        log::Level::Warn,
        format!(
            "The {} setting '{setting}' has no effect here and is ignored.",
            engine.name
        ),
    )
}

fn properties_ignored(engine: &Engine, setting: &str) -> (log::Level, String) {
    (
        log::Level::Warn,
        format!(
            "The {} setting '{setting}' has no effect, because the properties the cloud returns \
             are those the cloud request engine asks for.",
            engine.name
        ),
    )
}

#[test]
fn the_builders_are_found_by_dotnets_names() {
    for engine in [&DEVICE, &HARDWARE] {
        let lower = engine.builder.to_lowercase();
        for name in [engine.builder, engine.name, lower.as_str()] {
            let (pipeline, _) = build(vec![request_engine(), ElementOptions::new(name)]);
            let pipeline = pipeline.expect(name);
            assert_eq!(
                keys(&pipeline),
                ["cloud-response", engine.data_key],
                "{name}"
            );
        }
    }
}

/// .NET's run "dd and hw", where both engines took the properties the key
/// grants and read each response.
#[test]
fn the_engines_read_the_request_engine_made_before_them() {
    let (pipeline, stand_in) = build(vec![
        request_engine(),
        ElementOptions::new(DEVICE.name),
        ElementOptions::new(HARDWARE.name),
    ]);
    let pipeline = pipeline.expect("the pipeline");
    assert_eq!(keys(&pipeline), ["cloud-response", "device", "hardware"]);

    let request = pipeline
        .resource::<CloudRequestEngine>()
        .expect("the request engine");
    let device_engine = pipeline
        .resource::<DeviceDetectionCloudEngine>()
        .expect("the Device Detection cloud engine");
    let hardware_engine = pipeline
        .resource::<HardwareProfileCloudEngine>()
        .expect("the hardware profile cloud engine");
    assert!(Arc::ptr_eq(device_engine.request_engine(), &request));
    assert!(Arc::ptr_eq(hardware_engine.request_engine(), &request));
    assert_eq!(
        property_names(device_engine.as_ref()),
        ["IsMobile", "BrowserName"]
    );
    assert_eq!(device_engine.data_source_tier(), "CloudV4TAC");
    assert_eq!(property_names(hardware_engine.as_ref()), ["Profiles"]);
    assert_eq!(hardware_engine.data_source_tier(), "CloudV4TAC");

    let mobile = process(&pipeline, MOBILE);
    assert_eq!(device(&mobile), (true, "Mobile Safari".to_string(), false));
    assert_eq!(
        hardware(&mobile),
        [("Apple".to_string(), true), ("Samsung".to_string(), true)]
    );
    let tac = process(&pipeline, TAC);
    assert_eq!(device(&tac), (false, "Chrome".to_string(), false));
    assert_eq!(
        hardware(&tac),
        [("Apple".to_string(), true), ("Samsung".to_string(), false)]
    );
    assert_eq!(stand_in.data_requests(), 2);
}

/// .NET gives this message too. For an engine with no request engine at
/// all it gives it on its own, from the pipeline's constructor once every
/// element is made, and an engine placed before the request engine is
/// built and fails each process with it. Here the element that cannot be
/// made fails the build, naming the builder.
#[test]
fn without_a_request_engine_before_it_the_build_fails() {
    for engine in [&DEVICE, &HARDWARE] {
        let expected = failed(
            engine.builder,
            &format!(
                "The '{}' requires a 'CloudRequestEngine' before it in the Pipeline. This engine \
                 will be unable to produce results until this is corrected.",
                engine.name
            ),
        );
        let alone = build(vec![ElementOptions::new(engine.builder)]).0;
        assert_eq!(message(alone), expected, "{} alone", engine.name);
        let before = build(vec![ElementOptions::new(engine.builder), request_engine()]).0;
        assert_eq!(message(before), expected, "{} before", engine.name);
    }
}

/// Without the `reqwest-client` feature, as on wasm, a pipeline builder
/// given no client, or a client without the further `Arc`, fails the build
/// with a message saying how to give one.
#[cfg(not(feature = "reqwest-client"))]
#[test]
fn with_no_client_the_build_says_how_to_give_one() {
    let options = PipelineOptions::new()
        .element(request_engine())
        .element(ElementOptions::new(DEVICE.builder));
    let expected = failed(
        REQUEST_ENGINE,
        "no CloudHttpClient was given to PipelineBuilder::add_resource and the \
         `reqwest-client` feature is not enabled, so the CloudRequestEngine has no HTTP \
         transport. Give one with add_resource(Arc::new(client)), where client is an \
         Arc<dyn CloudHttpClient>, or enable the `reqwest-client` feature to use the \
         built-in reqwest client",
    );
    let none = PipelineBuilder::new().build_from_configuration(&registry(), &options);
    assert_eq!(message(none), expected);
    let unwrapped = PipelineBuilder::new()
        .add_resource(Arc::new(StandIn::default()))
        .build_from_configuration(&registry(), &options);
    assert_eq!(message(unwrapped), expected);
}

/// .NET's engine finds its request engine in the pipeline by type, which
/// finds none when there are two, so .NET's run with two request engines
/// before the Device Detection cloud engine failed with this message, on
/// its own. Here the element that cannot be made fails the build.
#[test]
fn with_two_request_engines_before_it_the_build_fails() {
    for engine in [&DEVICE, &HARDWARE] {
        let second =
            ElementOptions::new(REQUEST_ENGINE).parameter("ResourceKey", KEY_WITHOUT_PRODUCTS);
        assert_eq!(
            message(
                build(vec![
                    request_engine(),
                    second,
                    ElementOptions::new(engine.name)
                ])
                .0
            ),
            failed(
                engine.name,
                &format!(
                    "The '{}' requires a 'CloudRequestEngine' before it in the Pipeline. This \
                     engine will be unable to produce results until this is corrected.",
                    engine.name
                ),
            ),
            "{}",
            engine.name
        );
    }
}

/// The engines here have no lazy loading and no cache, and in both .NET
/// and here an engine's properties have no effect, so each of these is
/// read, checked as .NET checks it, and logged, and the answers are those
/// with no setting. .NET's answers for these were the same, apart from
/// the cache sizes, which the next test covers.
#[test]
fn settings_with_no_effect_here_are_read_and_logged() {
    for engine in [&DEVICE, &HARDWARE] {
        let cases = [
            (
                "LazyLoadingTimeout",
                "100",
                Some(ignored(engine, "SetLazyLoadingTimeout")),
            ),
            ("CacheSize", "10", Some(ignored(engine, "SetCacheSize"))),
            ("CacheSize", "0", Some(ignored(engine, "SetCacheSize"))),
            (
                "CacheHitOrMiss",
                "true",
                Some(ignored(engine, "SetCacheHitOrMiss")),
            ),
            ("CacheHitOrMiss", "false", None),
            (
                "Property",
                "IsMobile",
                Some(properties_ignored(engine, "SetProperty")),
            ),
            (
                "Properties",
                "IsMobile,BrowserName",
                Some(properties_ignored(engine, "SetProperties")),
            ),
        ];
        for (name, value, expected) in cases {
            let (pipeline, logged) = logged_by(|| after_request_engine(engine, &[(name, value)]));
            let pipeline = pipeline.unwrap_or_else(|e| panic!("{} {name}: {e}", engine.name));
            assert_eq!(
                logged,
                expected.into_iter().collect::<Vec<_>>(),
                "{} {name} {value}",
                engine.name
            );
            assert_eq!(
                property_names(&*made(&pipeline, engine)),
                property_names(&*made(&after_request_engine(engine, &[]).unwrap(), engine)),
                "{} {name}",
                engine.name
            );
            if engine.data_key == "device" {
                let desktop = process(&pipeline, DESKTOP);
                let mobile = process(&pipeline, MOBILE);
                assert_eq!(device(&desktop), (false, "Chrome".to_string(), false));
                assert_eq!(device(&mobile), (true, "Mobile Safari".to_string(), false));
            } else {
                let tac = process(&pipeline, TAC);
                assert_eq!(
                    hardware(&tac),
                    [("Apple".to_string(), true), ("Samsung".to_string(), false)]
                );
            }
        }

        // A list given in code is read as SetProperties.
        let element = ElementOptions::new(engine.builder)
            .parameter("Properties", vec!["IsMobile".to_string()]);
        let (pipeline, logged) = logged_by(|| build(vec![request_engine(), element]).0);
        pipeline.expect("a list of properties");
        assert_eq!(logged, [properties_ignored(engine, "SetProperties")]);
    }
}

/// .NET makes a cache of the size given, 0 included, and keys it on the
/// engine's evidence, which for a cloud engine is none. So in .NET's run
/// the mobile request after a desktop one was given the desktop answer,
/// marked as a cache hit when `CacheHitOrMiss` was true, from the cache of
/// size 10 and of size 0. The engines here have no cache, so each request
/// gets its own answer.
#[test]
fn a_cache_size_gives_no_cache() {
    for size in ["10", "0"] {
        let pipeline =
            after_request_engine(&DEVICE, &[("CacheSize", size), ("CacheHitOrMiss", "true")])
                .expect(size);
        let desktop = process(&pipeline, DESKTOP);
        let mobile = process(&pipeline, MOBILE);
        assert_eq!(device(&desktop), (false, "Chrome".to_string(), false));
        assert_eq!(device(&mobile), (true, "Mobile Safari".to_string(), false));
    }
}

/// .NET refuses a negative size as "Exception has been thrown by the
/// target of an invocation." with this as the inner message, and then the
/// process ends, as the half-made cache's finalizer throws.
#[test]
fn a_negative_cache_size_is_refused() {
    for engine in [&DEVICE, &HARDWARE] {
        assert_eq!(
            message(after_request_engine(engine, &[("CacheSize", "-1")])),
            failed(
                engine.builder,
                "capacity ('-1') must be a non-negative value. (Parameter 'capacity')\nActual \
                 value was -1."
            ),
            "{}",
            engine.name
        );
    }
}

#[test]
fn values_that_do_not_parse_are_refused() {
    for engine in [&DEVICE, &HARDWARE] {
        for (name, type_name) in [("CacheSize", "Int32"), ("CacheHitOrMiss", "Boolean")] {
            assert_eq!(
                message(after_request_engine(engine, &[(name, "x")])),
                failed(
                    engine.builder,
                    &format!(
                        "Method 'Set{name}' on builder '{}' for element 1 expects a parameter of \
                         type '{type_name}'. Failed to parse value 'x' using the static \
                         'TryParse' method.",
                        engine.builder_type
                    )
                ),
                "{} {name}",
                engine.name
            );
        }
    }
}

#[test]
fn code_only_settings_are_refused_as_dotnet_refuses_them() {
    for engine in [&DEVICE, &HARDWARE] {
        for (name, type_name) in [
            ("LazyLoading", "LazyLoadingConfiguration"),
            ("Cache", "CacheConfiguration"),
        ] {
            assert_eq!(
                message(after_request_engine(engine, &[(name, "x")])),
                failed(
                    engine.builder,
                    &format!(
                        "Method 'Set{name}' on builder '{}' for element 1 expects a parameter of \
                         type '{type_name}' but this type does not have the expected 'TryParse' \
                         method (or has multiples that cannot be resolved).",
                        engine.builder_type
                    )
                ),
                "{} {name}",
                engine.name
            );
        }
    }
}

/// .NET lists the same seven settings in the same order, giving
/// `RuntimeParameterInfo` as each type. A request engine setting, such as
/// `ResourceKey`, is not one of them, in .NET as here.
#[test]
fn an_unknown_setting_lists_dotnets_settings() {
    let listed = [
        "SetLazyLoading (LazyLoadingConfiguration)",
        "SetLazyLoadingTimeout (Int32)",
        "SetCache (CacheConfiguration)",
        "SetCacheSize (Int32)",
        "SetCacheHitOrMiss (Boolean)",
        "SetProperties (List`1)",
        "SetProperty (String)",
    ]
    .iter()
    .map(|s| format!("{s}\n"))
    .collect::<String>();
    for engine in [&DEVICE, &HARDWARE] {
        for (name, upper) in [("Unknown", "UNKNOWN"), ("ResourceKey", "RESOURCEKEY")] {
            assert_eq!(
                message(after_request_engine(engine, &[(name, "1")])),
                failed(
                    engine.builder,
                    &format!(
                        "Builder '{}' for element 1 has no 'Set' methods or 'Build' methods that \
                         match (case-insensitively) the following parameters: {upper}. The \
                         available configuration methods on this builder are: \n{listed}The \
                         available 'Build' methods have the following signatures: \n\n",
                        engine.builder_type
                    )
                ),
                "{} {name}",
                engine.name
            );
        }
    }
}

/// .NET logs the same error, then fails the build with "Failed to load
/// properties for 'device'. This usually means your resource key does not
/// include access to any properties under 'device'...". The engines here
/// list no properties for such a key and build, as they do in code.
#[test]
fn a_key_without_the_product_logs_dotnets_error() {
    for engine in [&DEVICE, &HARDWARE] {
        let request =
            ElementOptions::new(REQUEST_ENGINE).parameter("ResourceKey", KEY_WITHOUT_PRODUCTS);
        let ((pipeline, _), logged) =
            logged_by(|| build(vec![request, ElementOptions::new(engine.builder)]));
        let pipeline = pipeline.expect(engine.name);
        assert_eq!(
            logged,
            [(
                log::Level::Error,
                format!(
                    "Aspect properties could not be loaded for {} - '{}'",
                    engine.name, engine.data_key
                )
            )]
        );
        assert!(
            made(&pipeline, engine).aspect_properties().is_empty(),
            "{}",
            engine.name
        );
    }
}

#[test]
fn a_json_file_written_for_dotnet_makes_the_same_engines_as_code() {
    let options = PipelineOptions::from_json(
        r#"{
            // As an appsettings.json written for .NET gives it.
            "PipelineOptions": {
                "Elements": [
                    {
                        "BuilderName": "CloudRequestEngine",
                        "BuildParameters": { "ResourceKey": "AQkey" }
                    },
                    {
                        "BuilderName": "DeviceDetectionCloudEngine",
                        "BuildParameters": { "CacheHitOrMiss": false }
                    },
                    { "BuilderName": "HardwareProfileCloudEngine" }
                ]
            }
        }"#,
    )
    .expect("the JSON");
    let (from_file, _) = build_options(&options);
    let from_file = from_file.expect("the pipeline from the file");

    let client: Arc<dyn CloudHttpClient> = Arc::new(StandIn::default());
    let request = Arc::new(
        CloudRequestEngine::builder()
            .resource_key(RESOURCE_KEY)
            .http_client(client)
            .build()
            .expect("the request engine"),
    );
    let from_code = Pipeline::builder()
        .add_element(request.clone())
        .add_element(Arc::new(
            DeviceDetectionCloudEngine::builder()
                .cloud_request_engine(request.clone())
                .build(),
        ))
        .add_element(Arc::new(
            HardwareProfileCloudEngine::builder()
                .cloud_request_engine(request)
                .build(),
        ))
        .build()
        .expect("the pipeline from code");

    assert_eq!(keys(&from_file), keys(&from_code));
    for evidence in [DESKTOP, MOBILE, TAC] {
        let (file, code) = (process(&from_file, evidence), process(&from_code, evidence));
        assert_eq!(device(&file), device(&code), "{evidence:?}");
        assert_eq!(hardware(&file), hardware(&code), "{evidence:?}");
    }
}
