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

//! Name the properties an example asks for that the data file in use does
//! not carry, and say where to ask about the Enterprise data file.
//!
//! The on-premise examples run on the free data files and also ask for
//! properties only the Enterprise data file has. Each example passes the
//! property names its engine reports for the loaded data file, so a property
//! the file does not carry is told apart from one the file carries but has no
//! value for with the evidence given.

/// Printed in place of a value when the data file in use does not carry the
/// property.
pub const NEEDS_ENTERPRISE_DATA_FILE: &str = "needs the Enterprise data file";

/// Whether `available` holds `name`, ignoring case as property names do.
pub fn is_available(name: &str, available: &[String]) -> bool {
    available
        .iter()
        .any(|property| property.eq_ignore_ascii_case(name))
}

/// The names in `wanted` that `available` does not hold, in `wanted`'s order.
pub fn missing_properties<'a>(wanted: &[&'a str], available: &[String]) -> Vec<&'a str> {
    wanted
        .iter()
        .copied()
        .filter(|name| !is_available(name, available))
        .collect()
}

/// The message an example prints once when the data file in use does not
/// carry some of the properties it asks for. It names them and gives
/// `contact_url`, the example's own link for asking about a copy of the
/// Enterprise data file. `None` when nothing is missing.
pub fn enterprise_data_file_message(missing: &[&str], contact_url: &str) -> Option<String> {
    if missing.is_empty() {
        return None;
    }
    let need = if missing.len() == 1 {
        "It needs"
    } else {
        "They need"
    };
    Some(format!(
        "The data file in use does not carry {}. {need} the Enterprise data \
         file. Contact us about getting a copy at {contact_url}",
        join_names(missing)
    ))
}

/// `names` as a list for a sentence, such as `A, B and C`.
fn join_names(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [only] => (*only).to_owned(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn available() -> Vec<String> {
        vec!["IsMobile".to_owned(), "PlatformName".to_owned()]
    }

    #[test]
    fn availability_ignores_case() {
        assert!(is_available("ismobile", &available()));
        assert!(!is_available("HardwareVendor", &available()));
    }

    #[test]
    fn missing_keeps_the_order_asked_for() {
        let wanted = ["HardwareVendor", "IsMobile", "PriceBand"];
        assert_eq!(
            missing_properties(&wanted, &available()),
            vec!["HardwareVendor", "PriceBand"]
        );
    }

    #[test]
    fn nothing_missing_gives_no_message() {
        assert_eq!(enterprise_data_file_message(&[], "https://x"), None);
    }

    #[test]
    fn the_message_names_each_property_and_the_link() {
        let message = enterprise_data_file_message(&["A", "B", "C"], "https://x").unwrap();
        assert!(message.contains("does not carry A, B and C."));
        assert!(message.contains("They need the Enterprise data file."));
        assert!(message.ends_with("https://x"));
        let one = enterprise_data_file_message(&["A"], "https://x").unwrap();
        assert!(one.contains("does not carry A. It needs"));
    }
}
