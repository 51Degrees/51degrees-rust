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

//! The 51Did cloud engine made from a configuration, compared with what
//! .NET's FiftyOne.Did.Cloud 4.5.129 on pipeline 4.5.129 made from the same
//! configuration against a local stand-in for the cloud service. Each
//! expected answer is the one .NET gave, and each message is the one .NET
//! gave, apart from the differences the tests name.

use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, ThreadId};

use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine, HttpMethod,
};
use fiftyone_fodid_cloud::{configuration, FodIdCloudEngine, FodIdData, FODID_DATA_KEY};
use fiftyone_pipeline_core::configuration::{BuilderRegistry, ElementOptions, PipelineOptions};
use fiftyone_pipeline_core::{
    Error, Evidence, FlowData, FlowElement, Pipeline, PipelineBuilder, Result,
};
use fiftyone_pipeline_engines::{AspectData, AspectEngine};

const REQUEST_ENGINE: &str = "CloudRequestEngineBuilder";
const RESOURCE_KEY: &str = "AQkey";
/// The resource key the stand-in grants no product.
const KEY_WITHOUT_PRODUCTS: &str = "noid";
/// The resource key the stand-in grants the product under the name `fodid`
/// rather than the cloud service's `FODid`.
const KEY_WITH_LOWER_CASE_PRODUCT: &str = "lower";
const BUILDER: &str = "DidCloudEngineBuilder";
const NAME: &str = "DidCloudEngine";
const BUILDER_TYPE: &str = "fiftyone_fodid_cloud::engine::FodIdCloudEngineBuilder";
const NO_SUB_ELEMENTS: &str = "No SubElements";
const NO_INNER: &str = "No Inner Exception.";
const UK: &[(&str, &str)] = &[
    ("query.client-ip", "185.28.167.78"),
    ("query.id.usage", "non-marketing"),
];
const US: &[(&str, &str)] = &[
    ("query.client-ip", "8.8.8.8"),
    ("query.id.usage", "non-marketing"),
];

/// The product the stand-in grants, the same as .NET was given.
const GRANTED: &str = r#"{"DataTier":"CloudV5Bespoke","Properties":[
    {"Name":"IdProbGlobal","Type":"String","Category":"FODid"},
    {"Name":"IdRandGlobal","Type":"String","Category":"FODid"}]}"#;

/// IdProbGlobal and IdRandGlobal, and whether the data came from a cache,
/// as .NET gave them.
type Answer = (String, String, bool);

fn uk_answer() -> Answer {
    ("uk-prob".to_string(), "uk-rand".to_string(), false)
}

fn us_answer() -> Answer {
    ("us-prob".to_string(), "us-rand".to_string(), false)
}

/// Answers for the cloud service, the same as .NET was given. An address in
/// 185.28 gets the `uk` identifiers, and any other the `us` ones.
#[derive(Default)]
struct StandIn {
    requests: Mutex<Vec<CloudHttpRequest>>,
}

impl CloudHttpClient for StandIn {
    fn send(&self, request: &CloudHttpRequest) -> std::result::Result<CloudHttpResponse, String> {
        self.requests.lock().unwrap().push(request.clone());
        let body = if request.url.contains("evidencekeys") {
            r#"["query.client-ip","query.id.usage"]"#.to_string()
        } else if request.url.contains("accessibleproperties") {
            if request
                .url
                .contains(&format!("resource={KEY_WITHOUT_PRODUCTS}"))
            {
                r#"{"Products":{}}"#.to_string()
            } else if request
                .url
                .contains(&format!("resource={KEY_WITH_LOWER_CASE_PRODUCT}"))
            {
                format!(r#"{{"Products":{{"fodid":{GRANTED}}}}}"#)
            } else {
                format!(r#"{{"Products":{{"FODid":{GRANTED}}}}}"#)
            }
        } else if request.form.iter().any(|(_, v)| v.contains("185.28")) {
            r#"{"fodid":{"idprobglobal":"uk-prob","idrandglobal":"uk-rand"}}"#.to_string()
        } else {
            r#"{"fodid":{"idprobglobal":"us-prob","idrandglobal":"us-rand"}}"#.to_string()
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

/// The cloud request engine, then the 51Did cloud engine with `parameters`.
fn after_request_engine(parameters: &[(&str, &str)]) -> Result<Arc<Pipeline>> {
    let element = parameters
        .iter()
        .fold(ElementOptions::new(BUILDER), |element, (n, v)| {
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

/// .NET's message for a 51Did cloud engine with no single cloud request
/// engine before it.
fn no_request_engine() -> String {
    format!(
        "The '{NAME}' requires a 'CloudRequestEngine' before it in the Pipeline. This engine \
         will be unable to produce results until this is corrected."
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

fn fodid(data: &FlowData) -> Answer {
    let fodid = data.get(FODID_DATA_KEY).expect("the 51Did data");
    (
        fodid
            .id_prob_global()
            .value()
            .expect("IdProbGlobal")
            .clone(),
        fodid
            .id_rand_global()
            .value()
            .expect("IdRandGlobal")
            .clone(),
        fodid.cache_hit(),
    )
}

fn keys(pipeline: &Pipeline) -> Vec<&str> {
    pipeline
        .flow_elements()
        .iter()
        .map(|e| e.data_key())
        .collect()
}

fn made(pipeline: &Pipeline) -> Arc<FodIdCloudEngine> {
    pipeline
        .resource::<FodIdCloudEngine>()
        .expect("the 51Did cloud engine")
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

fn ignored(setting: &str) -> (log::Level, String) {
    (
        log::Level::Warn,
        format!("The {NAME} setting '{setting}' has no effect here and is ignored."),
    )
}

fn properties_ignored(setting: &str) -> (log::Level, String) {
    (
        log::Level::Warn,
        format!(
            "The {NAME} setting '{setting}' has no effect, because the properties the cloud \
             returns are those the cloud request engine asks for."
        ),
    )
}

/// .NET's engine has the data key `FODid`, its component name, where the
/// engine here has `fodid`. A data key is matched ignoring case, so in
/// .NET and here the data is found under either.
#[test]
fn the_builder_is_found_by_dotnets_names() {
    let lower = BUILDER.to_lowercase();
    for name in [BUILDER, NAME, lower.as_str()] {
        let (pipeline, _) = build(vec![request_engine(), ElementOptions::new(name)]);
        let pipeline = pipeline.expect(name);
        assert_eq!(keys(&pipeline), ["cloud-response", "fodid"], "{name}");
    }
}

/// .NET does not know the engine by the names of the type here, and
/// refuses each with this message, listing every builder in the assemblies
/// it loaded where the list here is the registry's.
#[test]
fn the_builder_is_not_found_by_its_rust_names() {
    for name in ["FodIdCloudEngineBuilder", "FodIdCloudEngine"] {
        assert_eq!(
            message(build(vec![request_engine(), ElementOptions::new(name)]).0),
            failed(
                name,
                &format!(
                    "Could not find builder matching '{name}' for element 1. Available builders \
                     are: CloudRequestEngineBuilder,DidCloudEngineBuilder"
                )
            ),
            "{name}"
        );
    }
}

/// The engine with no parameters after the request engine, where in .NET
/// the engine took the properties the key grants and read each response.
/// .NET's engine also gives the data tier the key's product names,
/// CloudV5Bespoke here, where the engine here gives none. .NET's engine
/// holds the granted properties once it is built. The engine here gives the
/// six properties every 51Did engine describes until it reads the granted
/// ones, which `properties_loaded` does, as processing does.
#[test]
fn the_engine_reads_the_request_engine_made_before_it() {
    let (pipeline, stand_in) = build(vec![request_engine(), ElementOptions::new(NAME)]);
    let pipeline = pipeline.expect("the pipeline");
    assert_eq!(keys(&pipeline), ["cloud-response", "fodid"]);

    let request = pipeline
        .resource::<CloudRequestEngine>()
        .expect("the request engine");
    let engine = made(&pipeline);
    assert!(Arc::ptr_eq(engine.cloud_request_engine(), &request));
    assert!(!engine.has_loaded_properties());
    assert_eq!(engine.aspect_properties().len(), 6);
    assert!(engine.properties_loaded());
    assert_eq!(
        property_names(engine.as_ref()),
        ["IdProbGlobal", "IdRandGlobal"]
    );
    assert_eq!(engine.data_source_tier(), "");

    let uk = process(&pipeline, UK);
    assert_eq!(fodid(&uk), uk_answer());
    let by_dotnets_key = uk.get_str("FODid").expect("the data under .NET's key");
    assert_eq!(
        by_dotnets_key
            .get("IdProbGlobal")
            .expect("IdProbGlobal")
            .as_str(),
        Some("uk-prob")
    );
    assert_eq!(fodid(&process(&pipeline, US)), us_answer());
    assert_eq!(stand_in.data_requests(), 2);
}

/// .NET gives this message too. For an engine with no request engine at
/// all it gives it on its own, from the pipeline's constructor once every
/// element is made, and an engine placed before the request engine is
/// built and fails each process with it. Here the element that cannot be
/// made fails the build, naming the builder.
#[test]
fn without_a_request_engine_before_it_the_build_fails() {
    let expected = failed(BUILDER, &no_request_engine());
    let alone = build(vec![ElementOptions::new(BUILDER)]).0;
    assert_eq!(message(alone), expected, "alone");
    let before = build(vec![ElementOptions::new(BUILDER), request_engine()]).0;
    assert_eq!(message(before), expected, "before");
}

/// .NET's engine finds its request engine in the pipeline by type, which
/// finds none when there are two, so .NET, given two request engines
/// before the engine, failed with this message, on its own. Here the
/// element that cannot be made fails the build.
#[test]
fn with_two_request_engines_before_it_the_build_fails() {
    let second = ElementOptions::new(REQUEST_ENGINE).parameter("ResourceKey", KEY_WITHOUT_PRODUCTS);
    assert_eq!(
        message(build(vec![request_engine(), second, ElementOptions::new(NAME)]).0),
        failed(NAME, &no_request_engine())
    );
}

/// The engine here has no lazy loading and no cache, and in both .NET and
/// here the engine's properties have no effect, so each of these is read,
/// checked as .NET checks it, and logged, and the answers are those with
/// no setting. .NET's answers for these were the same, apart from the
/// cache sizes, which the next test covers.
#[test]
fn settings_with_no_effect_here_are_read_and_logged() {
    let cases = [
        (
            "LazyLoadingTimeout",
            "100",
            Some(ignored("SetLazyLoadingTimeout")),
        ),
        ("CacheSize", "10", Some(ignored("SetCacheSize"))),
        ("CacheSize", "0", Some(ignored("SetCacheSize"))),
        ("CacheHitOrMiss", "true", Some(ignored("SetCacheHitOrMiss"))),
        ("CacheHitOrMiss", "false", None),
        (
            "Property",
            "IdProbGlobal",
            Some(properties_ignored("SetProperty")),
        ),
        (
            "Properties",
            "IdProbGlobal,IdRandGlobal",
            Some(properties_ignored("SetProperties")),
        ),
    ];
    let unset = after_request_engine(&[]).expect("no settings");
    assert!(made(&unset).properties_loaded());
    for (name, value, expected) in cases {
        let (pipeline, logged) = logged_by(|| after_request_engine(&[(name, value)]));
        let pipeline = pipeline.unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            logged,
            expected.into_iter().collect::<Vec<_>>(),
            "{name} {value}"
        );
        assert!(made(&pipeline).properties_loaded(), "{name}");
        assert_eq!(
            property_names(made(&pipeline).as_ref()),
            property_names(made(&unset).as_ref()),
            "{name}"
        );
        assert_eq!(
            fodid(&process(&pipeline, UK)),
            uk_answer(),
            "{name} {value}"
        );
        assert_eq!(
            fodid(&process(&pipeline, US)),
            us_answer(),
            "{name} {value}"
        );
    }

    // A list given in code is read as SetProperties.
    let element =
        ElementOptions::new(BUILDER).parameter("Properties", vec!["IdProbGlobal".to_string()]);
    let (pipeline, logged) = logged_by(|| build(vec![request_engine(), element]).0);
    pipeline.expect("a list of properties");
    assert_eq!(logged, [properties_ignored("SetProperties")]);
}

/// .NET makes a cache of the size given, 0 included, and keys it on the
/// engine's evidence, which for a cloud engine is none. So in .NET the
/// request from 8.8.8.8 after one from 185.28.167.78 was given the
/// first request's 51Dids, marked as a cache hit when `CacheHitOrMiss` was
/// true, from the cache of size 10 and of size 0. The engine here has no
/// cache, so each request gets its own 51Dids.
#[test]
fn a_cache_size_gives_no_cache() {
    for size in ["10", "0"] {
        let pipeline =
            after_request_engine(&[("CacheSize", size), ("CacheHitOrMiss", "true")]).expect(size);
        assert_eq!(fodid(&process(&pipeline, UK)), uk_answer(), "{size}");
        assert_eq!(fodid(&process(&pipeline, US)), us_answer(), "{size}");
    }
}

/// .NET refuses a negative size as "Exception has been thrown by the
/// target of an invocation." with this as the inner message.
#[test]
fn a_negative_cache_size_is_refused() {
    assert_eq!(
        message(after_request_engine(&[("CacheSize", "-1")])),
        failed(
            BUILDER,
            "capacity ('-1') must be a non-negative value. (Parameter 'capacity')\nActual value \
             was -1."
        )
    );
}

#[test]
fn values_that_do_not_parse_are_refused() {
    for (name, type_name) in [
        ("LazyLoadingTimeout", "Int32"),
        ("CacheSize", "Int32"),
        ("CacheHitOrMiss", "Boolean"),
    ] {
        assert_eq!(
            message(after_request_engine(&[(name, "x")])),
            failed(
                BUILDER,
                &format!(
                    "Method 'Set{name}' on builder '{BUILDER_TYPE}' for element 1 expects a \
                     parameter of type '{type_name}'. Failed to parse value 'x' using the static \
                     'TryParse' method."
                )
            ),
            "{name}"
        );
    }
}

#[test]
fn code_only_settings_are_refused_as_dotnet_refuses_them() {
    for (name, type_name) in [
        ("LazyLoading", "LazyLoadingConfiguration"),
        ("Cache", "CacheConfiguration"),
    ] {
        assert_eq!(
            message(after_request_engine(&[(name, "x")])),
            failed(
                BUILDER,
                &format!(
                    "Method 'Set{name}' on builder '{BUILDER_TYPE}' for element 1 expects a \
                     parameter of type '{type_name}' but this type does not have the expected \
                     'TryParse' method (or has multiples that cannot be resolved)."
                )
            ),
            "{name}"
        );
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
    // .NET matches a parameter to any public method of the builder by its
    // name, so for `Build` it finds the builder's `Build()` and refuses it
    // with "Method 'Build' on builder '...' for element 1 takes no
    // parameters This is not supported.". Here only the settings are
    // matched, so `Build` is a parameter no setting takes.
    for (name, upper) in [
        ("Unknown", "UNKNOWN"),
        ("ResourceKey", "RESOURCEKEY"),
        ("Build", "BUILD"),
    ] {
        assert_eq!(
            message(after_request_engine(&[(name, "1")])),
            failed(
                BUILDER,
                &format!(
                    "Builder '{BUILDER_TYPE}' for element 1 has no 'Set' methods or 'Build' \
                     methods that match (case-insensitively) the following parameters: {upper}. \
                     The available configuration methods on this builder are: \n{listed}The \
                     available 'Build' methods have the following signatures: \n\n"
                )
            ),
            "{name}"
        );
    }
}

/// .NET, given the engine twice after the request engine, made both, one
/// after the other, and gave these answers.
#[test]
fn the_engine_named_twice_is_made_twice() {
    let (pipeline, stand_in) = build(vec![
        request_engine(),
        ElementOptions::new(BUILDER),
        ElementOptions::new(BUILDER),
    ]);
    let pipeline = pipeline.expect("the pipeline");
    assert_eq!(keys(&pipeline), ["cloud-response", "fodid", "fodid"]);
    assert_eq!(fodid(&process(&pipeline, UK)), uk_answer());
    assert_eq!(fodid(&process(&pipeline, US)), us_answer());
    assert_eq!(stand_in.data_requests(), 2);
}

/// .NET logs the first sentence as an error, naming its engine
/// `DidCloudEngine` and its data key `FODid`, then fails the build with
/// "Failed to load properties for 'FODid'. This usually means your resource
/// key does not include access to any properties under 'FODid'...". The
/// cloud aspect engine specification says such an engine builds and logs the
/// missing product once as a warning, naming its data key, so the engine
/// here lists no properties for such a key, builds, as it does in code, and
/// logs both sentences as one warning once it reads the key's products.
#[test]
fn a_key_without_the_product_builds_and_logs_one_warning() {
    let request =
        ElementOptions::new(REQUEST_ENGINE).parameter("ResourceKey", KEY_WITHOUT_PRODUCTS);
    let (pipeline, logged) = logged_by(|| {
        let pipeline = build(vec![request, ElementOptions::new(BUILDER)])
            .0
            .expect("the pipeline");
        assert!(made(&pipeline).properties_loaded());
        pipeline
    });
    assert_eq!(
        logged,
        [(
            log::Level::Warn,
            "Aspect properties could not be loaded for FodIdCloudEngine - 'fodid'. The \
             resource key does not include access to any properties under 'fodid'."
                .to_string()
        )]
    );
    assert!(made(&pipeline).aspect_properties().is_empty());
}

/// The cloud service names the product `FODid`. .NET looks for that name
/// exactly, so with the product named `fodid` .NET logged the error
/// of the test above and failed the build in the same way. The engine
/// here finds the product ignoring case.
#[test]
fn a_product_named_in_lower_case_is_found() {
    let request =
        ElementOptions::new(REQUEST_ENGINE).parameter("ResourceKey", KEY_WITH_LOWER_CASE_PRODUCT);
    let (pipeline, logged) = logged_by(|| {
        let pipeline = build(vec![request, ElementOptions::new(BUILDER)])
            .0
            .expect("the pipeline");
        assert!(made(&pipeline).properties_loaded());
        pipeline
    });
    assert!(logged.is_empty(), "{logged:?}");
    assert_eq!(
        property_names(made(&pipeline).as_ref()),
        ["IdProbGlobal", "IdRandGlobal"]
    );
}

#[test]
fn a_json_file_written_for_dotnet_makes_the_same_engine_as_code() {
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
                        "BuilderName": "DidCloudEngine",
                        "BuildParameters": { "CacheHitOrMiss": false }
                    }
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
            FodIdCloudEngine::builder()
                .cloud_request_engine(request)
                .build()
                .expect("the 51Did cloud engine"),
        ))
        .build()
        .expect("the pipeline from code");

    assert_eq!(keys(&from_file), keys(&from_code));
    for evidence in [UK, US] {
        let (file, code) = (process(&from_file, evidence), process(&from_code, evidence));
        assert_eq!(fodid(&file), fodid(&code), "{evidence:?}");
        assert_eq!(
            fodid(&file),
            if evidence == UK {
                uk_answer()
            } else {
                us_answer()
            }
        );
    }
}
