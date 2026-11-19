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

//! The profiles holding a value of a property, looked up from the index a
//! data set builds when it loads naming the properties to index, are the
//! profiles reading every profile finds, in ascending order, and each holds
//! the value.
//!
//! The tests run on the Hash data file `_51DEGREES_DD_PATH` names, or the Lite
//! file in the `cxx` submodule, and skip when there is neither.
//! `tac_and_native_model_values_find_their_profiles` needs a file with the
//! TAC and NativeModel properties, such as the TAC file. When
//! `_51DEGREES_REQUIRE_TAC` is set, as the comparison workflow sets it, that
//! test fails rather than skips without one.

#![cfg(feature = "dd")]

#[allow(dead_code)]
#[path = "../../deprecated-names/environment.rs"]
mod deprecated_names;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use fiftyone_native::dd::{HashOptions, Manager, Results};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::Error;

const BROWSER_NAME: &str = "BrowserName";
const TAC: &str = "TAC";
const NATIVE_MODEL: &str = "NativeModel";

/// A property name no data file has.
const NO_SUCH_PROPERTY: &str = "NoSuchProperty";

/// Set where the TAC file test must run, so it fails rather than skips.
const REQUIRE_TAC: &str = "_51DEGREES_REQUIRE_TAC";

/// The separator a property's values are read with, which no value holds.
const SEPARATOR: &str = "\u{1F}";

/// Under a profile that reads the data file, the ids are compared with
/// reading every profile for every this many values, as each such read is
/// the slowest.
const FILE_PROFILE_FULL_READ_EVERY: usize = 10;

/// On the TAC file, the ids are compared with reading every profile for
/// about one value in 500.
const TAC_FULL_READ_EVERY: usize = 500;

/// The Hash data file `_51DEGREES_DD_PATH` names, or the Lite file in the
/// `cxx` submodule.
fn data_file() -> Option<PathBuf> {
    if let Some(path) = crate::deprecated_names::var_os("_51DEGREES_DD_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let lite = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .join("cxx")
        .join("data")
        .join("device-detection")
        .join("51Degrees-LiteV4.1.hash");
    lite.is_file().then_some(lite)
}

/// Options indexing the properties `names`.
fn indexing(names: &[&str]) -> HashOptions {
    let mut options = HashOptions::default();
    options
        .indexed_properties
        .extend(names.iter().map(|name| (*name).to_owned()));
    options
}

/// The data file at `path` with every property, indexing `indexed`.
fn open(path: &Path, profile: PerformanceProfile, indexed: &[&str]) -> Arc<Manager> {
    Manager::open_with_hash_options(path, profile, None, &indexing(indexed))
        .expect("the data file loads")
}

/// The values the data file holds for `property`, or [`None`] when it has no
/// such property.
fn values_of(manager: &Manager, property: &str) -> Option<Vec<String>> {
    let property = manager
        .data_file_properties()
        .expect("the properties read")
        .into_iter()
        .find(|found| found.name == property)?;
    let values = manager
        .data_file_property_values(property.index)
        .expect("the values read");
    Some(values.into_iter().map(|value| value.name).collect())
}

fn lookup(manager: &Manager, property: &str, value: &str) -> Vec<u32> {
    manager
        .profile_ids_for_property_value(property, value)
        .unwrap_or_else(|error| panic!("{property} {value:?} is looked up: {error}"))
}

/// Assert the ids ascend, and that each is a profile holding `value` of
/// `property`.
fn assert_each_holds(results: &mut Results, property: &str, value: &str, ids: &[u32]) {
    assert!(
        ids.windows(2).all(|pair| pair[0] < pair[1]),
        "the ids of {property} {value:?} do not ascend: {ids:?}"
    );
    for id in ids {
        let parsed = results
            .process_device_id(&id.to_string())
            .expect("the profile id is processed");
        assert_eq!(parsed, 1, "profile {id} of {property} {value:?}");
        let held = results
            .value_as_string(property, SEPARATOR)
            .expect("the value reads")
            .unwrap_or_default();
        assert!(
            held.split(SEPARATOR).any(|held| held == value),
            "profile {id} holds {property} {held:?}, not {value:?}"
        );
    }
}

/// Skip the TAC file test for `reason`, or fail when [`REQUIRE_TAC`] is set.
fn skip_tac(reason: &str) {
    assert!(
        std::env::var_os(REQUIRE_TAC).is_none(),
        "{REQUIRE_TAC} is set, but {reason}"
    );
    eprintln!("{reason}; skipping");
}

/// The properties indexed are those named, in order and less repeats,
/// matched exactly, with one the data file has no property of kept and giving
/// no profiles. None are indexed when none are named, and a name holding a
/// null byte is refused.
#[test]
fn the_properties_indexed_are_those_named() {
    let Some(path) = data_file() else {
        eprintln!("no Hash data file found; skipping the indexed names");
        return;
    };
    let lower = BROWSER_NAME.to_lowercase();
    let manager = open(
        &path,
        PerformanceProfile::InMemory,
        &[BROWSER_NAME, NO_SUCH_PROPERTY, BROWSER_NAME, &lower],
    );
    assert_eq!(
        manager.indexed_properties(),
        [BROWSER_NAME, NO_SUCH_PROPERTY, lower.as_str()]
    );
    assert!(lookup(&manager, NO_SUCH_PROPERTY, "Chrome").is_empty());
    assert!(lookup(&manager, NO_SUCH_PROPERTY, "").is_empty());

    let none = open(&path, PerformanceProfile::InMemory, &[]);
    assert!(none.indexed_properties().is_empty());
    let plain = Manager::open(&path, PerformanceProfile::LowMemory).expect("the data file loads");
    assert!(plain.indexed_properties().is_empty());

    let refused = Manager::open_with_hash_options(
        &path,
        PerformanceProfile::InMemory,
        None,
        &indexing(&["Browser\0Name"]),
    )
    .err()
    .expect("a name holding a null byte is refused");
    assert!(
        matches!(&refused, Error::Native { status, .. } if status == "InvalidInput"),
        "{refused:?}"
    );
}

/// For every BrowserName value, the profiles from the index ascend and each
/// holds the value, and they are the profiles reading every profile finds.
/// The profiles that read the data file run too, so the lookup reads through
/// the file's handles.
#[test]
fn every_value_finds_the_profiles_reading_every_profile_finds() {
    let Some(path) = data_file() else {
        eprintln!("no Hash data file found; skipping the lookups");
        return;
    };
    for (profile, full_read_every) in [
        (PerformanceProfile::InMemory, 1),
        (PerformanceProfile::LowMemory, FILE_PROFILE_FULL_READ_EVERY),
        (PerformanceProfile::Balanced, FILE_PROFILE_FULL_READ_EVERY),
    ] {
        let indexed = open(&path, profile, &[BROWSER_NAME]);
        let Some(values) = values_of(&indexed, BROWSER_NAME) else {
            eprintln!("the data file has no {BROWSER_NAME}; skipping the lookups");
            return;
        };
        let unindexed = open(&path, profile, &[]);
        let mut results = indexed.create_results().expect("results allocate");
        let mut profiles = 0;
        for (place, value) in values.iter().enumerate() {
            let ids = lookup(&indexed, BROWSER_NAME, value);
            assert_each_holds(&mut results, BROWSER_NAME, value, &ids);
            if place % full_read_every == 0 {
                assert_eq!(
                    lookup(&unindexed, BROWSER_NAME, value),
                    ids,
                    "{} {value:?}",
                    profile.as_str()
                );
            }
            profiles += ids.len();
        }
        assert!(
            profiles > 0,
            "{}: no {BROWSER_NAME} value has a profile",
            profile.as_str()
        );
    }
}

/// A value no profile holds, an empty value, a held value with more text
/// after it, a value or property name in other case, a property the data
/// file does not have, and a name or value holding a null byte give no
/// profiles, from the index and from reading every profile. The value with
/// more text after it shows that a value is matched in full, not by its
/// start.
#[test]
fn what_the_data_file_does_not_hold_finds_no_profiles() {
    let Some(path) = data_file() else {
        eprintln!("no Hash data file found; skipping the misses");
        return;
    };
    let indexed = open(&path, PerformanceProfile::InMemory, &[BROWSER_NAME]);
    let Some(values) = values_of(&indexed, BROWSER_NAME) else {
        eprintln!("the data file has no {BROWSER_NAME}; skipping the misses");
        return;
    };
    let value = values
        .iter()
        .find(|value| {
            let lower = value.to_lowercase();
            lower != **value
                && !values.contains(&lower)
                && !lookup(&indexed, BROWSER_NAME, value).is_empty()
        })
        .expect("a value with capitals that a profile holds");
    let unindexed = open(&path, PerformanceProfile::InMemory, &[]);
    let misses = [
        (BROWSER_NAME.to_owned(), String::from("No such browser")),
        (BROWSER_NAME.to_owned(), String::new()),
        (BROWSER_NAME.to_owned(), format!("{value} No such suffix")),
        (BROWSER_NAME.to_owned(), value.to_lowercase()),
        (BROWSER_NAME.to_lowercase(), value.clone()),
        (NO_SUCH_PROPERTY.to_owned(), value.clone()),
        (BROWSER_NAME.to_owned(), format!("{value}\0")),
        (format!("{BROWSER_NAME}\0"), value.clone()),
    ];
    for manager in [&indexed, &unindexed] {
        assert!(!lookup(manager, BROWSER_NAME, value).is_empty());
        for (property, missing) in &misses {
            assert!(
                lookup(manager, property, missing).is_empty(),
                "{property:?} {missing:?}, indexing {:?}",
                manager.indexed_properties()
            );
        }
    }
}

/// A data set loaded from memory indexes the properties named, and finds the
/// profiles the same data set loaded from the file finds.
#[test]
fn a_data_set_loaded_from_memory_indexes_the_properties_named() {
    let Some(path) = data_file() else {
        eprintln!("no Hash data file found; skipping the load from memory");
        return;
    };
    let from_file = open(&path, PerformanceProfile::InMemory, &[BROWSER_NAME]);
    let Some(values) = values_of(&from_file, BROWSER_NAME) else {
        eprintln!("the data file has no {BROWSER_NAME}; skipping the load from memory");
        return;
    };
    let data = std::fs::read(&path).expect("the data file reads");
    let from_memory = Manager::open_from_memory(
        data,
        PerformanceProfile::InMemory,
        None,
        &indexing(&[BROWSER_NAME]),
    )
    .expect("the data loads");
    assert_eq!(from_memory.indexed_properties(), [BROWSER_NAME]);
    for value in &values {
        assert_eq!(
            lookup(&from_memory, BROWSER_NAME, value),
            lookup(&from_file, BROWSER_NAME, value),
            "{value:?}"
        );
    }
}

/// On a data file with TAC and NativeModel, every value of each finds
/// profiles that ascend and each hold the value, and for every 500th value
/// they are the profiles reading every profile finds.
#[test]
fn tac_and_native_model_values_find_their_profiles() {
    let Some(path) = data_file() else {
        skip_tac("no Hash data file found");
        return;
    };
    let indexed = open(&path, PerformanceProfile::InMemory, &[TAC, NATIVE_MODEL]);
    let (Some(tacs), Some(models)) = (values_of(&indexed, TAC), values_of(&indexed, NATIVE_MODEL))
    else {
        skip_tac(&format!(
            "{} has no {TAC} and {NATIVE_MODEL} properties",
            path.display()
        ));
        return;
    };
    assert_eq!(indexed.indexed_properties(), [TAC, NATIVE_MODEL]);
    let mut results = indexed.create_results().expect("results allocate");
    let mut compared = Vec::new();
    for (property, values) in [(TAC, &tacs), (NATIVE_MODEL, &models)] {
        let (mut profiles, mut unheld) = (0, 0);
        for (place, value) in values.iter().enumerate() {
            let ids = lookup(&indexed, property, value);
            assert_each_holds(&mut results, property, value, &ids);
            if ids.is_empty() {
                unheld += 1;
            }
            profiles += ids.len();
            if place % TAC_FULL_READ_EVERY == 0 {
                compared.push((property, value.as_str(), ids));
            }
        }
        println!(
            "{property}: {} values, {profiles} profiles, {unheld} values no profile holds",
            values.len()
        );
        assert!(profiles > 0, "no {property} value has a profile");
    }
    drop(results);
    drop(indexed);

    let unindexed = open(&path, PerformanceProfile::InMemory, &[]);
    for (property, value, ids) in &compared {
        assert_eq!(
            &lookup(&unindexed, property, value),
            ids,
            "{property} {value:?}"
        );
    }
    println!(
        "{} values compared with reading every profile",
        compared.len()
    );
}
