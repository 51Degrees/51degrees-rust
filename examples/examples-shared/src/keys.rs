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

//! Resolve a cloud resource key and a license key from the environment, and
//! the obvious-placeholder check.
//!
//! A cloud example reads its 51Degrees resource key from one name,
//! `_51DEGREES_RESOURCE_KEY`, rather than hard-coding it. An example's own
//! live test reads the key it runs the example with from
//! `_51DEGREES_RESOURCE_KEY_BESPOKE` and hands it to the example. A live test
//! that needs a key outside the Bespoke product reads it from
//! `_51DEGREES_RESOURCE_KEY_BASIC`.
//!
//! Every name starts with an underscore because a POSIX shell cannot set a
//! name that starts with a digit.

use base64::Engine as _;

/// The resource-key environment variable.
///
/// The leading underscore makes the name a valid shell identifier, so
/// `export _51DEGREES_RESOURCE_KEY=...` works and every 51Degrees language
/// reads the same name.
pub const RESOURCE_KEY_ENV_VAR: &str = "_51DEGREES_RESOURCE_KEY";

/// The licence-key environment variable.
pub const LICENSE_KEY_ENV_VAR: &str = "_51DEGREES_LICENSE_KEY";

/// The environment variable an example's own live test reads the resource
/// key from, a key that carries every property the examples show.
pub const LIVE_TEST_RESOURCE_KEY_ENV_VAR: &str = "_51DEGREES_RESOURCE_KEY_BESPOKE";

/// The environment variable a live test reads a key outside the Bespoke
/// product from, which gets only the properties every key carries.
pub const LIVE_TEST_BASIC_RESOURCE_KEY_ENV_VAR: &str = "_51DEGREES_RESOURCE_KEY_BASIC";

/// Read a cloud resource key from [`RESOURCE_KEY_ENV_VAR`], trimmed of
/// surrounding whitespace. Returns `None` when the variable is unset or blank,
/// so a cloud example can print a clear "set a resource key" message rather
/// than failing obscurely.
pub fn resource_key_from_env() -> Option<String> {
    non_blank_env(RESOURCE_KEY_ENV_VAR)
}

/// Read the resource key an example's own live test runs the example with,
/// from [`LIVE_TEST_RESOURCE_KEY_ENV_VAR`], trimmed of surrounding whitespace.
/// Returns `None` when the variable is unset or blank, and the test then
/// checks nothing.
pub fn live_test_resource_key() -> Option<String> {
    non_blank_env(LIVE_TEST_RESOURCE_KEY_ENV_VAR)
}

/// Read the key outside the Bespoke product a live test runs with, from
/// [`LIVE_TEST_BASIC_RESOURCE_KEY_ENV_VAR`], trimmed of surrounding
/// whitespace. Returns `None` when the variable is unset or blank.
pub fn live_test_basic_resource_key() -> Option<String> {
    non_blank_env(LIVE_TEST_BASIC_RESOURCE_KEY_ENV_VAR)
}

/// Read a license key from [`LICENSE_KEY_ENV_VAR`], trimmed of surrounding
/// whitespace. Returns `None` when the variable is unset or blank, so a value
/// of only spaces counts as no key.
pub fn license_key_from_env() -> Option<String> {
    non_blank_env(LICENSE_KEY_ENV_VAR)
}

/// Read `name` from the environment, returning the trimmed value only when it is
/// present and not blank (a null-or-whitespace value is rejected).
fn non_blank_env(name: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => Some(value.trim().to_owned()),
        _ => None,
    }
}

/// Report whether `key` is *definitely* an invalid resource or license key.
///
/// This cannot confirm a key is valid, only that it is obviously not. A key is
/// treated as invalid when any of the following holds:
///
/// - it is empty or only whitespace,
/// - its trimmed length is shorter than 19 characters,
/// - it is not valid base64, or
/// - its base64-decoded form is shorter than 14 bytes.
///
/// These catch the common placeholder cases (an empty value, a truncated paste,
/// the literal text `"!!YOUR_RESOURCE_KEY!!"`) before a request is attempted.
pub fn is_invalid_key(key: &str) -> bool {
    let trimmed = key.trim();
    if trimmed.len() < 19 {
        return true;
    }
    // The standard base64 alphabet with padding. A decode failure means the
    // value is not a key.
    match base64::engine::general_purpose::STANDARD.decode(trimmed) {
        Ok(decoded) => decoded.len() < 14,
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Environment variables are process-global, so the env-mutating tests share
    // a lock to avoid interfering with each other when the test binary runs them
    // in parallel.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn clear_all() {
        std::env::remove_var(RESOURCE_KEY_ENV_VAR);
        std::env::remove_var(LIVE_TEST_RESOURCE_KEY_ENV_VAR);
        std::env::remove_var(LIVE_TEST_BASIC_RESOURCE_KEY_ENV_VAR);
    }

    #[test]
    fn returns_none_when_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_all();
        assert_eq!(resource_key_from_env(), None);
    }

    #[test]
    fn only_the_one_name_is_read() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_all();
        // A key under any other name is not the example's key.
        std::env::set_var("_51DEGREES_RESOURCE_KEY_BESPOKE", "another-key");
        assert_eq!(resource_key_from_env(), None);
        std::env::set_var(RESOURCE_KEY_ENV_VAR, "  the-key  ");
        assert_eq!(resource_key_from_env().as_deref(), Some("the-key"));
        clear_all();
    }

    #[test]
    fn a_live_test_reads_its_key_from_its_own_name() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_all();
        std::env::set_var(RESOURCE_KEY_ENV_VAR, "the-example's-key");
        assert_eq!(live_test_resource_key(), None);
        std::env::set_var(LIVE_TEST_RESOURCE_KEY_ENV_VAR, "  the-test's-key  ");
        assert_eq!(live_test_resource_key().as_deref(), Some("the-test's-key"));
        std::env::set_var(LIVE_TEST_RESOURCE_KEY_ENV_VAR, "   ");
        assert_eq!(live_test_resource_key(), None);
        clear_all();
    }

    #[test]
    fn a_live_test_reads_the_basic_key_from_its_own_name() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_all();
        std::env::set_var(LIVE_TEST_RESOURCE_KEY_ENV_VAR, "the-bespoke-key");
        assert_eq!(live_test_basic_resource_key(), None);
        std::env::set_var(LIVE_TEST_BASIC_RESOURCE_KEY_ENV_VAR, "  the-basic-key  ");
        assert_eq!(live_test_basic_resource_key().as_deref(), Some("the-basic-key"));
        clear_all();
    }

    #[test]
    fn a_license_key_is_read_trimmed() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(LICENSE_KEY_ENV_VAR, "  license-key  ");
        assert_eq!(license_key_from_env().as_deref(), Some("license-key"));
        std::env::remove_var(LICENSE_KEY_ENV_VAR);
    }

    #[test]
    fn a_license_key_of_only_spaces_is_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(LICENSE_KEY_ENV_VAR, "   ");
        assert_eq!(license_key_from_env(), None);
        std::env::remove_var(LICENSE_KEY_ENV_VAR);
    }

    #[test]
    fn the_name_can_be_exported() {
        // A POSIX shell rejects an identifier starting with a digit, so the
        // name the examples tell a developer to export has to start with a
        // letter or an underscore or it cannot be set on Linux or macOS.
        let start = RESOURCE_KEY_ENV_VAR
            .chars()
            .next()
            .expect("a non-empty name");
        assert!(
            start == '_' || start.is_ascii_alphabetic(),
            "{RESOURCE_KEY_ENV_VAR} cannot be set with export"
        );
    }

    #[test]
    fn obvious_placeholders_are_invalid() {
        assert!(is_invalid_key(""));
        assert!(is_invalid_key("   "));
        // Shorter than 19 characters.
        assert!(is_invalid_key("tooshort"));
        // Long enough but not base64.
        assert!(is_invalid_key("!!YOUR_RESOURCE_KEY!!"));
        // Long enough, valid base64, but decodes to fewer than 14 bytes.
        // "c2hvcnQtdmFsdWU=" decodes to "short-value" (11 bytes).
        assert!(is_invalid_key("c2hvcnQtdmFsdWU="));
    }

    #[test]
    fn plausible_key_is_not_flagged_invalid() {
        // A base64 string that is long enough and decodes to 21 bytes
        // ("AAAAAAAAAAAAAAAAAAAAA"), so none of the invalidity tests trip.
        let key = base64::engine::general_purpose::STANDARD.encode([b'A'; 21]);
        assert!(!is_invalid_key(&key));
    }
}
