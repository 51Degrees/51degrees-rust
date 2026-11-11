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

//! The shared conformance cases.
//!
//! Every language implementation of the element runs the case files in
//! `vendor/tests`, which are a copy of the ones every other language runs, and
//! must give the same answer for every one. If the Rust answer differs from the
//! answer in .NET, Node, Java, Python or PHP then this crate has failed at its
//! only job, so these tests are the point of the crate rather than a check on
//! it.
//!
//! The cases are read from disk, so they are compiled only off WebAssembly,
//! where a test has no file system.
#![cfg(not(target_family = "wasm"))]

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use fiftyone_derived_properties::{
    BuiltInScript, Lookup, MapSource, Outcome, Script, SourceValue, WeightedSourceValue,
};
use serde_norway::Value;

fn vendor() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor")
}

fn get<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    value.as_mapping()?.get(Value::String(name.to_owned()))
}

fn text_of(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        other => format!("{other:?}"),
    }
}

/// Turn one entry of a case's `Properties` block into what the source answers.
///
/// The forms are the ones the shared testing guide lists: a native value, the
/// string form, a source that carries its own no value message, and a list of
/// weighted values. A property that is not listed at all is absent, which is
/// what a case that leaves one out is saying.
fn lookup_of(value: &Value) -> Lookup {
    match value {
        Value::Bool(flag) => Lookup::Value(SourceValue::Bool(*flag)),
        Value::String(text) => Lookup::Value(SourceValue::Text(text.clone())),
        Value::Number(number) => {
            let number = number.as_f64().expect("a number in a case");
            if number.fract() == 0.0 {
                Lookup::Value(SourceValue::Int(number as i64))
            } else {
                Lookup::Value(SourceValue::Double(number))
            }
        }
        Value::Sequence(items) => {
            let candidates = items
                .iter()
                .map(|item| WeightedSourceValue {
                    weight: get(item, "Weight").and_then(Value::as_f64).unwrap_or(0.0),
                    value: match lookup_of(get(item, "Value").expect("a weighted value")) {
                        Lookup::Value(value) => value,
                        other => panic!("a weighted value cannot hold {other:?}"),
                    },
                })
                .collect();
            Lookup::Weighted(candidates)
        }
        Value::Mapping(_) => {
            if let Some(text) = get(value, "String") {
                Lookup::Value(SourceValue::Text(text_of(text)))
            } else if let Some(message) = get(value, "NoValue") {
                Lookup::NoValue(text_of(message))
            } else {
                panic!("a property in a case must be a value, String, NoValue or a weighted list")
            }
        }
        Value::Null => Lookup::Absent,
        other => panic!("a property in a case cannot be {other:?}"),
    }
}

/// Run one case file against its script, and return which rules were reached.
fn run_cases(script: &Script, cases_path: &PathBuf) -> BTreeSet<usize> {
    let text = fs::read_to_string(cases_path).expect("the cases file");
    let document: Value = serde_norway::from_str(&text).expect("the cases file is YAML");
    assert_eq!(
        get(&document, "Script").map(text_of).as_deref(),
        Some(script.name()),
        "the cases file names a different script"
    );
    let cases = get(&document, "Cases")
        .and_then(Value::as_sequence)
        .expect("Cases is a list");
    assert!(!cases.is_empty(), "Cases must list at least one case");

    let mut reached = BTreeSet::new();
    for case in cases {
        let name = get(case, "Name").map(text_of).unwrap_or_default();
        let mut source = MapSource::new();
        if let Some(properties) = get(case, "Properties").and_then(Value::as_mapping) {
            for (key, value) in properties {
                source.insert(&text_of(key), lookup_of(value));
            }
        }
        let expect = get(case, "Expect").expect("a case has an Expect");
        let evaluation = script.evaluate_detailed(&source);
        let outcome = &evaluation.outcome;
        if let Some(index) = evaluation.matched_rule {
            reached.insert(index);
        }

        if let Some(expected) = get(expect, "Value") {
            let expected = text_of(expected);
            match outcome {
                Outcome::Value(value) => assert_eq!(
                    value.to_text(),
                    expected,
                    "'{name}' expected '{expected}' but got '{}'",
                    value.to_text()
                ),
                Outcome::NoValue { message, .. } => {
                    panic!("'{name}' expected the value '{expected}' but there was none. {message}")
                }
            }
        } else if let Some(expected) = get(expect, "Missing").and_then(Value::as_sequence) {
            let expected: BTreeSet<String> = expected
                .iter()
                .map(|item| text_of(item).to_lowercase())
                .collect();
            match outcome {
                Outcome::Value(value) => panic!(
                    "'{name}' expected the missing properties {expected:?} but got '{}'",
                    value.to_text()
                ),
                Outcome::NoValue { missing, .. } => {
                    let actual: BTreeSet<String> =
                        missing.iter().map(|item| item.to_lowercase()).collect();
                    assert_eq!(actual, expected, "'{name}' named the wrong properties");
                }
            }
        } else {
            panic!("'{name}' has an Expect that is neither Value nor Missing");
        }
    }
    reached
}

#[test]
fn the_shipped_script_is_sound() {
    let script = BuiltInScript::HumanConfidence
        .compile()
        .unwrap_or_else(|faults| panic!("the shipped script did not validate:\n{faults}"));
    assert_eq!(script.name(), "HumanConfidence");
    assert_eq!(script.output().name, "HumanConfidence");
    // The script names ten source properties and every one of them is
    // necessary, which is what the cases below turn on.
    assert_eq!(script.source_properties().len(), 10);
    assert_eq!(script.output().dependencies.len(), 10);
}

#[test]
fn the_shared_cases_pass() {
    let script = BuiltInScript::HumanConfidence
        .compile()
        .expect("the script");
    let path = vendor().join("tests").join("HumanConfidence.cases.yaml");
    let reached = run_cases(&script, &path);
    // Every rule must be reached by at least one case, because nothing proves
    // what an unreached rule does.
    let uncovered: Vec<usize> = (0..script.rules().len())
        .filter(|index| !reached.contains(index))
        .collect();
    assert!(
        uncovered.is_empty(),
        "no case reaches the rules {uncovered:?}"
    );
}

#[test]
fn a_script_read_from_a_file_is_the_same_script() {
    // The shipped script is compiled in, and this proves the embedded copy is
    // the file it was taken from rather than something that has drifted.
    let path = vendor().join("scripts").join("HumanConfidence.yaml");
    let text = fs::read_to_string(&path).expect("the script file");
    assert_eq!(text, BuiltInScript::HumanConfidence.text());
}

#[test]
fn every_invalid_script_is_rejected() {
    let folder = vendor().join("tests").join("invalid");
    let mut files: Vec<PathBuf> = fs::read_dir(&folder)
        .expect("the invalid folder")
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| path.extension().map(|kind| kind == "yaml").unwrap_or(false))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "there are no rejection cases to run");

    for path in files {
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let text = fs::read_to_string(&path).expect("the rejection case");
        let document: Value = serde_norway::from_str(&text).expect("the case is YAML");
        let script_text = get(&document, "Script")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("{file}: Script must be the text of the script to reject"))
            .to_owned();
        // Only a case about the rule that a script name must equal its file
        // name gives a Name, and the rest are read with no file name at all.
        let name = get(&document, "Name")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let expect = get(&document, "Expect");

        let faults = match Script::compile_named(&script_text, name.as_deref(), &file) {
            Ok(_) => panic!("{file}: the script was expected to be rejected but it validated"),
            Err(faults) => faults,
        };

        let expected_paths = expect
            .and_then(|expect| get(expect, "Paths"))
            .and_then(Value::as_sequence)
            .cloned()
            .unwrap_or_default();
        for wanted in &expected_paths {
            let wanted = text_of(wanted);
            assert!(
                faults.faults().iter().any(|fault| fault.path == wanted),
                "{file}: expected a fault at '{wanted}'. Faults were:\n{faults}"
            );
        }
        let expected_mentions = expect
            .and_then(|expect| get(expect, "Mentions"))
            .and_then(Value::as_sequence)
            .cloned()
            .unwrap_or_default();
        for wanted in &expected_mentions {
            let wanted = text_of(wanted);
            assert!(
                faults
                    .faults()
                    .iter()
                    .any(|fault| fault.message.contains(&wanted)),
                "{file}: expected a fault mentioning '{wanted}'. Faults were:\n{faults}"
            );
        }
    }
}
