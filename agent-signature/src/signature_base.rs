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

//! Rebuilds the signature base, the exact text an agent signed, as RFC 9421
//! section 2.5 sets out.

use std::collections::HashSet;

use crate::structured_field::{serialize_item, Item};

/// The name of the last line of every signature base.
const SIGNATURE_PARAMS_NAME: &str = "@signature-params";

/// Finds the value of one covered component.
pub(crate) trait ComponentResolver {
    /// The value of the component named `name`, or `None` where it cannot be
    /// rebuilt.
    fn resolve(&self, name: &str, component: &Item) -> Option<String>;
}

/// Build the signature base: one line for each covered component in the
/// order the signer listed them, then the `@signature-params` line. Returns
/// `None` where a component is not a string, is listed twice, or cannot be
/// rebuilt.
pub(crate) fn build_signature_base(
    components: &[Item],
    signature_params: &str,
    resolver: &dyn ComponentResolver,
) -> Option<String> {
    let mut text = String::new();
    let mut seen = HashSet::new();
    for component in components {
        // RFC 9421 section 2.1 requires every component identifier to be a
        // string.
        let name = component.as_string()?;
        // Each identifier is written in its strict serialisation, which is
        // how a compliant signer wrote it whatever spelling the header used.
        let identifier = serialize_item(component);
        // A component listed twice makes the base ambiguous, which RFC 9421
        // section 2.5 forbids. Comparing the serialised identifiers catches
        // two spellings of one identifier as well as two identical ones.
        if !seen.insert(identifier.clone()) {
            return None;
        }
        let value = resolver.resolve(name, component)?;
        text.push_str(&identifier);
        text.push_str(": ");
        text.push_str(&value);
        text.push('\n');
    }
    text.push('"');
    text.push_str(SIGNATURE_PARAMS_NAME);
    text.push_str("\": ");
    text.push_str(signature_params);
    Some(text)
}

/// The `@authority` value RFC 9421 section 2.2.3 describes: the host
/// lowercased, with the port removed where it is the default one for the
/// scheme. A host in square brackets keeps them, and only a port after the
/// closing bracket is removed.
pub(crate) fn build_authority(host: &str, scheme: Option<&str>) -> Option<String> {
    if host.is_empty() {
        return None;
    }
    let authority = host.trim().to_lowercase();
    let default_port = if scheme == Some("http") {
        ":80"
    } else {
        ":443"
    };
    let search_from = authority.rfind(']').unwrap_or(0);
    let separator = authority[search_from..]
        .find(':')
        .map(|index| index + search_from);
    match separator {
        Some(index) if authority[index..] == *default_port => Some(authority[..index].to_owned()),
        _ => Some(authority),
    }
}

/// The bytes a signature is checked over. RFC 9421 builds the base as ASCII,
/// so a character outside ASCII becomes '?', as it does in the .NET
/// packages.
pub(crate) fn ascii_bytes(text: &str) -> Vec<u8> {
    text.chars()
        .map(|character| {
            if character.is_ascii() {
                character as u8
            } else {
                b'?'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_default_port_is_removed_from_the_authority() {
        assert_eq!(
            build_authority("Example.COM:443", Some("https")).as_deref(),
            Some("example.com")
        );
        assert_eq!(
            build_authority("example.com:80", Some("http")).as_deref(),
            Some("example.com")
        );
        assert_eq!(
            build_authority("example.com:8443", Some("https")).as_deref(),
            Some("example.com:8443")
        );
        assert_eq!(
            build_authority("example.com:80", Some("https")).as_deref(),
            Some("example.com:80")
        );
        assert_eq!(
            build_authority("[2001:db8::1]:443", None).as_deref(),
            Some("[2001:db8::1]")
        );
        assert_eq!(
            build_authority("[2001:db8::1]", None).as_deref(),
            Some("[2001:db8::1]")
        );
        assert_eq!(build_authority("", None), None);
    }
}
