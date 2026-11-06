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

//! Reading a setting's value from text, as .NET's `TryParse` methods,
//! `Enum.Parse` and the pipeline's comma-separated list reader do, so a
//! value that .NET accepts is accepted here and one it refuses is refused.
//! A .NET `TimeSpan` is read by [`TimeSpan`](super::TimeSpan).

use super::ConfigValue;

/// Why a value could not be read as a setting's type. The configuration
/// error names the setting and the value, and this gives the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseFailure {
    kind: FailureKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FailureKind {
    Text,
    Enumeration(Vec<String>),
    List,
    ListForText,
    NoTextForm,
}

impl ParseFailure {
    /// The text is not a value of the type.
    pub fn text() -> Self {
        ParseFailure {
            kind: FailureKind::Text,
        }
    }

    /// The text names none of `names`, the names of an enumeration's values.
    pub fn enumeration<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        ParseFailure {
            kind: FailureKind::Enumeration(names.into_iter().map(Into::into).collect()),
        }
    }

    /// A list was given where the type takes one value.
    pub fn list() -> Self {
        ParseFailure {
            kind: FailureKind::List,
        }
    }

    /// A list was given where the type is text. .NET passes it to the
    /// method as it is, and the call refuses it.
    pub(crate) fn list_for_text() -> Self {
        ParseFailure {
            kind: FailureKind::ListForText,
        }
    }

    /// The type has no text form, as a .NET type without a `TryParse`
    /// method has none.
    pub(crate) fn no_text_form() -> Self {
        ParseFailure {
            kind: FailureKind::NoTextForm,
        }
    }

    /// True when the method itself refused the value, rather than .NET's
    /// reading of it, so .NET tries no other method of the same name.
    pub(crate) fn ends_search(&self) -> bool {
        self.kind == FailureKind::ListForText
    }

    /// The message .NET gives, after `prefix`, which says which setting
    /// expects which type. The second part is the message of the cause, as
    /// .NET's inner exception gives it, if there is one.
    pub(crate) fn describe(&self, prefix: &str, value: &ConfigValue) -> (String, Option<String>) {
        match &self.kind {
            FailureKind::Text => (
                format!(
                    "{prefix}. Failed to parse value '{}' using the static 'TryParse' method.",
                    value.display()
                ),
                None,
            ),
            FailureKind::Enumeration(names) => (
                format!(
                    "{prefix}. Failed to parse enum value '{}'. Valid values are: {}",
                    value.display(),
                    names.join(", ")
                ),
                Some(format!(
                    "Requested value '{}' was not found.",
                    value.display()
                )),
            ),
            FailureKind::List => (
                format!("{prefix} but supplied object is of type 'String[]'."),
                None,
            ),
            FailureKind::ListForText => (
                "Object of type 'System.String[]' cannot be converted to type 'System.String'."
                    .to_string(),
                None,
            ),
            FailureKind::NoTextForm => (
                format!(
                    "{prefix} but this type does not have the expected 'TryParse' method (or \
                     has multiples that cannot be resolved)."
                ),
                None,
            ),
        }
    }
}

/// A type a setting can take, read from the text a configuration gives.
///
/// Implemented for `String`, `bool`, the integer types .NET builders take,
/// `f64`, [`TimeSpan`](super::TimeSpan), and `Vec<String>` and
/// `Box<[String]>` for .NET's ``List`1`` and `String[]`. A crate implements
/// it for its own enumeration with [`parse_enum`].
pub trait ConfigType: Sized + Send + 'static {
    /// The name of the matching .NET type, such as `Int32` or `Boolean`,
    /// which configuration errors give.
    const TYPE_NAME: &'static str;

    /// Read the value from text, accepting what .NET's parser accepts.
    fn parse_text(text: &str) -> Result<Self, ParseFailure>;

    /// Read the value from a list given in code. Only a list type accepts
    /// one.
    fn parse_list(_list: &[String]) -> Result<Self, ParseFailure> {
        Err(ParseFailure::list())
    }

    /// Read the value from either form.
    fn parse_value(value: &ConfigValue) -> Result<Self, ParseFailure> {
        match value {
            ConfigValue::Text(text) => Self::parse_text(text),
            ConfigValue::List(list) => Self::parse_list(list),
        }
    }
}

impl ConfigType for String {
    const TYPE_NAME: &'static str = "String";

    fn parse_text(text: &str) -> Result<Self, ParseFailure> {
        Ok(text.to_string())
    }

    fn parse_list(_list: &[String]) -> Result<Self, ParseFailure> {
        Err(ParseFailure::list_for_text())
    }
}

impl ConfigType for bool {
    const TYPE_NAME: &'static str = "Boolean";

    fn parse_text(text: &str) -> Result<Self, ParseFailure> {
        parse_bool(text).ok_or_else(ParseFailure::text)
    }
}

impl ConfigType for f64 {
    const TYPE_NAME: &'static str = "Double";

    fn parse_text(text: &str) -> Result<Self, ParseFailure> {
        parse_double(text).ok_or_else(ParseFailure::text)
    }
}

impl ConfigType for Vec<String> {
    const TYPE_NAME: &'static str = "List`1";

    fn parse_text(text: &str) -> Result<Self, ParseFailure> {
        Ok(parse_csv(text))
    }

    fn parse_list(list: &[String]) -> Result<Self, ParseFailure> {
        Ok(list.to_vec())
    }
}

impl ConfigType for Box<[String]> {
    const TYPE_NAME: &'static str = "String[]";

    fn parse_text(text: &str) -> Result<Self, ParseFailure> {
        Ok(parse_csv(text).into_boxed_slice())
    }

    fn parse_list(list: &[String]) -> Result<Self, ParseFailure> {
        Ok(list.into())
    }
}

macro_rules! integer_config_type {
    ($($t:ty => $name:literal),* $(,)?) => {
        $(
            impl ConfigType for $t {
                const TYPE_NAME: &'static str = $name;

                fn parse_text(text: &str) -> Result<Self, ParseFailure> {
                    parse_integer(text)
                        .and_then(|n| <$t>::try_from(n).ok())
                        .ok_or_else(ParseFailure::text)
                }
            }
        )*
    };
}

integer_config_type!(
    i32 => "Int32",
    u16 => "UInt16",
    u32 => "UInt32",
    i64 => "Int64",
    u64 => "UInt64",
);

/// Read the name of an enumeration's value as .NET's `Enum.Parse` does
/// when told to ignore case. Space around the text is ignored, and a whole
/// number gives the value with that number. `values` pairs each name with
/// its number and value.
///
/// Unlike .NET, a number no value has and names joined by commas are
/// refused, because neither gives a value of a Rust enumeration.
pub fn parse_enum<T: Copy>(text: &str, values: &[(&str, i64, T)]) -> Result<T, ParseFailure> {
    let trimmed = text.trim_matches(is_dotnet_white_space);
    let found = match parse_integer(trimmed) {
        Some(number) if starts_like_number(trimmed) => {
            values.iter().find(|(_, n, _)| *n as i128 == number)
        }
        _ => values
            .iter()
            .find(|(name, _, _)| equal_ignoring_case(name, trimmed)),
    };
    found
        .map(|(_, _, value)| *value)
        .ok_or_else(|| ParseFailure::enumeration(values.iter().map(|(name, _, _)| *name)))
}

fn starts_like_number(text: &str) -> bool {
    matches!(text.chars().next(), Some('0'..='9' | '+' | '-'))
}

/// True when the two names are the same ignoring case, as .NET's
/// `OrdinalIgnoreCase` compares them.
pub(crate) fn equal_ignoring_case(a: &str, b: &str) -> bool {
    a.chars()
        .flat_map(char::to_uppercase)
        .eq(b.chars().flat_map(char::to_uppercase))
}

/// The characters .NET's number parsing skips before and after a number.
fn is_dotnet_white_space(c: char) -> bool {
    matches!(c, '\u{9}'..='\u{d}' | ' ')
}

/// Remove the white space .NET's number parsing allows around a number,
/// and the trailing NUL characters it also ignores.
fn trim_number(text: &str) -> &str {
    text.trim_start_matches(is_dotnet_white_space)
        .trim_end_matches('\0')
        .trim_end_matches(is_dotnet_white_space)
}

/// Read a whole number as .NET's integer `TryParse` does with its default
/// style, which allows space around it and a leading sign. The caller
/// narrows it to the setting's type, refusing a number out of range.
fn parse_integer(text: &str) -> Option<i128> {
    let text = trim_number(text);
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // Leading zeros do not change the value, and dropping them keeps a
    // long run of them from overflowing the parse.
    let significant = digits.trim_start_matches('0');
    if significant.len() > 30 {
        return None;
    }
    let magnitude: i128 = if significant.is_empty() {
        0
    } else {
        significant.parse().ok()?
    };
    Some(if negative { -magnitude } else { magnitude })
}

/// Read a whole number as .NET's `int.TryParse` does.
pub(crate) fn parse_i32(text: &str) -> Option<i32> {
    parse_integer(text).and_then(|n| i32::try_from(n).ok())
}

/// Read a boolean as .NET's `bool.TryParse` does, being `True` or `False`
/// in any case, with white space and NUL characters around it ignored.
pub(crate) fn parse_bool(text: &str) -> Option<bool> {
    let trimmed = text.trim_matches(|c: char| c.is_whitespace() || c == '\0');
    if trimmed.eq_ignore_ascii_case("true") {
        Some(true)
    } else if trimmed.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

/// Read a number as .NET's `double.TryParse` does in the invariant
/// culture, which allows space around it, a leading sign, commas between
/// the digits before the decimal point, a decimal point and an exponent,
/// and the words `Infinity`, `-Infinity` and `NaN` in any case.
fn parse_double(text: &str) -> Option<f64> {
    let text = trim_number(text);
    if text.eq_ignore_ascii_case("nan") {
        return Some(f64::NAN);
    }
    if text.eq_ignore_ascii_case("infinity") || text.eq_ignore_ascii_case("+infinity") {
        return Some(f64::INFINITY);
    }
    if text.eq_ignore_ascii_case("-infinity") {
        return Some(f64::NEG_INFINITY);
    }
    let bytes = text.as_bytes();
    let mut plain = String::with_capacity(text.len());
    let mut index = 0;
    if let Some(sign @ (b'+' | b'-')) = bytes.first() {
        plain.push(*sign as char);
        index = 1;
    }
    let mut digits = 0;
    // Digits before the decimal point, with commas allowed once a digit
    // has been read.
    while index < bytes.len() {
        match bytes[index] {
            b @ b'0'..=b'9' => {
                plain.push(b as char);
                digits += 1;
            }
            b',' if digits > 0 => {}
            _ => break,
        }
        index += 1;
    }
    if bytes.get(index) == Some(&b'.') {
        plain.push('.');
        index += 1;
        while let Some(b @ b'0'..=b'9') = bytes.get(index) {
            plain.push(*b as char);
            digits += 1;
            index += 1;
        }
    }
    if digits == 0 {
        return None;
    }
    if let Some(b'e' | b'E') = bytes.get(index) {
        let mut exponent = String::from("e");
        let mut next = index + 1;
        if let Some(sign @ (b'+' | b'-')) = bytes.get(next) {
            exponent.push(*sign as char);
            next += 1;
        }
        let start = next;
        while let Some(b @ b'0'..=b'9') = bytes.get(next) {
            exponent.push(*b as char);
            next += 1;
        }
        if next > start {
            plain.push_str(&exponent);
            index = next;
        }
    }
    if index != bytes.len() {
        return None;
    }
    plain.parse().ok()
}

/// Split comma-separated text into its values, as the .NET pipeline's
/// configuration reads a list.
///
/// A value may be put in double quotes so that it can hold a comma, and a
/// double quote inside quotes is written as two. Space is kept as given.
/// An empty value between two commas is kept, but a comma at the end adds
/// no empty value, and empty text gives an empty list.
pub fn parse_csv(text: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                current.push('"');
                chars.next();
            }
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => values.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        values.push(current);
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_are_read_as_dotnet_reads_them() {
        assert_eq!(i32::parse_text("8"), Ok(8));
        assert_eq!(i32::parse_text(" \t-12\n"), Ok(-12));
        assert_eq!(i32::parse_text("+7"), Ok(7));
        assert_eq!(i32::parse_text("0007"), Ok(7));
        assert_eq!(i32::parse_text("5\0\0"), Ok(5));
        assert_eq!(i32::parse_text("2147483647"), Ok(i32::MAX));
        assert_eq!(i32::parse_text("-2147483648"), Ok(i32::MIN));
        assert_eq!(i32::parse_text("2147483648"), Err(ParseFailure::text()));
        assert_eq!(i32::parse_text("1.0"), Err(ParseFailure::text()));
        assert_eq!(i32::parse_text("1,000"), Err(ParseFailure::text()));
        assert_eq!(i32::parse_text("1e3"), Err(ParseFailure::text()));
        assert_eq!(i32::parse_text(""), Err(ParseFailure::text()));
        assert_eq!(i32::parse_text("-"), Err(ParseFailure::text()));
        assert_eq!(i32::parse_text("WrongType"), Err(ParseFailure::text()));
        assert_eq!(i32::parse_text("\u{a0}5"), Err(ParseFailure::text()));
        assert_eq!(u16::parse_text("65535"), Ok(u16::MAX));
        assert_eq!(u16::parse_text("65536"), Err(ParseFailure::text()));
        assert_eq!(u16::parse_text("-0"), Ok(0));
        assert_eq!(u16::parse_text("-1"), Err(ParseFailure::text()));
        assert_eq!(u64::parse_text("18446744073709551615"), Ok(u64::MAX));
        assert_eq!(
            i64::parse_text("00000000000000000000000000000000000000001"),
            Ok(1)
        );
    }

    #[test]
    fn booleans_are_read_as_dotnet_reads_them() {
        assert_eq!(bool::parse_text("True"), Ok(true));
        assert_eq!(bool::parse_text("false"), Ok(false));
        assert_eq!(bool::parse_text(" TRUE \0"), Ok(true));
        assert_eq!(bool::parse_text("1"), Err(ParseFailure::text()));
        assert_eq!(bool::parse_text("yes"), Err(ParseFailure::text()));
        assert_eq!(bool::parse_text(""), Err(ParseFailure::text()));
    }

    #[test]
    fn doubles_are_read_as_dotnet_reads_them() {
        assert_eq!(f64::parse_text("1"), Ok(1.0));
        assert_eq!(f64::parse_text(" 0.5 "), Ok(0.5));
        assert_eq!(f64::parse_text(".5"), Ok(0.5));
        assert_eq!(f64::parse_text("5."), Ok(5.0));
        assert_eq!(f64::parse_text("-1.5e2"), Ok(-150.0));
        assert_eq!(f64::parse_text("1E-1"), Ok(0.1));
        assert_eq!(f64::parse_text("1,000.25"), Ok(1000.25));
        assert_eq!(f64::parse_text("1,,0"), Ok(10.0));
        assert_eq!(f64::parse_text("Infinity"), Ok(f64::INFINITY));
        assert_eq!(f64::parse_text("-infinity"), Ok(f64::NEG_INFINITY));
        assert!(f64::parse_text("NaN").unwrap().is_nan());
        assert_eq!(f64::parse_text(",5"), Err(ParseFailure::text()));
        assert_eq!(f64::parse_text("1.2.3"), Err(ParseFailure::text()));
        assert_eq!(f64::parse_text("1e"), Err(ParseFailure::text()));
        assert_eq!(f64::parse_text("."), Err(ParseFailure::text()));
        assert_eq!(f64::parse_text("0x10"), Err(ParseFailure::text()));
        assert_eq!(f64::parse_text("inf"), Err(ParseFailure::text()));
    }

    // The cases of .NET's `PipelineBuilder_BuildFromConfiguration_List*`
    // tests, and the edges of its `ParseCsv`.
    #[test]
    fn lists_are_split_as_dotnet_splits_them() {
        assert_eq!(parse_csv("A,|"), vec!["A", "|"]);
        assert_eq!(parse_csv("|,\",\""), vec!["|", ","]);
        assert_eq!(parse_csv("|,\"\"\"\""), vec!["|", "\""]);
        assert_eq!(parse_csv("|"), vec!["|"]);
        assert_eq!(parse_csv("\"6\"\"\",\"12\"\"\""), vec!["6\"", "12\""]);
        assert_eq!(parse_csv(""), Vec::<String>::new());
        assert_eq!(parse_csv("a,"), vec!["a"]);
        assert_eq!(parse_csv(",a"), vec!["", "a"]);
        assert_eq!(parse_csv("a,,b"), vec!["a", "", "b"]);
        assert_eq!(parse_csv(" a , b "), vec![" a ", " b "]);
        assert_eq!(parse_csv("a\"b,c\"d"), vec!["ab,cd"]);
        assert_eq!(parse_csv("\"\""), Vec::<String>::new());
        assert_eq!(parse_csv("\"\","), vec![""]);
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Profile {
        Low,
        Balanced,
    }

    const PROFILES: &[(&str, i64, Profile)] = &[
        ("LowMemory", 0, Profile::Low),
        ("Balanced", 3, Profile::Balanced),
    ];

    #[test]
    fn enumerations_are_read_by_name_or_number() {
        assert_eq!(parse_enum("Balanced", PROFILES), Ok(Profile::Balanced));
        assert_eq!(parse_enum(" lowmemory ", PROFILES), Ok(Profile::Low));
        assert_eq!(parse_enum("3", PROFILES), Ok(Profile::Balanced));
        assert_eq!(parse_enum("+0", PROFILES), Ok(Profile::Low));
        let refused = ParseFailure::enumeration(["LowMemory", "Balanced"]);
        assert_eq!(parse_enum("High", PROFILES), Err(refused.clone()));
        assert_eq!(parse_enum("1", PROFILES), Err(refused.clone()));
        assert_eq!(parse_enum("LowMemory,Balanced", PROFILES), Err(refused));
    }

    #[test]
    fn a_list_is_refused_where_one_value_is_taken() {
        let list = ConfigValue::List(vec!["1".into()]);
        assert_eq!(i32::parse_value(&list), Err(ParseFailure::list()));
        assert_eq!(
            String::parse_value(&list),
            Err(ParseFailure::list_for_text())
        );
        assert_eq!(Vec::<String>::parse_value(&list), Ok(vec!["1".to_string()]));
    }
}
