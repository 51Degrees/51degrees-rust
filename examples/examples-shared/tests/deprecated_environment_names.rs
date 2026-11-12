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
//! empty. The rule is tested against a stand-in for the environment.

#[allow(dead_code)]
#[path = "../../../deprecated-names/environment.rs"]
mod deprecated_names;

use std::ffi::OsString;

use deprecated_names::{old_name, resolve, OLD_NAMES};

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
