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

//! Resolve a cloud resource key from the environment, and the
//! obvious-placeholder check.
//!
//! Cloud examples and example tests read a 51Degrees resource key from the
//! environment rather than hard-coding it. The resolver also consults the
//! CI-exported tiered names so the same resolver works in a local shell and on
//! the build agents.

use base64::Engine as _;

/// The resource-key environment variable, checked first.
///
/// The leading underscore makes the name a valid shell identifier, so
/// `export _51DEGREES_RESOURCE_KEY=...` works and every 51Degrees language
/// reads the same name. An explicit value here (set by a developer or by CI)
/// overrides every other source.
pub const RESOURCE_KEY_ENV_VAR: &str = "_51DEGREES_RESOURCE_KEY";

/// The same name without the leading underscore, checked second.
///
/// A POSIX shell rejects an identifier starting with a digit, so this name
/// cannot be set by `export` or by an inline assignment, and reaches a process
/// only through `env 51DEGREES_RESOURCE_KEY=... command`, PowerShell or a CI
/// definition. It is read so a setup already using it keeps working.
pub const RESOURCE_KEY_ENV_VAR_NO_UNDERSCORE: &str = "51DEGREES_RESOURCE_KEY";

/// The CI-exported bespoke-tier resource-key environment variable, checked second.
///
/// Preferred over the basic tier because a bespoke key is a superset: it grants the
/// basic-tier properties too, so every cloud example and test resolves a working
/// key from it, whereas a basic key would fail the paid-only examples.
pub const CI_RESOURCE_KEY_BESPOKE_ENV_VAR: &str = "_51DEGREES_RESOURCE_KEY_BESPOKE";

/// The CI-exported basic-tier resource-key environment variable, checked last.
pub const CI_RESOURCE_KEY_BASIC_ENV_VAR: &str = "_51DEGREES_RESOURCE_KEY_BASIC";

/// The order in which the resource-key environment variables are consulted.
///
/// An explicit `_51DEGREES_RESOURCE_KEY` wins, then the same name without the
/// leading underscore, then the bespoke CI key (a superset of the basic one),
/// then the basic CI key. The first variable set to a non-blank value supplies
/// the key.
pub const RESOURCE_KEY_ENV_VARS: [&str; 4] = [
    RESOURCE_KEY_ENV_VAR,
    RESOURCE_KEY_ENV_VAR_NO_UNDERSCORE,
    CI_RESOURCE_KEY_BESPOKE_ENV_VAR,
    CI_RESOURCE_KEY_BASIC_ENV_VAR,
];

/// Read a cloud resource key from the environment.
///
/// The variables in [`RESOURCE_KEY_ENV_VARS`] are checked in order and the first
/// one set to a non-blank value is returned, trimmed of surrounding whitespace.
/// Returns `None` when none is set (or all are blank), so a cloud example can
/// print a clear "set a resource key" message rather than failing obscurely.
pub fn resource_key_from_env() -> Option<String> {
    for name in RESOURCE_KEY_ENV_VARS {
        if let Some(value) = non_blank_env(name) {
            return Some(value);
        }
    }
    None
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
        for name in RESOURCE_KEY_ENV_VARS {
            std::env::remove_var(name);
        }
    }

    #[test]
    fn returns_none_when_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_all();
        assert_eq!(resource_key_from_env(), None);
    }

    #[test]
    fn aligned_name_wins_over_tiered() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_all();
        std::env::set_var(CI_RESOURCE_KEY_BESPOKE_ENV_VAR, "bespoke-key");
        std::env::set_var(CI_RESOURCE_KEY_BASIC_ENV_VAR, "basic-key");
        std::env::set_var(RESOURCE_KEY_ENV_VAR, "aligned-key");
        assert_eq!(resource_key_from_env().as_deref(), Some("aligned-key"));
        clear_all();
    }

    #[test]
    fn the_name_without_the_underscore_is_read() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_all();
        std::env::set_var(RESOURCE_KEY_ENV_VAR_NO_UNDERSCORE, "no-underscore-key");
        assert_eq!(
            resource_key_from_env().as_deref(),
            Some("no-underscore-key")
        );
        clear_all();
    }

    #[test]
    fn the_name_checked_first_can_be_exported() {
        // A POSIX shell rejects an identifier starting with a digit, so the
        // name the examples tell a developer to export has to start with a
        // letter or an underscore or it cannot be set on Linux or macOS.
        let first = RESOURCE_KEY_ENV_VARS[0];
        let start = first.chars().next().expect("a non-empty name");
        assert!(
            start == '_' || start.is_ascii_alphabetic(),
            "{first} cannot be set with export"
        );
    }

    #[test]
    fn falls_back_through_the_chain() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_all();
        // Only the basic CI name is set; it is last in the chain but still found.
        std::env::set_var(CI_RESOURCE_KEY_BASIC_ENV_VAR, "basic-key");
        assert_eq!(resource_key_from_env().as_deref(), Some("basic-key"));
        clear_all();
    }

    #[test]
    fn blank_value_is_skipped() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_all();
        std::env::set_var(RESOURCE_KEY_ENV_VAR, "   ");
        std::env::set_var(CI_RESOURCE_KEY_BESPOKE_ENV_VAR, "real-key");
        assert_eq!(resource_key_from_env().as_deref(), Some("real-key"));
        clear_all();
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
