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

//! The on-premise engine's property, component and value metadata, read from
//! the data file as the .NET engine reads it.
//!
//! These run against the Lite Hash data file, or the file `51DEGREES_DD_PATH`
//! names, and skip with a note when there is none. They check what every Hash
//! data file holds, so they pass against any tier.

use std::path::PathBuf;
use std::sync::Arc;

use fiftyone_device_detection_onpremise::{
    DeviceDetectionOnPremiseEngine, DeviceDetectionOnPremiseEngineBuilder,
};
use fiftyone_device_detection_shared::{DeviceData, DEVICE_DATA_KEY};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{ElementData, Evidence, FlowElement, Pipeline, PropertyValueType};
use fiftyone_pipeline_engines::{AspectEngine, MissingPropertyReason};

/// The match metrics, in the order the .NET engine lists them after the data
/// file's properties.
const METRICS: [&str; 7] = [
    "MatchedNodes",
    "Difference",
    "Drift",
    "DeviceId",
    "UserAgents",
    "Iterations",
    "Method",
];

/// A mobile Safari on iPhone User-Agent.
const MOBILE_USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
    AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";

/// The Hash data file `51DEGREES_DD_PATH` names, or the Lite file in the `cxx`
/// submodule.
fn data_file() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("51DEGREES_DD_PATH") {
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

/// An engine over the data file, restricted to `properties` when any are
/// given, or [`None`] with a note when there is no data file.
fn engine(properties: &[&str]) -> Option<Arc<DeviceDetectionOnPremiseEngine>> {
    let Some(path) = data_file() else {
        eprintln!("no Hash data file found; skipping");
        return None;
    };
    let mut builder = DeviceDetectionOnPremiseEngineBuilder::new(path)
        .performance_profile(PerformanceProfile::LowMemory);
    for property in properties {
        builder = builder.property(*property);
    }
    Some(builder.build().expect("the data file loads"))
}

/// The device data for a User-Agent from a one-element pipeline.
fn detect(
    engine: &Arc<DeviceDetectionOnPremiseEngine>,
    user_agent: &str,
) -> fiftyone_device_detection_shared::DeviceDataBase {
    let element: Arc<dyn FlowElement> = Arc::clone(engine) as Arc<dyn FlowElement>;
    let pipeline = Pipeline::builder()
        .add_element(element)
        .build()
        .expect("pipeline builds");
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", user_agent)
            .build(),
    );
    data.process().expect("processing succeeds");
    data.get(DEVICE_DATA_KEY)
        .expect("device data was produced")
        .clone()
}

fn names(engine: &DeviceDetectionOnPremiseEngine) -> Vec<String> {
    engine
        .aspect_properties()
        .iter()
        .map(|property| property.name().to_owned())
        .collect()
}

#[test]
fn every_data_file_property_is_listed_then_the_metrics() {
    let Some(engine) = engine(&[]) else {
        return;
    };
    let names = names(&engine);
    let (file, metrics) = names.split_at(names.len() - METRICS.len());
    assert_eq!(metrics, METRICS);
    assert!(file.iter().any(|name| name == "IsMobile"));
    assert!(engine.aspect_properties().iter().all(|p| p.available()));
    // The core and 51Degrees metadata list the same properties in the same
    // order.
    let core: Vec<&str> = engine
        .properties()
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    let fiftyone: Vec<&str> = engine
        .fiftyone_properties()
        .iter()
        .map(|p| p.name())
        .collect();
    assert_eq!(core, names);
    assert_eq!(fiftyone, names);
    assert_eq!(engine.available_properties(), names);
}

#[test]
fn a_restricted_engine_lists_every_property_and_marks_the_rest_unavailable() {
    let (Some(all), Some(one)) = (engine(&[]), engine(&["IsMobile"])) else {
        return;
    };
    assert_eq!(names(&one), names(&all));
    let available: Vec<&str> = one
        .aspect_properties()
        .iter()
        .filter(|p| p.available())
        .map(|p| p.name())
        .collect();
    let mut expected = vec!["IsMobile"];
    expected.extend(METRICS);
    assert_eq!(available, expected);
    assert_eq!(one.available_properties(), expected);
    let browser_name = one
        .properties()
        .iter()
        .find(|p| p.name == "BrowserName")
        .expect("BrowserName is in every Hash data file");
    assert!(!browser_name.available);
    // No script the engine gives no value for is offered as available.
    assert!(one
        .properties()
        .iter()
        .filter(|p| p.value_type == PropertyValueType::JavaScript)
        .all(|p| !p.available));
}

#[test]
fn property_metadata_comes_from_the_data_file() {
    let Some(engine) = engine(&[]) else {
        return;
    };
    let tier = engine.data_source_tier().to_owned();
    let find = |name: &str| {
        engine
            .fiftyone_properties()
            .iter()
            .find(|p| p.name() == name)
            .unwrap_or_else(|| panic!("{name} is in every Hash data file"))
    };

    let is_mobile = find("IsMobile");
    assert_eq!(is_mobile.core().value_type, PropertyValueType::Bool);
    assert_eq!(is_mobile.core().category, "Device");
    assert!(is_mobile
        .aspect()
        .description()
        .starts_with("Indicates if the device"));
    assert!(
        is_mobile
            .aspect()
            .data_tiers_where_present()
            .contains(&tier),
        "{:?}",
        is_mobile.aspect().data_tiers_where_present()
    );
    assert_eq!(is_mobile.component_id(), Some(1));
    assert_eq!(is_mobile.component_name(), "HardwarePlatform");
    assert!(is_mobile.mandatory() && is_mobile.show() && is_mobile.show_values());
    assert!(!is_mobile.list() && !is_mobile.obsolete());
    let default_value = is_mobile
        .default_value()
        .expect("IsMobile has a default value");
    assert_eq!(default_value.name, "False");
    assert!(!default_value.description.is_empty());
    // A data file property's values are read when asked for, not copied.
    assert!(is_mobile.values().is_empty());

    let browser_name = find("BrowserName");
    assert_eq!(browser_name.core().value_type, PropertyValueType::String);
    assert_eq!(browser_name.component_name(), "BrowserUA");
    assert!(
        browser_name.url().starts_with("http"),
        "{}",
        browser_name.url()
    );

    assert_eq!(
        find("JavascriptHardwareProfile").core().value_type,
        PropertyValueType::JavaScript
    );

    let method = find("Method");
    assert_eq!(method.core().category, "Device Metrics");
    assert_eq!(method.component_name(), "Metrics");
    assert_eq!(method.values().len(), 4);
}

#[test]
fn components_hold_their_properties_then_the_metrics() {
    let Some(engine) = engine(&[]) else {
        return;
    };
    let components = engine.components();
    let ids_and_names: Vec<(u8, &str)> = components
        .iter()
        .map(|c| (c.component_id, c.name.as_str()))
        .collect();
    assert_eq!(
        ids_and_names,
        [
            (1, "HardwarePlatform"),
            (2, "SoftwarePlatform"),
            (3, "BrowserUA"),
            (4, "Crawler"),
            (255, "Metrics"),
        ]
    );
    for component in &components[..4] {
        assert!(component.default_profile_id.is_some_and(|id| id > 0));
    }
    assert_eq!(components[4].default_profile_id, None);
    let metrics: Vec<&str> = components[4]
        .properties()
        .iter()
        .map(|p| p.name())
        .collect();
    assert_eq!(metrics, METRICS);
    assert!(components[0].property("IsMobile").is_some());
    assert!(components[2].property("BrowserName").is_some());

    // Each property is in exactly one component, its own.
    for property in engine.fiftyone_properties() {
        let holding: Vec<u8> = components
            .iter()
            .filter(|c| c.property(property.name()).is_some())
            .map(|c| c.component_id)
            .collect();
        assert_eq!(
            holding,
            [property.component_id().expect("a component")],
            "{}",
            property.name()
        );
    }
}

#[test]
fn property_values_are_read_from_the_data_file() {
    let Some(engine) = engine(&["IsMobile"]) else {
        return;
    };
    let mut is_mobile: Vec<String> = engine
        .property_values("IsMobile")
        .expect("the values are read")
        .expect("IsMobile is listed")
        .into_iter()
        .map(|value| value.name)
        .collect();
    is_mobile.sort_unstable();
    assert_eq!(is_mobile, ["False", "True"]);

    // An engine opened without a property still gives the values the data
    // file holds for it, as its metadata lists it.
    let browser_names = engine
        .property_values("BrowserName")
        .expect("the values are read")
        .expect("BrowserName is listed");
    assert!(browser_names.iter().any(|value| value.name == "Safari"));
    assert!(browser_names.iter().all(|value| !value.name.is_empty()));

    let methods: Vec<String> = engine
        .property_values("Method")
        .expect("the values are read")
        .expect("Method is listed")
        .into_iter()
        .map(|value| value.name)
        .collect();
    assert_eq!(methods, ["NONE", "PERFORMANCE", "COMBINED", "PREDICTIVE"]);
    assert_eq!(
        engine.property_values("MatchedNodes").unwrap(),
        Some(Vec::new())
    );

    assert_eq!(engine.property_values("NotAProperty").unwrap(), None);
    // Names are matched exactly, as .NET matches them.
    assert_eq!(engine.property_values("ismobile").unwrap(), None);
}

/// The issue's check. An engine restricted to IsMobile and asked for
/// BrowserName, for a property the data file lacks and for `NotAProperty`
/// gives .NET's reason and text each time.
#[test]
fn a_restricted_engine_gives_the_dotnet_reasons() {
    let Some(engine) = engine(&["IsMobile"]) else {
        return;
    };
    let browser_name = engine.missing_property_reason("BrowserName");
    assert_eq!(
        browser_name.reason,
        MissingPropertyReason::PropertyExcludedFromConfig
    );
    assert_eq!(
        browser_name.description,
        "Property 'BrowserName' not found in data for element 'device'. This is \
         because the property has been excluded when configuring the engine."
    );

    let not_a_property = engine.missing_property_reason("NotAProperty");
    assert_eq!(not_a_property.reason, MissingPropertyReason::Unknown);

    let lacked = ["HardwareVendor", "HardwareModel", "HardwareName"]
        .into_iter()
        .find(|name| !engine.properties().iter().any(|p| p.name == *name));
    match lacked {
        Some(name) => {
            let result = engine.missing_property_reason(name);
            assert_eq!(
                result.reason,
                MissingPropertyReason::DataFileUpgradeRequired
            );
            assert!(
                result.description.contains(
                    "It is available with the Unknown license/data for the \
                     DeviceDetectionOnPremiseEngine."
                ),
                "{}",
                result.description
            );
        }
        None => eprintln!("the data file holds every hardware property tried"),
    }

    // Device data gives the same reasons.
    let device = detect(&engine, MOBILE_USER_AGENT);
    assert!(*device.is_mobile().value().unwrap());
    for name in ["BrowserName", "NotAProperty"] {
        assert_eq!(
            device.get(name).unwrap_err().message,
            engine.missing_property_reason(name).description,
            "{name}"
        );
    }
    assert_eq!(
        device.browser_name().no_value_message(),
        Some(browser_name.description.as_str())
    );
}

#[test]
fn a_property_named_in_another_case_is_read() {
    let Some(engine) = engine(&["ismobile"]) else {
        return;
    };
    let is_mobile = engine
        .aspect_properties()
        .iter()
        .find(|p| p.name() == "IsMobile")
        .expect("IsMobile is listed with the data file's spelling");
    assert!(is_mobile.available());
    let device = detect(&engine, MOBILE_USER_AGENT);
    assert!(*device.is_mobile().value().expect("IsMobile has a value"));
}

/// A tier set on the builder is the one the engine reports, but the missing
/// property rules compare each property's tiers with the tier the data file
/// names, so a property left out of the configuration is still excluded
/// rather than one to upgrade for, from the engine and from device data.
#[test]
fn a_tier_set_on_the_builder_leaves_an_excluded_property_excluded() {
    let Some(path) = data_file() else {
        eprintln!("no Hash data file found; skipping");
        return;
    };
    let engine = Arc::new(
        DeviceDetectionOnPremiseEngineBuilder::new(path)
            .performance_profile(PerformanceProfile::LowMemory)
            .property("IsMobile")
            .data_source_tier("Premium")
            .build()
            .expect("the data file loads"),
    );
    assert_eq!(engine.data_source_tier(), "Premium");

    let browser_name = engine.missing_property_reason("BrowserName");
    assert_eq!(
        browser_name.reason,
        MissingPropertyReason::PropertyExcludedFromConfig
    );

    let device = detect(&engine, MOBILE_USER_AGENT);
    assert_eq!(
        device.browser_name().no_value_message(),
        Some(browser_name.description.as_str())
    );
}
