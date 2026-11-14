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

//! A small Mustache renderer covering the tags the bundled
//! `JavaScriptResource.mustache` template uses.
//!
//! It uses no `unsafe` code and no third-party engine.
//!
//! # Supported tags
//!
//! - `{{name}}`, `{{&name}}` and `{{{name}}}` variables, all written without
//!   HTML escaping.
//! - `{{#name}} ... {{/name}}` sections, rendered when the flag is true.
//! - `{{^name}} ... {{/name}}` inverted sections, rendered when the flag is
//!   false.
//!
//! Lists, partials, lambdas, dotted names and comments are not supported.
//!
//! # Output
//!
//! The output is the one Stubble gives, Stubble being the renderer .NET's
//! JavaScript builder uses, and follows the
//! [Mustache specification](https://github.com/mustache/spec/blob/master/specs/sections.yml).
//!
//! - A line holding only section tags and whitespace is removed, its line
//!   break included. A line break is a line feed, or a carriage return
//!   followed by a line feed. Whitespace is a space, tab, line feed, line
//!   tabulation, form feed or carriage return.
//! - Every other line is written as it is, so a section opened and closed
//!   inside a line of text leaves the text around it.
//! - The text after the last tag is written as it is, with nothing trimmed.

use std::fmt::Display;

/// The values a render [`Context`] can supply for a field.
///
/// Variable tags read a [`Value`]; section tags read [`Context::flag`].
pub enum Value<'a> {
    /// A borrowed string value, emitted verbatim.
    Str(&'a str),
    /// An integer value, emitted as its decimal form (used for `_sequence`).
    Int(i64),
}

impl Value<'_> {
    fn write_to(&self, out: &mut String) {
        match self {
            Value::Str(s) => out.push_str(s),
            Value::Int(n) => {
                use std::fmt::Write;
                // Writing an integer into a String cannot fail.
                let _ = write!(out, "{n}");
            }
        }
    }
}

impl<'a> From<&'a str> for Value<'a> {
    fn from(s: &'a str) -> Self {
        Value::Str(s)
    }
}

/// The data a template is rendered against.
///
/// A variable tag `{{name}}` calls [`Context::value`]; a section tag
/// `{{#name}}` or `{{^name}}` calls [`Context::flag`]. Returning `None` from
/// either means the name is unknown, which Mustache treats as false, so a
/// variable renders as empty, a section is skipped and an inverted section is
/// rendered.
pub trait Context {
    /// The value for a variable tag, or `None` if the name is unknown.
    fn value(&self, name: &str) -> Option<Value<'_>>;

    /// The boolean a section tag refers to, or `None` if the name is not a
    /// known boolean field.
    fn flag(&self, name: &str) -> Option<bool>;
}

/// A parsed node of the template.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    /// Literal text emitted verbatim.
    Text(String),
    /// A `{{name}}` / `{{&name}}` / `{{{name}}}` variable.
    Var(String),
    /// A `{{#name}} ... {{/name}}` section and its body.
    Section { name: String, body: Vec<Node> },
    /// A `{{^name}} ... {{/name}}` inverted section and its body.
    Inverted { name: String, body: Vec<Node> },
}

/// A parse error in the template source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// A `{{` was opened but never closed with `}}`.
    UnclosedTag,
    /// A `{{/name}}` closed a section that was not open, or closed the wrong
    /// section.
    MismatchedClose {
        /// The section the close tag named.
        found: String,
    },
    /// A section was opened with `{{#name}}` or `{{^name}}` but never closed.
    UnclosedSection {
        /// The section that was left open.
        name: String,
    },
}

impl Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::UnclosedTag => write!(f, "unclosed tag: a {{{{ has no matching }}}}"),
            ParseError::MismatchedClose { found } => {
                write!(f, "mismatched closing tag {{{{/{found}}}}}")
            }
            ParseError::UnclosedSection { name } => {
                write!(f, "unclosed section {{{{#{name}}}}} (or inverted)")
            }
        }
    }
}

impl std::error::Error for ParseError {}

/// A parsed Mustache template, ready to render.
#[derive(Debug, Clone)]
pub struct Template {
    nodes: Vec<Node>,
}

impl Template {
    /// Parse the template source.
    ///
    /// Returns a [`ParseError`] if a tag is unclosed or a section close does not
    /// match the open. The bundled template is valid, so this does not fail in
    /// practice.
    pub fn parse(source: &str) -> Result<Self, ParseError> {
        let tokens = remove_standalone_lines(lex(source)?);
        let mut iter = tokens.into_iter().peekable();
        let nodes = parse_nodes(&mut iter, None)?;
        Ok(Template { nodes })
    }

    /// Render this template against `context`.
    pub fn render(&self, context: &dyn Context) -> String {
        // The capacity only saves reallocation and does not change the output.
        let mut out = String::with_capacity(self.estimated_capacity());
        render_nodes(&self.nodes, context, &mut out);
        out
    }

    /// A rough output-size estimate used only to pre-size the buffer.
    fn estimated_capacity(&self) -> usize {
        fn size(nodes: &[Node]) -> usize {
            nodes
                .iter()
                .map(|n| match n {
                    Node::Text(t) => t.len(),
                    Node::Var(_) => 16,
                    Node::Section { body, .. } | Node::Inverted { body, .. } => size(body),
                })
                .sum()
        }
        size(&self.nodes) + 256
    }
}

/// A lexer token: either literal text or a tag.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Text(String),
    /// A variable tag `{{name}}`, `{{&name}}` or `{{{name}}}`.
    Var(String),
    /// A section open `{{#name}}`.
    SectionOpen(String),
    /// An inverted-section open `{{^name}}`.
    InvertedOpen(String),
    /// A section close `{{/name}}`.
    Close(String),
}

/// Split the source into literal-text and tag tokens.
///
/// Triple-brace `{{{name}}}` is recognized before double-brace so the third
/// brace is consumed. The name in a tag is trimmed of surrounding whitespace.
fn lex(source: &str) -> Result<Vec<Token>, ParseError> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut text_start = 0;
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'{' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            // Flush any literal text accumulated before this tag.
            if i > text_start {
                tokens.push(Token::Text(source[text_start..i].to_owned()));
            }

            // Detect a triple brace so the closing third brace is consumed.
            let triple = i + 2 < bytes.len() && bytes[i + 2] == b'{';
            let open_len = if triple { 3 } else { 2 };
            let close_pat: &[u8] = if triple { b"}}}" } else { b"}}" };

            let body_start = i + open_len;
            let close_rel =
                find_subslice(&bytes[body_start..], close_pat).ok_or(ParseError::UnclosedTag)?;
            let body = &source[body_start..body_start + close_rel];
            let tag = classify_tag(body, triple);
            tokens.push(tag);

            i = body_start + close_rel + close_pat.len();
            text_start = i;
        } else {
            i += 1;
        }
    }

    if text_start < bytes.len() {
        tokens.push(Token::Text(source[text_start..].to_owned()));
    }

    Ok(tokens)
}

/// Turn a tag body (the text between the braces) into a token.
///
/// The first non-space character is the sigil: `#` section, `^` inverted, `/`
/// close, `&` unescaped variable. A triple-brace tag is always an unescaped
/// variable. Anything else is a plain variable. The remaining name is trimmed.
fn classify_tag(body: &str, triple: bool) -> Token {
    if triple {
        return Token::Var(body.trim().to_owned());
    }
    let trimmed = body.trim_start();
    let mut chars = trimmed.chars();
    match chars.next() {
        Some('#') => Token::SectionOpen(chars.as_str().trim().to_owned()),
        Some('^') => Token::InvertedOpen(chars.as_str().trim().to_owned()),
        Some('/') => Token::Close(chars.as_str().trim().to_owned()),
        Some('&') => Token::Var(chars.as_str().trim().to_owned()),
        _ => Token::Var(trimmed.trim().to_owned()),
    }
}

/// Whether a character is whitespace as Stubble counts it, being a space,
/// tab, line feed, line tabulation, form feed or carriage return.
fn is_mustache_whitespace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r')
}

/// Remove the whitespace, line break included, from every line that holds
/// section tags and nothing else but whitespace.
///
/// A line ends after a line feed, so a carriage return before it is part of
/// the line break and a carriage return on its own is whitespace within the
/// line. A line holding a variable or any other text is kept as it is.
fn remove_standalone_lines(tokens: Vec<Token>) -> Vec<Token> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut line = Vec::new();
    for token in tokens {
        let text = match token {
            Token::Text(text) => text,
            tag => {
                line.push(tag);
                continue;
            }
        };
        let mut rest = text.as_str();
        while let Some(at) = rest.find('\n') {
            line.push(Token::Text(rest[..=at].to_owned()));
            end_line(&mut line, &mut out);
            rest = &rest[at + 1..];
        }
        if !rest.is_empty() {
            line.push(Token::Text(rest.to_owned()));
        }
    }
    end_line(&mut line, &mut out);
    out
}

/// Move one line's tokens to `out`, leaving out its whitespace when the line
/// holds only section tags and whitespace. Neighbouring text is joined so the
/// template holds no more text nodes than it needs.
fn end_line(line: &mut Vec<Token>, out: &mut Vec<Token>) {
    let has_tag = line.iter().any(|token| !matches!(token, Token::Text(_)));
    let standalone = has_tag
        && line.iter().all(|token| match token {
            Token::Text(text) => text.chars().all(is_mustache_whitespace),
            Token::Var(_) => false,
            Token::SectionOpen(_) | Token::InvertedOpen(_) | Token::Close(_) => true,
        });
    for token in line.drain(..) {
        match token {
            Token::Text(_) if standalone => {}
            Token::Text(text) => match out.last_mut() {
                Some(Token::Text(previous)) => previous.push_str(&text),
                _ => out.push(Token::Text(text)),
            },
            tag => out.push(tag),
        }
    }
}

/// Find the first occurrence of `needle` within `haystack`, returning its start
/// offset.
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Build the node tree from the token stream.
///
/// `open` is `Some(name)` while inside a section, so the matching close can be
/// validated. The function returns when it consumes the close for `open`, or at
/// end of input for the top level.
fn parse_nodes(
    tokens: &mut std::iter::Peekable<std::vec::IntoIter<Token>>,
    open: Option<&str>,
) -> Result<Vec<Node>, ParseError> {
    let mut nodes = Vec::new();

    while let Some(token) = tokens.next() {
        match token {
            Token::Text(text) => nodes.push(Node::Text(text)),
            Token::Var(name) => nodes.push(Node::Var(name)),
            Token::SectionOpen(name) => {
                let body = parse_nodes(tokens, Some(&name))?;
                nodes.push(Node::Section { name, body });
            }
            Token::InvertedOpen(name) => {
                let body = parse_nodes(tokens, Some(&name))?;
                nodes.push(Node::Inverted { name, body });
            }
            Token::Close(name) => {
                return match open {
                    Some(expected) if expected == name => Ok(nodes),
                    _ => Err(ParseError::MismatchedClose { found: name }),
                };
            }
        }
    }

    // End of input: every open section must have been closed.
    match open {
        Some(name) => Err(ParseError::UnclosedSection {
            name: name.to_owned(),
        }),
        None => Ok(nodes),
    }
}

/// Render a node list against the context into `out`.
fn render_nodes(nodes: &[Node], context: &dyn Context, out: &mut String) {
    for node in nodes {
        match node {
            Node::Text(text) => out.push_str(text),
            Node::Var(name) => {
                if let Some(value) = context.value(name) {
                    value.write_to(out);
                }
                // An unknown variable renders as empty.
            }
            Node::Section { name, body } => {
                // A boolean section renders its body only when the flag is true.
                // An unknown flag skips the body.
                if context.flag(name) == Some(true) {
                    render_nodes(body, context, out);
                }
            }
            Node::Inverted { name, body } => {
                // An inverted section renders its body when the flag is false
                // or unknown.
                if context.flag(name) != Some(true) {
                    render_nodes(body, context, out);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestCtx;
    impl Context for TestCtx {
        fn value(&self, name: &str) -> Option<Value<'_>> {
            match name {
                "name" => Some(Value::Str("world")),
                "num" => Some(Value::Int(42)),
                _ => None,
            }
        }
        fn flag(&self, name: &str) -> Option<bool> {
            match name {
                "on" => Some(true),
                "off" => Some(false),
                _ => None,
            }
        }
    }

    fn render(src: &str) -> String {
        Template::parse(src).unwrap().render(&TestCtx)
    }

    #[test]
    fn plain_variable() {
        assert_eq!(render("hello {{name}}!"), "hello world!");
    }

    #[test]
    fn unescaped_variants_match_plain() {
        // No HTML escaping anywhere, so all three forms emit the raw value.
        assert_eq!(render("{{name}}|{{&name}}|{{{name}}}"), "world|world|world");
    }

    #[test]
    fn integer_value_is_bare() {
        assert_eq!(render("n={{num}};"), "n=42;");
    }

    #[test]
    fn unknown_variable_is_empty() {
        assert_eq!(render("[{{missing}}]"), "[]");
    }

    #[test]
    fn section_renders_when_true() {
        assert_eq!(render("{{#on}}yes{{/on}}"), "yes");
        assert_eq!(render("{{#off}}yes{{/off}}"), "");
    }

    #[test]
    fn inverted_renders_when_false() {
        assert_eq!(render("{{^off}}no{{/off}}"), "no");
        assert_eq!(render("{{^on}}no{{/on}}"), "");
    }

    #[test]
    fn nested_sections() {
        let src = "{{#on}}A{{^off}}B{{/off}}C{{/on}}";
        assert_eq!(render(src), "ABC");
    }

    #[test]
    fn literal_braces_without_pair_are_text() {
        // A single brace is literal text, not a tag.
        assert_eq!(render("a { b } c"), "a { b } c");
    }

    #[test]
    fn the_text_after_the_last_tag_is_kept() {
        assert_eq!(render("x{{name}}   \r\n\r\n"), "xworld   \r\n\r\n");
    }

    #[test]
    fn a_line_holding_only_section_tags_is_removed() {
        let src = "a\n    {{#on}}\n    body\n    {{/on}}\n    z";
        assert_eq!(render(src), "a\n    body\n    z");
        let src = "a\n{{#off}}\nb\n{{/off}}\nc\n{{^off}}\nd\n{{/off}}\n";
        assert_eq!(render(src), "a\nc\nd\n");
    }

    #[test]
    fn a_carriage_return_before_a_line_feed_goes_with_the_line() {
        let src = "a\r\n  {{#on}}\r\n  b\r\n  {{/on}}\r\nc";
        assert_eq!(render(src), "a\r\n  b\r\nc");
    }

    #[test]
    fn a_carriage_return_alone_is_whitespace_within_the_line() {
        // The first line is " \r{{#on}}\r\n", which holds only a tag and
        // whitespace, so all of it goes.
        assert_eq!(render(" \r{{#on}}\r\nb{{/on}}"), "b");
        // With text on the same line, nothing on it is removed.
        assert_eq!(render("a\r{{#on}}\nb{{/on}}"), "a\r\nb");
    }

    #[test]
    fn line_tabulation_and_form_feed_are_whitespace() {
        assert_eq!(render("\u{0B}\u{0C}{{#on}}\nb{{/on}}"), "b");
    }

    #[test]
    fn a_line_holding_two_section_tags_is_removed() {
        let src = "{{#on}}{{^off}}\nb\n{{/off}}{{/on}}\n";
        assert_eq!(render(src), "b\n");
    }

    #[test]
    fn a_line_holding_a_variable_is_kept() {
        assert_eq!(render("  {{name}}\n{{#on}}\n{{/on}}\n"), "  world\n");
        assert_eq!(render("{{#on}}{{name}}{{/on}}\n"), "world\n");
    }

    #[test]
    fn a_last_line_with_no_line_break_is_removed_when_standalone() {
        assert_eq!(render("{{#on}}\nb\n  {{/on}}"), "b\n");
    }

    #[test]
    fn an_unknown_flag_skips_a_section_and_renders_an_inverted_one() {
        assert_eq!(render("{{#missing}}yes{{/missing}}"), "");
        assert_eq!(render("{{^missing}}no{{/missing}}"), "no");
    }

    /// The standalone cases in the Mustache specification's sections and
    /// inverted files, with the flag the specification calls `boolean` named
    /// `on` here, and an inverted section on `off`.
    #[test]
    fn the_specification_standalone_cases() {
        let cases = [
            (
                "| This Is\n{{#on}}\n|\n{{/on}}\n| A Line\n",
                "| This Is\n|\n| A Line\n",
            ),
            (
                "| This Is\n  {{#on}}\n|\n  {{/on}}\n| A Line\n",
                "| This Is\n|\n| A Line\n",
            ),
            ("|\r\n{{#on}}\r\n{{/on}}\r\n|", "|\r\n|"),
            ("  {{#on}}\n#{{/on}}\n/", "#\n/"),
            ("#{{#on}}\n/\n  {{/on}}", "#\n/\n"),
            (" {{#on}}YES{{/on}}\n {{#on}}GOOD{{/on}}\n", " YES\n GOOD\n"),
            (
                "| This Is\n{{^off}}\n|\n{{/off}}\n| A Line\n",
                "| This Is\n|\n| A Line\n",
            ),
            ("|\r\n{{^off}}\r\n{{/off}}\r\n|", "|\r\n|"),
            ("  {{^off}}\n^{{/off}}\n/", "^\n/"),
            ("^{{^off}}\n/\n  {{/off}}", "^\n/\n"),
            (" {{^off}}NO{{/off}}\n {{^off}}WAY{{/off}}\n", " NO\n WAY\n"),
            (
                "| This Is\n  {{^off}}\n|\n  {{/off}}\n| A Line\n",
                "| This Is\n|\n| A Line\n",
            ),
        ];
        for (template, expected) in cases {
            assert_eq!(render(template), expected, "{template:?}");
        }
    }

    /// Every case in the Mustache specification's comments file, where a
    /// comment writes nothing and a line holding only comments and
    /// whitespace is removed with its line break.
    #[test]
    fn the_specification_comment_cases() {
        let cases = [
            ("12345{{! Comment Block! }}67890", "1234567890"),
            (
                "12345{{!\n  This is a\n  multi-line comment...\n}}67890\n",
                "1234567890\n",
            ),
            ("Begin.\n{{! Comment Block! }}\nEnd.\n", "Begin.\nEnd.\n"),
            (
                "Begin.\n  {{! Indented Comment Block! }}\nEnd.\n",
                "Begin.\nEnd.\n",
            ),
            ("|\r\n{{! Standalone Comment }}\r\n|", "|\r\n|"),
            ("  {{! I'm Still Standalone }}\n!", "!"),
            ("!\n  {{! I'm Still Standalone }}", "!\n"),
            (
                "Begin.\n{{!\nSomething's going on here...\n}}\nEnd.\n",
                "Begin.\nEnd.\n",
            ),
            (
                "Begin.\n  {{!\n    Something's going on here...\n  }}\nEnd.\n",
                "Begin.\nEnd.\n",
            ),
            ("  12 {{! 34 }}\n", "  12 \n"),
            ("12345 {{! Comment Block! }} 67890", "12345  67890"),
            (
                "comments never show: >{{! comment }}<",
                "comments never show: ><",
            ),
        ];
        for (template, expected) in cases {
            assert_eq!(render(template), expected, "{template:?}");
        }
    }

    /// A partial, a set delimiter tag and the inheritance tags are refused,
    /// each named as written.
    #[test]
    fn tags_the_renderer_cannot_follow_are_refused() {
        for (source, tag) in [
            ("a{{> header}}b", "{{> header}}"),
            ("{{=<% %>=}}", "{{=<% %>=}}"),
            ("{{<parent}}{{/parent}}", "{{<parent}}"),
            ("{{$block}}x{{/block}}", "{{$block}}"),
        ] {
            match Template::parse(source) {
                Err(error) => {
                    assert!(error.to_string().starts_with(tag), "{source:?}: {error}")
                }
                Ok(_) => panic!("{source:?} was accepted"),
            }
        }
    }

    #[test]
    fn unclosed_tag_errors() {
        assert_eq!(
            Template::parse("a {{name").unwrap_err(),
            ParseError::UnclosedTag
        );
    }

    #[test]
    fn unclosed_section_errors() {
        assert!(matches!(
            Template::parse("{{#on}}body").unwrap_err(),
            ParseError::UnclosedSection { .. }
        ));
    }

    #[test]
    fn mismatched_close_errors() {
        assert!(matches!(
            Template::parse("{{#on}}body{{/off}}").unwrap_err(),
            ParseError::MismatchedClose { .. }
        ));
    }
}
