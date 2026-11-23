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

//! Writes the builder's values as indented JSON text, in the form .NET's
//! JSON builder gives with Newtonsoft.Json's default settings.
//!
//! - Two spaces for each level, `": "` after a key, and each array item
//!   and object key on its own line. An empty array or object is `[]` or
//!   `{}`.
//! - Keys in lower case, apart from `javascriptProperties`.
//! - A double in .NET's shortest round trip form, with `.0` added when the
//!   text has no decimal point or exponent, so `1.0`, `0.1`, `1E-07` and
//!   `1E+21`. A 32-bit float the same way with the digits a float needs,
//!   so the float 0.1 is `0.1` and the float 1e9 is `1E+09`. NaN and the
//!   infinities are the strings `"NaN"`, `"Infinity"` and `"-Infinity"`.
//! - In a string, `"` and `\` escaped, `\b`, `\f`, `\n`, `\r` and `\t` for
//!   those characters, and `\u` with four lower case hex digits for the
//!   other characters below U+0020 and for U+0085, U+2028 and U+2029.
//!   Every other character, `<`, `&` and `'` included, is written as it is.

use std::borrow::Cow;

use crate::constants::JAVASCRIPT_PROPERTIES_KEY;
use crate::value::{JsonObject, JsonValue};

/// The line ending the default writer uses, being the platform's, as .NET's
/// `Environment.NewLine` is.
#[cfg(windows)]
pub const NEW_LINE: &str = "\r\n";

/// The line ending the default writer uses, being the platform's, as .NET's
/// `Environment.NewLine` is.
#[cfg(not(windows))]
pub const NEW_LINE: &str = "\n";

/// The spaces written for each level of nesting.
const INDENT: &str = "  ";

/// A double with more digits before its decimal point than this, or than
/// its own significant digit count if that is more, is written with an
/// exponent in .NET's round trip form.
const DOUBLE_ROUND_TRIP_DIGITS: usize = 17;

/// The same limit for a 32-bit float.
const FLOAT_ROUND_TRIP_DIGITS: usize = 9;

/// A number with more zeros than this between its decimal point and its
/// first digit is written with an exponent, so 0.0001 is written in full
/// and 0.00001 as `1E-05`.
const MAX_LEADING_ZEROS: i32 = 3;

/// The fewest exponent digits written.
const MIN_EXPONENT_DIGITS: usize = 2;

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Write an object as indented JSON text, ending each line with
/// `new_line`.
///
/// The default [`write_json`](crate::JsonBuilderSteps::write_json) step
/// calls this with [`NEW_LINE`]. A step that wants the same text on every
/// platform passes `"\n"`.
pub fn write_indented(object: &JsonObject, new_line: &str) -> String {
    let mut out = String::with_capacity(256);
    write_object(&mut out, object, 0, new_line);
    out
}

/// A double as .NET's JSON builder writes it.
pub(crate) fn double_text(value: f64) -> String {
    special_text(value).unwrap_or_else(|| {
        round_trip_text(
            value.is_sign_negative(),
            &format!("{:e}", value.abs()),
            DOUBLE_ROUND_TRIP_DIGITS,
        )
    })
}

/// A 32-bit float as .NET's JSON builder writes it. Rust's shortest form of
/// an `f32` holds the digits a float needs, not those of the wider `f64`.
pub(crate) fn float_text(value: f32) -> String {
    special_text(f64::from(value)).unwrap_or_else(|| {
        round_trip_text(
            value.is_sign_negative(),
            &format!("{:e}", value.abs()),
            FLOAT_ROUND_TRIP_DIGITS,
        )
    })
}

/// NaN and the infinities as the strings .NET's builder writes, or `None`
/// for a finite number.
fn special_text(value: f64) -> Option<String> {
    if value.is_nan() {
        Some("\"NaN\"".to_owned())
    } else if value.is_infinite() {
        Some(if value > 0.0 {
            "\"Infinity\"".to_owned()
        } else {
            "\"-Infinity\"".to_owned()
        })
    } else {
        None
    }
}

/// The round trip form of a finite number, from Rust's shortest exponent
/// form of its absolute value (such as `1.25e-7`), which holds the same
/// digits as .NET's.
///
/// .NET writes the digits in full unless the number has more digits before
/// its decimal point than `round_trip_digits` (or the digit count, if
/// more), or more than three zeros between its decimal point and its first
/// digit. Then it writes one digit, the rest after a point, then `E`, the
/// sign and at least two exponent digits. Newtonsoft.Json then adds `.0` to
/// text with no point and no exponent. Negative zero keeps its sign.
fn round_trip_text(negative: bool, shortest: &str, round_trip_digits: usize) -> String {
    let (mantissa, exponent) = shortest
        .split_once('e')
        .expect("Rust's exponent form always has an e");
    let exponent: i32 = exponent
        .parse()
        .expect("Rust's exponent form always has a whole exponent");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();

    let mut text = String::with_capacity(digits.len() + 8);
    if negative {
        text.push('-');
    }
    if digits == "0" {
        text.push_str("0.0");
        return text;
    }

    // The number is 0.DIGITS times ten to the power of point.
    let point = exponent + 1;
    let max_point = digits.len().max(round_trip_digits) as i32;
    if point > max_point || point < -MAX_LEADING_ZEROS {
        text.push_str(&digits[..1]);
        if digits.len() > 1 {
            text.push('.');
            text.push_str(&digits[1..]);
        }
        text.push('E');
        text.push(if exponent < 0 { '-' } else { '+' });
        text.push_str(&format!(
            "{:0width$}",
            exponent.unsigned_abs(),
            width = MIN_EXPONENT_DIGITS
        ));
    } else if point <= 0 {
        text.push_str("0.");
        text.extend(std::iter::repeat_n('0', point.unsigned_abs() as usize));
        text.push_str(&digits);
    } else {
        let point = point as usize;
        if digits.len() > point {
            text.push_str(&digits[..point]);
            text.push('.');
            text.push_str(&digits[point..]);
        } else {
            text.push_str(&digits);
            text.extend(std::iter::repeat_n('0', point - digits.len()));
            text.push_str(".0");
        }
    }
    text
}

fn write_value(out: &mut String, value: &JsonValue, depth: usize, new_line: &str) {
    match value {
        JsonValue::Null => out.push_str("null"),
        JsonValue::Bool(true) => out.push_str("true"),
        JsonValue::Bool(false) => out.push_str("false"),
        JsonValue::Integer(i) => out.push_str(&i.to_string()),
        JsonValue::Double(d) => out.push_str(&double_text(*d)),
        JsonValue::Float(f) => out.push_str(&float_text(*f)),
        JsonValue::String(s) => write_string(out, s),
        JsonValue::Array(items) => write_array(out, items, depth, new_line),
        JsonValue::Object(object) => write_object(out, object, depth, new_line),
    }
}

fn write_object(out: &mut String, object: &JsonObject, depth: usize, new_line: &str) {
    out.push('{');
    if object.is_empty() {
        out.push('}');
        return;
    }
    for (index, (key, value)) in object.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        start_line(out, depth + 1, new_line);
        write_string(out, &key_text(key));
        out.push_str(": ");
        write_value(out, value, depth + 1, new_line);
    }
    start_line(out, depth, new_line);
    out.push('}');
}

fn write_array(out: &mut String, items: &[JsonValue], depth: usize, new_line: &str) {
    out.push('[');
    if items.is_empty() {
        out.push(']');
        return;
    }
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        start_line(out, depth + 1, new_line);
        write_value(out, item, depth + 1, new_line);
    }
    start_line(out, depth, new_line);
    out.push(']');
}

fn start_line(out: &mut String, depth: usize, new_line: &str) {
    out.push_str(new_line);
    for _ in 0..depth {
        out.push_str(INDENT);
    }
}

/// A key in lower case, apart from `javascriptProperties`, as .NET's
/// contract resolver writes every key.
fn key_text(key: &str) -> Cow<'_, str> {
    if key == JAVASCRIPT_PROPERTIES_KEY || key.chars().all(is_lower_case) {
        Cow::Borrowed(key)
    } else {
        Cow::Owned(key.to_lowercase())
    }
}

/// True if lower casing the character leaves it as it is.
fn is_lower_case(c: char) -> bool {
    let mut lower = c.to_lowercase();
    lower.next() == Some(c) && lower.next().is_none()
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' || matches!(c, '\u{85}' | '\u{2028}' | '\u{2029}') => {
                let code = c as u32;
                out.push_str("\\u");
                for shift in [12, 8, 4, 0] {
                    out.push(HEX_DIGITS[((code >> shift) & 0xf) as usize] as char);
                }
            }
            _ => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doubles_are_in_dotnet_round_trip_form() {
        let cases = [
            (1.0, "1.0"),
            (0.1, "0.1"),
            (1e-7, "1E-07"),
            (1e21, "1E+21"),
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (-60.0, "-60.0"),
            (0.0001, "0.0001"),
            (0.00001, "1E-05"),
            (1.5, "1.5"),
            (123.456, "123.456"),
            (1e16, "10000000000000000.0"),
            (1e17, "1E+17"),
            (12345678901234568.0, "12345678901234568.0"),
            (123456789012345680.0, "1.2345678901234568E+17"),
            (0.30000000000000004, "0.30000000000000004"),
            (f64::MAX, "1.7976931348623157E+308"),
            (f64::MIN_POSITIVE, "2.2250738585072014E-308"),
            (5e-324, "5E-324"),
            (f64::NAN, "\"NaN\""),
            (f64::INFINITY, "\"Infinity\""),
            (f64::NEG_INFINITY, "\"-Infinity\""),
        ];
        for (value, expected) in cases {
            assert_eq!(double_text(value), expected, "{value:e}");
        }
    }

    #[test]
    fn floats_are_in_dotnet_round_trip_form() {
        let cases = [
            (1.0, "1.0"),
            (0.1, "0.1"),
            (0.3, "0.3"),
            (1e-7, "1E-07"),
            (1e21, "1E+21"),
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (0.0001, "0.0001"),
            (0.00001, "1E-05"),
            (std::f32::consts::PI, "3.1415927"),
            (16_777_216.0, "16777216.0"),
            // The nearest float is 123456792, whose shortest digits are
            // 12345679, with nine digits before the point.
            (123_456_789_i32 as f32, "123456790.0"),
            (1e9, "1E+09"),
            (f32::MAX, "3.4028235E+38"),
            (f32::from_bits(1), "1E-45"),
            (f32::NAN, "\"NaN\""),
            (f32::INFINITY, "\"Infinity\""),
            (f32::NEG_INFINITY, "\"-Infinity\""),
        ];
        for (value, expected) in cases {
            assert_eq!(float_text(value), expected, "{value:e}");
        }
        // The same numbers as doubles have more digits or no exponent.
        assert_eq!(double_text(f64::from(0.3_f32)), "0.30000001192092896");
        assert_eq!(double_text(1e9), "1000000000.0");
    }

    /// A latitude given to 3 decimal places is written with those places, as
    /// .NET writes it.
    #[test]
    fn a_rounded_latitude_is_written_to_three_places() {
        // A stored Declination of 6751 converts to 18.542741172521133.
        let latitude = fiftyone_pipeline_core::round_angle(6751.0 * 90.0 / 32767.0);
        assert_eq!(float_text(latitude), "18.543");
        assert_eq!(double_text(f64::from(latitude)), "18.542999267578125");
    }

    #[test]
    fn strings_are_escaped_as_newtonsoft_escapes_them() {
        let mut out = String::new();
        write_string(
            &mut out,
            "a\"b\\c\u{1}\u{1f}\u{7f}\u{85}\u{2028}\u{2029}<&'\u{e9}\u{8}\u{c}\n\r\t\u{1f600}",
        );
        assert_eq!(
            out,
            "\"a\\\"b\\\\c\\u0001\\u001f\u{7f}\\u0085\\u2028\\u2029<&'\u{e9}\\b\\f\\n\\r\\t\u{1f600}\""
        );
    }

    #[test]
    fn keys_are_lower_case_apart_from_javascript_properties() {
        assert_eq!(key_text("device"), "device");
        assert_eq!(key_text("BrowserName"), "browsername");
        assert_eq!(key_text("javascriptProperties"), "javascriptProperties");
        assert_eq!(key_text("JavascriptProperties"), "javascriptproperties");
    }

    #[test]
    fn layout_is_newtonsofts_indented_layout() {
        let mut inner = JsonObject::new();
        inner.insert("a", 1i64);
        inner.insert(
            "b",
            JsonValue::Array(vec![JsonValue::from("x"), JsonValue::Double(2.5)]),
        );
        inner.insert("c", JsonObject::new());
        inner.insert("d", JsonValue::Array(Vec::new()));
        let mut object = JsonObject::new();
        object.insert("device", inner);
        object.insert("e", JsonValue::Null);
        assert_eq!(
            write_indented(&object, "\r\n"),
            "{\r\n  \"device\": {\r\n    \"a\": 1,\r\n    \"b\": [\r\n      \"x\",\r\n      2.5\r\n    ],\r\n    \"c\": {},\r\n    \"d\": []\r\n  },\r\n  \"e\": null\r\n}"
        );
        assert_eq!(write_indented(&JsonObject::new(), "\n"), "{}");
    }
}
