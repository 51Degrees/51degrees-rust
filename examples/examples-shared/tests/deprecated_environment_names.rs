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

//! The file every test and example reads a renamed environment variable
//! through, `deprecated-names/environment.rs`. The new name is read first,
//! and the deprecated name it replaced is read when the new one is unset or
//! empty. The rule is tested against a stand-in for the environment. What is
//! said on standard error is tested in a child process, because the line is
//! said once for the life of a process and a test's own standard error
//! cannot be read from inside it.

#[allow(dead_code)]
#[path = "../../../deprecated-names/environment.rs"]
mod deprecated_names;

use std::ffi::OsString;
use std::process::Command;

use deprecated_names::{old_name, resolve, OLD_NAMES};

/// The variable the child process reads.
const READ_BY_CHILD: &str = "_51DEGREES_DD_PATH";

/// Set only for the child process, to the value it must read. Without it
/// [`the_child_reads_a_renamed_variable_twice`] does nothing.
const CHILD_EXPECTS: &str = "_51DEGREES_DEPRECATED_NAMES_CHILD_EXPECTS";

/// The start of a name that a shell cannot set, split so the test that
/// refuses such names does not find one here.
const DIGIT_INITIAL: &str = concat!("5", "1DEGREES_");

/// The one deprecated name that spells the number out, split in the same way.
const SPELLED_OUT_CXX_DIR: &str = concat!("FIFTY", "ONE_CXX_DIR");

/// A stand-in for the environment, holding "new" and "old" as given.
fn holding(
    new: Option<&'static str>,
    old: Option<&'static str>,
) -> impl Fn(&str) -> Option<OsString> {
    move |name| match name {
        "new" => new.map(OsString::from),
        "old" => old.map(OsString::from),
        _ => None,
    }
}

fn value(text: &str) -> Option<OsString> {
    Some(OsString::from(text))
}

#[test]
fn only_the_old_name_gives_its_value() {
    assert_eq!(
        resolve("new", Some("old"), &holding(None, Some("from old"))),
        (value("from old"), Some("old"))
    );
}

#[test]
fn an_empty_new_name_leaves_the_old_one_read() {
    assert_eq!(
        resolve("new", Some("old"), &holding(Some(""), Some("from old"))),
        (value("from old"), Some("old"))
    );
}

#[test]
fn an_old_name_set_to_nothing_is_still_read() {
    assert_eq!(
        resolve("new", Some("old"), &holding(None, Some(""))),
        (value(""), Some("old"))
    );
}

#[test]
fn both_names_give_the_new_one() {
    assert_eq!(
        resolve(
            "new",
            Some("old"),
            &holding(Some("from new"), Some("from old"))
        ),
        (value("from new"), None)
    );
}

#[test]
fn only_the_new_name_gives_its_value() {
    assert_eq!(
        resolve("new", Some("old"), &holding(Some("from new"), None)),
        (value("from new"), None)
    );
}

#[test]
fn neither_name_gives_nothing() {
    assert_eq!(
        resolve("new", Some("old"), &holding(None, None)),
        (None, None)
    );
}

#[test]
fn an_empty_new_name_alone_is_given_as_it_is() {
    assert_eq!(
        resolve("new", Some("old"), &holding(Some(""), None)),
        (value(""), None)
    );
}

#[test]
fn a_name_that_replaced_none_is_read_alone() {
    assert_eq!(
        resolve("new", None, &holding(None, Some("from old"))),
        (None, None)
    );
}

/// Runs [`the_child_reads_a_renamed_variable_twice`] in a new process with
/// the variable's two names set as given, and counts the deprecation line on
/// its standard error.
fn deprecation_lines_from_a_child(new: Option<&str>, old: Option<&str>, expects: &str) -> usize {
    let old_name = old_name(READ_BY_CHILD).expect("the variable replaced an old name");
    let mut child = Command::new(std::env::current_exe().expect("this test binary"));
    child
        .args([
            "--exact",
            "the_child_reads_a_renamed_variable_twice",
            "--nocapture",
        ])
        .env(CHILD_EXPECTS, expects)
        .env_remove(READ_BY_CHILD)
        .env_remove(old_name);
    if let Some(new) = new {
        child.env(READ_BY_CHILD, new);
    }
    if let Some(old) = old {
        child.env(old_name, old);
    }
    let output = child.output().expect("the child process runs");
    let standard_error = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "the child process failed:\n{}\n{standard_error}",
        String::from_utf8_lossy(&output.stdout)
    );
    let line = format!(
        "The environment variable '{old_name}' is deprecated and '{READ_BY_CHILD}' replaces it."
    );
    standard_error.matches(&line).count()
}

#[test]
fn the_child_reads_a_renamed_variable_twice() {
    let Some(expected) = std::env::var_os(CHILD_EXPECTS) else {
        return;
    };
    for _ in 0..2 {
        assert_eq!(
            deprecated_names::var_os(READ_BY_CHILD),
            Some(expected.clone())
        );
    }
}

#[test]
fn an_old_name_is_reported_once_on_standard_error() {
    assert_eq!(
        deprecation_lines_from_a_child(None, Some("from old"), "from old"),
        1
    );
}

#[test]
fn nothing_is_reported_while_the_new_name_is_read() {
    assert_eq!(
        deprecation_lines_from_a_child(Some("from new"), Some("from old"), "from new"),
        0
    );
}

#[test]
fn each_renamed_variable_has_its_old_name() {
    let current: Vec<&str> = OLD_NAMES.iter().map(|(current, _)| *current).collect();
    assert_eq!(
        current,
        [
            "_51DEGREES_RESOURCE_KEY",
            "_51DEGREES_LICENSE_KEY",
            "_51DEGREES_DD_PATH",
            "_51DEGREES_IPI_PATH",
            "_51DEGREES_IPI_ENTERPRISE_SHARE",
            "_51DEGREES_CLOUD_ENDPOINT",
            "_51DEGREES_CXX_DIR",
        ]
    );
    for (current, old) in OLD_NAMES {
        let expected = if current == "_51DEGREES_CXX_DIR" {
            SPELLED_OUT_CXX_DIR.to_owned()
        } else {
            format!("{DIGIT_INITIAL}{}", &current["_51DEGREES_".len()..])
        };
        assert_eq!(old, expected, "{current}");
        assert_eq!(old_name(current), Some(old));
    }
    assert_eq!(old_name("_51DEGREES_REQUIRE_TAC"), None);
}
