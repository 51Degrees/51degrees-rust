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

//! Reads the `PipelineOptions` section of a JSON configuration file as
//! .NET's JSON configuration reader and binder read it.
//!
//! The reader is written here rather than taken from `serde_json` because
//! .NET's reader allows comments and trailing commas, keeps a number as the
//! text written, and finds names ignoring case, and the pipeline core has
//! no JSON dependency.

use std::cmp::Ordering;

use super::options::{ConfigValue, ElementOptions, PipelineOptions};
use super::text::{equal_ignoring_case, parse_i32};
use crate::error::{Error, Result};

/// The deepest nesting read, as .NET's JSON reader allows by default.
const MAX_DEPTH: usize = 64;

/// A JSON value. A number, `true` and `false` are kept as the text .NET's
/// reader gives them.
#[derive(Debug, PartialEq)]
enum Node {
    Object(Vec<(String, Node)>),
    Array(Vec<Node>),
    Text(String),
    Null,
}

pub(crate) fn read_pipeline_options(text: &str) -> Result<PipelineOptions> {
    let root = Parser::new(text).document()?;
    let Node::Object(sections) = root else {
        return Err(Error::configuration(
            "The configuration's top level must be a JSON object.",
        ));
    };
    check_duplicates(&sections, "")?;
    let section = find(&sections, "PipelineOptions").ok_or_else(|| {
        Error::configuration("The configuration has no 'PipelineOptions' section.")
    })?;
    let Node::Object(fields) = section else {
        return Err(Error::configuration(
            "The configuration's 'PipelineOptions' section must be a JSON object.",
        ));
    };
    let path = "PipelineOptions";
    let mut options = PipelineOptions::new();
    if let Some(elements) = find(fields, "Elements") {
        options.elements = read_elements(elements, &format!("{path}:Elements"))?;
    }
    // .NET binds the obsolete name into the same dictionary.
    for name in ["BuildParameters", "PipelineBuilderParameters"] {
        if let Some(parameters) = find(fields, name) {
            options
                .build_parameters
                .extend(read_parameters(parameters, &format!("{path}:{name}"))?);
        }
    }
    options
        .build_parameters
        .sort_by(|(a, _), (b, _)| compare_keys(a, b));
    Ok(options)
}

fn read_elements(node: &Node, path: &str) -> Result<Vec<ElementOptions>> {
    match node {
        Node::Null => Ok(Vec::new()),
        Node::Array(items) => items
            .iter()
            .enumerate()
            .map(|(index, item)| read_element(item, &format!("{path}:{index}")))
            .collect(),
        _ => Err(Error::configuration(format!(
            "'{path}' must be a JSON array of elements."
        ))),
    }
}

fn read_element(node: &Node, path: &str) -> Result<ElementOptions> {
    let Node::Object(fields) = node else {
        return Err(Error::configuration(format!(
            "'{path}' must be a JSON object describing an element."
        )));
    };
    // .NET's reader gives a null as empty text, so the name is empty rather
    // than not set, and the error says so.
    let builder_name = match find(fields, "BuilderName") {
        None => None,
        Some(Node::Null) => Some(String::new()),
        Some(Node::Text(name)) => Some(name.clone()),
        Some(_) => {
            return Err(Error::configuration(format!(
                "'{path}:BuilderName' must be text."
            )))
        }
    };
    let mut build_parameters = match find(fields, "BuildParameters") {
        Some(parameters) => read_parameters(parameters, &format!("{path}:BuildParameters"))?,
        None => Vec::new(),
    };
    build_parameters.sort_by(|(a, _), (b, _)| compare_keys(a, b));
    let sub_elements = match find(fields, "SubElements") {
        Some(elements) => read_elements(elements, &format!("{path}:SubElements"))?,
        None => Vec::new(),
    };
    Ok(ElementOptions {
        builder_name,
        build_parameters,
        sub_elements,
    })
}

fn read_parameters(node: &Node, path: &str) -> Result<Vec<(String, ConfigValue)>> {
    let fields = match node {
        Node::Null => return Ok(Vec::new()),
        Node::Object(fields) => fields,
        _ => {
            return Err(Error::configuration(format!(
                "'{path}' must be a JSON object of names and values."
            )))
        }
    };
    fields
        .iter()
        .map(|(name, value)| match value {
            Node::Text(text) => Ok((name.clone(), ConfigValue::Text(text.clone()))),
            Node::Null => Ok((name.clone(), ConfigValue::Text(String::new()))),
            Node::Array(_) | Node::Object(_) => Err(Error::configuration(format!(
                "'{path}:{name}' is a JSON array or object. Give each build \
                 parameter as text, and a list as comma-separated text."
            ))),
        })
        .collect()
}

/// The value of the field called `name`, ignoring case.
fn find<'a>(fields: &'a [(String, Node)], name: &str) -> Option<&'a Node> {
    fields
        .iter()
        .find(|(key, _)| equal_ignoring_case(key, name))
        .map(|(_, value)| value)
}

/// Refuse two names in one object that differ only in case, as .NET's
/// reader refuses two values with one path.
fn check_duplicates(fields: &[(String, Node)], path: &str) -> Result<()> {
    for (index, (name, value)) in fields.iter().enumerate() {
        let here = if path.is_empty() {
            name.clone()
        } else {
            format!("{path}:{name}")
        };
        if fields[..index]
            .iter()
            .any(|(earlier, _)| equal_ignoring_case(earlier, name))
        {
            return Err(Error::configuration(format!(
                "A duplicate key '{here}' was found."
            )));
        }
        check_node_duplicates(value, &here)?;
    }
    Ok(())
}

fn check_node_duplicates(node: &Node, path: &str) -> Result<()> {
    match node {
        Node::Object(fields) => check_duplicates(fields, path),
        Node::Array(items) => items
            .iter()
            .enumerate()
            .try_for_each(|(index, item)| check_node_duplicates(item, &format!("{path}:{index}"))),
        _ => Ok(()),
    }
}

/// Order two configuration keys as .NET's `ConfigurationKeyComparer` does.
/// Each part between colons is compared in turn, whole numbers before other
/// text and in numeric order, and other text ignoring case.
pub(crate) fn compare_keys(x: &str, y: &str) -> Ordering {
    let mut xs = x.split(':').filter(|part| !part.is_empty());
    let mut ys = y.split(':').filter(|part| !part.is_empty());
    loop {
        match (xs.next(), ys.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(a), Some(b)) => {
                let order = match (parse_i32(a), parse_i32(b)) {
                    (Some(a), Some(b)) => a.cmp(&b),
                    (Some(_), None) => Ordering::Less,
                    (None, Some(_)) => Ordering::Greater,
                    (None, None) => a
                        .chars()
                        .flat_map(char::to_uppercase)
                        .cmp(b.chars().flat_map(char::to_uppercase)),
                };
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}

/// A JSON reader allowing comments and trailing commas, as .NET's
/// configuration reader does.
struct Parser<'a> {
    text: &'a str,
    position: usize,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Self {
        Parser {
            text: text.strip_prefix('\u{feff}').unwrap_or(text),
            position: 0,
        }
    }

    fn document(mut self) -> Result<Node> {
        self.skip_space()?;
        let node = self.value(0)?;
        self.skip_space()?;
        if self.position != self.text.len() {
            return Err(self.error("text after the end of the JSON"));
        }
        Ok(node)
    }

    /// The error `what`, placed at the line and column of the position, or
    /// of the character it falls inside.
    fn error(&self, what: &str) -> Error {
        let mut end = self.position.min(self.text.len());
        while !self.text.is_char_boundary(end) {
            end -= 1;
        }
        let before = &self.text[..end];
        let line = before.matches('\n').count() + 1;
        let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
        Error::configuration(format!(
            "The configuration is not valid JSON, {what} at line {line} column {column}."
        ))
    }

    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.position).copied()
    }

    fn skip_space(&mut self) -> Result<()> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\n' | b'\r') => self.position += 1,
                Some(b'/') => match self.text.as_bytes().get(self.position + 1) {
                    Some(b'/') => {
                        let rest = &self.text[self.position..];
                        self.position += rest.find('\n').unwrap_or(rest.len());
                    }
                    Some(b'*') => {
                        let rest = &self.text[self.position + 2..];
                        let end = rest
                            .find("*/")
                            .ok_or_else(|| self.error("a comment that is not closed"))?;
                        self.position += end + 4;
                    }
                    _ => return Err(self.error("an unexpected '/'")),
                },
                _ => return Ok(()),
            }
        }
    }

    fn value(&mut self, depth: usize) -> Result<Node> {
        if depth >= MAX_DEPTH {
            return Err(self.error("nesting deeper than 64"));
        }
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(Node::Text(self.string()?)),
            Some(b'-' | b'0'..=b'9') => Ok(Node::Text(self.number()?)),
            Some(b't') => self.literal("true", Node::Text("True".into())),
            Some(b'f') => self.literal("false", Node::Text("False".into())),
            Some(b'n') => self.literal("null", Node::Null),
            None => Err(self.error("the end of the text where a value was expected")),
            _ => Err(self.error("an unexpected character where a value was expected")),
        }
    }

    fn literal(&mut self, word: &str, node: Node) -> Result<Node> {
        if self.text[self.position..].starts_with(word) {
            self.position += word.len();
            Ok(node)
        } else {
            Err(self.error("an unexpected word"))
        }
    }

    fn object(&mut self, depth: usize) -> Result<Node> {
        self.position += 1;
        let mut fields = Vec::new();
        loop {
            self.skip_space()?;
            match self.peek() {
                Some(b'}') => {
                    self.position += 1;
                    return Ok(Node::Object(fields));
                }
                Some(b'"') => {}
                _ => return Err(self.error("a missing name in an object")),
            }
            let name = self.string()?;
            self.skip_space()?;
            if self.peek() != Some(b':') {
                return Err(self.error("a missing ':' after a name"));
            }
            self.position += 1;
            self.skip_space()?;
            let value = self.value(depth + 1)?;
            fields.push((name, value));
            self.skip_space()?;
            match self.peek() {
                Some(b',') => self.position += 1,
                Some(b'}') => {}
                _ => return Err(self.error("a missing ',' or '}' in an object")),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Node> {
        self.position += 1;
        let mut items = Vec::new();
        loop {
            self.skip_space()?;
            if self.peek() == Some(b']') {
                self.position += 1;
                return Ok(Node::Array(items));
            }
            items.push(self.value(depth + 1)?);
            self.skip_space()?;
            match self.peek() {
                Some(b',') => self.position += 1,
                Some(b']') => {}
                _ => return Err(self.error("a missing ',' or ']' in an array")),
            }
        }
    }

    fn number(&mut self) -> Result<String> {
        let bytes = self.text.as_bytes();
        let start = self.position;
        let mut at = start;
        let digits = |at: &mut usize| {
            let from = *at;
            while bytes.get(*at).is_some_and(u8::is_ascii_digit) {
                *at += 1;
            }
            *at - from
        };
        if bytes.get(at) == Some(&b'-') {
            at += 1;
        }
        let whole = at;
        if digits(&mut at) == 0 || (bytes[whole] == b'0' && at - whole > 1) {
            self.position = at;
            return Err(self.error("a number that is not valid"));
        }
        if bytes.get(at) == Some(&b'.') {
            at += 1;
            if digits(&mut at) == 0 {
                self.position = at;
                return Err(self.error("a number that is not valid"));
            }
        }
        if let Some(b'e' | b'E') = bytes.get(at) {
            at += 1;
            if let Some(b'+' | b'-') = bytes.get(at) {
                at += 1;
            }
            if digits(&mut at) == 0 {
                self.position = at;
                return Err(self.error("a number that is not valid"));
            }
        }
        self.position = at;
        Ok(self.text[start..at].to_string())
    }

    fn string(&mut self) -> Result<String> {
        self.position += 1;
        let mut value = String::new();
        loop {
            let rest = &self.text[self.position..];
            let mut chars = rest.char_indices();
            let Some((_, c)) = chars.next() else {
                return Err(self.error("a string that is not closed"));
            };
            match c {
                '"' => {
                    self.position += 1;
                    return Ok(value);
                }
                '\\' => {
                    self.position += 1;
                    value.push(self.escape()?);
                }
                c if (c as u32) < 0x20 => {
                    return Err(self.error("a control character in a string"));
                }
                c => {
                    value.push(c);
                    self.position += c.len_utf8();
                }
            }
        }
    }

    /// The character the escape after a backslash gives. A bad escape is
    /// reported at the escaped character, which may take more than one byte,
    /// so the position moves past it only when it is good.
    fn escape(&mut self) -> Result<char> {
        let c = self
            .peek()
            .ok_or_else(|| self.error("a string that is not closed"))?;
        let escaped = match c {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => {
                self.position += 1;
                return self.unicode_escape();
            }
            _ => return Err(self.error("a bad escape in a string")),
        };
        self.position += 1;
        Ok(escaped)
    }

    /// The character a `\u` escape gives, the position being after the `u`.
    fn unicode_escape(&mut self) -> Result<char> {
        let high = self.hex4()?;
        if !(0xD800..0xDC00).contains(&high) {
            return char::from_u32(high).ok_or_else(|| self.error("a lone surrogate in a string"));
        }
        if !self.text[self.position..].starts_with("\\u") {
            return Err(self.error("a lone surrogate in a string"));
        }
        self.position += 2;
        let low = self.hex4()?;
        if !(0xDC00..0xE000).contains(&low) {
            return Err(self.error("a lone surrogate in a string"));
        }
        let code = 0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00);
        char::from_u32(code).ok_or_else(|| self.error("a bad escape"))
    }

    fn hex4(&mut self) -> Result<u32> {
        let digits = self
            .text
            .get(self.position..self.position + 4)
            .filter(|d| d.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| self.error("a bad \\u escape in a string"))?;
        self.position += 4;
        Ok(u32::from_str_radix(digits, 16).expect("four hexadecimal digits"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Node> {
        Parser::new(text).document()
    }

    fn message(error: Error) -> String {
        match error {
            Error::PipelineConfiguration { message } => message,
            other => panic!("not a configuration error: {other}"),
        }
    }

    #[test]
    fn values_are_kept_as_the_text_dotnet_gives() {
        let node = parse(r#"{"a": 1.50, "b": -2e3, "c": true, "d": false, "e": null, "f": "x\"\u00e9\ud83d\ude00"}"#)
            .unwrap();
        assert_eq!(
            node,
            Node::Object(vec![
                ("a".into(), Node::Text("1.50".into())),
                ("b".into(), Node::Text("-2e3".into())),
                ("c".into(), Node::Text("True".into())),
                ("d".into(), Node::Text("False".into())),
                ("e".into(), Node::Null),
                ("f".into(), Node::Text("x\"\u{e9}\u{1f600}".into())),
            ])
        );
    }

    #[test]
    fn comments_and_trailing_commas_are_allowed() {
        let node = parse(
            "\u{feff}// first\n{ /* a */ \"a\": [1, 2,], /* b */ \"b\": {\"c\": \"d\",}, }\n/* end */",
        )
        .unwrap();
        assert_eq!(
            node,
            Node::Object(vec![
                (
                    "a".into(),
                    Node::Array(vec![Node::Text("1".into()), Node::Text("2".into())])
                ),
                (
                    "b".into(),
                    Node::Object(vec![("c".into(), Node::Text("d".into()))])
                ),
            ])
        );
    }

    #[test]
    fn json_that_is_not_valid_is_refused_with_its_place() {
        for (text, what) in [
            (
                "{\"a\": 01}",
                "a number that is not valid at line 1 column 9",
            ),
            ("{\"a\": 1.}", "a number that is not valid"),
            ("{\"a\": .5}", "an unexpected character"),
            ("{\"a\":\n tru}", "an unexpected word at line 2 column 2"),
            ("{\"a\": \"b}", "a string that is not closed"),
            ("{\"a\": \"\t\"}", "a control character"),
            (
                "{\"a\": \"\\x\"}",
                "a bad escape in a string at line 1 column 9",
            ),
            (
                "{\"a\": \"\\é\"}",
                "a bad escape in a string at line 1 column 9",
            ),
            (
                "{\"a\": \"\\😀\"}",
                "a bad escape in a string at line 1 column 9",
            ),
            (
                "{\"a\":\n\"C:\\Ärzte\"}",
                "a bad escape in a string at line 2 column 5",
            ),
            ("{\"a\": \"\\", "a string that is not closed"),
            ("{\"a\": \"\\u12\"}", "a bad \\u escape"),
            ("{\"a\": \"\\uéééé\"}", "a bad \\u escape"),
            ("{\"a\": \"\\ud800\\é\"}", "a lone surrogate"),
            ("{\"a\": \"\\ud800\"}", "a lone surrogate"),
            ("{\"a\" 1}", "a missing ':'"),
            ("{\"a\": 1 \"b\": 2}", "a missing ',' or '}'"),
            ("[1 2]", "a missing ',' or ']'"),
            ("{a: 1}", "a missing name"),
            ("{} {}", "text after the end"),
            ("{/* open", "a comment that is not closed"),
            ("", "the end of the text"),
            ("{,}", "a missing name"),
        ] {
            let found = message(parse(text).unwrap_err());
            assert!(found.contains(what), "{text:?} gave {found:?}");
        }
        let deep = format!("{}{}", "[".repeat(65), "]".repeat(65));
        assert!(message(parse(&deep).unwrap_err()).contains("nesting deeper than 64"));
        let allowed = format!("{}{}", "[".repeat(64), "]".repeat(64));
        assert!(parse(&allowed).is_ok());
    }

    #[test]
    fn keys_are_ordered_as_dotnet_orders_them() {
        let mut keys = vec![
            "SetProperty",
            "b",
            "10",
            "SetProperties",
            "A",
            "2",
            "a:2",
            "a:10",
            "-1",
        ];
        keys.sort_by(|a, b| compare_keys(a, b));
        assert_eq!(
            keys,
            vec![
                "-1",
                "2",
                "10",
                "A",
                "a:2",
                "a:10",
                "b",
                "SetProperties",
                "SetProperty"
            ]
        );
        assert_eq!(compare_keys("abc", "ABC"), Ordering::Equal);
        assert_eq!(compare_keys(":a", "a"), Ordering::Equal);
    }
}
