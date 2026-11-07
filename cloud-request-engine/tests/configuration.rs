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

//! The cloud request engine made from a configuration, compared with what
//! the .NET pipeline 4.5.129 made from the same configuration against a
//! local stand-in for the cloud service. Each expected value is the one
//! .NET held, and each message is the one .NET gave, apart from the
//! differences the tests name.

use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, ThreadId};
use std::time::Duration;

use fiftyone_cloud_request_engine::{
    configuration, CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine,
    CloudRequestEngineBuilder, HttpMethod, CLOUD_ENDPOINT_ENV_VAR, CLOUD_URI_DEFAULT,
};
use fiftyone_pipeline_core::configuration::{
    BuildContext, BuilderDefinition, BuilderRegistry, ElementOptions, PipelineOptions,
};
use fiftyone_pipeline_core::{
    Error, Evidence, EvidenceKeyFilterWhitelist, FlowData, FlowElement, Pipeline, PipelineBuilder,
    PropertyMetaData, Result,
};
use fiftyone_pipeline_engines::AspectData;

const BUILDER_NAME: &str = "CloudRequestEngineBuilder";
const BUILDER_TYPE: &str =
    "fiftyone_cloud_request_engine::configuration::CloudRequestEngineSettings";
const NO_SUB_ELEMENTS: &str = "No SubElements";
const NO_INNER: &str = "No Inner Exception.";
const RESOURCE_KEY: &str = "AQkey";
const LICENSE_KEY: &str = "LK";
const DATA_RESPONSE: &str = r#"{"device":{"ismobile":true}}"#;

/// Answers for the cloud service, recording every request. The
/// accessible properties request for the resource key `bad` is answered
/// 400, as the cloud answers a key it does not know.
#[derive(Default)]
struct StandIn {
    requests: Mutex<Vec<CloudHttpRequest>>,
}

impl CloudHttpClient for StandIn {
    fn send(&self, request: &CloudHttpRequest) -> std::result::Result<CloudHttpResponse, String> {
        self.requests.lock().unwrap().push(request.clone());
        let (status, body) = if request.url.contains("resource=bad") {
            (400, r#"{"errors":["bad resource key"]}"#)
        } else if request.url.contains("evidencekeys") {
            (200, r#"["header.user-agent","query.user-agent"]"#)
        } else if request.url.contains("accessibleproperties") {
            (200, r#"{"Products":{}}"#)
        } else {
            (200, DATA_RESPONSE)
        };
        Ok(CloudHttpResponse {
            status,
            body: body.to_string(),
            retry_after: None,
        })
    }
}

impl StandIn {
    /// The address of each GET, which the discovery requests are.
    fn discovery(&self) -> Vec<String> {
        let requests = self.requests.lock().unwrap();
        let mut urls: Vec<String> = requests
            .iter()
            .filter(|r| r.method == HttpMethod::Get)
            .map(|r| r.url.clone())
            .collect();
        urls.sort();
        urls
    }

    /// The data requests, being each POST.
    fn data(&self) -> Vec<CloudHttpRequest> {
        let requests = self.requests.lock().unwrap();
        requests
            .iter()
            .filter(|r| r.method == HttpMethod::Post)
            .cloned()
            .collect()
    }
}

fn registry() -> BuilderRegistry {
    let mut registry = BuilderRegistry::new();
    configuration::add_builders(&mut registry);
    registry
}

/// The pipeline `options` make, with the stand-in given as the client.
fn build_with(options: &PipelineOptions) -> (Result<Arc<Pipeline>>, Arc<StandIn>) {
    build_with_registry(&registry(), options)
}

fn build_with_registry(
    registry: &BuilderRegistry,
    options: &PipelineOptions,
) -> (Result<Arc<Pipeline>>, Arc<StandIn>) {
    let stand_in = Arc::new(StandIn::default());
    let client: Arc<dyn CloudHttpClient> = stand_in.clone();
    let result = PipelineBuilder::new()
        .add_resource(Arc::new(client))
        .build_from_configuration(registry, options);
    (result, stand_in)
}

/// Build parameters by name, each with its value as text.
type Parameters<'a> = &'a [(&'a str, &'a str)];

fn options(parameters: &[(&str, &str)]) -> PipelineOptions {
    let element = parameters.iter().fold(
        ElementOptions::new(BUILDER_NAME),
        |element, (name, value)| element.parameter(*name, *value),
    );
    PipelineOptions::new().element(element)
}

/// The pipeline made with the resource key and `parameters`.
fn with(parameters: &[(&str, &str)]) -> (Result<Arc<Pipeline>>, Arc<StandIn>) {
    let mut all = vec![("ResourceKey", RESOURCE_KEY)];
    all.extend_from_slice(parameters);
    build_with(&options(&all))
}

/// The engine made with the resource key and `parameters`.
fn engine_with(parameters: &[(&str, &str)]) -> (Arc<CloudRequestEngine>, Arc<StandIn>) {
    let (pipeline, stand_in) = with(parameters);
    let pipeline = pipeline.expect("the pipeline");
    (engine(&pipeline), stand_in)
}

fn engine(pipeline: &Pipeline) -> Arc<CloudRequestEngine> {
    pipeline
        .resource::<CloudRequestEngine>()
        .expect("the cloud request engine")
}

fn message(result: Result<Arc<Pipeline>>) -> String {
    match result {
        Err(Error::PipelineConfiguration { message }) => message,
        Err(other) => panic!("not a configuration error: {other}"),
        Ok(_) => panic!("the pipeline was built"),
    }
}

/// The message .NET gives for an element that cannot be made.
fn failed(message: &str) -> String {
    format!(
        "Configuration failed to build.\nBuilderName: {BUILDER_NAME}\nSubElement Builder \
         Names: {NO_SUB_ELEMENTS}\n{message}\n{NO_INNER}\n"
    )
}

/// The message the engine's builder gives in code, with the stand-in as
/// its client.
fn refused_in_code(builder: CloudRequestEngineBuilder) -> String {
    match builder.http_client(Arc::new(StandIn::default())).build() {
        Err(error) => error.message().into_owned(),
        Ok(_) => panic!("the engine was built"),
    }
}

/// The base URL an endpoint not given is made from.
fn default_base() -> String {
    std::env::var(CLOUD_ENDPOINT_ENV_VAR).unwrap_or_else(|_| CLOUD_URI_DEFAULT.to_string())
}

fn process(pipeline: &Arc<Pipeline>, user_agent: &str) -> FlowData {
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", user_agent)
            .build(),
    );
    data.process().expect("processing");
    data
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

/// The warnings the current thread has logged. The tests in this binary
/// run in parallel, so each reads only its own.
fn warnings() -> Vec<String> {
    let me = thread::current().id();
    logger()
        .entries
        .lock()
        .unwrap()
        .iter()
        .filter(|(id, level, _)| *id == me && *level == log::Level::Warn)
        .map(|(_, _, message)| message.clone())
        .collect()
}

/// The warnings logged while `run` runs.
fn warnings_from<T>(run: impl FnOnce() -> T) -> (T, Vec<String>) {
    let before = warnings().len();
    let result = run();
    (result, warnings().split_off(before))
}

fn ignored(setting: &str) -> String {
    format!("The cloud request engine setting '{setting}' has no effect here and is ignored.")
}

#[test]
fn the_builder_is_found_by_dotnets_names() {
    for name in [
        "CloudRequestEngineBuilder",
        "CloudRequestEngine",
        "cloudrequestenginebuilder",
    ] {
        let options = PipelineOptions::new()
            .element(ElementOptions::new(name).parameter("ResourceKey", RESOURCE_KEY));
        let pipeline = build_with(&options).0.expect(name);
        let keys: Vec<&str> = pipeline
            .flow_elements()
            .iter()
            .map(|e| e.data_key())
            .collect();
        assert_eq!(keys, ["cloud-response"], "{name}");
    }
}

/// .NET's builder has its own recovery defaults (100 failures in 10
/// seconds, and exponential backoff from 2 to 10 seconds). The engine here
/// keeps the specification's, as it does in code, and the timeout of 2
/// seconds that .NET and the specification share. The expected values are
/// written out rather than taken from the crate's constants, so a change
/// to a constant fails here.
#[test]
fn a_setting_not_given_keeps_the_engine_builders_default() {
    // The specification's "Configuration options" table for the engine.
    const TIMEOUT_SECONDS: u64 = 2;
    const FAILURES_TO_ENTER_RECOVERY: u32 = 10;
    const FAILURES_WINDOW_SECONDS: u64 = 100;
    const RECOVERY_SECONDS: u64 = 60;
    let (engine, stand_in) = engine_with(&[]);
    let base = default_base();
    assert_eq!(engine.resource_key(), Some(RESOURCE_KEY));
    assert_eq!(engine.data_endpoint(), format!("{base}json"));
    assert_eq!(
        stand_in.discovery(),
        [
            format!("{base}accessibleproperties?resource={RESOURCE_KEY}"),
            format!("{base}evidencekeys"),
        ]
    );
    assert_eq!(engine.timeout(), Duration::from_secs(TIMEOUT_SECONDS));
    let recovery = engine.recovery_config();
    assert_eq!(
        recovery.failures_to_enter_recovery,
        FAILURES_TO_ENTER_RECOVERY
    );
    assert_eq!(
        recovery.window,
        Duration::from_secs(FAILURES_WINDOW_SECONDS)
    );
    assert_eq!(recovery.recovery, Duration::from_secs(RECOVERY_SECONDS));
    assert_eq!(engine.cloud_request_origin(), None);
    assert!(engine.values().is_empty());
}

#[test]
fn the_end_point_sets_all_three_endpoints_and_the_last_given_wins() {
    let base = default_base();
    let cases: [(Parameters, [&str; 3]); 6] = [
        (
            &[("EndPoint", "http://example.test/api")],
            [
                "http://example.test/api/json",
                "http://example.test/api/accessibleproperties",
                "http://example.test/api/evidencekeys",
            ],
        ),
        (
            &[("EndPoint", "http://example.test/api/")],
            [
                "http://example.test/api/json",
                "http://example.test/api/accessibleproperties",
                "http://example.test/api/evidencekeys",
            ],
        ),
        (
            &[
                ("EndPoint", "http://example.test/api"),
                ("DataEndpoint", "http://data.test/x"),
            ],
            [
                "http://data.test/x",
                "http://example.test/api/accessibleproperties",
                "http://example.test/api/evidencekeys",
            ],
        ),
        (
            &[
                ("DataEndpoint", "http://data.test/x"),
                ("EndPoint", "http://example.test/api"),
            ],
            [
                "http://example.test/api/json",
                "http://example.test/api/accessibleproperties",
                "http://example.test/api/evidencekeys",
            ],
        ),
        (
            &[
                (
                    "PropertiesEndpoint",
                    "http://p.test/accessibleproperties?x=1",
                ),
                ("EvidenceKeysEndpoint", "http://e.test/evidencekeys?y=2"),
            ],
            [
                "",
                "http://p.test/accessibleproperties?x=1",
                "http://e.test/evidencekeys?y=2",
            ],
        ),
        (
            &[("DataEndpoint", "http://data.test/x")],
            ["http://data.test/x", "", ""],
        ),
    ];
    for (parameters, [data, properties, evidence_keys]) in cases {
        let (engine, stand_in) = engine_with(parameters);
        let data = if data.is_empty() {
            format!("{base}json")
        } else {
            data.to_string()
        };
        let properties = if properties.is_empty() {
            format!("{base}accessibleproperties")
        } else {
            properties.to_string()
        };
        let evidence_keys = if evidence_keys.is_empty() {
            format!("{base}evidencekeys")
        } else {
            evidence_keys.to_string()
        };
        assert_eq!(engine.data_endpoint(), data, "{parameters:?}");
        let mut expected = vec![
            format!("{properties}?resource={RESOURCE_KEY}"),
            evidence_keys,
        ];
        expected.sort();
        assert_eq!(stand_in.discovery(), expected, "{parameters:?}");
    }
}

/// An endpoint given as empty or white space after `SetEndPoint` takes the
/// default, not the one made from the base URL, as in .NET.
#[test]
fn an_endpoint_left_empty_takes_the_default() {
    for empty in ["", " "] {
        let (engine, stand_in) = engine_with(&[
            ("EndPoint", "http://example.test/api"),
            ("DataEndpoint", empty),
        ]);
        assert_eq!(engine.data_endpoint(), format!("{}json", default_base()));
        assert_eq!(
            stand_in.discovery(),
            [
                format!("http://example.test/api/accessibleproperties?resource={RESOURCE_KEY}"),
                "http://example.test/api/evidencekeys".to_string(),
            ]
        );
    }
}

/// .NET builds an engine from an `EndPoint` that is empty or not absolute,
/// logs each discovery request as failed and leaves discovery to the first
/// request. The engine here refuses it, as it does in code.
#[test]
fn an_end_point_that_is_not_an_absolute_url_is_refused() {
    for (end_point, data) in [("", "/json"), ("relative", "relative/json")] {
        let expected = refused_in_code(
            CloudRequestEngine::builder()
                .resource_key(RESOURCE_KEY)
                .data_endpoint(data)
                .properties_endpoint(data.replace("json", "accessibleproperties"))
                .evidence_keys_endpoint(data.replace("json", "evidencekeys")),
        );
        assert!(expected.contains(&format!("'{data}' is not a valid absolute URL")));
        assert_eq!(
            message(with(&[("EndPoint", end_point)]).0),
            failed(&expected),
            "'{end_point}'"
        );
    }
}

/// The credentials are refused as they are in code, following the cloud
/// request engine specification's accepted combinations. .NET 4.5.129
/// refuses any configuration without a resource key, as "A resource key
/// is required to access the 51Degrees cloud service...", and accepts
/// properties named with a resource key, which it never sends. The
/// specification's `RequestedProperties` with a resource key is refused
/// as `Property` is, as the specification's table says it must not be
/// given with one.
#[test]
fn the_credentials_are_refused_as_they_are_in_code() {
    let cases: [(Parameters, CloudRequestEngineBuilder); 6] = [
        (&[], CloudRequestEngine::builder()),
        (
            &[("ResourceKey", "")],
            CloudRequestEngine::builder().resource_key(""),
        ),
        (
            &[("ResourceKey", " ")],
            CloudRequestEngine::builder().resource_key(" "),
        ),
        (
            &[("LicenseKey", LICENSE_KEY)],
            CloudRequestEngine::builder().license_key(LICENSE_KEY),
        ),
        (
            &[
                ("ResourceKey", RESOURCE_KEY),
                ("Property", "device.ismobile"),
            ],
            CloudRequestEngine::builder()
                .resource_key(RESOURCE_KEY)
                .values(["device.ismobile"]),
        ),
        (
            &[
                ("ResourceKey", RESOURCE_KEY),
                ("RequestedProperties", "device.ismobile"),
            ],
            CloudRequestEngine::builder()
                .resource_key(RESOURCE_KEY)
                .values(["device.ismobile"]),
        ),
    ];
    for (parameters, builder) in cases {
        assert_eq!(
            message(build_with(&options(parameters)).0),
            failed(&refused_in_code(builder)),
            "{parameters:?}"
        );
    }
}

#[test]
fn a_license_key_is_sent_with_the_resource_key() {
    let (pipeline, stand_in) = with(&[("LicenseKey", LICENSE_KEY)]);
    let pipeline = pipeline.expect("the pipeline");
    process(&pipeline, "UA");
    let form = &stand_in.data()[0].form;
    assert!(form.contains(&("resource".to_string(), RESOURCE_KEY.to_string())));
    assert!(form.contains(&("license".to_string(), LICENSE_KEY.to_string())));

    // An empty license key is not sent, as in .NET.
    let (pipeline, stand_in) = with(&[("LicenseKey", "")]);
    process(&pipeline.expect("the pipeline"), "UA");
    assert!(!stand_in.data()[0].form.iter().any(|(k, _)| k == "license"));
}

/// .NET's builder adds to its properties with `SetProperty` and
/// `SetProperties`, and the engine here asks for them, as the
/// specification's Requested Properties, under a license key alone.
#[test]
fn the_properties_are_added_to_in_order() {
    let options = PipelineOptions::new().element(
        ElementOptions::new(BUILDER_NAME)
            .parameter("LicenseKey", LICENSE_KEY)
            .parameter("Property", "device.ismobile")
            .parameter("Properties", "device.browsername,device.platformname"),
    );
    let (pipeline, stand_in) = build_with(&options);
    let pipeline = pipeline.expect("the pipeline");
    let made = engine(&pipeline);
    assert_eq!(
        made.values(),
        [
            "device.ismobile",
            "device.browsername",
            "device.platformname"
        ]
    );
    assert_eq!(made.resource_key(), None);
    process(&pipeline, "UA");
    let form = &stand_in.data()[0].form;
    assert!(form.contains(&(
        "values".to_string(),
        "device.ismobile,device.browsername,device.platformname".to_string()
    )));

    let options = PipelineOptions::new().element(
        ElementOptions::new(BUILDER_NAME)
            .parameter("LicenseKey", LICENSE_KEY)
            .parameter("Properties", vec!["device.ismobile", "device.hardwarename"]),
    );
    let pipeline = build_with(&options).0.expect("the pipeline");
    assert_eq!(
        engine(&pipeline).values(),
        ["device.ismobile", "device.hardwarename"]
    );
}

/// The cloud request engine specification names the properties
/// `RequestedProperties` in its configuration table, which reads them as
/// `SetProperties` does, in any case. It is an alternate name, matched as
/// .NET matches one, so `SetRequestedProperties` matches nothing. .NET
/// 4.5.129 has no method of either name and refuses both as a parameter
/// that matches no method.
#[test]
fn the_specifications_name_for_the_properties_is_read() {
    for name in ["RequestedProperties", "requestedproperties"] {
        let options = PipelineOptions::new().element(
            ElementOptions::new(BUILDER_NAME)
                .parameter("LicenseKey", LICENSE_KEY)
                .parameter("Property", "device.ismobile")
                .parameter(name, "device.browsername,device.platformname"),
        );
        let (pipeline, stand_in) = build_with(&options);
        let pipeline = pipeline.expect("the pipeline");
        assert_eq!(
            engine(&pipeline).values(),
            [
                "device.ismobile",
                "device.browsername",
                "device.platformname"
            ],
            "{name}"
        );
        process(&pipeline, "UA");
        assert!(
            stand_in.data()[0].form.contains(&(
                "values".to_string(),
                "device.ismobile,device.browsername,device.platformname".to_string()
            )),
            "{name}"
        );
    }

    let options = PipelineOptions::new().element(
        ElementOptions::new(BUILDER_NAME)
            .parameter("LicenseKey", LICENSE_KEY)
            .parameter("RequestedProperties", vec!["device.hardwarename"]),
    );
    let pipeline = build_with(&options).0.expect("the pipeline");
    assert_eq!(engine(&pipeline).values(), ["device.hardwarename"]);

    let refused = message(
        with(&[
            ("LicenseKey", LICENSE_KEY),
            ("SetRequestedProperties", "device.ismobile"),
        ])
        .0,
    );
    assert!(
        refused.contains("the following parameters: SETREQUESTEDPROPERTIES."),
        "{refused}"
    );
}

/// .NET gives the cloud's refusal as the inner message of "Exception has
/// been thrown by the target of an invocation.", which is "Error returned
/// from 51Degrees cloud service: bad resource key". The crates give the
/// engine's message, as in code.
#[test]
fn a_refused_discovery_request_fails_the_build() {
    let expected = match CloudRequestEngine::builder()
        .resource_key("bad")
        .http_client(Arc::new(StandIn::default()))
        .build()
    {
        Err(error) => error.message().into_owned(),
        Ok(_) => panic!("the engine was built"),
    };
    assert!(expected.contains("bad resource key"), "{expected}");
    assert_eq!(
        message(build_with(&options(&[("ResourceKey", "bad")])).0),
        failed(&expected)
    );
}

/// .NET sets no limit for 0 or less, and refuses a timeout beyond what its
/// `HttpClient` holds (24 days, 20 hours, 31 minutes and 23.647 seconds),
/// which the engine here accepts.
#[test]
fn the_timeout_is_read_as_dotnet_reads_it() {
    for (seconds, expected) in [
        ("5", Duration::from_secs(5)),
        ("0", Duration::ZERO),
        ("-1", Duration::ZERO),
        ("2147483647", Duration::from_secs(2147483647)),
    ] {
        let (engine, _) = engine_with(&[("TimeOutSeconds", seconds)]);
        assert_eq!(engine.timeout(), expected, "{seconds}");
    }
    let (engine, _) = engine_with(&[("TimeoutSeconds", "7")]);
    assert_eq!(engine.timeout(), Duration::from_secs(7));
    assert_eq!(
        message(with(&[("TimeOutSeconds", "x")]).0),
        failed(&format!(
            "Method 'SetTimeOutSeconds' on builder '{BUILDER_TYPE}' for element 0 expects a \
             parameter of type 'Int32'. Failed to parse value 'x' using the static 'TryParse' \
             method."
        ))
    );
}

#[test]
fn the_recovery_settings_are_read_as_dotnet_reads_them() {
    for failures in ["0", "-1", "1001"] {
        assert_eq!(
            message(with(&[("FailuresToEnterRecovery", failures)]).0),
            failed(&format!(
                "failuresToEnterRecovery must be within 1 and 1000 (both inclusive). Received: \
                 {failures}. (Parameter 'failuresToEnterRecovery')"
            )),
            "{failures}"
        );
    }
    for failures in [1, 100] {
        let ((engine, _), logged) =
            warnings_from(|| engine_with(&[("FailuresToEnterRecovery", &failures.to_string())]));
        assert_eq!(
            engine.recovery_config().failures_to_enter_recovery,
            failures
        );
        assert!(logged.is_empty(), "{logged:?}");
    }
    // .NET takes up to 1000, and the engine here holds 100 at most.
    for failures in [101, 1000] {
        let ((engine, _), logged) =
            warnings_from(|| engine_with(&[("FailuresToEnterRecovery", &failures.to_string())]));
        assert_eq!(engine.recovery_config().failures_to_enter_recovery, 100);
        assert_eq!(
            logged,
            [format!(
                "The cloud request engine takes at most 100 failures to enter recovery, so it \
                 uses 100 where {failures} was given."
            )]
        );
    }
    assert_eq!(
        message(with(&[("FailuresToEnterRecovery", "x")]).0),
        failed(&format!(
            "Method 'SetFailuresToEnterRecovery' on builder '{BUILDER_TYPE}' for element 0 \
             expects a parameter of type 'Int32'. Failed to parse value 'x' using the static \
             'TryParse' method."
        ))
    );

    for seconds in ["0", "-1"] {
        assert_eq!(
            message(with(&[("FailuresWindowSeconds", seconds)]).0),
            failed(&format!(
                "failuresWindowSeconds must be positive, Received: {seconds}. (Parameter \
                 'failuresWindowSeconds')"
            )),
            "{seconds}"
        );
    }
    let (engine, _) = engine_with(&[("FailuresWindowSeconds", "5")]);
    assert_eq!(engine.recovery_config().window, Duration::from_secs(5));

    // A period too long to hold, which .NET accepts, is held at 100 years.
    let longest = Duration::from_secs_f64(100.0 * 365.25 * 24.0 * 60.0 * 60.0);
    for (seconds, expected) in [
        ("2.5", Duration::from_millis(2500)),
        ("0", Duration::ZERO),
        ("-1", Duration::ZERO),
        ("NaN", Duration::ZERO),
        ("Infinity", longest),
        ("1e300", longest),
    ] {
        let (engine, _) = engine_with(&[("RecoverySeconds", seconds)]);
        assert_eq!(engine.recovery_config().recovery, expected, "{seconds}");
    }
}

/// .NET uses exponential backoff unless `UseExponentialBackoff` is false,
/// and then its `RecoverySeconds`. The engine here always uses
/// `RecoverySeconds`, has no lazy loading, and always records a cache hit,
/// so these are read, checked as .NET checks them, and logged.
#[test]
fn settings_with_no_effect_here_are_read_and_logged() {
    let cases: [(&str, &str, bool); 8] = [
        ("UseExponentialBackoff", "true", true),
        ("UseExponentialBackoff", "false", false),
        ("ExponentialBackoffInitialDelay", "3", true),
        ("ExponentialBackoffMaxDelay", "30", true),
        ("ExponentialBackoffMultiplier", "1.5", true),
        ("LazyLoadingTimeout", "100", true),
        ("CacheHitOrMiss", "false", true),
        ("CacheHitOrMiss", "true", false),
    ];
    for (name, value, logged) in cases {
        let ((pipeline, _), warnings) = warnings_from(|| with(&[(name, value)]));
        pipeline.expect(name);
        let expected = if logged {
            vec![ignored(&format!("Set{name}"))]
        } else {
            Vec::new()
        };
        assert_eq!(warnings, expected, "{name} {value}");
    }
    assert_eq!(
        message(with(&[("UseExponentialBackoff", "x")]).0),
        failed(&format!(
            "Method 'SetUseExponentialBackoff' on builder '{BUILDER_TYPE}' for element 0 expects \
             a parameter of type 'Boolean'. Failed to parse value 'x' using the static 'TryParse' \
             method."
        ))
    );
}

#[test]
fn code_only_settings_are_refused_as_dotnet_refuses_them() {
    for (name, type_name) in [
        ("LazyLoading", "LazyLoadingConfiguration"),
        ("Cache", "CacheConfiguration"),
    ] {
        assert_eq!(
            message(with(&[(name, "x")]).0),
            failed(&format!(
                "Method 'Set{name}' on builder '{BUILDER_TYPE}' for element 0 expects a parameter \
                 of type '{type_name}' but this type does not have the expected 'TryParse' method \
                 (or has multiples that cannot be resolved)."
            )),
            "{name}"
        );
    }
}

/// .NET makes a cache of the size given, 0 included, and refuses a
/// negative size, as the inner message of "Exception has been thrown by
/// the target of an invocation.". The engine here holds at least one
/// response in each part of its cache, and the largest size takes memory
/// only as responses arrive.
#[test]
fn the_cache_size_gives_the_engine_a_cache() {
    for size in ["10", "0", "2147483647"] {
        let (pipeline, stand_in) = with(&[("CacheSize", size)]);
        let pipeline = pipeline.expect(size);
        let first = process(&pipeline, "UA");
        let second = process(&pipeline, "UA");
        assert_eq!(stand_in.data().len(), 1, "{size}");
        assert!(!first.get(CloudRequestEngine::DATA_KEY).unwrap().cache_hit());
        assert!(second
            .get(CloudRequestEngine::DATA_KEY)
            .unwrap()
            .cache_hit());
    }

    // With no size given there is no cache.
    let (pipeline, stand_in) = with(&[]);
    let pipeline = pipeline.expect("the pipeline");
    process(&pipeline, "UA");
    process(&pipeline, "UA");
    assert_eq!(stand_in.data().len(), 2);

    assert_eq!(
        message(with(&[("CacheSize", "-1")]).0),
        failed(
            "capacity ('-1') must be a non-negative value. (Parameter 'capacity')\nActual value \
             was -1."
        )
    );
}

#[test]
fn the_cloud_request_origin_is_sent() {
    let (pipeline, stand_in) = with(&[("CloudRequestOrigin", "https://origin.test")]);
    let pipeline = pipeline.expect("the pipeline");
    assert_eq!(
        engine(&pipeline).cloud_request_origin(),
        Some("https://origin.test")
    );
    process(&pipeline, "UA");
    let requests = stand_in.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    for request in requests.iter() {
        assert_eq!(request.origin.as_deref(), Some("https://origin.test"));
    }
}

/// The settings .NET's run "cre all" gave, where .NET held a timeout of 9
/// seconds, 20 failures in 30 seconds, a simple recovery of 40 seconds, the
/// origin and a cache.
#[test]
fn every_setting_at_once() {
    let (pipeline, stand_in) = with(&[
        ("EndPoint", "http://cloud.test/api/v4/"),
        ("TimeOutSeconds", "9"),
        ("FailuresToEnterRecovery", "20"),
        ("FailuresWindowSeconds", "30"),
        ("RecoverySeconds", "40"),
        ("UseExponentialBackoff", "false"),
        ("CloudRequestOrigin", "https://o.test"),
        ("CacheSize", "50"),
    ]);
    let pipeline = pipeline.expect("the pipeline");
    let engine = engine(&pipeline);
    assert_eq!(engine.data_endpoint(), "http://cloud.test/api/v4/json");
    assert_eq!(engine.timeout(), Duration::from_secs(9));
    let recovery = engine.recovery_config();
    assert_eq!(recovery.failures_to_enter_recovery, 20);
    assert_eq!(recovery.window, Duration::from_secs(30));
    assert_eq!(recovery.recovery, Duration::from_secs(40));
    assert_eq!(engine.cloud_request_origin(), Some("https://o.test"));
    process(&pipeline, "UA");
    process(&pipeline, "UA");
    assert_eq!(stand_in.data().len(), 1, "the second came from the cache");
}

/// A builder after the engine finds it, as the cloud engines will.
#[test]
fn a_builder_after_it_finds_the_engine() {
    struct Follower {
        filter: EvidenceKeyFilterWhitelist,
    }

    impl FlowElement for Follower {
        fn process(&self, _data: &mut FlowData) -> Result<()> {
            Ok(())
        }
        fn data_key(&self) -> &str {
            "follower"
        }
        fn evidence_key_filter(&self) -> &dyn fiftyone_pipeline_core::EvidenceKeyFilter {
            &self.filter
        }
        fn properties(&self) -> &[PropertyMetaData] {
            &[]
        }
    }

    let found: Arc<Mutex<Option<Arc<CloudRequestEngine>>>> = Arc::default();
    let seen = Arc::clone(&found);
    let registry = registry().with(
        BuilderDefinition::new(
            "FollowerBuilder",
            move |_: (), context: &mut BuildContext| {
                *seen.lock().unwrap() = context.latest_resource::<CloudRequestEngine>();
                Ok(Arc::new(Follower {
                    filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
                }) as Arc<dyn FlowElement>)
            },
        )
        .build_method(Vec::new(), |_, _| Ok(())),
    );
    let options = PipelineOptions::new()
        .element(ElementOptions::new("CloudRequestEngine").parameter("ResourceKey", RESOURCE_KEY))
        .element(ElementOptions::new("Follower"));
    let pipeline = build_with_registry(&registry, &options)
        .0
        .expect("the pipeline");
    let held = engine(&pipeline);
    let found = found
        .lock()
        .unwrap()
        .clone()
        .expect("the follower found it");
    assert!(Arc::ptr_eq(&found, &held));
    assert_eq!(
        Arc::as_ptr(&pipeline.flow_elements()[0]) as *const (),
        Arc::as_ptr(&held) as *const ()
    );
}

/// .NET lists the same 22 settings in the same order, giving
/// `RuntimeParameterInfo` as each type.
#[test]
fn an_unknown_setting_lists_dotnets_settings() {
    let settings = [
        "SetEndPoint (String)",
        "SetDataEndpoint (String)",
        "SetPropertiesEndpoint (String)",
        "SetEvidenceKeysEndpoint (String)",
        "SetResourceKey (String)",
        "SetLicenseKey (String)",
        "SetTimeOutSeconds (Int32)",
        "SetFailuresToEnterRecovery (Int32)",
        "SetFailuresWindowSeconds (Int32)",
        "SetRecoverySeconds (Double)",
        "SetUseExponentialBackoff (Boolean)",
        "SetExponentialBackoffInitialDelay (Double)",
        "SetExponentialBackoffMaxDelay (Double)",
        "SetExponentialBackoffMultiplier (Double)",
        "SetCloudRequestOrigin (String)",
        "SetLazyLoading (LazyLoadingConfiguration)",
        "SetLazyLoadingTimeout (Int32)",
        "SetCache (CacheConfiguration)",
        "SetCacheSize (Int32)",
        "SetCacheHitOrMiss (Boolean)",
        "SetProperties (List`1)",
        "SetProperty (String)",
    ];
    let listed: String = settings.iter().map(|s| format!("{s}\n")).collect();
    assert_eq!(
        message(with(&[("Unknown", "1")]).0),
        failed(&format!(
            "Builder '{BUILDER_TYPE}' for element 0 has no 'Set' methods or 'Build' methods that \
             match (case-insensitively) the following parameters: UNKNOWN. The available \
             configuration methods on this builder are: \n{listed}The available 'Build' methods \
             have the following signatures: \n\n"
        ))
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
                        "BuildParameters": {
                            "ResourceKey": "AQkey",
                            "EndPoint": "http://cloud.test/api/v4",
                            "TimeOutSeconds": 9,
                            "FailuresToEnterRecovery": 20,
                            "FailuresWindowSeconds": 30,
                            "RecoverySeconds": 40.5,
                            "CloudRequestOrigin": "https://o.test",
                            "UseExponentialBackoff": false,
                        }
                    }
                ]
            }
        }"#,
    )
    .expect("the JSON");
    let (pipeline, file_stand_in) = build_with(&options);
    let from_file = engine(&pipeline.expect("the pipeline"));

    let code_stand_in = Arc::new(StandIn::default());
    let from_code = CloudRequestEngine::builder()
        .resource_key(RESOURCE_KEY)
        .endpoint("http://cloud.test/api/v4")
        .timeout_seconds(9)
        .failures_to_enter_recovery(20)
        .failures_window_seconds(30)
        .recovery_seconds(40.5)
        .cloud_request_origin("https://o.test")
        .http_client(code_stand_in.clone())
        .build()
        .expect("the engine");

    assert_eq!(from_file.resource_key(), from_code.resource_key());
    assert_eq!(from_file.data_endpoint(), from_code.data_endpoint());
    assert_eq!(file_stand_in.discovery(), code_stand_in.discovery());
    assert_eq!(from_file.timeout(), from_code.timeout());
    let (file, code) = (from_file.recovery_config(), from_code.recovery_config());
    assert_eq!(
        (file.failures_to_enter_recovery, file.window, file.recovery),
        (code.failures_to_enter_recovery, code.window, code.recovery)
    );
    assert_eq!(
        from_file.cloud_request_origin(),
        from_code.cloud_request_origin()
    );
}

/// With no client given to the pipeline builder, the engine uses the
/// built-in one, and without the `reqwest-client` feature there is none,
/// so the build fails as it does in code.
#[cfg(not(feature = "reqwest-client"))]
#[test]
fn with_no_client_and_no_built_in_one_the_build_fails_as_in_code() {
    let expected = match CloudRequestEngine::builder()
        .resource_key(RESOURCE_KEY)
        .build()
    {
        Err(error) => error.message().into_owned(),
        Ok(_) => panic!("the engine was built"),
    };
    let result = PipelineBuilder::new()
        .build_from_configuration(&registry(), &options(&[("ResourceKey", RESOURCE_KEY)]));
    assert_eq!(message(result), failed(&expected));
}
