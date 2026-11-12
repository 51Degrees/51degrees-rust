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

//! The builder's matching settings and expected concurrency reach the data
//! set, and a refresh keeps them.
//!
//! These run against the Lite Hash data file, or the file `_51DEGREES_DD_PATH`
//! names, and skip with a note when neither is found.

#[allow(dead_code)]
#[path = "../../deprecated-names/environment.rs"]
mod deprecated_names;

use std::path::PathBuf;
use std::sync::Arc;

use fiftyone_device_detection_onpremise::{
    DeviceDetectionOnPremiseEngine, DeviceDetectionOnPremiseEngineBuilder,
};
use fiftyone_device_detection_shared::{DeviceData, DEVICE_DATA_KEY};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{Evidence, FlowElement, Pipeline};
use fiftyone_pipeline_engines::OnPremiseAspectEngine;

/// A desktop Chrome on Windows User-Agent.
const DESKTOP_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// A User-Agent that the Lite file matches only in the crawler component
/// with the profiles' own settings, and in more components with a drift or
/// a difference of 10.
const NOTHING_TO_MATCH: &str = "nothing to match";

/// The Hash data file `_51DEGREES_DD_PATH` names, or the Lite file in the
/// `cxx` submodule.
fn data_file() -> Option<PathBuf> {
    if let Ok(path) = crate::deprecated_names::var("_51DEGREES_DD_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .to_path_buf();
    [workspace.clone(), workspace.parent()?.to_path_buf()]
        .into_iter()
        .map(|root| root.join("cxx/data/device-detection/51Degrees-LiteV4.1.hash"))
        .find(|path| path.is_file())
}

/// The builder for the data file under `HighPerformance`.
fn builder(path: &PathBuf) -> DeviceDetectionOnPremiseEngineBuilder {
    DeviceDetectionOnPremiseEngineBuilder::new(path)
        .performance_profile(PerformanceProfile::HighPerformance)
}

/// The match method, device id, difference, drift and whether the device is
/// mobile, for a User-Agent.
fn detect(
    engine: &Arc<DeviceDetectionOnPremiseEngine>,
    user_agent: &str,
) -> (String, String, i64, i64, Option<bool>) {
    let element: Arc<dyn FlowElement> = engine.clone();
    let pipeline = Pipeline::builder().add_element(element).build().unwrap();
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", user_agent)
            .build(),
    );
    data.process().unwrap();
    let device = data.get(DEVICE_DATA_KEY).unwrap();
    (
        device.method().value().unwrap().clone(),
        device.device_id().value().unwrap().clone(),
        *device.difference().value().unwrap(),
        *device.drift().value().unwrap(),
        device.is_mobile().value().ok().copied(),
    )
}

/// The matched User-Agents of a detection.
fn matched_user_agents(engine: &Arc<DeviceDetectionOnPremiseEngine>) -> Vec<String> {
    let element: Arc<dyn FlowElement> = engine.clone();
    let pipeline = Pipeline::builder().add_element(element).build().unwrap();
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", DESKTOP_USER_AGENT)
            .build(),
    );
    data.process().unwrap();
    let device = data.get(DEVICE_DATA_KEY).unwrap();
    let user_agents = device.user_agents();
    user_agents.value().unwrap().clone()
}

/// The number of processors, as the default expected concurrency counts them.
fn processors() -> u16 {
    std::thread::available_parallelism().map_or(1, |count| count.get() as u16)
}

#[test]
fn the_graph_settings_choose_the_graph_detection_uses() {
    let Some(path) = data_file() else {
        eprintln!("no Hash data file found; skipping the graph settings");
        return;
    };
    let predictive = builder(&path).build().unwrap();
    assert_eq!(detect(&predictive, DESKTOP_USER_AGENT).0, "PREDICTIVE");

    let performance = builder(&path)
        .use_performance_graph(true)
        .use_predictive_graph(false)
        .build()
        .unwrap();
    assert_eq!(detect(&performance, DESKTOP_USER_AGENT).0, "PERFORMANCE");

    // The performance graph is tried first, so it matches here too.
    let both = builder(&path)
        .use_performance_graph(true)
        .use_predictive_graph(true)
        .build()
        .unwrap();
    assert_eq!(detect(&both, DESKTOP_USER_AGENT).0, "PERFORMANCE");
}

#[test]
fn a_data_set_with_neither_graph_is_refused() {
    let Some(path) = data_file() else {
        eprintln!("no Hash data file found; skipping neither graph");
        return;
    };
    let error = builder(&path)
        .use_performance_graph(false)
        .use_predictive_graph(false)
        .build()
        .err()
        .expect("the C library refuses a configuration with no graph");
    assert!(error.to_string().contains("InvalidConfig"), "{error}");
}

/// The number of components a device id names a profile for.
fn matched_components(device_id: &str) -> usize {
    device_id.split('-').filter(|id| *id != "0").count()
}

#[test]
fn a_difference_lets_a_match_with_that_difference_through() {
    let Some(path) = data_file() else {
        eprintln!("no Hash data file found; skipping difference");
        return;
    };
    let exact = builder(&path).build().unwrap();
    let (_, exact_id, difference, drift, _) = detect(&exact, NOTHING_TO_MATCH);
    assert_eq!((difference, drift), (0, 0));

    let loose = builder(&path).difference(10).build().unwrap();
    let (_, device_id, difference, drift, _) = detect(&loose, NOTHING_TO_MATCH);
    assert!(
        matched_components(&device_id) > matched_components(&exact_id),
        "{exact_id} then {device_id}"
    );
    assert!(difference > 0, "{difference}");
    assert_eq!(drift, 0);
}

#[test]
fn a_drift_lets_a_match_with_that_drift_through() {
    let Some(path) = data_file() else {
        eprintln!("no Hash data file found; skipping drift");
        return;
    };
    let exact = builder(&path).build().unwrap();
    let (_, exact_id, _, _, _) = detect(&exact, NOTHING_TO_MATCH);

    let loose = builder(&path).drift(10).build().unwrap();
    let (_, device_id, difference, drift, _) = detect(&loose, NOTHING_TO_MATCH);
    assert!(
        matched_components(&device_id) > matched_components(&exact_id),
        "{exact_id} then {device_id}"
    );
    assert!(drift > 0, "{drift}");
    assert_eq!(difference, 0);
}

#[test]
fn allow_unmatched_gives_the_default_profiles() {
    let Some(path) = data_file() else {
        eprintln!("no Hash data file found; skipping allow unmatched");
        return;
    };
    let unmatched = builder(&path).build().unwrap();
    let (method, device_id, _, _, is_mobile) = detect(&unmatched, "");
    assert_eq!((method.as_str(), device_id.as_str()), ("NONE", "0-0-0-0"));
    assert_eq!(is_mobile, None);

    let defaulted = builder(&path).allow_unmatched(true).build().unwrap();
    let (method, device_id, _, _, is_mobile) = detect(&defaulted, "");
    assert_eq!(method, "NONE");
    assert!(device_id.split('-').all(|id| id != "0"), "{device_id}");
    assert_eq!(is_mobile, Some(false));
}

#[test]
fn matched_user_agents_are_recorded_unless_turned_off() {
    let Some(path) = data_file() else {
        eprintln!("no Hash data file found; skipping matched User-Agents");
        return;
    };
    let recorded = builder(&path).build().unwrap();
    assert!(!matched_user_agents(&recorded).is_empty());

    let off = builder(&path)
        .update_matched_user_agent(false)
        .build()
        .unwrap();
    assert_eq!(matched_user_agents(&off), Vec::<String>::new());
    assert_eq!(off.hash_options().update_matched_user_agent, Some(false));
}

#[test]
fn the_concurrency_is_the_processors() {
    let Some(path) = data_file() else {
        eprintln!("no Hash data file found; skipping concurrency");
        return;
    };
    for (profile, expected) in [
        (PerformanceProfile::LowMemory, processors()),
        (PerformanceProfile::HighPerformance, processors()),
        (PerformanceProfile::Balanced, processors()),
        (PerformanceProfile::Default, processors()),
    ] {
        let engine = DeviceDetectionOnPremiseEngineBuilder::new(&path)
            .performance_profile(profile)
            .build()
            .unwrap();
        assert_eq!(engine.concurrency(), expected, "{}", profile.as_str());
        assert_eq!(
            engine.hash_options().concurrency,
            Some(expected),
            "{}",
            profile.as_str()
        );
    }

    let named = DeviceDetectionOnPremiseEngineBuilder::new(&path)
        .performance_profile(PerformanceProfile::LowMemory)
        .concurrency(3)
        .build()
        .unwrap();
    assert_eq!(named.concurrency(), 3);
}

#[test]
fn a_named_concurrency_beyond_the_values_cache_loads() {
    let Some(path) = data_file() else {
        eprintln!("no Hash data file found; skipping the concurrency");
        return;
    };
    // More than the whole square root, 22, of the Balanced values cache's
    // 500 entries.
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(&path)
        .performance_profile(PerformanceProfile::Balanced)
        .concurrency(500)
        .build()
        .unwrap();
    assert_eq!(engine.concurrency(), 500);
}

#[test]
fn a_refresh_keeps_the_settings() {
    let Some(path) = data_file() else {
        eprintln!("no Hash data file found; skipping refresh");
        return;
    };
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(&path)
        .performance_profile(PerformanceProfile::InMemory)
        .use_performance_graph(true)
        .use_predictive_graph(false)
        .allow_unmatched(true)
        .build()
        .unwrap();
    let options = engine.hash_options().clone();
    engine.refresh(None).expect("the data file loads again");
    assert_eq!(engine.hash_options(), &options);
    assert_eq!(detect(&engine, DESKTOP_USER_AGENT).0, "PERFORMANCE");
    assert_eq!(detect(&engine, "").4, Some(false));
}
