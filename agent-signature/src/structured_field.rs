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

//! RFC 8941 Structured Field Values: the items, parameters, inner lists and
//! dictionaries the three signature headers are written in, a parser that
//! refuses anything the standard does not allow, and the strict serialisation
//! RFC 9421 builds the signature base from.
//!
//! Every character the standard allows is ASCII, so the parser works on bytes
//! and refuses any other byte.

use crate::codec;

/// A value with no parameters.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BareItem {
    /// An integer of at most fifteen digits.
    Integer(i64),
    /// A decimal, held as a whole number of thousandths because the standard
    /// allows at most three digits after the point, so it is held exactly.
    Decimal(i64),
    /// A string of printable ASCII.
    String(String),
    /// A token.
    Token(String),
    /// A byte sequence.
    Bytes(Vec<u8>),
    /// A boolean.
    Boolean(bool),
}

/// A parameter name and its value.
pub(crate) type Parameter = (String, BareItem);

/// Parameters in the order they were written. A repeated name keeps the
/// position of its first appearance and the value of its last.
pub(crate) type Parameters = Vec<Parameter>;

/// A bare item with its parameters.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Item {
    pub(crate) value: BareItem,
    pub(crate) parameters: Parameters,
}

impl Item {
    /// The value of the named parameter.
    pub(crate) fn parameter(&self, name: &str) -> Option<&BareItem> {
        parameter(&self.parameters, name)
    }

    /// The named parameter as text, where it is a string or a token.
    pub(crate) fn string_parameter(&self, name: &str) -> Option<&str> {
        string_parameter(&self.parameters, name)
    }

    /// The value as text, where it is a string.
    pub(crate) fn as_string(&self) -> Option<&str> {
        match &self.value {
            BareItem::String(text) => Some(text),
            _ => None,
        }
    }
}

/// A dictionary member, which is an item or an inner list.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Member {
    Item(Item),
    InnerList(Vec<Item>, Parameters),
}

/// A dictionary, with its members in the order they were written.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Dictionary {
    members: Vec<(String, Member)>,
}

impl Dictionary {
    /// The members in order.
    pub(crate) fn members(&self) -> &[(String, Member)] {
        &self.members
    }

    /// The member with the given key.
    pub(crate) fn get(&self, key: &str) -> Option<&Member> {
        self.members
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, member)| member)
    }

    /// Add a member. RFC 8941 section 4.2.2 step 5 overwrites a repeated key
    /// in its original position.
    fn add(&mut self, key: String, member: Member) {
        match self.members.iter_mut().find(|(name, _)| *name == key) {
            Some(existing) => existing.1 = member,
            None => self.members.push((key, member)),
        }
    }
}

/// The value of the named parameter.
pub(crate) fn parameter<'a>(parameters: &'a [Parameter], name: &str) -> Option<&'a BareItem> {
    parameters
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
}

/// The named parameter as text, where it is a string or a token.
pub(crate) fn string_parameter<'a>(parameters: &'a [Parameter], name: &str) -> Option<&'a str> {
    match parameter(parameters, name)? {
        BareItem::String(text) | BareItem::Token(text) => Some(text),
        _ => None,
    }
}

/// Parse a field value as a dictionary. An empty field value is an empty
/// dictionary. Returns `None` for anything RFC 8941 does not allow.
pub(crate) fn parse_dictionary(input: &str) -> Option<Dictionary> {
    let mut parser = Parser::new(input);
    let mut dictionary = Dictionary::default();
    parser.skip_spaces();
    while !parser.at_end() {
        let key = parser.key()?;
        let member = if parser.peek() == Some(b'=') {
            parser.position += 1;
            parser.member_value()?
        } else {
            // A member written without a value is the boolean true with
            // whatever parameters follow it.
            let parameters = parser.parameters()?;
            Member::Item(Item {
                value: BareItem::Boolean(true),
                parameters,
            })
        };
        dictionary.add(key, member);
        parser.skip_optional_whitespace();
        if parser.at_end() {
            break;
        }
        if parser.peek() != Some(b',') {
            return None;
        }
        parser.position += 1;
        parser.skip_optional_whitespace();
        // RFC 8941 forbids a trailing comma.
        if parser.at_end() {
            return None;
        }
    }
    Some(dictionary)
}

/// Parse a field value as a single item, with nothing before or after it but
/// spaces.
pub(crate) fn parse_item(input: &str) -> Option<Item> {
    let mut parser = Parser::new(input);
    parser.skip_spaces();
    let item = parser.item()?;
    parser.skip_spaces();
    if !parser.at_end() {
        return None;
    }
    Some(item)
}

/// The strict serialisation of a dictionary member, as RFC 8941 section 4.1
/// writes it.
pub(crate) fn serialize_member(member: &Member) -> String {
    let mut out = String::new();
    match member {
        Member::Item(item) => append_item(&mut out, item),
        Member::InnerList(items, parameters) => append_inner_list(&mut out, items, parameters),
    }
    out
}

/// The strict serialisation of an item.
pub(crate) fn serialize_item(item: &Item) -> String {
    let mut out = String::new();
    append_item(&mut out, item);
    out
}

fn append_item(out: &mut String, item: &Item) {
    append_bare_item(out, &item.value);
    append_parameters(out, &item.parameters);
}

fn append_inner_list(out: &mut String, items: &[Item], parameters: &[Parameter]) {
    out.push('(');
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push(' ');
        }
        append_item(out, item);
    }
    out.push(')');
    append_parameters(out, parameters);
}

fn append_parameters(out: &mut String, parameters: &[Parameter]) {
    for (name, value) in parameters {
        out.push(';');
        out.push_str(name);
        if *value == BareItem::Boolean(true) {
            continue;
        }
        out.push('=');
        append_bare_item(out, value);
    }
}

fn append_bare_item(out: &mut String, value: &BareItem) {
    match value {
        BareItem::Integer(number) => out.push_str(&number.to_string()),
        BareItem::Decimal(thousandths) => append_decimal(out, *thousandths),
        BareItem::String(text) => {
            out.push('"');
            for character in text.chars() {
                if character == '"' || character == '\\' {
                    out.push('\\');
                }
                out.push(character);
            }
            out.push('"');
        }
        // The parser builds a token only from characters the token grammar
        // allows, so it goes out as it came in.
        BareItem::Token(text) => out.push_str(text),
        BareItem::Bytes(bytes) => {
            out.push(':');
            out.push_str(&codec::encode_standard(bytes));
            out.push(':');
        }
        BareItem::Boolean(flag) => out.push_str(if *flag { "?1" } else { "?0" }),
    }
}

/// A decimal with at least one digit after the point and no trailing zeros,
/// as RFC 8941 section 4.1.5 writes it.
fn append_decimal(out: &mut String, thousandths: i64) {
    if thousandths < 0 {
        out.push('-');
    }
    let magnitude = thousandths.unsigned_abs();
    out.push_str(&(magnitude / 1000).to_string());
    out.push('.');
    let fraction = format!("{:03}", magnitude % 1000);
    let trimmed = fraction.trim_end_matches('0');
    out.push_str(if trimmed.is_empty() { "0" } else { trimmed });
}

/// The most digits a whole number holds, RFC 8941 section 4.2.4.
const MAXIMUM_INTEGER_DIGITS: usize = 15;

/// The most characters a decimal holds, counting the point.
const MAXIMUM_DECIMAL_CHARACTERS: usize = 16;

/// The most digits before the point of a decimal.
const MAXIMUM_DECIMAL_INTEGER_DIGITS: usize = 12;

/// The most digits after the point of a decimal.
const MAXIMUM_DECIMAL_FRACTION_DIGITS: usize = 3;

/// A cursor over the bytes of a field value.
struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Self {
        Parser {
            text,
            bytes: text.as_bytes(),
            position: 0,
        }
    }

    fn at_end(&self) -> bool {
        self.position >= self.bytes.len()
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn skip_spaces(&mut self) {
        while self.peek() == Some(b' ') {
            self.position += 1;
        }
    }

    fn skip_optional_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.position += 1;
        }
    }

    fn member_value(&mut self) -> Option<Member> {
        if self.peek() == Some(b'(') {
            let items = self.inner_list()?;
            let parameters = self.parameters()?;
            return Some(Member::InnerList(items, parameters));
        }
        self.item().map(Member::Item)
    }

    fn inner_list(&mut self) -> Option<Vec<Item>> {
        if self.peek() != Some(b'(') {
            return None;
        }
        self.position += 1;
        let mut items = Vec::new();
        loop {
            self.skip_spaces();
            match self.peek() {
                // The closing bracket is missing.
                None => return None,
                Some(b')') => {
                    self.position += 1;
                    return Some(items);
                }
                Some(_) => {}
            }
            items.push(self.item()?);
            if !matches!(self.peek(), None | Some(b' ' | b')')) {
                return None;
            }
        }
    }

    fn item(&mut self) -> Option<Item> {
        let value = self.bare_item()?;
        let parameters = self.parameters()?;
        Some(Item { value, parameters })
    }

    fn parameters(&mut self) -> Option<Parameters> {
        let mut parameters: Parameters = Vec::new();
        while self.peek() == Some(b';') {
            self.position += 1;
            self.skip_spaces();
            let name = self.key()?;
            let mut value = BareItem::Boolean(true);
            if self.peek() == Some(b'=') {
                self.position += 1;
                value = self.bare_item()?;
            }
            // RFC 8941 section 4.2.3.2 step 7 overwrites a repeated parameter
            // in its original position, so the last one written wins.
            match parameters.iter_mut().find(|(key, _)| *key == name) {
                Some(existing) => existing.1 = value,
                None => parameters.push((name, value)),
            }
        }
        Some(parameters)
    }

    fn key(&mut self) -> Option<String> {
        let first = self.peek()?;
        if !first.is_ascii_lowercase() && first != b'*' {
            return None;
        }
        let start = self.position;
        self.position += 1;
        while let Some(current) = self.peek() {
            if current.is_ascii_lowercase()
                || current.is_ascii_digit()
                || matches!(current, b'_' | b'-' | b'.' | b'*')
            {
                self.position += 1;
            } else {
                break;
            }
        }
        Some(self.text[start..self.position].to_owned())
    }

    fn bare_item(&mut self) -> Option<BareItem> {
        match self.peek()? {
            b'"' => self.string(),
            b':' => self.byte_sequence(),
            b'?' => self.boolean(),
            b'-' => self.number(),
            current if current.is_ascii_digit() => self.number(),
            current if current.is_ascii_alphabetic() || current == b'*' => Some(self.token()),
            _ => None,
        }
    }

    fn string(&mut self) -> Option<BareItem> {
        // Skip the opening quote.
        self.position += 1;
        let mut value = String::new();
        while let Some(current) = self.peek() {
            self.position += 1;
            match current {
                b'\\' => {
                    let escaped = self.peek()?;
                    self.position += 1;
                    if escaped != b'"' && escaped != b'\\' {
                        return None;
                    }
                    value.push(char::from(escaped));
                }
                b'"' => return Some(BareItem::String(value)),
                0x20..=0x7e => value.push(char::from(current)),
                _ => return None,
            }
        }
        // The closing quote is missing.
        None
    }

    fn byte_sequence(&mut self) -> Option<BareItem> {
        // Skip the opening colon.
        self.position += 1;
        let start = self.position;
        loop {
            match self.peek()? {
                b':' => break,
                // RFC 8941 section 4.2.7 allows only the base64 alphabet
                // between the colons.
                current
                    if current.is_ascii_alphanumeric() || matches!(current, b'+' | b'/' | b'=') =>
                {
                    self.position += 1;
                }
                _ => return None,
            }
        }
        let encoded = &self.text[start..self.position];
        // Skip the closing colon.
        self.position += 1;
        // RFC 8941 section 4.2.7 says a parser should not fail when the
        // padding is absent, so it is put back before decoding.
        codec::decode_standard_padding_optional(encoded).map(BareItem::Bytes)
    }

    fn boolean(&mut self) -> Option<BareItem> {
        // Skip the question mark.
        self.position += 1;
        let value = match self.peek()? {
            b'0' => false,
            b'1' => true,
            _ => return None,
        };
        self.position += 1;
        Some(BareItem::Boolean(value))
    }

    fn number(&mut self) -> Option<BareItem> {
        let start = self.position;
        let negative = self.peek() == Some(b'-');
        if negative {
            self.position += 1;
        }
        // A number needs at least one digit, so a lone minus sign is not one.
        if !self.peek()?.is_ascii_digit() {
            return None;
        }
        // RFC 8941 section 4.2.4 limits how long a number may be, and a
        // number over the limit is a field that cannot be read.
        let mut integer_digits = 0;
        let mut fraction_digits = 0;
        let mut decimal_point = None;
        let mut length = 0;
        while let Some(current) = self.peek() {
            if current.is_ascii_digit() {
                if decimal_point.is_some() {
                    fraction_digits += 1;
                } else {
                    integer_digits += 1;
                }
            } else if current == b'.' && decimal_point.is_none() {
                if integer_digits > MAXIMUM_DECIMAL_INTEGER_DIGITS {
                    return None;
                }
                decimal_point = Some(self.position);
            } else {
                break;
            }
            length += 1;
            self.position += 1;
            let limit = if decimal_point.is_some() {
                MAXIMUM_DECIMAL_CHARACTERS
            } else {
                MAXIMUM_INTEGER_DIGITS
            };
            if length > limit {
                return None;
            }
        }
        let digits_start = if negative { start + 1 } else { start };
        match decimal_point {
            None => {
                let magnitude: i64 = self.text[digits_start..self.position].parse().ok()?;
                Some(BareItem::Integer(if negative {
                    -magnitude
                } else {
                    magnitude
                }))
            }
            Some(point) => {
                // A decimal has between one and three digits after the point.
                if !(1..=MAXIMUM_DECIMAL_FRACTION_DIGITS).contains(&fraction_digits) {
                    return None;
                }
                let whole: i64 = self.text[digits_start..point].parse().ok()?;
                let fraction: i64 = self.text[point + 1..self.position].parse().ok()?;
                let scale = 10_i64.pow((MAXIMUM_DECIMAL_FRACTION_DIGITS - fraction_digits) as u32);
                let magnitude = whole * 1000 + fraction * scale;
                Some(BareItem::Decimal(if negative {
                    -magnitude
                } else {
                    magnitude
                }))
            }
        }
    }

    fn token(&mut self) -> BareItem {
        let start = self.position;
        self.position += 1;
        while let Some(current) = self.peek() {
            if current.is_ascii_alphanumeric()
                || matches!(
                    current,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                        | b':'
                        | b'/'
                )
            {
                self.position += 1;
            } else {
                break;
            }
        }
        BareItem::Token(self.text[start..self.position].to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signature_input_reads_and_writes_back_strictly() {
        let dictionary = parse_dictionary(
            "sig1=(\"@authority\" \"signature-agent\";key=\"agent1\");created=1700000000; \
             keyid=\"abc\";tag=\"web-bot-auth\"",
        )
        .unwrap();
        let member = dictionary.get("sig1").unwrap();
        // The space after the semicolon is legal and the strict form drops it.
        assert_eq!(
            serialize_member(member),
            "(\"@authority\" \"signature-agent\";key=\"agent1\");created=1700000000;\
             keyid=\"abc\";tag=\"web-bot-auth\""
        );
    }

    #[test]
    fn a_member_with_no_value_is_true() {
        let dictionary = parse_dictionary("a, b=?0, c;x=1").unwrap();
        assert_eq!(serialize_member(dictionary.get("a").unwrap()), "?1");
        assert_eq!(serialize_member(dictionary.get("b").unwrap()), "?0");
        assert_eq!(serialize_member(dictionary.get("c").unwrap()), "?1;x=1");
    }

    #[test]
    fn a_repeated_key_keeps_its_place_and_takes_the_last_value() {
        let dictionary = parse_dictionary("a=1, b=2, a=3").unwrap();
        let keys: Vec<&str> = dictionary
            .members()
            .iter()
            .map(|(key, _)| key.as_str())
            .collect();
        assert_eq!(keys, ["a", "b"]);
        assert_eq!(serialize_member(dictionary.get("a").unwrap()), "3");
    }

    #[test]
    fn decimals_are_written_without_trailing_zeros() {
        let item = parse_item("1.50").unwrap();
        assert_eq!(serialize_item(&item), "1.5");
        let item = parse_item("-0.125").unwrap();
        assert_eq!(serialize_item(&item), "-0.125");
        let item = parse_item("7.0").unwrap();
        assert_eq!(serialize_item(&item), "7.0");
    }

    #[test]
    fn numbers_over_the_limits_are_refused() {
        assert!(parse_item("1234567890123456").is_none());
        assert!(parse_item("123456789012345").is_some());
        assert!(parse_item("1234567890123.5").is_none());
        assert!(parse_item("1.2345").is_none());
        assert!(parse_item("1.").is_none());
        assert!(parse_item("-").is_none());
    }

    #[test]
    fn byte_sequences_without_padding_are_read() {
        let item = parse_item(":YWJj:").unwrap();
        assert_eq!(item.value, BareItem::Bytes(b"abc".to_vec()));
        let item = parse_item(":YQ:").unwrap();
        assert_eq!(item.value, BareItem::Bytes(b"a".to_vec()));
        assert_eq!(serialize_item(&item), ":YQ==:");
        assert!(parse_item(":Y Q:").is_none());
    }

    #[test]
    fn strings_refuse_what_the_standard_does_not_allow() {
        assert!(parse_item("\"a\\nb\"").is_none());
        assert!(parse_item("\"caf\u{e9}\"").is_none());
        assert!(parse_item("\"open").is_none());
        let item = parse_item("\"a\\\"b\"").unwrap();
        assert_eq!(serialize_item(&item), "\"a\\\"b\"");
    }

    #[test]
    fn malformed_dictionaries_are_refused() {
        assert!(parse_dictionary("a=1,").is_none());
        assert!(parse_dictionary("a=(1 2").is_none());
        assert!(parse_dictionary("A=1").is_none());
        assert!(parse_dictionary("a=1 b=2").is_none());
        assert!(parse_dictionary("").unwrap().members().is_empty());
    }
}
