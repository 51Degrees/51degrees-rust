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

//! Every 51Degrees environment variable name in this repository starts with
//! `_51DEGREES_`. The underscore is there because a POSIX shell cannot set a
//! name that starts with a digit, and the number is written as digits rather
//! than spelled out in words because the shorter form is preferred. The tests
//! read every file git tracks, which leaves out build output and the
//! submodules, and fail on any name that starts with the digit, or with the
//! number spelled out where an environment variable is named. The
//! exceptions are the deprecated names that are still read when the name
//! that replaced each is unset or empty. A crate's own old name is allowed
//! only in that crate's folder ([`STILL_READ`]), and the names the tests and
//! the examples read are allowed only in the one file they all read them
//! through ([`SHARED_READER`]).

use std::path::{Path, PathBuf};
use std::process::Command;

/// The start of a name that a shell cannot set, split so this file does not
/// match itself.
const DIGIT_INITIAL: &str = concat!("5", "1DEGREES_");

/// The deprecated name of the cloud endpoint variable, split so this file
/// does not match itself.
const DEPRECATED_CLOUD_ENDPOINT: &str = concat!("5", "1DEGREES_CLOUD_ENDPOINT");

/// The deprecated name of the resource key variable, which one workflow
/// step still reads, split so this file does not match itself.
const DEPRECATED_RESOURCE_KEY: &str = concat!("5", "1DEGREES_RESOURCE_KEY");

/// The deprecated name of the variable that points a build at a checkout of
/// the C sources, split so this file does not match itself.
const DEPRECATED_CXX_DIR: &str = concat!("FIFTY", "ONE_CXX_DIR");

/// Each deprecated name that may still be written, with the folders of the
/// crates that still read it. Anywhere else the name is refused.
const STILL_READ: [(&str, &[&str]); 3] = [
    (DEPRECATED_CLOUD_ENDPOINT, &["cloud-request-engine/"]),
    (
        DEPRECATED_RESOURCE_KEY,
        &[".github/workflows/pull-request.yml"],
    ),
    (
        DEPRECATED_CXX_DIR,
        &[
            "common-sys/",
            "device-detection-sys/",
            "ip-intelligence-sys/",
        ],
    ),
];

/// The folder of the file every test and example reads a renamed variable
/// through, which holds each of [`READ_THROUGH_SHARED_READER`] once.
const SHARED_READER: &str = "deprecated-names/";

/// The deprecated names [`SHARED_READER`] reads, each split so this file
/// does not match itself.
const READ_THROUGH_SHARED_READER: [&str; 7] = [
    DEPRECATED_RESOURCE_KEY,
    concat!("5", "1DEGREES_LICENSE_KEY"),
    concat!("5", "1DEGREES_DD_PATH"),
    concat!("5", "1DEGREES_IPI_PATH"),
    concat!("5", "1DEGREES_IPI_ENTERPRISE_SHARE"),
    DEPRECATED_CLOUD_ENDPOINT,
    DEPRECATED_CXX_DIR,
];

/// True when `name` is a deprecated name that the file at `relative` may
/// hold, because the code there still reads it.
fn still_read(relative: &str, name: &str) -> bool {
    if relative.starts_with(SHARED_READER) && READ_THROUGH_SHARED_READER.contains(&name) {
        return true;
    }
    STILL_READ.iter().any(|(deprecated, folders)| {
        name == *deprecated && folders.iter().any(|folder| relative.starts_with(*folder))
    })
}

/// The start of a name with the number spelled out in words, split so this
/// file does not match itself.
const SPELLED_OUT: &str = concat!("FIFTY", "ONE_");

/// The start of the C library's own names, such as the definitions it is
/// compiled with, which are not environment variables.
const C_LIBRARY: &str = concat!("FIFTY", "ONE_DEGREES_");

/// The bytes after which a name in code, a script or a document is an
/// environment variable's, as in a string, a code span, `NAME=value` or
/// `$NAME`.
const NAME_BEFORE: &[u8] = b"\"'`=$";

/// The text after which a name is an environment variable's in PowerShell and
/// in a workflow expression, as in `$env:NAME` and `env.NAME`.
const ENV_BEFORE: [&str; 2] = ["env:", "env."];

/// The repository root, two levels above this crate.
fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// The files git tracks, as paths relative to `root`.
fn tracked_files(root: &Path) -> Vec<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8_lossy(path).into_owned())
        .collect()
}

/// A byte that can be part of an environment variable name.
fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Each name in `line` that starts with [`DIGIT_INITIAL`] and is not the end
/// of a longer name, such as one with a leading underscore.
fn digit_initial_names(line: &[u8]) -> Vec<String> {
    let needle = DIGIT_INITIAL.as_bytes();
    let mut names = Vec::new();
    let mut at = 0;
    while at < line.len() {
        if line[at..].starts_with(needle) && (at == 0 || !is_name_byte(line[at - 1])) {
            let end = line[at..]
                .iter()
                .position(|byte| !is_name_byte(*byte))
                .map_or(line.len(), |length| at + length);
            names.push(String::from_utf8_lossy(&line[at..end]).into_owned());
            at = end;
        } else {
            at += 1;
        }
    }
    names
}

/// True for a file in which every name is an environment variable's, being
/// a workflow, whose `env` keys are names.
fn is_workflow(relative: &str) -> bool {
    relative.ends_with(".yml") || relative.ends_with(".yaml")
}

/// Each name in `line` that starts with [`SPELLED_OUT`], is not the end of a
/// longer name or one of [`C_LIBRARY`]'s, and is written where an environment
/// variable is named. In a workflow that is anywhere, and elsewhere it is
/// straight after one of [`NAME_BEFORE`] or [`ENV_BEFORE`].
fn spelled_out_names(line: &[u8], workflow: bool) -> Vec<String> {
    let needle = SPELLED_OUT.as_bytes();
    let mut names = Vec::new();
    let mut at = 0;
    while at < line.len() {
        if !line[at..].starts_with(needle) || (at > 0 && is_name_byte(line[at - 1])) {
            at += 1;
            continue;
        }
        let end = line[at..]
            .iter()
            .position(|byte| !is_name_byte(*byte))
            .map_or(line.len(), |length| at + length);
        let before = &line[..at];
        let named_as_variable = workflow
            || before.last().is_some_and(|byte| NAME_BEFORE.contains(byte))
            || ENV_BEFORE
                .iter()
                .any(|text| before.ends_with(text.as_bytes()));
        if named_as_variable && !line[at..].starts_with(C_LIBRARY.as_bytes()) {
            names.push(String::from_utf8_lossy(&line[at..end]).into_owned());
        }
        at = end;
    }
    names
}

#[test]
fn the_spelled_out_matcher_finds_only_environment_variable_names() {
    let variable = format!("{SPELLED_OUT}CXX_DIR");
    let constant = format!("{SPELLED_OUT}PREFIX");
    let c_library = format!("{C_LIBRARY}REDUCED_FILE");
    let line = format!(
        "var(\"{variable}\") `{variable}` rustc-env={variable} $env:{variable} \
         constants::{constant} (\"{c_library}\") EVIDENCE_{constant}"
    );
    assert_eq!(spelled_out_names(line.as_bytes(), false), vec![variable; 4]);
    let key = format!("  {SPELLED_OUT}KEY: value");
    assert_eq!(
        spelled_out_names(key.as_bytes(), true),
        vec![format!("{SPELLED_OUT}KEY")]
    );
}

#[test]
fn no_file_names_a_variable_with_the_number_spelled_out() {
    let root = repository_root();
    let mut found = Vec::new();
    for relative in tracked_files(&root) {
        let path = root.join(&relative);
        // A submodule is tracked as a directory, and its files belong to
        // another repository.
        if !path.is_file() {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("read {relative}: {error}"));
        let workflow = is_workflow(&relative);
        for (index, line) in bytes.split(|byte| *byte == b'\n').enumerate() {
            for name in spelled_out_names(line, workflow) {
                if !still_read(&relative, &name) {
                    found.push(format!("{relative}:{}: {name}", index + 1));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "{} environment variable names spell the number out, where the \
         shorter _51DEGREES_ form is preferred:\n{}",
        found.len(),
        found.join("\n")
    );
}

#[test]
fn the_name_matcher_finds_only_names_that_start_with_the_digit() {
    let digit_initial = format!("{DIGIT_INITIAL}DD_PATH");
    let line = format!("export {digit_initial}=a _{digit_initial}=b X{digit_initial}");
    assert_eq!(digit_initial_names(line.as_bytes()), vec![digit_initial]);
}

#[test]
fn a_deprecated_name_is_allowed_only_where_it_is_still_read() {
    assert!(still_read(
        "cloud-request-engine/src/constants.rs",
        DEPRECATED_CLOUD_ENDPOINT
    ));
    assert!(still_read("common-sys/build.rs", DEPRECATED_CXX_DIR));
    assert!(still_read(
        "device-detection-sys/src/cxx_dir.rs",
        DEPRECATED_CXX_DIR
    ));
    assert!(still_read(
        "ip-intelligence-sys/tests/cxx_dir.rs",
        DEPRECATED_CXX_DIR
    ));
    assert!(still_read(
        ".github/workflows/pull-request.yml",
        DEPRECATED_RESOURCE_KEY
    ));
    assert!(!still_read(
        ".github/workflows/examples.yml",
        DEPRECATED_RESOURCE_KEY
    ));
    for name in READ_THROUGH_SHARED_READER {
        assert!(still_read("deprecated-names/environment.rs", name));
        assert!(!still_read("native/tests/smoke.rs", name));
        assert!(!still_read("examples/examples-shared/src/keys.rs", name));
    }
    // Not in another crate, an example, a workflow or a document.
    for elsewhere in [
        "fodid-client/src/client.rs",
        "examples/examples-shared/src/endpoint.rs",
        ".github/workflows/pull-request.yml",
        "README.md",
    ] {
        assert!(!still_read(elsewhere, DEPRECATED_CLOUD_ENDPOINT));
        assert!(!still_read(elsewhere, DEPRECATED_CXX_DIR));
    }
    // Each name only in its own crates.
    assert!(!still_read(
        "common-sys/build.rs",
        DEPRECATED_CLOUD_ENDPOINT
    ));
    assert!(!still_read(
        "cloud-request-engine/src/engine.rs",
        DEPRECATED_CXX_DIR
    ));
    // A longer name that begins with a deprecated one is another name.
    let longer = format!("{DEPRECATED_CLOUD_ENDPOINT}_OTHER");
    assert!(!still_read(
        "cloud-request-engine/src/constants.rs",
        &longer
    ));
}

#[test]
fn no_file_names_a_variable_that_starts_with_a_digit() {
    let root = repository_root();
    let mut found = Vec::new();
    for relative in tracked_files(&root) {
        let path = root.join(&relative);
        // A submodule is tracked as a directory, and its files belong to
        // another repository.
        if !path.is_file() {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("read {relative}: {error}"));
        for (index, line) in bytes.split(|byte| *byte == b'\n').enumerate() {
            for name in digit_initial_names(line) {
                if !still_read(&relative, &name) {
                    found.push(format!("{relative}:{}: {name}", index + 1));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "{} names start with a digit, which a POSIX shell cannot set, so \
         each must start with an underscore:\n{}",
        found.len(),
        found.join("\n")
    );
}
