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

//! Reading text as a whole number the way .NET's `int.TryParse` does, which
//! decides whether a TAC is valid.

/// Read `text` as .NET's `int.TryParse` reads it, being white space, an
/// optional sign, the digits 0 to 9, white space, then any number of NUL
/// characters. White space here is space and U+0009 to U+000D. `None` when
/// the text is not a 32-bit integer.
pub(crate) fn parse_int(text: &str) -> Option<i32> {
    text.trim_end_matches('\0')
        .trim_matches(is_number_white_space)
        .parse()
        .ok()
}

/// The white space .NET allows around a number.
fn is_number_white_space(c: char) -> bool {
    matches!(c, ' ' | '\t'..='\r')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_read_as_dotnet_reads_an_integer() {
        for (text, expected) in [
            ("3", Some(3)),
            (" \t+7\r\n", Some(7)),
            ("-2", Some(-2)),
            ("0005", Some(5)),
            ("2147483647", Some(i32::MAX)),
            ("-2147483648", Some(i32::MIN)),
            ("5\0\0", Some(5)),
            ("5 \0", Some(5)),
            ("2147483648", None),
            ("5\0 ", None),
            ("\u{a0}5", None),
            ("\u{663}", None),
            ("1.0", None),
            ("five", None),
            ("+", None),
            (" ", None),
            ("", None),
        ] {
            assert_eq!(parse_int(text), expected, "{text:?}");
        }
    }
}
