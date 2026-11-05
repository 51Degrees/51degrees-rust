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

//! Compares the lines the .NET and Rust producers write for the same corpus.
//!
//! Each difference names the case, a path into the line and a kind. A
//! difference is allowed only when an entry in the allowed differences file
//! matches it, and every entry gives its reason. `parity/README.md` describes
//! the line format, the paths and the kinds.

use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;
use serde_json::{Map, Value};

/// Stands in for a session id the Sequence element generated, which is
/// random, so that the rest of the line can still be compared.
pub const GENERATED_SESSION_ID: &str = "<generated session id>";

/// The evidence key that, when a case supplies it, fixes the session id.
const SESSION_ID_KEY: &str = "query.session-id";

/// The length of a session id in .NET's form, a GUID in its "D" format.
const GUID_LENGTH: usize = 36;

/// The offsets of the hyphens in a GUID in its "D" format.
const GUID_HYPHENS: [usize; 4] = [8, 13, 18, 23];

/// Text longer than this is cut in a report.
const REPORT_TEXT_LIMIT: usize = 300;

/// One corpus case.
#[derive(Debug, Clone, Deserialize)]
pub struct Case {
    pub id: String,
    #[serde(default)]
    pub about: String,
    pub evidence: Map<String, Value>,
}

/// One difference between the two lines for a case.
#[derive(Debug, Clone, PartialEq)]
pub struct Difference {
    pub case: String,
    pub path: String,
    pub kind: Kind,
    pub dotnet: Value,
    pub rust: Value,
}

/// What differs at a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// The producer wrote no line for the case.
    MissingLine,
    /// Only .NET has this element, property or key.
    OnlyInDotnet,
    /// Only Rust has this element, property or key.
    OnlyInRust,
    /// A property name differs in case.
    Name,
    /// One side has a value and the other has none, or a property is missing.
    State,
    /// The value types differ.
    Type,
    /// The values differ.
    Value,
    /// The reasons given for no value differ.
    NoValueMessage,
    /// The same keys in a different order.
    Order,
    /// The same structure, written differently.
    Text,
    /// One side wrote null and the other did not.
    Presence,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::MissingLine => "missingLine",
            Kind::OnlyInDotnet => "onlyInDotnet",
            Kind::OnlyInRust => "onlyInRust",
            Kind::Name => "name",
            Kind::State => "state",
            Kind::Type => "type",
            Kind::Value => "value",
            Kind::NoValueMessage => "noValueMessage",
            Kind::Order => "order",
            Kind::Text => "text",
            Kind::Presence => "presence",
        }
    }
}

/// An entry in the allowed differences file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Allowed {
    /// A pattern for the path, where `*` matches any run of characters.
    pub path: String,
    /// The kind, or any kind when absent.
    #[serde(default)]
    pub kind: Option<String>,
    /// Patterns for the case ids, or every case when absent.
    #[serde(default)]
    pub cases: Option<Vec<String>>,
    /// The data files the entry applies to, or all when absent.
    #[serde(default)]
    pub data: Option<String>,
    /// Why the difference is allowed. Required.
    pub reason: String,
    /// The issue that removes the difference, when there is one.
    #[serde(default)]
    pub issue: Option<u32>,
    /// What .NET gave, or anything when absent.
    #[serde(default)]
    pub dotnet: Option<Side>,
    /// What the crates gave, or anything when absent.
    #[serde(default)]
    pub rust: Option<Side>,
}

/// What one side of a difference gave, so that an entry allows only the
/// change its reason describes. For a property it is the property's value,
/// and for anything else the value at the path.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Side {
    /// The side gave one of these values, compared as data.
    #[serde(default)]
    pub value: Option<Vec<Value>>,
    /// The side gave no value for the property.
    #[serde(default, rename = "noValue")]
    pub no_value: bool,
}

impl Side {
    /// Whether `given`, one side of a difference at `path`, is what this
    /// names.
    fn matches(&self, path: &str, given: &Value) -> bool {
        let (value, no_value) = match property_of(path, given) {
            Some(property) => (property.get("value"), property.contains_key("noValue")),
            None => (Some(given), false),
        };
        (self.no_value && no_value)
            || self.value.as_ref().is_some_and(|values| {
                value.is_some_and(|value| values.iter().any(|v| same(v, value)))
            })
    }
}

/// `given` as a property, when the difference is at a property's path and
/// `given` is the whole property rather than its value.
fn property_of<'a>(path: &str, given: &'a Value) -> Option<&'a Map<String, Value>> {
    let property = given.as_object()?;
    (path.starts_with("properties.") && property.contains_key("name")).then_some(property)
}

impl Allowed {
    /// Whether this entry applies to the run over the named data files.
    pub fn applies_to(&self, data: &str) -> bool {
        self.data.as_deref().is_none_or(|d| d == data)
    }

    pub fn matches(&self, difference: &Difference) -> bool {
        glob(&self.path, &difference.path)
            && self
                .kind
                .as_deref()
                .is_none_or(|kind| kind == difference.kind.name())
            && self
                .cases
                .as_ref()
                .is_none_or(|cases| cases.iter().any(|c| glob(c, &difference.case)))
            && self
                .dotnet
                .as_ref()
                .is_none_or(|side| side.matches(&difference.path, &difference.dotnet))
            && self
                .rust
                .as_ref()
                .is_none_or(|side| side.matches(&difference.path, &difference.rust))
    }

    /// What is wrong with the entry, if anything. A side that names neither
    /// a value nor no value would match nothing.
    pub fn problem(&self) -> Option<String> {
        [("dotnet", &self.dotnet), ("rust", &self.rust)]
            .into_iter()
            .find(|(_, side)| {
                side.as_ref()
                    .is_some_and(|side| side.value.is_none() && !side.no_value)
            })
            .map(|(name, _)| {
                format!(
                    "the entry for `{}` gives `{name}` neither a value nor noValue",
                    self.path
                )
            })
    }
}

/// Matches `text` against `pattern`, where `*` matches any run of characters
/// and every other character matches itself.
pub fn glob(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        if p < pattern.len() && pattern[p] == '*' {
            star = Some((p, t));
            p += 1;
        } else if p < pattern.len() && pattern[p] == text[t] {
            p += 1;
            t += 1;
        } else if let Some((star_p, star_t)) = star {
            p = star_p + 1;
            t = star_t + 1;
            star = Some((star_p, star_t + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|c| *c == '*')
}

/// Compares every case's lines. A case either side has no line for is one
/// difference of kind [`Kind::MissingLine`].
pub fn compare_all(
    cases: &[Case],
    dotnet: &HashMap<String, Value>,
    rust: &HashMap<String, Value>,
) -> Vec<Difference> {
    let mut differences = Vec::new();
    for case in cases {
        match (dotnet.get(&case.id), rust.get(&case.id)) {
            (Some(d), Some(r)) => differences.extend(compare_case(case, d, r)),
            (d, r) => differences.push(Difference {
                case: case.id.clone(),
                path: String::new(),
                kind: Kind::MissingLine,
                dotnet: Value::Bool(d.is_some()),
                rust: Value::Bool(r.is_some()),
            }),
        }
    }
    differences
}

/// Compares the two lines written for one case.
pub fn compare_case(case: &Case, dotnet: &Value, rust: &Value) -> Vec<Difference> {
    let (dotnet, rust) = if case.evidence.contains_key(SESSION_ID_KEY) {
        (dotnet.clone(), rust.clone())
    } else {
        (
            replace_generated_session_id(dotnet),
            replace_generated_session_id(rust),
        )
    };
    let mut out = Out {
        case: &case.id,
        differences: Vec::new(),
    };
    for field in ["processError", "session", "evidenceAfter", "errors"] {
        out.values(field, field_of(&dotnet, field), field_of(&rust, field));
    }
    out.properties(
        field_of(&dotnet, "properties"),
        field_of(&rust, "properties"),
    );
    out.json_text("json", field_of(&dotnet, "json"), field_of(&rust, "json"));
    for field in ["javascript", "usage"] {
        out.text(field, field_of(&dotnet, field), field_of(&rust, field));
    }
    out.differences
}

fn field_of<'a>(line: &'a Value, field: &str) -> &'a Value {
    line.get(field).unwrap_or(&Value::Null)
}

/// Replaces the session id the line says was used, when it has .NET's form,
/// with [`GENERATED_SESSION_ID`] wherever it appears in the line. Any other
/// form is left, so it shows as a difference.
pub fn replace_generated_session_id(line: &Value) -> Value {
    let Some(id) = line
        .get("session")
        .and_then(|s| s.get("id"))
        .and_then(Value::as_str)
    else {
        return line.clone();
    };
    if !is_guid(id) {
        return line.clone();
    }
    let id = id.to_owned();
    replace_text(line, &id)
}

fn replace_text(value: &Value, id: &str) -> Value {
    match value {
        Value::String(s) => Value::String(s.replace(id, GENERATED_SESSION_ID)),
        Value::Array(items) => Value::Array(items.iter().map(|v| replace_text(v, id)).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), replace_text(v, id)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Whether `text` is a GUID in its "D" format with lowercase digits.
pub fn is_guid(text: &str) -> bool {
    text.len() == GUID_LENGTH
        && text.char_indices().all(|(i, c)| {
            if GUID_HYPHENS.contains(&i) {
                c == '-'
            } else {
                c.is_ascii_digit() || ('a'..='f').contains(&c)
            }
        })
}

struct Out<'a> {
    case: &'a str,
    differences: Vec<Difference>,
}

impl Out<'_> {
    fn push(&mut self, path: &str, kind: Kind, dotnet: &Value, rust: &Value) {
        self.differences.push(Difference {
            case: self.case.to_owned(),
            path: path.to_owned(),
            kind,
            dotnet: dotnet.clone(),
            rust: rust.clone(),
        });
    }

    /// Compares two values as data, so that numbers compare by value and
    /// object keys in any order.
    fn values(&mut self, path: &str, dotnet: &Value, rust: &Value) {
        if !same(dotnet, rust) {
            self.push(path, Kind::Value, dotnet, rust);
        }
    }

    fn properties(&mut self, dotnet: &Value, rust: &Value) {
        let empty = Map::new();
        let d = dotnet.as_object().unwrap_or(&empty);
        let r = rust.as_object().unwrap_or(&empty);
        for (element, d_element, r_element) in union(d, r) {
            let path = format!("properties.{element}");
            match (d_element, r_element) {
                (Some(d), None) => self.push(&path, Kind::OnlyInDotnet, d, &Value::Null),
                (None, Some(r)) => self.push(&path, Kind::OnlyInRust, &Value::Null, r),
                (Some(d), Some(r)) => self.element(&path, d, r),
                (None, None) => {}
            }
        }
    }

    fn element(&mut self, path: &str, dotnet: &Value, rust: &Value) {
        let (Some(d), Some(r)) = (dotnet.as_object(), rust.as_object()) else {
            if dotnet.is_null() != rust.is_null() {
                self.push(path, Kind::Presence, dotnet, rust);
            }
            return;
        };
        for (name, d_property, r_property) in union(d, r) {
            let path = format!("{path}.{name}");
            match (d_property, r_property) {
                (Some(d), None) => self.push(&path, Kind::OnlyInDotnet, d, &Value::Null),
                (None, Some(r)) => self.push(&path, Kind::OnlyInRust, &Value::Null, r),
                (Some(d), Some(r)) => self.property(&path, d, r),
                (None, None) => {}
            }
        }
    }

    fn property(&mut self, path: &str, dotnet: &Value, rust: &Value) {
        let d_name = dotnet.get("name");
        let r_name = rust.get("name");
        if d_name != r_name {
            self.push(
                path,
                Kind::Name,
                d_name.unwrap_or(&Value::Null),
                r_name.unwrap_or(&Value::Null),
            );
        }
        let (d_state, r_state) = (state(dotnet), state(rust));
        if d_state != r_state {
            self.push(path, Kind::State, dotnet, rust);
            return;
        }
        match d_state {
            // The values are compared when the types differ as well, so an
            // entry allowing a type never hides a value.
            "value" => {
                if dotnet.get("type") != rust.get("type") {
                    self.push(path, Kind::Type, dotnet, rust);
                }
                if !same(&dotnet["value"], &rust["value"]) {
                    self.push(path, Kind::Value, &dotnet["value"], &rust["value"]);
                }
            }
            "noValue" | "missing" if dotnet[d_state] != rust[r_state] => {
                self.push(path, Kind::NoValueMessage, &dotnet[d_state], &rust[r_state]);
            }
            _ => {}
        }
    }

    /// Compares builder output that should be JSON, by structure first and
    /// then as text.
    fn json_text(&mut self, path: &str, dotnet: &Value, rust: &Value) {
        let (Some(d), Some(r)) = (dotnet.as_str(), rust.as_str()) else {
            if dotnet != rust {
                self.push(path, Kind::Presence, dotnet, rust);
            }
            return;
        };
        let (Ok(d_value), Ok(r_value)) = (
            serde_json::from_str::<Value>(d),
            serde_json::from_str::<Value>(r),
        ) else {
            self.text(path, dotnet, rust);
            return;
        };
        let before = self.differences.len();
        self.structure(path, &d_value, &r_value);
        if self.differences.len() == before && d != r {
            self.text(path, dotnet, rust);
        }
    }

    fn structure(&mut self, path: &str, dotnet: &Value, rust: &Value) {
        match (dotnet, rust) {
            (Value::Object(d), Value::Object(r)) => {
                for (key, d_value, r_value) in union(d, r) {
                    let path = format!("{path}.{key}");
                    match (d_value, r_value) {
                        (Some(d), None) => self.push(&path, Kind::OnlyInDotnet, d, &Value::Null),
                        (None, Some(r)) => self.push(&path, Kind::OnlyInRust, &Value::Null, r),
                        (Some(d), Some(r)) => self.structure(&path, d, r),
                        (None, None) => {}
                    }
                }
                let d_keys: Vec<&String> = d.keys().filter(|k| r.contains_key(*k)).collect();
                let r_keys: Vec<&String> = r.keys().filter(|k| d.contains_key(*k)).collect();
                if d_keys != r_keys {
                    self.push(path, Kind::Order, &keys(&d_keys), &keys(&r_keys));
                }
            }
            (Value::Array(d), Value::Array(r)) if d.len() == r.len() => {
                for (i, (d, r)) in d.iter().zip(r).enumerate() {
                    self.structure(&format!("{path}[{i}]"), d, r);
                }
            }
            (d, r) if kind_of(d) != kind_of(r) => self.push(path, Kind::Type, d, r),
            (d, r) if d != r => self.push(path, Kind::Value, d, r),
            _ => {}
        }
    }

    /// Compares text exactly, reporting the first line that differs.
    fn text(&mut self, path: &str, dotnet: &Value, rust: &Value) {
        match (dotnet.as_str(), rust.as_str()) {
            (Some(d), Some(r)) if d != r => {
                let (line, d_line, r_line) = first_different_line(d, r);
                self.push(
                    path,
                    Kind::Text,
                    &Value::String(format!("line {line}: {}", cut(d_line))),
                    &Value::String(format!("line {line}: {}", cut(r_line))),
                );
            }
            (Some(_), Some(_)) => {}
            _ => {
                if dotnet != rust {
                    self.push(path, Kind::Presence, dotnet, rust);
                }
            }
        }
    }
}

fn keys(keys: &[&String]) -> Value {
    Value::Array(keys.iter().map(|k| Value::String((*k).clone())).collect())
}

fn state(property: &Value) -> &'static str {
    if property.get("value").is_some() {
        "value"
    } else if property.get("noValue").is_some() {
        "noValue"
    } else if property.get("missing").is_some() {
        "missing"
    } else {
        "unknown"
    }
}

fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Equal as data, with numbers compared by value and object keys in any
/// order.
pub fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same(x, y))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
        }
        _ => a == b,
    }
}

/// The keys of both maps, .NET's order first then any only Rust has.
fn union<'a>(
    dotnet: &'a Map<String, Value>,
    rust: &'a Map<String, Value>,
) -> Vec<(&'a String, Option<&'a Value>, Option<&'a Value>)> {
    let mut out: Vec<_> = dotnet
        .iter()
        .map(|(k, v)| (k, Some(v), rust.get(k)))
        .collect();
    out.extend(
        rust.iter()
            .filter(|(k, _)| !dotnet.contains_key(*k))
            .map(|(k, v)| (k, None, Some(v))),
    );
    out
}

fn first_different_line<'a>(a: &'a str, b: &'a str) -> (usize, &'a str, &'a str) {
    let mut a_lines = a.lines();
    let mut b_lines = b.lines();
    let mut number = 1;
    loop {
        match (a_lines.next(), b_lines.next()) {
            (Some(x), Some(y)) if x == y => number += 1,
            (x, y) => return (number, x.unwrap_or("<end>"), y.unwrap_or("<end>")),
        }
    }
}

fn cut(text: &str) -> String {
    if text.chars().count() <= REPORT_TEXT_LIMIT {
        text.to_owned()
    } else {
        let head: String = text.chars().take(REPORT_TEXT_LIMIT).collect();
        format!("{head}...")
    }
}

/// The outcome of checking differences against the allowed entries.
pub struct Outcome<'a> {
    /// Differences no entry allows.
    pub failures: Vec<&'a Difference>,
    /// How many differences each applicable entry allowed, by entry index.
    pub allowed_counts: BTreeMap<usize, usize>,
    /// Applicable entries that allowed nothing.
    pub unused: Vec<usize>,
}

pub fn check<'a>(differences: &'a [Difference], allowed: &[Allowed], data: &str) -> Outcome<'a> {
    let applicable: Vec<usize> = (0..allowed.len())
        .filter(|i| allowed[*i].applies_to(data))
        .collect();
    let mut failures = Vec::new();
    let mut allowed_counts: BTreeMap<usize, usize> = applicable.iter().map(|i| (*i, 0)).collect();
    for difference in differences {
        match applicable.iter().find(|i| allowed[**i].matches(difference)) {
            Some(i) => *allowed_counts.entry(*i).or_default() += 1,
            None => failures.push(difference),
        }
    }
    let unused = allowed_counts
        .iter()
        .filter(|(_, n)| **n == 0)
        .map(|(i, _)| *i)
        .collect();
    Outcome {
        failures,
        allowed_counts,
        unused,
    }
}

/// A Markdown report of the outcome, with the differences no entry allows
/// grouped by path and kind.
pub fn report(
    cases: &[Case],
    differences: &[Difference],
    allowed: &[Allowed],
    outcome: &Outcome<'_>,
    data: &str,
) -> String {
    let about: HashMap<&str, &str> = cases
        .iter()
        .map(|c| (c.id.as_str(), c.about.as_str()))
        .collect();
    let mut text = format!(
        "## Comparison with the .NET packages ({data})\n\n\
         {} cases, {} differences, {} not allowed.\n\n",
        cases.len(),
        differences.len(),
        outcome.failures.len()
    );
    if !outcome.failures.is_empty() {
        text.push_str("### Not allowed\n\n");
        let mut groups: BTreeMap<(&str, Kind), Vec<&Difference>> = BTreeMap::new();
        for d in &outcome.failures {
            groups
                .entry((generalise(&d.path), d.kind))
                .or_default()
                .push(d);
        }
        for ((path, kind), list) in groups {
            text.push_str(&format!(
                "- `{path}` {}, {} case(s)\n",
                kind.name(),
                list.len()
            ));
            for d in list.iter().take(3) {
                text.push_str(&format!(
                    "  - `{}` ({}) at `{}`: .NET `{}`, Rust `{}`\n",
                    d.case,
                    about.get(d.case.as_str()).copied().unwrap_or(""),
                    d.path,
                    cut(&d.dotnet.to_string()),
                    cut(&d.rust.to_string()),
                ));
            }
        }
        text.push('\n');
    }
    text.push_str("### Allowed\n\n| Path | Kind | Count | Issue | Reason |\n| --- | --- | --- | --- | --- |\n");
    for (i, count) in &outcome.allowed_counts {
        let entry = &allowed[*i];
        text.push_str(&format!(
            "| `{}` | {} | {} | {} | {} |\n",
            entry.path,
            entry.kind.as_deref().unwrap_or("any"),
            count,
            entry.issue.map(|n| format!("#{n}")).unwrap_or_default(),
            entry.reason,
        ));
    }
    if !outcome.unused.is_empty() {
        text.push_str("\n### Entries that allowed nothing\n\n");
        for i in &outcome.unused {
            text.push_str(&format!(
                "- `{}`: {}\n",
                allowed[*i].path, allowed[*i].reason
            ));
        }
    }
    text
}

/// The path with array indexes replaced, so one group holds every index.
fn generalise(path: &str) -> &str {
    path.split('[').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn case(id: &str, session: bool) -> Case {
        let mut evidence = Map::new();
        if session {
            evidence.insert(SESSION_ID_KEY.into(), json!("abc"));
        }
        Case {
            id: id.into(),
            about: String::new(),
            evidence,
        }
    }

    fn line(is_mobile: bool) -> Value {
        json!({
            "id": "a",
            "processError": null,
            "properties": {"device": {
                "ismobile": {"name": "IsMobile", "type": "bool", "value": is_mobile},
                "browsername": {"name": "BrowserName", "noValue": "No match"}
            }},
            "session": {"id": "abc", "sequence": 1},
            "evidenceAfter": {"query.session-id": "abc", "query.sequence": "1"},
            "errors": [],
            "json": "{\"device\":{\"ismobile\":true}}",
            "javascript": "var a = 1;\nvar b = 2;",
            "usage": null
        })
    }

    #[test]
    fn same_lines_have_no_difference() {
        assert!(compare_case(&case("a", true), &line(true), &line(true)).is_empty());
    }

    #[test]
    fn a_changed_value_is_a_difference_at_its_path() {
        let differences = compare_case(&case("a", true), &line(true), &line(false));
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].path, "properties.device.ismobile");
        assert_eq!(differences[0].kind, Kind::Value);
    }

    #[test]
    fn a_changed_answer_fails_unless_allowed() {
        let differences = compare_case(&case("a", true), &line(true), &line(false));
        let outcome = check(&differences, &[], "lite");
        assert_eq!(outcome.failures.len(), 1);
        let allowed: Vec<Allowed> = serde_json::from_value(json!([{
            "path": "properties.device.*", "kind": "value", "reason": "test"
        }]))
        .unwrap();
        let outcome = check(&differences, &allowed, "lite");
        assert!(outcome.failures.is_empty());
        assert_eq!(outcome.allowed_counts[&0], 1);
    }

    #[test]
    fn an_entry_for_other_data_does_not_apply() {
        let differences = compare_case(&case("a", true), &line(true), &line(false));
        let allowed: Vec<Allowed> = serde_json::from_value(json!([{
            "path": "*", "data": "enterprise", "reason": "test"
        }]))
        .unwrap();
        let outcome = check(&differences, &allowed, "lite");
        assert_eq!(outcome.failures.len(), 1);
        assert!(outcome.unused.is_empty());
    }

    #[test]
    fn an_entry_that_allows_nothing_is_reported() {
        let allowed: Vec<Allowed> =
            serde_json::from_value(json!([{"path": "json", "reason": "test"}])).unwrap();
        let outcome = check(&[], &allowed, "lite");
        assert_eq!(outcome.unused, vec![0]);
    }

    #[test]
    fn an_entry_needs_a_reason() {
        let parsed: Result<Vec<Allowed>, _> = serde_json::from_value(json!([{"path": "json"}]));
        assert!(parsed.is_err());
    }

    #[test]
    fn json_key_order_is_a_difference() {
        let mut rust = line(true);
        rust["json"] = json!("{\"b\":1,\"a\":2}");
        let mut dotnet = line(true);
        dotnet["json"] = json!("{\"a\":2,\"b\":1}");
        let differences = compare_case(&case("a", true), &dotnet, &rust);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].kind, Kind::Order);
        assert_eq!(differences[0].path, "json");
    }

    #[test]
    fn json_written_differently_is_a_text_difference() {
        let mut rust = line(true);
        rust["json"] = json!("{\"a\": 1.0}");
        let mut dotnet = line(true);
        dotnet["json"] = json!("{\"a\":1.0}");
        let differences = compare_case(&case("a", true), &dotnet, &rust);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].kind, Kind::Text);
    }

    #[test]
    fn json_paths_name_the_key() {
        let mut rust = line(true);
        rust["json"] = json!("{\"device\":{\"ismobile\":false}}");
        let differences = compare_case(&case("a", true), &line(true), &rust);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].path, "json.device.ismobile");
    }

    #[test]
    fn javascript_reports_the_first_line_that_differs() {
        let mut rust = line(true);
        rust["javascript"] = json!("var a = 1;\nvar b = 3;");
        let differences = compare_case(&case("a", true), &line(true), &rust);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].rust, json!("line 2: var b = 3;"));
    }

    #[test]
    fn a_property_only_one_side_has_is_reported() {
        let mut rust = line(true);
        rust["properties"]["device"]
            .as_object_mut()
            .unwrap()
            .remove("browsername");
        let differences = compare_case(&case("a", true), &line(true), &rust);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].kind, Kind::OnlyInDotnet);
        assert_eq!(differences[0].path, "properties.device.browsername");
    }

    #[test]
    fn numbers_compare_by_value() {
        let mut dotnet = line(true);
        dotnet["session"]["sequence"] = json!(1.0);
        assert!(compare_case(&case("a", true), &dotnet, &line(true)).is_empty());
    }

    #[test]
    fn a_generated_session_id_is_replaced_only_without_one_in_the_evidence() {
        let mut dotnet = line(true);
        dotnet["session"]["id"] = json!("0f8fad5b-d9cb-469f-a165-70867728950e");
        dotnet["javascript"] = json!("sid=0f8fad5b-d9cb-469f-a165-70867728950e");
        let mut rust = line(true);
        rust["session"]["id"] = json!("7c9e6679-7425-40de-944b-e07fc1f90ae7");
        rust["javascript"] = json!("sid=7c9e6679-7425-40de-944b-e07fc1f90ae7");
        assert!(compare_case(&case("a", false), &dotnet, &rust).is_empty());
        assert_eq!(compare_case(&case("a", true), &dotnet, &rust).len(), 2);
    }

    #[test]
    fn a_session_id_in_another_form_is_not_replaced() {
        let mut dotnet = line(true);
        dotnet["session"]["id"] = json!("0f8fad5b-d9cb-469f-a165-70867728950e");
        let mut rust = line(true);
        rust["session"]["id"] = json!("not-a-guid");
        let differences = compare_case(&case("a", false), &dotnet, &rust);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].path, "session");
    }

    #[test]
    fn a_missing_line_is_a_difference() {
        let cases = vec![case("a", true)];
        let dotnet = HashMap::from([("a".to_owned(), line(true))]);
        let differences = compare_all(&cases, &dotnet, &HashMap::new());
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].kind, Kind::MissingLine);
    }

    /// A line where the crates give `ismobile` as `rust`, for a case with a
    /// User-Agent where .NET gives `dotnet`.
    fn ismobile_lines(dotnet: Value, rust: Value) -> (Value, Value) {
        let mut d = line(true);
        d["properties"]["device"]["ismobile"] = dotnet;
        let mut r = line(true);
        r["properties"]["device"]["ismobile"] = rust;
        (d, r)
    }

    fn entries(value: Value) -> Vec<Allowed> {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn an_entry_naming_each_side_allows_only_that_change() {
        let allowed = entries(json!([{
            "path": "properties.device.*", "kind": "state",
            "dotnet": {"value": [0, false]}, "rust": {"noValue": true},
            "reason": "test"
        }]));
        let no_value = json!({"name": "IsMobile", "noValue": "none"});

        let (d, r) = ismobile_lines(
            json!({"name": "IsMobile", "type": "bool", "value": false}),
            no_value.clone(),
        );
        let differences = compare_case(&case("a", true), &d, &r);
        assert_eq!(differences.len(), 1);
        assert!(check(&differences, &allowed, "lite").failures.is_empty());

        let (d, r) = ismobile_lines(
            json!({"name": "IsMobile", "type": "bool", "value": true}),
            no_value,
        );
        let differences = compare_case(&case("a", true), &d, &r);
        assert_eq!(check(&differences, &allowed, "lite").failures.len(), 1);
    }

    #[test]
    fn a_side_value_compares_numbers_by_value() {
        let allowed = entries(json!([{
            "path": "json.device.*", "kind": "onlyInDotnet",
            "dotnet": {"value": [0]}, "reason": "test"
        }]));
        let mut dotnet = line(true);
        dotnet["json"] = json!("{\"device\":{\"ismobile\":true,\"width\":0.0}}");
        let differences = compare_case(&case("a", true), &dotnet, &line(true));
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].path, "json.device.width");
        assert!(check(&differences, &allowed, "lite").failures.is_empty());
    }

    #[test]
    fn a_null_side_allows_only_null() {
        let allowed = entries(json!([{
            "path": "json.device.*", "kind": "onlyInDotnet",
            "dotnet": {"value": [null]}, "reason": "test"
        }]));
        let mut rust = line(true);
        rust["json"] = json!("{\"device\":{}}");
        let differences = compare_case(&case("a", true), &line(true), &rust);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].path, "json.device.ismobile");
        assert_eq!(check(&differences, &allowed, "lite").failures.len(), 1);

        let mut dotnet = line(true);
        dotnet["json"] = json!("{\"device\":{\"ismobile\":null}}");
        let differences = compare_case(&case("a", true), &dotnet, &rust);
        assert!(check(&differences, &allowed, "lite").failures.is_empty());
    }

    #[test]
    fn a_type_difference_also_compares_the_values() {
        let (d, r) = ismobile_lines(
            json!({"name": "IsMobile", "type": "bool", "value": false}),
            json!({"name": "IsMobile", "type": "string", "value": "Unknown"}),
        );
        let kinds: Vec<Kind> = compare_case(&case("a", true), &d, &r)
            .iter()
            .map(|d| d.kind)
            .collect();
        assert_eq!(kinds, [Kind::Type, Kind::Value]);

        let (d, r) = ismobile_lines(
            json!({"name": "IsMobile", "type": "javascript", "value": "x()"}),
            json!({"name": "IsMobile", "type": "string", "value": "x()"}),
        );
        let kinds: Vec<Kind> = compare_case(&case("a", true), &d, &r)
            .iter()
            .map(|d| d.kind)
            .collect();
        assert_eq!(kinds, [Kind::Type]);
    }

    #[test]
    fn a_side_naming_nothing_is_a_problem() {
        let allowed = entries(json!([
            {"path": "json", "dotnet": {}, "reason": "test"},
            {"path": "json", "rust": {"noValue": true}, "reason": "test"}
        ]));
        assert!(allowed[0].problem().is_some_and(|p| p.contains("`dotnet`")));
        assert!(allowed[1].problem().is_none());
    }

    #[test]
    fn glob_matches_runs_of_any_characters() {
        assert!(glob("properties.ip.*", "properties.ip.countrycode"));
        assert!(glob("*", ""));
        assert!(glob("a*c*e", "abcde"));
        assert!(!glob("properties.ip.*", "properties.device.ismobile"));
        assert!(!glob("json", "json.device"));
    }
}
