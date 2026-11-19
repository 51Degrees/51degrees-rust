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

//! The engine looks up the profiles holding a property value in the data
//! set it has loaded, from the index of the properties the builder names,
//! gives each profile's values, and a refresh indexes the same properties
//! again.
//!
//! The tests need a Hash data file with the `BrowserName` property, the one
//! `_51DEGREES_DD_PATH` names or the Lite file in the `cxx` submodule, and skip
//! with a note when there is none.

#[allow(dead_code)]
#[path = "../../deprecated-names/environment.rs"]
mod deprecated_names;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use fiftyone_device_detection_onpremise::{
    DeviceDetectionOnPremiseEngine, DeviceDetectionOnPremiseEngineBuilder, PROFILE_ID_PROPERTY_NAME,
};
use fiftyone_device_detection_shared::DEVICE_DATA_KEY;
use fiftyone_native::dd::{HashOptions, Manager};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{ElementData, Evidence, FlowElement, Pipeline};
use fiftyone_pipeline_engines::OnPremiseAspectEngine;

/// The property looked up, which every Hash data file has.
const BROWSER_NAME: &str = "BrowserName";

/// A property name no data file has.
const NO_SUCH_PROPERTY: &str = "NoSuchProperty";

/// The evidence key the engine reads profile ids from.
const PROFILE_IDS_KEY: &str = "query.51D_ProfileIds";

/// An engine built with the default performance profile reads the data file
/// through its caches, so its ids are compared with reading every profile for
/// every this many values, as each such read is the slowest.
const FILE_PROFILE_FULL_READ_EVERY: usize = 10;

/// The Hash data file `_51DEGREES_DD_PATH` names, else the Lite file in the
/// `cxx` submodule, or `None` with a note that the test is skipped.
fn data_file() -> Option<PathBuf> {
    let given = crate::deprecated_names::var("_51DEGREES_DD_PATH")
        .ok()
        .map(PathBuf::from);
    let bundled = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cxx/data/device-detection/51Degrees-LiteV4.1.hash");
    let found = given
        .into_iter()
        .chain([bundled])
        .find(|path| path.is_file());
    if found.is_none() {
        eprintln!("skipped: no Hash data file at _51DEGREES_DD_PATH or cxx/data");
    }
    found
}

/// The values the engine lists for `BrowserName`, or `None` with a note when
/// the data file has no such property.
fn browser_names(engine: &DeviceDetectionOnPremiseEngine) -> Option<Vec<String>> {
    let values = engine
        .property_values(BROWSER_NAME)
        .expect("the values read")
        .map(|values| values.into_iter().map(|value| value.name).collect());
    if values.is_none() {
        eprintln!("skipped: the data file has no {BROWSER_NAME}");
    }
    values
}

/// The ids of the profiles holding each value, read by a manager that
/// indexes nothing, so every lookup reads every profile.
fn ids_reading_every_profile(path: &Path, values: &[String]) -> Vec<Vec<u32>> {
    let manager = Manager::open_with_hash_options(
        path,
        PerformanceProfile::InMemory,
        None,
        &HashOptions::default(),
    )
    .expect("the data file loads");
    assert!(manager.indexed_properties().is_empty());
    values
        .iter()
        .map(|value| {
            manager
                .profile_ids_for_property_value(BROWSER_NAME, value)
                .expect("the lookup succeeds")
        })
        .collect()
}

/// The ids the engine gives for each value.
fn ids(engine: &DeviceDetectionOnPremiseEngine, values: &[String]) -> Vec<Vec<u32>> {
    values
        .iter()
        .map(|value| {
            engine
                .profile_ids_for_property_value(BROWSER_NAME, value)
                .expect("the lookup succeeds")
        })
        .collect()
}

/// An engine with `BrowserName` named to index gives, for every value, the
/// ids reading every profile gives, and so does an engine that indexes
/// nothing, built with the default performance profile, for every tenth
/// value. Each list is in ascending order and at least one is not empty.
/// A value, a property name in other case and a property the data file does
/// not have give none.
#[test]
fn every_value_gives_the_ids_reading_every_profile_gives() {
    let Some(path) = data_file() else {
        return;
    };
    let indexed = DeviceDetectionOnPremiseEngineBuilder::new(&path)
        .indexed_property(BROWSER_NAME)
        .build()
        .expect("the data file loads");
    let Some(values) = browser_names(&indexed) else {
        return;
    };
    assert_eq!(indexed.indexed_properties(), [BROWSER_NAME]);
    let expected = ids_reading_every_profile(&path, &values);
    assert!(expected.iter().any(|ids| !ids.is_empty()));
    for ids in &expected {
        assert!(ids.windows(2).all(|pair| pair[0] < pair[1]), "{ids:?}");
    }
    assert_eq!(ids(&indexed, &values), expected);

    let unindexed = DeviceDetectionOnPremiseEngineBuilder::new(&path)
        .build()
        .expect("the data file loads");
    assert!(unindexed.indexed_properties().is_empty());
    assert!(unindexed.hash_options().indexed_properties.is_empty());
    let compared = values
        .iter()
        .zip(&expected)
        .step_by(FILE_PROFILE_FULL_READ_EVERY);
    for (value, want) in compared {
        let found = unindexed
            .profile_ids_for_property_value(BROWSER_NAME, value)
            .expect("the lookup succeeds");
        assert_eq!(&found, want, "{value:?}");
    }

    for (property, value) in [
        (BROWSER_NAME, "No such browser"),
        ("browsername", values[0].as_str()),
        (NO_SUCH_PROPERTY, values[0].as_str()),
    ] {
        let found = indexed
            .profile_ids_for_property_value(property, value)
            .expect("the lookup succeeds");
        assert!(found.is_empty(), "{property} {value:?}: {found:?}");
    }
}

/// The first and last id found for each value, given to the engine as
/// profile ids evidence, give a profile whose `BrowserName` is the value.
#[test]
fn an_id_given_as_profile_ids_evidence_gives_the_value() {
    let Some(path) = data_file() else {
        return;
    };
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(&path)
        .indexed_property(BROWSER_NAME)
        .build()
        .expect("the data file loads");
    let Some(values) = browser_names(&engine) else {
        return;
    };
    let element: Arc<dyn FlowElement> = engine.clone();
    let pipeline = Pipeline::builder()
        .add_element(element)
        .build()
        .expect("the pipeline builds");
    let mut checked = 0;
    for (value, found) in values.iter().zip(ids(&engine, &values)) {
        for id in [found.first(), found.last()].into_iter().flatten() {
            let mut data = pipeline.create_flow_data_with(
                Evidence::builder()
                    .add(PROFILE_IDS_KEY, id.to_string())
                    .build(),
            );
            data.process().expect("the profile id processes");
            let device = data.get(DEVICE_DATA_KEY).expect("device data");
            let name = device.get(BROWSER_NAME).expect("BrowserName has a value");
            assert_eq!(name.as_str(), Some(value.as_str()), "profile {id}");
            checked += 1;
        }
    }
    assert!(checked > 0, "no value is held by any profile");
}

/// Each value's profiles are those its ids give, in the same order, each
/// holding its id as text and the value. The first profile found gives every
/// property the engine gives for its id as profile ids evidence, with the
/// same value, and a value no profile holds gives none.
#[test]
fn the_profiles_of_a_value_are_those_its_ids_give() {
    let Some(path) = data_file() else {
        return;
    };
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(&path)
        .indexed_property(BROWSER_NAME)
        .build()
        .expect("the data file loads");
    let Some(values) = browser_names(&engine) else {
        return;
    };
    let mut first = None;
    for (value, found) in values.iter().zip(ids(&engine, &values)) {
        let profiles = engine
            .profiles_for_property_value(BROWSER_NAME, value)
            .expect("the profiles read");
        assert_eq!(profiles.len(), found.len(), "{value}");
        for (profile, id) in profiles.iter().zip(&found) {
            let held = profile
                .get(PROFILE_ID_PROPERTY_NAME)
                .expect("ProfileId has a value");
            assert_eq!(held.as_str(), Some(id.to_string().as_str()));
            let name = profile.get(BROWSER_NAME).expect("BrowserName has a value");
            assert_eq!(name.as_str(), Some(value.as_str()), "profile {id}");
        }
        if first.is_none() {
            first = found.first().copied().zip(profiles.into_iter().next());
        }
    }
    let (id, profile) = first.expect("a value is held by a profile");

    let element: Arc<dyn FlowElement> = engine.clone();
    let pipeline = Pipeline::builder()
        .add_element(element)
        .build()
        .expect("the pipeline builds");
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add(PROFILE_IDS_KEY, id.to_string())
            .build(),
    );
    data.process().expect("the profile id processes");
    let device = data.get(DEVICE_DATA_KEY).expect("device data");
    let keys = device.keys();
    assert!(keys
        .iter()
        .any(|key| key.eq_ignore_ascii_case(BROWSER_NAME)));
    for key in &keys {
        assert_eq!(profile.get(key).ok(), device.get(key).ok(), "{key}");
    }

    let none = engine
        .profiles_for_property_value(BROWSER_NAME, "No such browser")
        .expect("the lookup succeeds");
    assert!(none.is_empty());
}

/// The ids and reader of a value are the ids the engine gives for it, and
/// the reader gives each id the profile `profiles_for_property_value` gives,
/// with the same properties and values.
#[test]
fn the_reader_of_a_value_gives_the_profiles_of_its_ids() {
    let Some(path) = data_file() else {
        return;
    };
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(&path)
        .indexed_property(BROWSER_NAME)
        .build()
        .expect("the data file loads");
    let Some(values) = browser_names(&engine) else {
        return;
    };
    let mut compared = 0;
    for (value, found) in values.iter().zip(ids(&engine, &values)) {
        let (ids, reader) = engine
            .profile_ids_and_reader_for_property_value(BROWSER_NAME, value)
            .expect("the ids are found");
        assert_eq!(ids, found, "{value}");
        let profiles = engine
            .profiles_for_property_value(BROWSER_NAME, value)
            .expect("the profiles read");
        assert_eq!(profiles.len(), ids.len(), "{value}");
        for (id, profile) in ids.iter().zip(&profiles) {
            let read = reader.read_profile(*id);
            let keys = profile.keys();
            assert_eq!(read.keys(), keys, "profile {id}");
            // Every value of the first profile, and the id and key property
            // of the rest, as reading every value of every profile is slow.
            let names: Vec<&str> = if compared == 0 {
                keys.iter().map(String::as_str).collect()
            } else {
                vec![PROFILE_ID_PROPERTY_NAME, BROWSER_NAME]
            };
            for name in names {
                assert_eq!(
                    read.get(name).ok(),
                    profile.get(name).ok(),
                    "profile {id} {name}"
                );
            }
            compared += 1;
        }
    }
    assert!(
        compared > 0,
        "no value of {BROWSER_NAME} is held by a profile"
    );
}

/// The reader of a value, and the profiles of a value, found before a refresh
/// from another data file read each id's profile from the data set the ids
/// were found in, as those of an engine that was not refreshed read it. Runs
/// when the TAC file is beside the Lite file.
#[test]
fn the_profiles_of_a_value_come_from_its_ids_data_set_after_a_refresh() {
    let Some(path) = data_file() else {
        return;
    };
    let tac = path.with_file_name("TAC-HashV41.hash");
    if !tac.is_file() {
        eprintln!("skipped: no TAC file beside {}", path.display());
        return;
    }
    let build = || {
        DeviceDetectionOnPremiseEngineBuilder::new(&path)
            .indexed_property(BROWSER_NAME)
            .build()
            .expect("the data file loads")
    };
    let reference = build();
    let engine = build();
    let Some(all) = browser_names(&reference) else {
        return;
    };
    let all_ids = ids(&reference, &all);
    let values: Vec<String> = all
        .into_iter()
        .zip(all_ids)
        .filter(|(_, found)| !found.is_empty())
        .map(|(value, _)| value)
        .take(3)
        .collect();
    assert!(
        !values.is_empty(),
        "no value of {BROWSER_NAME} has a profile"
    );
    let found: Vec<_> = values
        .iter()
        .map(|value| {
            let (ids, reader) = engine
                .profile_ids_and_reader_for_property_value(BROWSER_NAME, value)
                .expect("the ids are found");
            let profiles = engine
                .profiles_for_property_value(BROWSER_NAME, value)
                .expect("the profiles read");
            (ids, reader, profiles)
        })
        .collect();
    let bytes = std::fs::read(&tac).expect("the TAC file reads");
    engine
        .refresh_from_memory(None, &bytes)
        .expect("the TAC file loads");
    for (value, (ids, reader, profiles)) in values.iter().zip(&found) {
        let (expected_ids, expected) = reference
            .profile_ids_and_reader_for_property_value(BROWSER_NAME, value)
            .expect("the ids are found");
        assert_eq!(ids, &expected_ids, "{value}");
        assert_eq!(profiles.len(), ids.len(), "{value}");
        for (id, profile) in ids.iter().zip(profiles) {
            let want = expected.read_profile(*id);
            let read = reader.read_profile(*id);
            for (got, how) in [(&read, "read"), (profile, "found")] {
                assert_eq!(got.keys(), want.keys(), "profile {id} {how}");
                for name in want.keys() {
                    assert_eq!(
                        got.get(&name).ok(),
                        want.get(&name).ok(),
                        "profile {id} {name} {how}"
                    );
                }
            }
        }
    }
}

/// An engine restricted to another property still indexes and finds the
/// profiles of the property named to index, which it does not give.
#[test]
fn an_engine_restricted_to_another_property_finds_the_indexed_one() {
    let Some(path) = data_file() else {
        return;
    };
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(&path)
        .property("IsMobile")
        .indexed_property(BROWSER_NAME)
        .build()
        .expect("the data file loads");
    assert_eq!(engine.indexed_properties(), [BROWSER_NAME]);
    assert!(!engine
        .available_properties()
        .iter()
        .any(|name| name == BROWSER_NAME));
    let full = DeviceDetectionOnPremiseEngineBuilder::new(&path)
        .build()
        .expect("the data file loads");
    let Some(values) = browser_names(&full) else {
        return;
    };
    assert_eq!(
        ids(&engine, &values),
        ids_reading_every_profile(&path, &values)
    );
}

/// The builder keeps the names in the order given less exact repeats, a
/// name in other case and one the data file has no property of among them,
/// and the data set indexes the same names after a refresh from the file
/// and from memory, giving the same ids.
#[test]
fn a_refresh_indexes_the_same_properties() {
    let Some(path) = data_file() else {
        return;
    };
    let named = [BROWSER_NAME, NO_SUCH_PROPERTY, "browsername"];
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(&path)
        .performance_profile(PerformanceProfile::LowMemory)
        .indexed_properties([BROWSER_NAME, NO_SUCH_PROPERTY])
        .indexed_property(BROWSER_NAME)
        .indexed_property("browsername")
        .build()
        .expect("the data file loads");
    assert_eq!(engine.hash_options().indexed_properties, named);
    assert_eq!(engine.indexed_properties(), named);
    let Some(values) = browser_names(&engine) else {
        return;
    };
    let before = ids(&engine, &values);

    engine.refresh(None).expect("the data file loads again");
    assert_eq!(
        engine.indexed_properties(),
        named,
        "refreshed from the file"
    );
    assert_eq!(ids(&engine, &values), before, "refreshed from the file");

    let data = std::fs::read(&path).expect("the data file reads");
    engine
        .refresh_from_memory(None, &data)
        .expect("the data file loads from memory");
    assert_eq!(engine.indexed_properties(), named, "refreshed from memory");
    assert_eq!(ids(&engine, &values), before, "refreshed from memory");
}
