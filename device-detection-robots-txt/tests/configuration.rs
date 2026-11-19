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

//! The robots.txt engine made from a configuration, with the settings and
//! messages of .NET's `RobotsTxtEngineBuilder`. The comparison with .NET
//! builds the same configurations (`parity/corpus/configuration.json`).
//! The test that makes the engine needs the Lite Hash data file in the
//! `cxx` submodule, or the file `_51DEGREES_DD_PATH` names, and skips
//! without one.

use std::path::PathBuf;
use std::sync::Arc;

use fiftyone_device_detection_robots_txt::{configuration, RobotsTxtEngine};
use fiftyone_pipeline_core::configuration::{BuilderRegistry, ElementOptions, PipelineOptions};
use fiftyone_pipeline_core::{Error, FlowElement, Pipeline, PipelineBuilder, Result};

const BUILDER: &str = "RobotsTxtEngineBuilder";
const BUILDER_TYPE: &str = "fiftyone_device_detection_robots_txt::engine::RobotsTxtEngineBuilder";
const NO_SUB_ELEMENTS: &str = "No SubElements";
const NO_INNER: &str = "No Inner Exception.";

fn registry() -> BuilderRegistry {
    let mut registry = BuilderRegistry::new();
    fiftyone_device_detection_onpremise::configuration::add_builders(&mut registry);
    configuration::add_builders(&mut registry);
    registry
}

fn build(options: &PipelineOptions) -> Result<Arc<Pipeline>> {
    PipelineBuilder::new().build_from_configuration(&registry(), options)
}

/// The engine named `name` with `parameters`, alone in a pipeline.
fn named(name: &str, parameters: &[(&str, &str)]) -> Result<Arc<Pipeline>> {
    let element = parameters
        .iter()
        .fold(ElementOptions::new(name), |element, (n, v)| {
            element.parameter(*n, *v)
        });
    build(&PipelineOptions::new().element(element))
}

fn with(parameters: &[(&str, &str)]) -> Result<Arc<Pipeline>> {
    named(BUILDER, parameters)
}

fn message(result: Result<Arc<Pipeline>>) -> String {
    match result {
        Err(Error::PipelineConfiguration { message }) => message,
        Err(other) => panic!("not a configuration error: {other}"),
        Ok(_) => panic!("the pipeline was built"),
    }
}

/// The message .NET gives for an element that cannot be made, `inner`
/// being its inner exception's message.
fn failed_with(message: &str, inner: &str) -> String {
    format!(
        "Configuration failed to build.\nBuilderName: {BUILDER}\nSubElement Builder Names: \
         {NO_SUB_ELEMENTS}\n{message}\n{inner}\n"
    )
}

fn failed(message: &str) -> String {
    failed_with(message, NO_INNER)
}

/// The Hash data file `_51DEGREES_DD_PATH` names, or the Lite file in the `cxx`
/// submodule.
fn data_file() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("_51DEGREES_DD_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .to_path_buf();
    let roots = [workspace.clone(), workspace.parent()?.to_path_buf()];
    roots
        .into_iter()
        .map(|root| {
            root.join("cxx")
                .join("data")
                .join("device-detection")
                .join("51Degrees-LiteV4.1.hash")
        })
        .find(|path| path.is_file())
}

/// .NET's `GetRuntimeMethods` lists the builder's own methods, then those
/// of `PropertyKeyedEngineBuilderBase`, then those of
/// `AspectEngineBuilderBase` that it does not override, keeping each
/// public method taking one value. It gives `RuntimeParameterInfo` as each
/// type. The only build method takes nothing.
#[test]
fn an_unknown_setting_lists_dotnets_settings() {
    let listed = [
        "SetPerformanceProfile (PerformanceProfiles)",
        "SetTdlSourceResolver (ITdlSourceResolver)",
        "UseDefaultTdlSourceResolver (String)",
        "SetUserAgent (String)",
        "SetCacheSize (Int32)",
        "SetCacheHitOrMiss (Boolean)",
        "SetCache (CacheConfiguration)",
        "SetLazyLoading (LazyLoadingConfiguration)",
        "SetLazyLoadingTimeout (Int32)",
        "SetProperties (List`1)",
        "SetProperty (String)",
    ]
    .iter()
    .map(|s| format!("{s}\n"))
    .collect::<String>();
    assert_eq!(
        message(with(&[("NoSuchSetting", "1")])),
        failed(&format!(
            "Builder '{BUILDER_TYPE}' for element 0 has no 'Set' methods or 'Build' methods \
             that match (case-insensitively) the following parameters: NOSUCHSETTING. The \
             available configuration methods on this builder are: \n{listed}The available \
             'Build' methods have the following signatures: \n\n"
        ))
    );
}

/// Each setting .NET reads is read, so the pipeline gets as far as adding
/// the engine, which refuses a pipeline with no Device Detection engine.
#[test]
fn the_settings_dotnet_reads_are_read() {
    for name in [BUILDER, "RobotsTxtEngine", "robotstxtenginebuilder"] {
        let refused = named(
            name,
            &[
                ("UserAgent", "Example/1.0"),
                ("UseDefaultTdlSourceResolver", "Example/2.0"),
                ("CacheSize", "100"),
                ("CacheHitOrMiss", "true"),
                ("LazyLoadingTimeout", "1000"),
                ("Properties", "PlainText"),
                ("Property", "AnnotatedText"),
            ],
        )
        .err()
        .expect("the pipeline is refused")
        .to_string();
        assert!(
            refused.contains(
                "Ensure DeviceDetectionOnPremiseEngine is added to the pipeline before the \
                 robots.txt engine."
            ),
            "{name}: {refused}"
        );
    }
}

/// .NET's builder throws `NotImplementedException` once the profile is
/// read, and gives it as the inner message of "Exception has been thrown by
/// the target of an invocation.". The crates give the refusal itself.
#[test]
fn a_performance_profile_is_refused() {
    for text in ["MaxPerformance", "lowmemory", "7"] {
        assert_eq!(
            message(with(&[("PerformanceProfile", text)])),
            failed("The method or operation is not implemented."),
            "{text}"
        );
    }
    assert_eq!(
        message(with(&[("PerformanceProfile", "99999999999")])),
        failed("Value was either too large or too small for an Int32.")
    );
    assert_eq!(
        message(with(&[("PerformanceProfile", "Bogus")])),
        failed_with(
            &format!(
                "Method 'SetPerformanceProfile' on builder '{BUILDER_TYPE}' for element 0 \
                 expects a parameter of type 'PerformanceProfiles'. Failed to parse enum value \
                 'Bogus'. Valid values are: LowMemory, MaxPerformance, HighPerformance, \
                 Balanced, BalancedTemp"
            ),
            "Requested value 'Bogus' was not found."
        )
    );
}

/// .NET's GitHub resolver refuses blank text, and .NET gives the refusal as
/// the inner message of "Exception has been thrown by the target of an
/// invocation.". The crates give the refusal itself.
#[test]
fn a_blank_user_agent_is_refused() {
    for setting in ["UserAgent", "UseDefaultTdlSourceResolver"] {
        for text in ["", "  "] {
            assert_eq!(
                message(with(&[(setting, text)])),
                failed("User-Agent must be supplied. (Parameter 'userAgent')"),
                "{setting} '{text}'"
            );
        }
    }
}

#[test]
fn a_setting_given_in_code_only_is_refused() {
    for (name, setting, type_name) in [
        (
            "TdlSourceResolver",
            "SetTdlSourceResolver",
            "ITdlSourceResolver",
        ),
        ("Cache", "SetCache", "CacheConfiguration"),
        ("LazyLoading", "SetLazyLoading", "LazyLoadingConfiguration"),
    ] {
        assert_eq!(
            message(with(&[(name, "x")])),
            failed(&format!(
                "Method '{setting}' on builder '{BUILDER_TYPE}' for element 0 expects a \
                 parameter of type '{type_name}' but this type does not have the expected \
                 'TryParse' method (or has multiples that cannot be resolved)."
            )),
            "{name}"
        );
    }
}

/// After a Device Detection engine, the engine is made with the properties
/// named, and the pipeline holds it.
#[test]
fn the_engine_is_made_after_device_detection() {
    let Some(path) = data_file() else {
        eprintln!("no Hash data file found; skipping");
        return;
    };
    let device = ElementOptions::new("DeviceDetectionHashEngineBuilder")
        .parameter("DataFile", path.to_string_lossy().into_owned())
        .parameter("CreateTempDataCopy", "false")
        .parameter("AutoUpdate", "false")
        .parameter("DataFileSystemWatcher", "false")
        .parameter("DataUpdateOnStartup", "false")
        .parameter("PerformanceProfile", "LowMemory");
    let robots = ElementOptions::new("RobotsTxtEngine")
        .parameter("UserAgent", "Example/1.0")
        .parameter("Properties", "plaintext");
    let pipeline = build(&PipelineOptions::new().element(device).element(robots))
        .expect("the pipeline builds");
    let engine = pipeline
        .resource::<RobotsTxtEngine>()
        .expect("the robots.txt engine");
    assert!(engine.model().is_some());
    let names: Vec<&str> = pipeline
        .flow_elements()
        .iter()
        .map(|element| element.data_key())
        .collect();
    assert_eq!(names, ["device", "robotstxt"]);
    let properties: Vec<&str> = engine
        .properties()
        .iter()
        .map(|property| property.name.as_str())
        .collect();
    assert_eq!(properties, ["PlainText"]);
}
