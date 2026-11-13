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

//! The environment variables that point the build script at a 51degrees-cxx
//! checkout. `_51DEGREES_CXX_DIR` is read first, and the deprecated name it
//! replaced is read when the first is not set. The build script and this
//! test share `src/cxx_dir.rs`, because a test cannot call a build script.

#[path = "../src/cxx_dir.rs"]
mod cxx_dir;

use cxx_dir::{cxx_dir_override, CXX_DIR_VARIABLE};

/// The deprecated name the variable had before.
const OLD_VARIABLE: &str = "FIFTYONE_CXX_DIR";

/// A stand-in for the environment, holding the two variables as given.
fn holding(
    new: Option<&'static str>,
    old: Option<&'static str>,
) -> impl Fn(&str) -> Option<String> {
    move |name| match name {
        CXX_DIR_VARIABLE => new.map(str::to_owned),
        OLD_VARIABLE => old.map(str::to_owned),
        _ => None,
    }
}

#[test]
fn the_new_constant_holds_the_new_name() {
    assert_eq!(CXX_DIR_VARIABLE, "_51DEGREES_CXX_DIR");
}

#[test]
fn the_old_constant_holds_the_old_name() {
    assert_eq!(cxx_dir::OLD_CXX_DIR_VARIABLE, OLD_VARIABLE);
}

#[test]
fn only_the_old_variable_gives_its_checkout() {
    assert_eq!(
        cxx_dir_override(holding(None, Some("old"))),
        Some(("old".to_owned(), OLD_VARIABLE))
    );
}

#[test]
fn both_variables_give_the_new_one() {
    assert_eq!(
        cxx_dir_override(holding(Some("new"), Some("old"))),
        Some(("new".to_owned(), CXX_DIR_VARIABLE))
    );
}

#[test]
fn only_the_new_variable_gives_its_checkout() {
    assert_eq!(
        cxx_dir_override(holding(Some("new"), None)),
        Some(("new".to_owned(), CXX_DIR_VARIABLE))
    );
}

#[test]
fn neither_variable_gives_none() {
    assert_eq!(cxx_dir_override(holding(None, None)), None);
}

#[test]
fn an_empty_new_variable_leaves_the_old_one_read() {
    assert_eq!(
        cxx_dir_override(holding(Some(""), Some("old"))),
        Some(("old".to_owned(), OLD_VARIABLE))
    );
}

#[test]
fn a_variable_set_to_nothing_alone_still_counts_as_set() {
    assert_eq!(
        cxx_dir_override(holding(Some(""), None)),
        Some((String::new(), CXX_DIR_VARIABLE))
    );
    assert_eq!(
        cxx_dir_override(holding(None, Some(""))),
        Some((String::new(), OLD_VARIABLE))
    );
}
