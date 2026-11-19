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

//! Reads an environment variable under its current name or, when that is
//! unset or empty, under the deprecated name it replaced, and says once on
//! standard error that the old name is deprecated. The tests of every crate
//! and the examples include this file by path, so each old name is written
//! in one place. It uses the standard library alone.

use std::env::VarError;
use std::ffi::OsString;
use std::sync::Mutex;

/// Each current name with the deprecated name it replaced.
pub const OLD_NAMES: [(&str, &str); 7] = [
    ("_51DEGREES_RESOURCE_KEY", "51DEGREES_RESOURCE_KEY"),
    ("_51DEGREES_LICENSE_KEY", "51DEGREES_LICENSE_KEY"),
    ("_51DEGREES_DD_PATH", "51DEGREES_DD_PATH"),
    ("_51DEGREES_IPI_PATH", "51DEGREES_IPI_PATH"),
    (
        "_51DEGREES_IPI_ENTERPRISE_SHARE",
        "51DEGREES_IPI_ENTERPRISE_SHARE",
    ),
    ("_51DEGREES_CLOUD_ENDPOINT", "51DEGREES_CLOUD_ENDPOINT"),
    ("_51DEGREES_CXX_DIR", "FIFTYONE_CXX_DIR"),
];

/// The deprecated names already reported, so each is reported once.
static REPORTED: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

/// The deprecated name `name` replaced, if it replaced one.
pub fn old_name(name: &str) -> Option<&'static str> {
    OLD_NAMES
        .iter()
        .find(|(current, _)| *current == name)
        .map(|(_, old)| *old)
}

/// What `name` reads as, and the deprecated name the value came from when it
/// did not come from `name`. `read` gives the value of a variable.
pub fn resolve(
    name: &str,
    old: Option<&'static str>,
    read: &dyn Fn(&str) -> Option<OsString>,
) -> (Option<OsString>, Option<&'static str>) {
    let current = read(name);
    if current.as_ref().is_some_and(|value| !value.is_empty()) {
        return (current, None);
    }
    match old.and_then(|old| read(old).map(|value| (value, old))) {
        Some((value, old)) => (Some(value), Some(old)),
        None => (current, None),
    }
}

fn read_environment(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}

fn report(old: &'static str, name: &str) {
    let mut reported = REPORTED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !reported.contains(&old) {
        reported.push(old);
        eprintln!("The environment variable '{old}' is deprecated and '{name}' replaces it.");
    }
}

fn as_text(value: Option<OsString>) -> Result<String, VarError> {
    value
        .ok_or(VarError::NotPresent)?
        .into_string()
        .map_err(VarError::NotUnicode)
}

/// As [`std::env::var_os`], with `old` as the deprecated name of `name`.
pub fn var_os_with_old(name: &str, old: Option<&'static str>) -> Option<OsString> {
    let (value, from) = resolve(name, old, &read_environment);
    if let Some(old) = from {
        report(old, name);
    }
    value
}

/// As [`std::env::var_os`], for a name in [`OLD_NAMES`] or any other.
pub fn var_os(name: &str) -> Option<OsString> {
    var_os_with_old(name, old_name(name))
}

/// As [`std::env::var`], for a name in [`OLD_NAMES`] or any other.
pub fn var(name: &str) -> Result<String, VarError> {
    as_text(var_os(name))
}

/// As [`std::env::var`], for a reader whose deprecated name is not the one
/// in [`OLD_NAMES`].
pub fn var_with_old(name: &str, old: &'static str) -> Result<String, VarError> {
    as_text(var_os_with_old(name, Some(old)))
}
