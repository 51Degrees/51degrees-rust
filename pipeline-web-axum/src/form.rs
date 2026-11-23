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

//! A minimal `application/x-www-form-urlencoded` decoder.
//!
//! Both the query string and a form POST body use the same encoding, so one
//! decoder serves both. It is kept here rather than pulled from a crate because
//! the rules are small and the adapter only needs the common case: pairs split
//! on `&`, names and values split on the first `=`, `+` decoded to a space, and
//! `%XX` decoded to the byte it names.
//!
//! Decoding is lenient. A malformed percent escape (too short or not hex) is
//! passed through verbatim rather than rejected, and bytes that do not form
//! valid UTF-8 are recovered lossily, so a stray byte never drops a whole field.
//! Evidence values are strings, and a faithful best effort is more useful here
//! than a hard error.

/// Decode an `application/x-www-form-urlencoded` payload into `(name, value)`
/// pairs.
///
/// Pairs are separated by `&`. Within a pair the first `=` separates the name
/// from the value; a pair with no `=` becomes a name with an empty value. An
/// empty segment (a leading, trailing or doubled `&`) is skipped. Both halves
/// are percent- and `+`-decoded.
pub fn parse_form_urlencoded(input: &[u8]) -> Vec<(String, String)> {
    let mut pairs = Vec::new();

    for segment in input.split(|&byte| byte == b'&') {
        if segment.is_empty() {
            continue;
        }
        let (name, value) = match segment.iter().position(|&byte| byte == b'=') {
            Some(index) => (&segment[..index], &segment[index + 1..]),
            None => (segment, &[][..]),
        };
        pairs.push((decode_component(name), decode_component(value)));
    }

    pairs
}

/// Percent- and `+`-decode a single component into a lossy UTF-8 string.
///
/// `+` becomes a space, `%XX` becomes the byte `0xXX`, and any other byte is
/// kept as is. A `%` not followed by two hex digits is emitted literally.
fn decode_component(input: &[u8]) -> String {
    let mut bytes = Vec::with_capacity(input.len());
    let mut index = 0;

    while index < input.len() {
        match input[index] {
            b'+' => {
                bytes.push(b' ');
                index += 1;
            }
            b'%' => {
                // A valid escape needs two more bytes, both hex digits.
                if let (Some(&high), Some(&low)) = (input.get(index + 1), input.get(index + 2)) {
                    if let (Some(high), Some(low)) = (hex_value(high), hex_value(low)) {
                        bytes.push((high << 4) | low);
                        index += 3;
                        continue;
                    }
                }
                // Not a valid escape: keep the percent sign verbatim.
                bytes.push(b'%');
                index += 1;
            }
            byte => {
                bytes.push(byte);
                index += 1;
            }
        }
    }

    String::from_utf8_lossy(&bytes).into_owned()
}

/// The numeric value of a single ASCII hex digit, or `None` if it is not one.
fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_pairs_and_decodes() {
        assert_eq!(
            parse_form_urlencoded(b"a=1&b=two+words&c=%21"),
            vec![
                ("a".to_owned(), "1".to_owned()),
                ("b".to_owned(), "two words".to_owned()),
                ("c".to_owned(), "!".to_owned()),
            ]
        );
    }

    #[test]
    fn value_less_pair_has_empty_value() {
        assert_eq!(
            parse_form_urlencoded(b"flag&x=1"),
            vec![
                ("flag".to_owned(), String::new()),
                ("x".to_owned(), "1".to_owned()),
            ]
        );
    }

    #[test]
    fn empty_segments_are_skipped() {
        assert_eq!(
            parse_form_urlencoded(b"&a=1&&b=2&"),
            vec![
                ("a".to_owned(), "1".to_owned()),
                ("b".to_owned(), "2".to_owned()),
            ]
        );
    }

    #[test]
    fn malformed_escape_passes_through() {
        // A trailing percent and a non-hex escape are kept literally.
        assert_eq!(
            parse_form_urlencoded(b"a=50%&b=%zz"),
            vec![
                ("a".to_owned(), "50%".to_owned()),
                ("b".to_owned(), "%zz".to_owned()),
            ]
        );
    }

    #[test]
    fn empty_input_is_empty() {
        assert!(parse_form_urlencoded(b"").is_empty());
    }
}
