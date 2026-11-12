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

//! The environment variables the examples read the cloud endpoint from.
//! `_51DEGREES_CLOUD_ENDPOINT` is read first, and the deprecated name it
//! replaced is read only when the first is unset or blank. Kept in its own
//! test binary because the environment belongs to the whole process, and each
//! test holds [`SERIAL`] so no other test changes the variables under it.

use std::sync::{Mutex, MutexGuard};

use examples_shared::cloud_endpoint_from_env;

/// The variable read first.
const VARIABLE: &str = "_51DEGREES_CLOUD_ENDPOINT";

/// The deprecated name the variable had before.
const OLD_VARIABLE: &str = "51DEGREES_CLOUD_ENDPOINT";

const NEW_BASE: &str = "https://new.example.test/api/v4/";
const OLD_BASE: &str = "https://old.example.test/api/v4/";

/// Held by each test for its whole run.
static SERIAL: Mutex<()> = Mutex::new(());

fn start() -> MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn set(name: &str, value: Option<&str>) {
    match value {
        Some(value) => std::env::set_var(name, value),
        None => std::env::remove_var(name),
    }
}

/// What the examples read with the two variables set as given. Both are
/// cleared again afterwards.
fn endpoint_with(new: Option<&str>, old: Option<&str>) -> Option<String> {
    set(VARIABLE, new);
    set(OLD_VARIABLE, old);
    let read = cloud_endpoint_from_env();
    set(VARIABLE, None);
    set(OLD_VARIABLE, None);
    read
}

#[test]
fn only_the_old_variable_gives_its_endpoint() {
    let _serial = start();
    assert_eq!(
        endpoint_with(None, Some(OLD_BASE)),
        Some(OLD_BASE.to_owned())
    );
}

#[test]
fn a_blank_new_variable_leaves_the_old_one_read() {
    let _serial = start();
    assert_eq!(
        endpoint_with(Some(" "), Some(OLD_BASE)),
        Some(OLD_BASE.to_owned())
    );
}

#[test]
fn both_variables_give_the_new_one() {
    let _serial = start();
    assert_eq!(
        endpoint_with(Some(NEW_BASE), Some(OLD_BASE)),
        Some(NEW_BASE.to_owned())
    );
}

#[test]
fn neither_variable_gives_none() {
    let _serial = start();
    assert_eq!(endpoint_with(None, None), None);
}
