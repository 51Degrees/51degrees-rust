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

//! The TAC, Native and property keyed engines made from a configuration,
//! with the settings and messages of .NET's builders. The test that makes
//! the engines needs the Lite Hash data file in the `cxx` submodule, or the
//! file `_51DEGREES_DD_PATH` names, and skips without one.

use std::path::PathBuf;
use std::sync::Arc;

use fiftyone_device_detection_property_keyed::{configuration, PropertyKeyedDeviceEngine};
use fiftyone_device_detection_shared::MultiDeviceData;
use fiftyone_pipeline_core::configuration::{BuilderRegistry, ElementOptions, PipelineOptions};
use fiftyone_pipeline_core::{Error, Evidence, Pipeline, PipelineBuilder, Result, TypedKey};

/// Each builder's name, and its full name in errors.
const BUILDERS: [(&str, &str); 3] = [
    (
        "TacEngineBuilder",
        "fiftyone_device_detection_property_keyed::engine::TacEngineBuilder",
    ),
    (
        "NativeEngineBuilder",
        "fiftyone_device_detection_property_keyed::engine::NativeEngineBuilder",
    ),
    (
        "PropertyKeyedDeviceEngineBuilder",
        "fiftyone_device_detection_property_keyed::engine::PropertyKeyedDeviceEngineBuilder",
    ),
];
const GENERIC: &str = "PropertyKeyedDeviceEngineBuilder";
const NO_SUB_ELEMENTS: &str = "No SubElements";
const NO_INNER: &str = "No Inner Exception.";
const NO_DEVICE_DETECTION: &str =
    "Ensure DeviceDetectionOnPremiseEngine is added to the pipeline before the property keyed \
     engine.";

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

fn message(result: Result<Arc<Pipeline>>) -> String {
    match result {
        Err(Error::PipelineConfiguration { message }) => message,
        Err(other) => panic!("not a configuration error: {other}"),
        Ok(_) => panic!("the pipeline was built"),
    }
}

/// The message .NET gives for the element `builder` that cannot be made,
/// `inner` being its inner exception's message.
fn failed_with(builder: &str, message: &str, inner: &str) -> String {
    format!(
        "Configuration failed to build.\nBuilderName: {builder}\nSubElement Builder Names: \
         {NO_SUB_ELEMENTS}\n{message}\n{inner}\n"
    )
}

fn failed(builder: &str, message: &str) -> String {
    failed_with(builder, message, NO_INNER)
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

/// .NET's `GetRuntimeMethods` lists the methods of
/// `PropertyKeyedDeviceEngineBaseBuilder`, then those of
/// `PropertyKeyedEngineBuilderBase`, then those of `AspectEngineBuilderBase`
/// that it does not override, keeping each public method taking one value,
/// as the robots.txt builder's listing shows for the last two. None of the
/// three builders declares a public method of its own. The only build
/// method takes nothing.
#[test]
fn an_unknown_setting_lists_dotnets_settings() {
    let listed = [
        "SetKeyProperty (String)",
        "SetElementDataKey (String)",
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
    for (builder, full_name) in BUILDERS {
        assert_eq!(
            message(named(builder, &[("NoSuchSetting", "1")])),
            failed(
                builder,
                &format!(
                    "Builder '{full_name}' for element 0 has no 'Set' methods or 'Build' \
                     methods that match (case-insensitively) the following parameters: \
                     NOSUCHSETTING. The available configuration methods on this builder are: \
                     \n{listed}The available 'Build' methods have the following signatures: \
                     \n\n"
                )
            ),
            "{builder}"
        );
    }
}

/// Each setting .NET reads is read, so the pipeline gets as far as adding
/// the engine, which refuses a pipeline with no Device Detection engine.
#[test]
fn the_settings_dotnet_reads_are_read() {
    for (builder, _) in BUILDERS {
        let short = builder.trim_end_matches("Builder");
        let lower = builder.to_lowercase();
        for name in [builder, short, lower.as_str()] {
            let refused = named(
                name,
                &[
                    ("KeyProperty", "HardwareName"),
                    ("ElementDataKey", "devices"),
                    ("CacheSize", "100"),
                    ("CacheHitOrMiss", "true"),
                    ("LazyLoadingTimeout", "1000"),
                    ("Properties", "Profiles"),
                    ("Property", "Profiles"),
                ],
            )
            .err()
            .expect("the pipeline is refused")
            .to_string();
            assert!(refused.contains(NO_DEVICE_DETECTION), "{name}: {refused}");
        }
    }
}

#[test]
fn a_setting_given_in_code_only_is_refused() {
    for (builder, full_name) in BUILDERS {
        for (name, setting, type_name) in [
            ("Cache", "SetCache", "CacheConfiguration"),
            ("LazyLoading", "SetLazyLoading", "LazyLoadingConfiguration"),
        ] {
            assert_eq!(
                message(named(builder, &[(name, "x")])),
                failed(
                    builder,
                    &format!(
                        "Method '{setting}' on builder '{full_name}' for element 0 expects a \
                         parameter of type '{type_name}' but this type does not have the \
                         expected 'TryParse' method (or has multiples that cannot be \
                         resolved)."
                    )
                ),
                "{builder} {name}"
            );
        }
    }
}

/// The property keyed builder has no key property until one is set, and
/// refuses an empty one, as .NET's `CreateEngine` does.
#[test]
fn the_property_keyed_builder_needs_a_key_property() {
    let needed = failed(GENERIC, "KeyProperty must be set before building");
    assert_eq!(message(named(GENERIC, &[])), needed);
    assert_eq!(message(named(GENERIC, &[("KeyProperty", "")])), needed);
}

/// The TAC and Native engines refuse an empty key property or data key with
/// the message of .NET's `ArgumentException`. .NET gives it as the inner
/// message of "Exception has been thrown by the target of an invocation.".
/// The crates give the refusal itself.
#[test]
fn an_empty_key_property_or_data_key_is_refused() {
    for builder in ["TacEngineBuilder", "NativeEngineBuilder"] {
        assert_eq!(
            message(named(builder, &[("KeyProperty", "")])),
            failed(
                builder,
                "keyProperty is required and cannot be null or empty. (Parameter \
                 'keyProperty')"
            ),
            "{builder}"
        );
    }
    for builder in ["TacEngineBuilder", "NativeEngineBuilder", GENERIC] {
        assert_eq!(
            message(named(
                builder,
                &[("KeyProperty", "TAC"), ("ElementDataKey", "")]
            )),
            failed(
                builder,
                "elementDataKey is required and cannot be null or empty. (Parameter \
                 'elementDataKey')"
            ),
            "{builder}"
        );
    }
}

/// After a Device Detection engine, each engine is made with the key
/// property and data key configured, and the pipeline holds them. The
/// property keyed engine finds `IsMobile`, which every data file holds,
/// ignoring the case it was configured in, and looks up its value.
#[test]
fn the_engines_are_made_after_device_detection() {
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
    let tac = ElementOptions::new("TacEngine").parameter("CacheSize", "100");
    let native = ElementOptions::new("NativeEngineBuilder");
    let mobile = ElementOptions::new("PropertyKeyedDeviceEngine")
        .parameter("KeyProperty", "ismobile")
        .parameter("ElementDataKey", "mobile");
    let pipeline = build(
        &PipelineOptions::new()
            .element(device)
            .element(tac)
            .element(native)
            .element(mobile),
    )
    .expect("the pipeline builds");
    let keys: Vec<&str> = pipeline
        .flow_elements()
        .iter()
        .map(|element| element.data_key())
        .collect();
    assert_eq!(keys, ["device", "hardware", "hardware", "mobile"]);
    let engines: Vec<Arc<PropertyKeyedDeviceEngine>> = pipeline
        .flow_elements()
        .iter()
        .filter_map(|element| {
            Arc::clone(element)
                .into_any()?
                .downcast::<PropertyKeyedDeviceEngine>()
                .ok()
        })
        .collect();
    let key_properties: Vec<&str> = engines.iter().map(|e| e.key_property()).collect();
    assert_eq!(key_properties, ["TAC", "NativeModel", "ismobile"]);
    assert_eq!(engines[2].evidence_key(), Some("query.ismobile"));
    let first = pipeline
        .resource::<PropertyKeyedDeviceEngine>()
        .expect("the pipeline holds the engines");
    assert!(Arc::ptr_eq(&first, &engines[0]));

    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("query.ismobile", "True").build());
    data.process().expect("the request is processed");
    let profiles = data
        .get(TypedKey::<MultiDeviceData>::new("mobile"))
        .expect("the property keyed engine's data")
        .profiles();
    assert!(!profiles.is_empty());
    assert!(data.warnings().is_empty());
}
