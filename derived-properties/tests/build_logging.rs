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

//! What the element logs at build, at every level. This is its own test binary
//! because a process has one logger, and the other logging tests keep only
//! warnings and errors.

use std::sync::{Mutex, MutexGuard};

use fiftyone_derived_properties::{BuiltInScript, DerivedPropertyElement};

/// Every record, as "LEVEL message".
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Held by each test for its whole run.
static SERIAL: Mutex<()> = Mutex::new(());

struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        LOGGED
            .lock()
            .unwrap()
            .push(format!("{} {}", record.level(), record.args()));
    }

    fn flush(&self) {}
}

static LOGGER: TestLogger = TestLogger;

/// Install the logger at `level`, wait for any other test to finish and clear
/// the records it left.
fn start(level: log::LevelFilter) -> MutexGuard<'static, ()> {
    let _ = log::set_logger(&LOGGER);
    let guard = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    log::set_max_level(level);
    LOGGED.lock().unwrap().clear();
    guard
}

fn take() -> Vec<String> {
    std::mem::take(&mut *LOGGED.lock().unwrap())
}

/// The commit the shipped scripts came from. Where `vendor` is the
/// derived-properties submodule this asks git, so the test proves the element
/// logs the commit the submodule is at. On WebAssembly, and in a crate from
/// crates.io, which has no git, it is the commit the build found.
fn scripts_commit() -> String {
    #[cfg(not(target_family = "wasm"))]
    {
        let vendor = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor");
        if vendor.join(".git").exists() {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(&vendor)
                .args(["rev-parse", "HEAD"])
                .output()
                .expect("git runs");
            assert!(output.status.success(), "git rev-parse HEAD failed");
            return String::from_utf8(output.stdout)
                .expect("git prints UTF-8")
                .trim()
                .to_owned();
        }
    }
    env!("_51DEGREES_DERIVED_PROPERTIES_COMMIT").to_owned()
}

/// A script named `name` producing an int, deprecated where `note` is given.
fn script(name: &str, note: Option<&str>) -> String {
    let deprecation = note
        .map(|note| format!("Deprecated: true\nDeprecationNote: {note}\n"))
        .unwrap_or_default();
    format!(
        "Format: 1\nName: {name}\nVersion: 2.1.0\n{deprecation}\
         Output:\n  Name: {name}\n  Description: A demonstration.\n  ValueType: int\n  \
         IsList: false\n\
         Rules:\n  - When: {{ Property: src.N, Lt: 2 }}\n    Then: 1\n  - Else: 0\n"
    )
}

/// One information line per script and, at debug level, its canonical JSON,
/// with a deprecated script's warning between the two, as .NET writes them.
#[test]
fn each_script_logs_its_line_its_warning_and_its_canonical_json_in_order() {
    let _guard = start(log::LevelFilter::Debug);
    let element = DerivedPropertyElement::builder()
        .add_script("First", &script("First", None))
        .add_script("Second", &script("Second", Some("Use First instead.")))
        .build()
        .expect("the scripts are sound");
    let scripts = element.scripts();
    assert_eq!(
        take(),
        vec![
            "INFO Derived property script 'First' version 2.1.0 (format 1) from code \
             produces 'First' as int."
                .to_owned(),
            format!(
                "DEBUG Derived property script 'First' compiled to {}",
                scripts[0].to_canonical_json()
            ),
            "INFO Derived property script 'Second' version 2.1.0 (format 1) from code \
             produces 'Second' as int."
                .to_owned(),
            "WARN Derived property script 'Second' is deprecated. Use First instead.".to_owned(),
            format!(
                "DEBUG Derived property script 'Second' compiled to {}",
                scripts[1].to_canonical_json()
            ),
        ]
    );
    assert!(scripts[1]
        .to_canonical_json()
        .contains("\"DeprecationNote\": \"Use First instead.\""));
}

/// A shipped script says it is built in, and one line before the scripts names
/// the derived-properties commit the shipped scripts came from.
#[test]
fn a_shipped_script_logs_the_commit_its_scripts_came_from_first() {
    let commit = scripts_commit();
    assert!(
        commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit()),
        "'{commit}' is not a commit"
    );
    let _guard = start(log::LevelFilter::Info);
    let element = DerivedPropertyElement::builder()
        .add_script("First", &script("First", None))
        .add_built_in(BuiltInScript::HumanConfidence)
        .build()
        .expect("the scripts are sound");
    let shipped = &element.scripts()[1];
    assert_eq!(shipped.source(), "built in HumanConfidence");
    assert_eq!(
        take(),
        vec![
            format!(
                "INFO Built in derived property scripts came from derived-properties \
                 commit {commit}."
            ),
            "INFO Derived property script 'First' version 2.1.0 (format 1) from code \
             produces 'First' as int."
                .to_owned(),
            format!(
                "INFO Derived property script 'HumanConfidence' version {} (format 1) from \
                 built in HumanConfidence produces 'HumanConfidence' as string.",
                shipped.version()
            ),
        ]
    );
}

/// Scripts of the caller's own name no commit, and nothing is logged at debug
/// level where the logger does not take it.
#[test]
fn scripts_of_your_own_log_no_commit_and_no_debug_line_above_debug() {
    let _guard = start(log::LevelFilter::Info);
    DerivedPropertyElement::builder()
        .add_script("First", &script("First", None))
        .build()
        .expect("the script is sound");
    assert_eq!(
        take(),
        vec![
            "INFO Derived property script 'First' version 2.1.0 (format 1) from code \
              produces 'First' as int."
                .to_owned()
        ]
    );
}

/// A builder with faults logs nothing, because no element is built.
#[test]
fn a_build_that_fails_logs_nothing() {
    let _guard = start(log::LevelFilter::Debug);
    let result = DerivedPropertyElement::builder()
        .add_script("First", &script("First", None))
        .add_script("Other", &script("First", None))
        .build();
    assert!(result.is_err());
    assert_eq!(take(), Vec::<String>::new());
}
