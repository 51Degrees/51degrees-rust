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

//! The JavaScript builder made from a configuration, compared with what the
//! .NET pipeline 4.5.129 made from the same configuration. Each expected
//! value is the one .NET held, and each message is the one .NET gave, apart
//! from the differences the tests name.

use std::any::type_name;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, ThreadId};

use fiftyone_javascript_builder::{
    configuration, JavaScriptBuilderElement, JavaScriptBuilderElementBuilder,
    JAVASCRIPT_BUILDER_DATA_KEY,
};
use fiftyone_json_builder::JsonBuilderElement;
use fiftyone_pipeline_core::configuration::{BuilderRegistry, ElementOptions, PipelineOptions};
use fiftyone_pipeline_core::{Error, Evidence, Pipeline, PipelineBuilder, Result};
use fiftyone_pipeline_engines_fiftyone::constants::{EVIDENCE_SEQUENCE, EVIDENCE_SESSIONID};
use fiftyone_pipeline_engines_fiftyone::SequenceElement;

const BUILDER_NAME: &str = "JavaScriptBuilderElementBuilder";
const NO_SUB_ELEMENTS: &str = "No SubElements";
const NO_INNER: &str = "No Inner Exception.";

fn registry() -> BuilderRegistry {
    let mut registry = BuilderRegistry::new();
    fiftyone_pipeline_engines_fiftyone::configuration::add_builders(&mut registry);
    fiftyone_json_builder::configuration::add_builders(&mut registry);
    configuration::add_builders(&mut registry);
    registry
}

fn build(options: &PipelineOptions) -> Result<Arc<Pipeline>> {
    PipelineBuilder::new().build_from_configuration(&registry(), options)
}

fn one(element: ElementOptions) -> Result<Arc<Pipeline>> {
    build(&PipelineOptions::new().element(element))
}

fn with(parameters: &[(&str, &str)]) -> Result<Arc<Pipeline>> {
    let element = parameters.iter().fold(
        ElementOptions::new(BUILDER_NAME),
        |element, (name, value)| element.parameter(*name, *value),
    );
    one(element)
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

fn keys(pipeline: &Pipeline) -> Vec<&str> {
    pipeline
        .flow_elements()
        .iter()
        .map(|e| e.data_key())
        .collect()
}

/// The host, endpoint, protocol, object name, cookies and minify settings
/// the element holds, in the order .NET's builder declares them.
type Settings = (String, String, String, String, bool, bool);

fn settings(pipeline: &Pipeline) -> Settings {
    let element = pipeline
        .resource::<JavaScriptBuilderElement>()
        .expect("the JavaScript builder");
    (
        element.host().to_string(),
        element.endpoint().to_string(),
        element.protocol().to_string(),
        element.object_name().to_string(),
        element.enable_cookies(),
        element.minify(),
    )
}

fn held(
    host: &str,
    endpoint: &str,
    protocol: &str,
    object_name: &str,
    cookies: bool,
    minify: bool,
) -> Settings {
    (
        host.to_string(),
        endpoint.to_string(),
        protocol.to_string(),
        object_name.to_string(),
        cookies,
        minify,
    )
}

/// The defaults of .NET's builder, being no host or protocol, so the
/// request's are used, the default JSON endpoint and the object `fod`, with
/// cookies and minifying on.
fn defaults() -> Settings {
    held("", "/51dpipeline/json", "", "fod", true, true)
}

/// The script `pipeline` writes for the second request of one session.
fn script(pipeline: &Arc<Pipeline>) -> String {
    let evidence = Evidence::builder()
        .add(EVIDENCE_SESSIONID, "session")
        .add(EVIDENCE_SEQUENCE, "1")
        .build();
    let mut data = pipeline.create_flow_data_with(evidence);
    data.process().expect("processing succeeds");
    data.get(JAVASCRIPT_BUILDER_DATA_KEY)
        .expect("the JavaScript builder's data")
        .javascript()
        .to_owned()
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

/// The records the current thread has logged. The tests in this binary run
/// in parallel, so each reads only its own.
fn records() -> Vec<(log::Level, String)> {
    let me = thread::current().id();
    logger()
        .entries
        .lock()
        .unwrap()
        .iter()
        .filter(|(id, _, _)| *id == me)
        .map(|(_, level, message)| (*level, message.clone()))
        .collect()
}

#[test]
fn the_javascript_builder_is_found_by_dotnets_names() {
    for name in [
        "JavaScriptBuilderElementBuilder",
        "JavaScriptBuilderElement",
        "javascriptbuilderelementbuilder",
    ] {
        let pipeline = one(ElementOptions::new(name)).unwrap();
        assert_eq!(keys(&pipeline), ["javascriptbuilderelement"], "{name}");
    }

    // .NET only adds "Builder" to the name. It lists every builder it
    // loaded, and the crates list those in the registry.
    assert_eq!(
        message(one(ElementOptions::new("JavaScriptBuilder"))),
        "Configuration failed to build.\nBuilderName: JavaScriptBuilder\nSubElement Builder \
         Names: No SubElements\nCould not find builder matching 'JavaScriptBuilder' for \
         element 0. Available builders are: SequenceElementBuilder,SetHeadersElementBuilder,\
         ShareUsageBuilder,JsonBuilderElementBuilder,JavaScriptBuilderElementBuilder\n\
         No Inner Exception.\n"
    );
}

#[test]
fn the_defaults_are_those_of_dotnet() {
    assert_eq!(settings(&with(&[]).unwrap()), defaults());
}

#[test]
fn every_setting_is_read_as_dotnet_reads_it() {
    let all = with(&[
        ("Host", "example.com"),
        ("Endpoint", "/api/v4/json"),
        ("Protocol", "https"),
        ("ObjectName", "myObj"),
        ("EnableCookies", "false"),
        ("Minify", "false"),
    ])
    .unwrap();
    assert_eq!(
        settings(&all),
        held(
            "example.com",
            "/api/v4/json",
            "https",
            "myObj",
            false,
            false
        )
    );

    let (host, endpoint, protocol, object_name, _, _) = defaults();
    for (parameters, expected) in [
        (
            vec![("EnableCookies", "FALSE")],
            held(&host, &endpoint, &protocol, &object_name, false, true),
        ),
        (vec![("EnableCookies", " true ")], defaults()),
        (
            vec![("SetEnableCookies", "false")],
            held(&host, &endpoint, &protocol, &object_name, false, true),
        ),
        (
            vec![("enablecookies", "false")],
            held(&host, &endpoint, &protocol, &object_name, false, true),
        ),
        (
            vec![("Host", " a b ")],
            held(" a b ", &endpoint, &protocol, &object_name, true, true),
        ),
        (vec![("Host", "")], defaults()),
        (
            vec![("Endpoint", "")],
            held(&host, "", &protocol, &object_name, true, true),
        ),
        (
            vec![("Protocol", "HTTPS")],
            held(&host, &endpoint, "HTTPS", &object_name, true, true),
        ),
        (
            vec![("Protocol", "http"), ("SetProtocol", "https")],
            held(&host, &endpoint, "https", &object_name, true, true),
        ),
        (
            vec![("ObjectName", "$a")],
            held(&host, &endpoint, &protocol, "$a", true, true),
        ),
        (
            vec![("Minify", "false")],
            held(&host, &endpoint, &protocol, &object_name, true, false),
        ),
    ] {
        assert_eq!(
            settings(&with(&parameters).unwrap()),
            expected,
            "{parameters:?}"
        );
    }
}

#[test]
fn values_dotnet_cannot_read_are_refused() {
    let builder = type_name::<JavaScriptBuilderElementBuilder>();
    for (name, setting, value) in [
        ("EnableCookies", "SetEnableCookies", "0"),
        ("EnableCookies", "SetEnableCookies", ""),
        ("EnableCookies", "SetEnableCookies", "yes"),
        ("Minify", "SetMinify", "x"),
    ] {
        assert_eq!(
            message(with(&[(name, value)])),
            failed(&format!(
                "Method '{setting}' on builder '{builder}' for element 0 expects a parameter \
                 of type 'Boolean'. Failed to parse value '{value}' using the static \
                 'TryParse' method."
            )),
            "{name} '{value}'"
        );
    }
}

/// .NET gives the builder's refusal as the inner message of "Exception has
/// been thrown by the target of an invocation.", and its text lacks the
/// quote before `https`. The crates give the builder's message, as in code.
#[test]
fn a_protocol_other_than_http_or_https_is_refused() {
    for protocol in ["ftp", "", " http"] {
        assert_eq!(
            message(with(&[("Protocol", protocol)])),
            failed(&format!(
                "Invalid protocol in configuration ({protocol}), must be 'http' or 'https'"
            )),
            "'{protocol}'"
        );
    }
}

/// .NET logs the refusal at critical level and gives it as the inner
/// message of "Exception has been thrown by the target of an invocation.",
/// as "JavaScriptBuilder ObjectName '1bad' is invalid. This must be a valid
/// JavaScript identifier that is not a reserved word." The crates log it at
/// error level, the log facade's highest, and give the builder's message,
/// as in code.
#[test]
fn an_object_name_that_is_not_an_identifier_is_refused_and_logged() {
    for name in [
        "1bad",
        "",
        "class",
        "fiftyoneDegreesManager",
        "undefined",
        " a",
    ] {
        let before = records().len();
        let refusal = format!(
            "The JavaScript object name '{name}' is not valid. It must be a valid JavaScript \
             identifier that is not a reserved word."
        );
        assert_eq!(
            message(with(&[("ObjectName", name)])),
            failed(&refusal),
            "'{name}'"
        );
        assert_eq!(
            records()[before..],
            [(log::Level::Error, refusal.clone())],
            "'{name}'"
        );
    }
}

/// .NET names a `List<string>` given in code ``List`1`` and an array
/// `String[]`, and the crates have one list form.
#[test]
fn a_list_is_refused_as_dotnet_refuses_it() {
    assert_eq!(
        message(one(
            ElementOptions::new(BUILDER_NAME).parameter("Host", vec!["a"])
        )),
        failed("Object of type 'System.String[]' cannot be converted to type 'System.String'.")
    );
    let builder = type_name::<JavaScriptBuilderElementBuilder>();
    assert_eq!(
        message(one(
            ElementOptions::new(BUILDER_NAME).parameter("EnableCookies", vec!["true"])
        )),
        failed(&format!(
            "Method 'SetEnableCookies' on builder '{builder}' for element 0 expects a \
             parameter of type 'Boolean' but supplied object is of type 'String[]'."
        ))
    );
}

/// .NET's configuration reader gives a JSON `null` as empty text, so a
/// `null` host or endpoint is empty and a `null` object name or cookies
/// setting is refused as empty text is.
#[test]
fn a_json_null_is_read_as_empty_text() {
    let file = |parameters: &str| {
        PipelineOptions::from_json(&format!(
            r#"{{ "PipelineOptions": {{ "Elements": [ {{
                "BuilderName": "{BUILDER_NAME}", "BuildParameters": {{ {parameters} }} }} ] }} }}"#
        ))
        .unwrap()
    };
    let (_, _, protocol, object_name, _, _) = defaults();
    assert_eq!(
        settings(&build(&file(r#""Host": null, "Endpoint": null"#)).unwrap()),
        held("", "", &protocol, &object_name, true, true)
    );
    assert_eq!(
        message(build(&file(r#""ObjectName": null"#))),
        failed(
            "The JavaScript object name '' is not valid. It must be a valid JavaScript \
             identifier that is not a reserved word."
        )
    );
    let builder = type_name::<JavaScriptBuilderElementBuilder>();
    assert_eq!(
        message(build(&file(r#""EnableCookies": null"#))),
        failed(&format!(
            "Method 'SetEnableCookies' on builder '{builder}' for element 0 expects a \
             parameter of type 'Boolean'. Failed to parse value '' using the static \
             'TryParse' method."
        ))
    );
}

/// The crates' own steps, template and user prompt settings are set in
/// code only, so a configuration naming one fails as it does in .NET,
/// whose builder has none.
#[test]
fn an_unknown_setting_lists_the_six_settings() {
    let builder = type_name::<JavaScriptBuilderElementBuilder>();
    for (name, upper) in [
        ("Unknown", "UNKNOWN"),
        ("UserPrompt", "USERPROMPT"),
        ("Template", "TEMPLATE"),
        ("Steps", "STEPS"),
    ] {
        assert_eq!(
            message(with(&[(name, "1")])),
            failed(&format!(
                "Builder '{builder}' for element 0 has no 'Set' methods or 'Build' methods \
                 that match (case-insensitively) the following parameters: {upper}. The \
                 available configuration methods on this builder are: \n\
                 SetEnableCookies (Boolean)\n\
                 SetHost (String)\n\
                 SetEndpoint (String)\n\
                 SetProtocol (String)\n\
                 SetObjectName (String)\n\
                 SetMinify (Boolean)\n\
                 The available 'Build' methods have the following signatures: \n\n"
            )),
            "{name}"
        );
    }
}

/// The file .NET read, giving its elements and settings, and the element
/// made from it writes the script that the same element made in code
/// writes.
#[test]
fn a_json_file_written_for_dotnet_makes_the_same_element_as_code() {
    let options = PipelineOptions::from_json(
        r#"{
          "PipelineOptions": {
            "Elements": [
              { "BuilderName": "SequenceElementBuilder" },
              { "BuilderName": "JsonBuilderElementBuilder" },
              {
                "BuilderName": "JavaScriptBuilderElement",
                "BuildParameters": {
                  "Host": "example.com",
                  "Endpoint": "/api/v4/json",
                  "Protocol": "https",
                  "ObjectName": "myObj",
                  "EnableCookies": false,
                  "Minify": false
                }
              }
            ]
          }
        }"#,
    )
    .unwrap();
    let configured = build(&options).unwrap();
    assert_eq!(
        keys(&configured),
        ["sequence", "json-builder", "javascriptbuilderelement"]
    );
    assert_eq!(
        settings(&configured),
        held(
            "example.com",
            "/api/v4/json",
            "https",
            "myObj",
            false,
            false
        )
    );

    let element = JavaScriptBuilderElement::builder()
        .set_host("example.com")
        .set_endpoint("/api/v4/json")
        .set_protocol("https")
        .unwrap()
        .set_object_name("myObj")
        .unwrap()
        .set_enable_cookies(false)
        .set_minify(false)
        .build();
    let coded = Pipeline::builder()
        .add_element(Arc::new(SequenceElement::new()))
        .add_element(Arc::new(JsonBuilderElement::new()))
        .add_element(Arc::new(element))
        .build()
        .unwrap();
    let written = script(&configured);
    assert_eq!(written, script(&coded));
    assert!(
        written.contains("https://example.com/api/v4/json"),
        "{written}"
    );
    assert!(written.contains("myObj"), "{written}");
}
