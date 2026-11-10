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
//! an underscore, because a POSIX shell cannot set a name that starts with a
//! digit. The test reads every file git tracks, which leaves out build output
//! and the submodules, and fails on any name that starts with the digit.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The start of a name that a shell cannot set, split so this file does not
/// match itself.
const DIGIT_INITIAL: &str = concat!("5", "1DEGREES_");

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

#[test]
fn the_name_matcher_finds_only_names_that_start_with_the_digit() {
    let digit_initial = format!("{DIGIT_INITIAL}DD_PATH");
    let line = format!("export {digit_initial}=a _{digit_initial}=b X{digit_initial}");
    assert_eq!(digit_initial_names(line.as_bytes()), vec![digit_initial]);
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
                found.push(format!("{relative}:{}: {name}", index + 1));
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
