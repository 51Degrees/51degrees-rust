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

//! Removing credentials from text that is about to be shown to a person.
//!
//! A cloud request carries a resource key, and sometimes a licence key, and
//! both of them reach the places an error message likes to quote, being the
//! request address and the service's own reply, which repeats the key back
//! when it cannot read it. An error is printed by tests, by logs and by
//! whatever a consumer does with it, so anything that reaches an error has to
//! have the credentials taken out of it first.
//!
//! [`redact_with`] is for code that knows the credentials it is holding, which
//! is the stronger form because it matches the exact value whatever the value
//! looks like. [`redact`] is the backstop for code that does not know them, and
//! it works on shape alone, which is why the error types call it on every
//! free-text field they print.
//!
//! The same rules are carried by the `redact` module of the `fodid-client`
//! crate, because the 51Did client is published on its own with a deliberately
//! small set of dependencies and does not depend on this crate. Change the two
//! together.

use std::borrow::Cow;

/// The marker written in place of anything removed, so a reader can tell the
/// difference between a value that was taken out and one that was never there.
pub const REDACTED: &str = "[redacted]";

/// The shortest value [`redact_with`] treats as a secret. A shorter one is
/// ignored, because removing every occurrence of a two or three character
/// string would destroy the message rather than clean it.
const MINIMUM_SECRET_LENGTH: usize = 8;

/// Query-string parameter names whose value is a credential. Matched without
/// regard to case.
const CREDENTIAL_PARAMETERS: [&str; 4] = ["resource", "license", "licence", "resourcekey"];

/// Path segments after which the next segment of a URL is a credential. The
/// 51Did routes take the resource key as part of the route rather than as a
/// query parameter, so the segment after `key` or `verify` is the key itself.
const CREDENTIAL_ROUTE_MARKERS: [&str; 4] = ["key", "verify", "resource", "resourcekey"];

/// The prefix a 51Degrees resource key starts with, used by the shape-based
/// backstop.
const KEY_PREFIX: &str = "AQ";

/// How many key characters must follow [`KEY_PREFIX`] before a run is treated
/// as a key. Long enough that ordinary words do not match.
const KEY_MINIMUM_TAIL: usize = 12;

/// Remove anything that has the shape of a credential from `text`.
///
/// This knows no actual values, so it works on shape alone, being the value of
/// a `resource`, `license` or `licence` query parameter, the URL path segment
/// that follows a `key`, `verify` or `resource` segment, and any run that
/// starts `AQ` and carries at least twelve more key characters.
///
/// Use [`redact_with`] wherever the caller holds the values, because matching
/// the value itself also catches a key this one would not recognise.
///
/// The text comes back borrowed when there was nothing to remove.
pub fn redact(text: &str) -> Cow<'_, str> {
    redact_with(text, &[] as &[&str])
}

/// Remove `secrets`, and anything that has the shape of a credential, from
/// `text`.
///
/// Each secret is removed both as it is and in its percent-encoded form, since
/// a value in a URL has usually been encoded by the time it reaches an error.
/// A secret shorter than eight characters, or one that is only whitespace, is
/// ignored. The shape-based rules of [`redact`] run afterwards, so a credential
/// the caller did not know about is still taken out.
///
/// The text comes back borrowed when there was nothing to remove.
pub fn redact_with<'a, S: AsRef<str>>(text: &'a str, secrets: &[S]) -> Cow<'a, str> {
    let mut current: Cow<'a, str> = Cow::Borrowed(text);

    for secret in secrets {
        let secret = secret.as_ref().trim();
        if secret.len() < MINIMUM_SECRET_LENGTH {
            continue;
        }
        if let Some(replaced) = replace_all(&current, secret) {
            current = Cow::Owned(replaced);
        }
        let encoded = percent_encode(secret);
        if encoded != secret {
            if let Some(replaced) = replace_all(&current, &encoded) {
                current = Cow::Owned(replaced);
            }
        }
    }

    if let Some(replaced) = redact_by_shape(&current) {
        current = Cow::Owned(replaced);
    }
    current
}

/// Replace every occurrence of `needle` with [`REDACTED`], or return `None`
/// when the needle is not there, so the caller can keep a borrowed value.
fn replace_all(haystack: &str, needle: &str) -> Option<String> {
    if haystack.contains(needle) {
        Some(haystack.replace(needle, REDACTED))
    } else {
        None
    }
}

/// Percent-encode `value` the way a URL path segment or query value is
/// encoded, leaving only the unreserved characters of RFC 3986 as they are.
fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Apply the three shape-based rules in one pass, or return `None` when none of
/// them matched.
///
/// The ranges every rule wants removed are collected first and the text is
/// rebuilt once, so two rules that cover the same run (a key-shaped value in a
/// `resource` parameter, for example) produce one marker rather than two.
fn redact_by_shape(text: &str) -> Option<String> {
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    collect_parameter_values(text, &mut ranges);
    collect_route_segments(text, &mut ranges);
    collect_key_shaped_runs(text, &mut ranges);
    if ranges.is_empty() {
        return None;
    }

    ranges.sort_unstable();
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;
    for (start, end) in ranges {
        // A range already covered by one that has been written is skipped, and
        // one that only overlaps its end carries on from where that ended.
        if end <= cursor {
            continue;
        }
        let start = start.max(cursor);
        out.push_str(&text[cursor..start]);
        out.push_str(REDACTED);
        cursor = end;
    }
    out.push_str(&text[cursor..]);
    Some(out)
}

/// Find the value of every credential-carrying query parameter.
fn collect_parameter_values(text: &str, ranges: &mut Vec<(usize, usize)>) {
    let lowered = text.to_ascii_lowercase();
    for name in CREDENTIAL_PARAMETERS {
        for (index, _) in lowered.match_indices(name) {
            if !is_parameter_start(&lowered, index) {
                continue;
            }
            let after_name = index + name.len();
            if lowered.as_bytes().get(after_name) != Some(&b'=') {
                continue;
            }
            let start = after_name + 1;
            let end = start + value_length(&text[start..], is_parameter_value_end);
            if end > start {
                ranges.push((start, end));
            }
        }
    }
}

/// True when the parameter name at `index` starts a parameter rather than
/// ending a longer word, so `resource=` matches where `myresource=` does not.
fn is_parameter_start(text: &str, index: usize) -> bool {
    if index == 0 {
        return true;
    }
    match text.as_bytes()[index - 1] {
        b'?' | b'&' | b';' => true,
        byte => !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'%')),
    }
}

/// True for a character that ends a query parameter value, being the start of
/// the next parameter, the fragment, or the end of a URL quoted inside prose.
fn is_parameter_value_end(byte: u8) -> bool {
    matches!(
        byte,
        b'&' | b'#' | b'\'' | b'"' | b'<' | b'>' | b')' | b',' | b'\\'
    ) || byte.is_ascii_whitespace()
}

/// Find the path segment after any marker segment of a URL, which is where the
/// 51Did routes carry the resource key.
///
/// Only text that looks like a URL is examined, so prose that happens to carry
/// the word `key` is left alone.
fn collect_route_segments(text: &str, ranges: &mut Vec<(usize, usize)>) {
    for (scheme_index, _) in text.match_indices("://") {
        let mut cursor = scheme_index + "://".len();
        // Step over the authority, which is everything up to the first slash.
        cursor += value_length(&text[cursor..], |byte| byte == b'/' || is_url_end(byte));
        let mut previous: Option<&str> = None;
        while text.as_bytes().get(cursor) == Some(&b'/') {
            cursor += 1;
            let length = value_length(&text[cursor..], |byte| {
                matches!(byte, b'/' | b'?' | b'#') || is_url_end(byte)
            });
            let segment = &text[cursor..cursor + length];
            let follows_marker = previous.is_some_and(|name| {
                CREDENTIAL_ROUTE_MARKERS
                    .iter()
                    .any(|marker| name.eq_ignore_ascii_case(marker))
            });
            if length > 0 && follows_marker {
                ranges.push((cursor, cursor + length));
            }
            previous = Some(segment);
            cursor += length;
        }
    }
}

/// True for a character that cannot be part of a URL in running text, so a URL
/// quoted inside a sentence stops where the sentence takes over.
fn is_url_end(byte: u8) -> bool {
    matches!(byte, b'\'' | b'"' | b'<' | b'>' | b')' | b',' | b'\\') || byte.is_ascii_whitespace()
}

/// Find every run that has the shape of a resource key, wherever it sits. This
/// is what catches the service quoting the key back inside its own message.
fn collect_key_shaped_runs(text: &str, ranges: &mut Vec<(usize, usize)>) {
    let bytes = text.as_bytes();
    for (index, _) in text.match_indices(KEY_PREFIX) {
        if index > 0 && is_key_byte(bytes[index - 1]) {
            continue;
        }
        let mut end = index + KEY_PREFIX.len();
        while end < bytes.len() && is_key_byte(bytes[end]) {
            end += 1;
        }
        if end - index - KEY_PREFIX.len() >= KEY_MINIMUM_TAIL {
            ranges.push((index, end));
        }
    }
}

/// True for a character a base64url resource key is made of.
fn is_key_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
}

/// The number of bytes before the first byte `is_end` accepts, or the whole
/// length when it accepts none.
fn value_length(text: &str, is_end: impl Fn(u8) -> bool) -> usize {
    text.bytes().position(is_end).unwrap_or(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The only key-shaped value written anywhere in this repository. It is not
    /// a real resource key and the service refuses it.
    const NOT_A_KEY: &str = "AQ-NOT-A-REAL-KEY-000000";

    /// [`NOT_A_KEY`] as a message shows it, with its last 4 characters.
    const NOT_A_KEY_MASKED: &str = "********************0000";

    #[test]
    fn text_with_nothing_to_remove_is_borrowed() {
        let out = redact("cloud request failed with status 503");
        assert!(matches!(out, Cow::Borrowed(_)));
        assert_eq!(out, "cloud request failed with status 503");
    }

    #[test]
    fn a_resource_parameter_value_shows_only_its_last_four_characters() {
        let out = redact(
            "https://cloud.51degrees.com/api/v4/accessibleproperties?resource=secretvalue1234",
        );
        assert_eq!(
            out,
            "https://cloud.51degrees.com/api/v4/accessibleproperties?resource=***********1234"
        );
    }

    #[test]
    fn a_licence_parameter_value_is_masked_whatever_the_spelling() {
        for name in ["license", "licence", "License"] {
            let text =
                format!("https://cloud/json?resource=abcdefghijkl&{name}=licencevalue1234&x=1");
            let out = redact(&text);
            assert!(
                !out.contains("licencevalue1234"),
                "{name} was left in {out}"
            );
            assert!(
                out.contains(&format!("{name}=************1234&x=1")),
                "the masked key or the rest of the query was lost: {out}"
            );
        }
    }

    #[test]
    fn each_key_in_a_licence_keys_parameter_shows_its_own_last_four_characters() {
        for name in ["LicenseKeys", "LicenseKey", "licensekeys"] {
            let text = format!(
                "https://distributor.example.test/api/v2/download?{name}=\
                 ABCDEFGHIJKL|MNOPQRSTUVWX&Download=True&Type=HashV41"
            );
            let out = redact(&text);
            assert_eq!(
                out,
                format!(
                    "https://distributor.example.test/api/v2/download?{name}=\
                     ********IJKL|********UVWX&Download=True&Type=HashV41"
                )
            );
        }
    }

    #[test]
    fn keys_separated_by_an_encoded_bar_are_masked_one_by_one() {
        let out = redact("https://d.example.test/f?LicenseKeys=ABCDEFGHIJKL%7CMNOPQRSTUVWX");
        assert_eq!(
            out,
            "https://d.example.test/f?LicenseKeys=********IJKL%7C********UVWX"
        );
    }

    #[test]
    fn a_parameter_name_that_only_ends_in_resource_is_left_alone() {
        let out = redact("https://cloud/json?myresource=keepthisvalue&a=1");
        assert_eq!(out, "https://cloud/json?myresource=keepthisvalue&a=1");
    }

    #[test]
    fn a_key_in_a_route_shows_only_its_last_four_characters() {
        let out = redact(
            "failed to send request to 'https://cloud.51degrees.com/api/v4/id/key/somekeyvalue'",
        );
        assert_eq!(
            out,
            "failed to send request to 'https://cloud.51degrees.com/api/v4/id/key/********alue'"
        );
    }

    #[test]
    fn a_key_in_a_verify_route_is_masked_and_the_query_survives() {
        let out = redact("https://cloud/api/v4/id/verify/somekeyvalue?51did=abc&owid=abc");
        assert_eq!(
            out,
            "https://cloud/api/v4/id/verify/********alue?51did=abc&owid=abc"
        );
    }

    #[test]
    fn prose_about_a_key_is_not_a_route() {
        let out = redact("the published signing key could not be used");
        assert_eq!(out, "the published signing key could not be used");
    }

    #[test]
    fn a_key_shaped_run_is_masked_wherever_it_sits() {
        let text = format!("'{NOT_A_KEY}' could not be read as a valid resource key.");
        let out = redact(&text);
        assert_eq!(
            out,
            format!("'{NOT_A_KEY_MASKED}' could not be read as a valid resource key.")
        );
    }

    #[test]
    fn a_word_beginning_aq_is_not_a_key() {
        let out = redact("the AQueduct failed");
        assert_eq!(out, "the AQueduct failed");
    }

    #[test]
    fn a_known_key_shows_only_its_last_four_characters() {
        let out = redact_with(
            "the service refused 'plain-old-credential'",
            &["plain-old-credential"],
        );
        assert_eq!(out, "the service refused '****************tial'");
    }

    #[test]
    fn a_known_key_is_masked_in_its_percent_encoded_form() {
        let out = redact_with("the service refused 'a%2Fb%2Bc-value'", &["a/b+c-value"]);
        assert_eq!(out, "the service refused '***********alue'");
    }

    #[test]
    fn a_known_key_of_four_characters_or_fewer_is_replaced_entirely() {
        let out = redact_with("the licence key 'KEY1' was refused", &["KEY1"]);
        assert_eq!(out, "the licence key '****' was refused");
    }

    #[test]
    fn a_blank_secret_is_ignored() {
        let out = redact_with("nothing to see", &["   ", ""]);
        assert_eq!(out, "nothing to see");
    }

    #[test]
    fn a_masked_key_is_left_as_it_is() {
        let text = "the licence '************MNOP' and ?resource=***********1234";
        let out = redact_with(text, &["ABCDEFGHIJKLMNOP"]);
        assert_eq!(out, text);
    }

    #[test]
    fn two_rules_covering_the_same_run_mask_it_once() {
        let text = format!("https://cloud/json?resource={NOT_A_KEY}");
        let out = redact(&text);
        assert_eq!(
            out,
            format!("https://cloud/json?resource={NOT_A_KEY_MASKED}")
        );
    }

    #[test]
    fn the_reason_survives_masking() {
        let text = format!(
            "Cloud service at 'https://cloud/json?resource={NOT_A_KEY}' returned status \
             code '400' with content {{\"errors\":[\"'{NOT_A_KEY}' could not be read as a \
             valid resource key.\"]}}"
        );
        let out = redact(&text);
        assert!(!out.contains(NOT_A_KEY), "the key survived: {out}");
        assert!(out.contains("status code '400'"));
        assert!(out.contains("could not be read as a valid resource key"));
    }

    #[test]
    fn multi_byte_text_is_not_split() {
        let text = format!("réponse refusée pour '{NOT_A_KEY}' à Paris");
        let out = redact(&text);
        assert_eq!(
            out,
            format!("réponse refusée pour '{NOT_A_KEY_MASKED}' à Paris")
        );
    }
}
