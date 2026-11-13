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

use std::collections::{BTreeMap, HashMap, HashSet};

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

/// The two line endings a text may use.
const CRLF: &str = "\r\n";
const LF: &str = "\n";

/// Stands in for a builder's type name in a configuration error, because
/// .NET gives the .NET full name and the crates the Rust name.
pub const BUILDER_TYPE: &str = "{type}";

/// The text before a builder's type name in a configuration error, and the
/// texts that may follow it, for an element's builder and the pipeline's.
const BEFORE_BUILDER_TYPE: &str = "uilder '";
const AFTER_BUILDER_TYPE: &[&str] = &["' for element", "' for pipeline"];

/// The end of the line a configuration error lists the builder's settings
/// after, one to a line, up to the line starting with
/// [`BUILD_METHODS_HEADING`].
const SETTINGS_HEADING: &str = "The available configuration methods on this builder are:";
const BUILD_METHODS_HEADING: &str = "The available 'Build' methods";

/// The method .NET lists among the settings when it takes one parameter.
const BUILD_METHOD: &str = "Build";

/// The text a configuration error lists the available builders after.
const AVAILABLE_BUILDERS: &str = "Available builders are: ";

/// The character in the name of a .NET generic type, such as
/// ``SingleFileAspectEngineBuilderBase`2``.
const GENERIC_MARK: char = '`';

/// The elements of a usage record that hold the time it was written or name
/// the runtime, which differ between the languages by design. Their text is
/// left out before two records are compared.
const USAGE_RUNTIME_ELEMENTS: &[&str] = &[
    "DateSent",
    "Version",
    "Language",
    "LanguageVersion",
    "Platform",
    "FlowElement",
];

/// The same for a record from the custom sink, which writes its own Flow
/// Element names, so those must be the same on both sides.
const USAGE_CUSTOM_RUNTIME_ELEMENTS: &[&str] = &[
    "DateSent",
    "Version",
    "Language",
    "LanguageVersion",
    "Platform",
];

/// The last of the values that are the same in every usage record. The
/// evidence entries follow it and are never left out.
const USAGE_LAST_STATIC_ELEMENT: &str = "Platform";

/// How many characters before the first that differs a report shows of a
/// usage record.
const REPORT_RECORD_CONTEXT: usize = 60;

/// The start of a CDATA section in a usage record.
const CDATA_START: &str = "<![CDATA[";

/// The end of a CDATA section in a usage record.
const CDATA_END: &str = "]]>";

/// One corpus case. A case that is not a request, such as a 51Did to read,
/// has no evidence. Fields the comparer does not use are ignored.
#[derive(Debug, Clone, Deserialize)]
pub struct Case {
    pub id: String,
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub evidence: Map<String, Value>,
}

/// The id that appears more than once in `cases`, if any.
pub fn repeated_id(cases: &[Case]) -> Option<&str> {
    let mut seen = HashSet::new();
    cases
        .iter()
        .map(|c| c.id.as_str())
        .find(|id| !seen.insert(*id))
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
    /// The same text once each CRLF is read as LF. Each side is the number
    /// of lines it ends with CRLF.
    LineEndings,
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
            Kind::LineEndings => "lineEndings",
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
    /// The side gave text matching one of these patterns, where `*` matches
    /// any run of characters. A text difference gives the line number, so a
    /// pattern can allow a line wherever it falls.
    #[serde(default)]
    pub pattern: Option<Vec<String>>,
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
            || self.pattern.as_ref().is_some_and(|patterns| {
                value
                    .and_then(Value::as_str)
                    .is_some_and(|text| patterns.iter().any(|p| glob(p, text)))
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

    /// What is wrong with the entry, if anything. A side that names no
    /// value or pattern and does not ask for no value would match nothing.
    pub fn problem(&self) -> Option<String> {
        [("dotnet", &self.dotnet), ("rust", &self.rust)]
            .into_iter()
            .find(|(_, side)| {
                side.as_ref().is_some_and(|side| {
                    side.value.is_none() && side.pattern.is_none() && !side.no_value
                })
            })
            .map(|(name, _)| {
                format!(
                    "the entry for `{}` gives `{name}` no value, pattern or noValue",
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
    out.text(
        "javascript",
        field_of(&dotnet, "javascript"),
        field_of(&rust, "javascript"),
    );
    out.record(
        "usage",
        field_of(&dotnet, "usage"),
        field_of(&rust, "usage"),
        USAGE_RUNTIME_ELEMENTS,
    );
    out.record(
        "usageCustom",
        field_of(&dotnet, "usageCustom"),
        field_of(&rust, "usageCustom"),
        USAGE_CUSTOM_RUNTIME_ELEMENTS,
    );
    for field in [
        "fodid",
        "metadata",
        "engineMetadata",
        "derived",
        "robotsTxtEngine",
        "propertyKeyedEngines",
        "indexedValues",
        "profileIds",
        "warnings",
        "hardwareJson",
    ] {
        out.data(field, field_of(&dotnet, field), field_of(&rust, field));
    }
    for field in ["plainText", "annotatedText"] {
        out.text(field, field_of(&dotnet, field), field_of(&rust, field));
    }
    out.data(
        "configuration",
        &normalise_configuration(field_of(&dotnet, "configuration")),
        &normalise_configuration(field_of(&rust, "configuration")),
    );
    out.differences
}

/// A configuration line's `configuration` with its error, if it has one,
/// passed through [`normalise_configuration_error`].
fn normalise_configuration(configuration: &Value) -> Value {
    let mut configuration = configuration.clone();
    if let Some(error) = configuration.get("error").and_then(Value::as_str) {
        configuration["error"] = Value::String(normalise_configuration_error(error));
    }
    configuration
}

/// A configuration error with the parts the two languages write differently
/// by design made the same, so the rest is compared exactly. Line endings
/// become LF. The builder's type name becomes [`BUILDER_TYPE`]. In the list
/// of settings, the parameter type after each name is left out, because
/// .NET gives `RuntimeParameterInfo` for every one, then a name listed again
/// straight after itself is listed once and `Build` is left out, because
/// .NET lists both forms of some usage sharing settings and lists
/// `Build(Stream)` among the IP Intelligence engine's settings. Generic
/// types are left out of the available builders, because .NET lists its
/// abstract base builders there, which no configuration can name.
pub fn normalise_configuration_error(error: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut in_settings = false;
    for line in error.replace(CRLF, LF).split(LF) {
        if line.starts_with(BUILD_METHODS_HEADING) {
            in_settings = false;
        }
        if in_settings {
            let name = line.split_once(" (").map_or(line, |(name, _)| name);
            if name != BUILD_METHOD && lines.last().is_none_or(|last| last != name) {
                lines.push(name.to_owned());
            }
            continue;
        }
        let mut line = without_builder_type(line);
        if let Some(at) = line.find(AVAILABLE_BUILDERS) {
            let start = at + AVAILABLE_BUILDERS.len();
            let names: Vec<&str> = line[start..]
                .split(',')
                .filter(|name| !name.contains(GENERIC_MARK))
                .collect();
            line = format!("{}{}", &line[..start], names.join(","));
        }
        in_settings = line.trim_end().ends_with(SETTINGS_HEADING);
        lines.push(line);
    }
    lines.join(LF)
}

/// `line` with each builder type name, between [`BEFORE_BUILDER_TYPE`] and
/// one of [`AFTER_BUILDER_TYPE`], replaced by [`BUILDER_TYPE`].
fn without_builder_type(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(at) = rest.find(BEFORE_BUILDER_TYPE) {
        let start = at + BEFORE_BUILDER_TYPE.len();
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        if let Some(end) = rest.find('\'') {
            if AFTER_BUILDER_TYPE
                .iter()
                .any(|after| rest[end..].starts_with(after))
            {
                out.push_str(BUILDER_TYPE);
                rest = &rest[end..];
            }
        }
    }
    out.push_str(rest);
    out
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

    /// Compares two values written as data, key by key and item by item, so
    /// that each difference names the key or index where it is. Key order is
    /// not compared, and numbers compare by value.
    fn data(&mut self, path: &str, dotnet: &Value, rust: &Value) {
        match (dotnet, rust) {
            (Value::Object(d), Value::Object(r)) => {
                for (key, d_value, r_value) in union(d, r) {
                    self.data_or_only(&format!("{path}.{key}"), d_value, r_value);
                }
            }
            (Value::Array(d), Value::Array(r)) => {
                for i in 0..d.len().max(r.len()) {
                    self.data_or_only(&format!("{path}[{i}]"), d.get(i), r.get(i));
                }
            }
            (d, r) if d.is_null() != r.is_null() => self.push(path, Kind::Presence, d, r),
            (d, r) if !same(d, r) => self.push(path, Kind::Value, d, r),
            _ => {}
        }
    }

    /// [`Out::data`] when both sides have the key or item, and otherwise a
    /// difference naming the side that has it.
    fn data_or_only(&mut self, path: &str, dotnet: Option<&Value>, rust: Option<&Value>) {
        match (dotnet, rust) {
            (Some(d), Some(r)) => self.data(path, d, r),
            (Some(d), None) => self.push(path, Kind::OnlyInDotnet, d, &Value::Null),
            (None, Some(r)) => self.push(path, Kind::OnlyInRust, &Value::Null, r),
            (None, None) => {}
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

    /// Compares text exactly, reporting the first line that differs, or
    /// only the line endings when nothing else differs.
    fn text(&mut self, path: &str, dotnet: &Value, rust: &Value) {
        match (dotnet.as_str(), rust.as_str()) {
            (Some(d), Some(r)) if d != r && d.replace(CRLF, LF) == r.replace(CRLF, LF) => {
                self.push(
                    path,
                    Kind::LineEndings,
                    &Value::from(d.matches(CRLF).count()),
                    &Value::from(r.matches(CRLF).count()),
                );
            }
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

    /// Compares two usage records as text, once the text of each element
    /// named in `blanked` is left out, and shows where they first differ.
    fn record(&mut self, path: &str, dotnet: &Value, rust: &Value, blanked: &[&str]) {
        let (Some(d), Some(r)) = (dotnet.as_str(), rust.as_str()) else {
            if dotnet != rust {
                self.push(path, Kind::Presence, dotnet, rust);
            }
            return;
        };
        let d = blank_elements(&cdata_as_text(d), blanked);
        let r = blank_elements(&cdata_as_text(r), blanked);
        if let Some(at) = first_difference(&d, &r) {
            self.push(
                path,
                Kind::Text,
                &Value::String(around(&d, at)),
                &Value::String(around(&r, at)),
            );
        }
    }
}

/// `record` with each CDATA section written as the same text with `&`, `<`
/// and `>` as entities, which is how the crates write every evidence value.
/// A section with no end is left as it is.
fn cdata_as_text(record: &str) -> String {
    let mut out = String::with_capacity(record.len());
    let mut rest = record;
    while let Some(start) = rest.find(CDATA_START) {
        out.push_str(&rest[..start]);
        let inner = &rest[start + CDATA_START.len()..];
        let Some(end) = inner.find(CDATA_END) else {
            out.push_str(&rest[start..]);
            return out;
        };
        for c in inner[..end].chars() {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                c => out.push(c),
            }
        }
        rest = &inner[end + CDATA_END.len()..];
    }
    out.push_str(rest);
    out
}

/// `record` with each element named in `names` written as `<Name />`, up to
/// the end of the record's last static element. The evidence entries after
/// that are left as they are.
fn blank_elements(record: &str, names: &[&str]) -> String {
    let end = static_end(record);
    let mut out = String::with_capacity(record.len());
    let mut rest = &record[..end];
    'next: while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        rest = &rest[open..];
        for name in names {
            if let Some(length) = element_length(rest, name) {
                out.push('<');
                out.push_str(name);
                out.push_str(" />");
                rest = &rest[length..];
                continue 'next;
            }
        }
        out.push('<');
        rest = &rest[1..];
    }
    out.push_str(rest);
    out.push_str(&record[end..]);
    out
}

/// The length of the element `name` at the start of `text`, written empty or
/// with text, or `None` when `text` does not start with it.
fn element_length(text: &str, name: &str) -> Option<usize> {
    const EMPTY_END: &str = " />";
    let after = text.strip_prefix('<')?.strip_prefix(name)?;
    let opened = 1 + name.len();
    if after.starts_with(EMPTY_END) {
        return Some(opened + EMPTY_END.len());
    }
    let inner = after.strip_prefix('>')?;
    let close = format!("</{name}>");
    let length = inner.find(&close)?;
    Some(opened + 1 + length + close.len())
}

/// Where the values that are the same in every record end, being after the
/// record's first [`USAGE_LAST_STATIC_ELEMENT`], or the end of the record when
/// it has none.
fn static_end(record: &str) -> usize {
    record
        .find(&format!("<{USAGE_LAST_STATIC_ELEMENT}"))
        .and_then(|start| {
            element_length(&record[start..], USAGE_LAST_STATIC_ELEMENT).map(|length| start + length)
        })
        .unwrap_or(record.len())
}

/// The byte offset of the first character at which `a` and `b` differ, or
/// `None` when they are the same.
fn first_difference(a: &str, b: &str) -> Option<usize> {
    if a == b {
        return None;
    }
    Some(
        a.char_indices()
            .zip(b.chars())
            .find(|((_, x), y)| x != y)
            .map_or(a.len().min(b.len()), |((i, _), _)| i),
    )
}

/// The number of characters before the byte offset `at`, then the text from
/// up to [`REPORT_RECORD_CONTEXT`] characters before `at`.
fn around(text: &str, at: usize) -> String {
    let before = &text[..at];
    let start = before
        .char_indices()
        .rev()
        .nth(REPORT_RECORD_CONTEXT - 1)
        .map_or(0, |(i, _)| i);
    let lead = if start > 0 { "..." } else { "" };
    format!(
        "after {} characters: {lead}{}",
        before.chars().count(),
        cut(&text[start..])
    )
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

    /// The errors a pipeline returns as the sub-errors of one error are
    /// compared, as a pipeline whose errors are not suppressed returns them.
    #[test]
    fn errors_returned_as_sub_errors_are_compared() {
        let mut dotnet = line(true);
        dotnet["thrown"] = json!(["The failing element always fails."]);
        let mut rust = line(true);
        rust["thrown"] = json!([]);
        let differences = compare_case(&case("a", true), &dotnet, &rust);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].path, "thrown");
        assert_eq!(differences[0].kind, Kind::Value);
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
    fn text_that_differs_only_in_line_endings_is_its_own_kind() {
        let mut dotnet = line(true);
        dotnet["javascript"] = json!("var a = 1;\r\nvar b = 2;\r\n");
        let mut rust = line(true);
        rust["javascript"] = json!("var a = 1;\nvar b = 2;\n");
        let differences = compare_case(&case("a", true), &dotnet, &rust);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].path, "javascript");
        assert_eq!(differences[0].kind, Kind::LineEndings);
        assert_eq!(differences[0].dotnet, json!(2));
        assert_eq!(differences[0].rust, json!(0));
    }

    #[test]
    fn text_that_differs_in_line_endings_and_content_is_text() {
        let mut dotnet = line(true);
        dotnet["javascript"] = json!("var a = 1;\r\nvar b = 2;");
        let mut rust = line(true);
        rust["javascript"] = json!("var a = 1;\nvar b = 3;");
        let differences = compare_case(&case("a", true), &dotnet, &rust);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].kind, Kind::Text);
        assert_eq!(differences[0].rust, json!("line 2: var b = 3;"));
    }

    #[test]
    fn a_lone_carriage_return_is_not_a_line_ending() {
        let mut dotnet = line(true);
        dotnet["javascript"] = json!("var a = 1;\rvar b = 2;");
        let mut rust = line(true);
        rust["javascript"] = json!("var a = 1;\nvar b = 2;");
        let differences = compare_case(&case("a", true), &dotnet, &rust);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].kind, Kind::Text);
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

    /// A usage record in the form .NET writes, with the values that differ
    /// by design and one evidence value given.
    fn usage_record(date: &str, platform: &str, flow_element: &str, value: &str) -> String {
        format!(
            "<Device><SessionId>abc</SessionId><Sequence>2</Sequence>\
             <DateSent>{date}</DateSent><ClientIP /><Version>4.5.1</Version>\
             <Product>Pipeline</Product><FlowElement>{flow_element}</FlowElement>\
             <Language>rust</Language><LanguageVersion>1.94</LanguageVersion>\
             <ServerIP>10.0.0.1</ServerIP><Platform>{platform}</Platform>\
             <header Name=\"User-Agent\"><![CDATA[{value}]]></header></Device>"
        )
    }

    fn with_usage(field: &str, record: &str) -> Value {
        let mut value = line(true);
        value[field] = json!(record);
        value
    }

    #[test]
    fn blanking_writes_only_the_named_elements_empty() {
        assert_eq!(
            blank_elements(
                &usage_record("2026-11-05T19:00:00", "linux", "a", "b"),
                USAGE_RUNTIME_ELEMENTS
            ),
            "<Device><SessionId>abc</SessionId><Sequence>2</Sequence>\
             <DateSent /><ClientIP /><Version /><Product>Pipeline</Product>\
             <FlowElement /><Language /><LanguageVersion />\
             <ServerIP>10.0.0.1</ServerIP><Platform />\
             <header Name=\"User-Agent\"><![CDATA[b]]></header></Device>"
        );
    }

    #[test]
    fn usage_records_differing_only_in_runtime_values_are_the_same() {
        let dotnet = usage_record("2026-11-05T19:00:00", "Unix 6.8.0", "FiftyOne.A", "x")
            .replace("<Language>rust</Language>", "<Language>dotnet</Language>")
            .replace("<Version>4.5.1</Version>", "<Version>4.5.129</Version>");
        let rust = usage_record("2026-11-05T19:00:01", "linux", "", "x")
            .replace("<FlowElement></FlowElement>", "<FlowElement />");
        let differences = compare_case(
            &case("a", true),
            &with_usage("usage", &dotnet),
            &with_usage("usage", &rust),
        );
        assert!(differences.is_empty(), "{differences:?}");
    }

    #[test]
    fn a_usage_record_difference_shows_where_it_starts() {
        let differences = compare_case(
            &case("a", true),
            &with_usage("usage", &usage_record("d", "p", "f", "a<b")),
            &with_usage("usage", &usage_record("d", "p", "f", "a<c")),
        );
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].path, "usage");
        assert_eq!(differences[0].kind, Kind::Text);
        let shown = differences[0].dotnet.as_str().unwrap();
        assert!(shown.starts_with("after "), "{shown}");
        assert!(shown.contains(">a&lt;b</header>"), "{shown}");
        let shown = differences[0].rust.as_str().unwrap();
        assert!(shown.contains(">a&lt;c</header>"), "{shown}");
    }

    #[test]
    fn a_value_in_a_cdata_section_is_the_same_as_the_value_as_text() {
        // .NET writes a]]>b as two sections, split inside the ]]>.
        let dotnet = usage_record("d", "p", "f", "x<y&z]]]]><![CDATA[>w");
        let rust = usage_record("d", "p", "f", "v").replace("<![CDATA[v]]>", "x&lt;y&amp;z]]&gt;w");
        let differences = compare_case(
            &case("a", true),
            &with_usage("usage", &dotnet),
            &with_usage("usage", &rust),
        );
        assert!(differences.is_empty(), "{differences:?}");

        assert_eq!(
            cdata_as_text("<a><![CDATA[x<y]]></a><b><![CDATA[]]></b><c>&amp;</c>"),
            "<a>x&lt;y</a><b></b><c>&amp;</c>"
        );
        assert_eq!(cdata_as_text("<a><![CDATA[open"), "<a><![CDATA[open");
    }

    #[test]
    fn a_marked_value_still_differs_from_a_value_in_a_cdata_section() {
        let dotnet = usage_record("d", "p", "f", "x\\x0001")
            .replace("</Device>", "<BadSchema>true</BadSchema></Device>");
        let rust = usage_record("d", "p", "f", "v").replace(
            "<header Name=\"User-Agent\"><![CDATA[v]]>",
            "<header escaped=\"true\" Name=\"User-Agent\">x\\x0001",
        );
        let differences = compare_case(
            &case("a", true),
            &with_usage("usage", &dotnet),
            &with_usage("usage", &rust),
        );
        assert_eq!(differences.len(), 1, "{differences:?}");
        assert_eq!(differences[0].kind, Kind::Text);
        let shown = differences[0].rust.as_str().unwrap();
        assert!(shown.contains("<header escaped=\"true\""), "{shown}");
    }

    #[test]
    fn evidence_after_the_static_values_is_never_blanked() {
        // An evidence key with no prefix is written as an element of its own
        // name, so it can share a name with a static value.
        let record = |text: &str| {
            usage_record("d", "p", "f", "a")
                .replace("</Device>", &format!("<Version>{text}</Version></Device>"))
        };
        let differences = compare_case(
            &case("a", true),
            &with_usage("usage", &record("1")),
            &with_usage("usage", &record("2")),
        );
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].path, "usage");
    }

    #[test]
    fn the_custom_record_keeps_its_flow_element_names() {
        let dotnet = with_usage("usageCustom", &usage_record("d1", "p1", "Custom 1", "a"));
        let later = with_usage("usageCustom", &usage_record("d2", "p2", "Custom 1", "a"));
        let renamed = with_usage("usageCustom", &usage_record("d1", "p1", "Custom 2", "a"));
        assert!(compare_case(&case("a", true), &dotnet, &later).is_empty());
        let differences = compare_case(&case("a", true), &dotnet, &renamed);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].path, "usageCustom");
        assert_eq!(differences[0].kind, Kind::Text);
    }

    #[test]
    fn a_usage_record_on_one_side_only_is_a_presence_difference() {
        let rust = with_usage("usage", &usage_record("d", "p", "f", "a"));
        let differences = compare_case(&case("a", true), &line(true), &rust);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].path, "usage");
        assert_eq!(differences[0].kind, Kind::Presence);
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

    /// A case with no evidence, as a 51Did case is.
    fn data_case(id: &str) -> Case {
        Case {
            id: id.into(),
            about: String::new(),
            evidence: Map::new(),
        }
    }

    /// A line for a 51Did that parsed, as both producers write it.
    fn fodid_line() -> Value {
        json!({
            "id": "rust-probabilistic",
            "fodid": {
                "status": "Valid",
                "type": "Probabilistic",
                "usage": 3,
                "usageIsIndirect": false,
                "matchKey": "00112233",
                "signatureStatus": "SignatureValid"
            }
        })
    }

    /// A line for the 51Did metadata, with `count` properties.
    fn metadata_line(count: usize) -> Value {
        let properties: Vec<Value> = (0..count)
            .map(|i| json!({"name": format!("Id{i}"), "description": format!("About {i}")}))
            .collect();
        json!({
            "id": "metadata",
            "metadata": {"name": "FODid", "componentId": 255, "properties": properties}
        })
    }

    #[test]
    fn a_case_needs_no_evidence_and_may_carry_other_fields() {
        let case: Case = serde_json::from_value(json!({
            "id": "rust-empty", "about": "An empty string.",
            "form": "text", "input": "", "publicKey": null
        }))
        .unwrap();
        assert_eq!(case.id, "rust-empty");
        assert!(case.evidence.is_empty());
    }

    #[test]
    fn a_repeated_case_id_is_found() {
        let cases = [data_case("a"), data_case("b"), data_case("a")];
        assert_eq!(repeated_id(&cases), Some("a"));
        assert_eq!(repeated_id(&cases[..2]), None);
    }

    #[test]
    fn fodid_lines_that_agree_have_no_difference() {
        let mut dotnet = json!({"id": "rust-probabilistic", "fodid": {}});
        let rust = fodid_line();
        // .NET writing the keys in another order, and a number as a double.
        for (key, value) in rust["fodid"].as_object().unwrap().iter().rev() {
            dotnet["fodid"][key] = value.clone();
        }
        dotnet["fodid"]["usage"] = json!(3.0);
        let differences = compare_case(&data_case("rust-probabilistic"), &dotnet, &rust);
        assert!(differences.is_empty(), "{differences:?}");
    }

    #[test]
    fn a_changed_fodid_field_is_a_value_difference_at_its_path() {
        let mut rust = fodid_line();
        rust["fodid"]["matchKey"] = json!("00112234");
        let differences = compare_case(&data_case("a"), &fodid_line(), &rust);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].path, "fodid.matchKey");
        assert_eq!(differences[0].kind, Kind::Value);
        assert_eq!(differences[0].dotnet, json!("00112233"));
        assert_eq!(differences[0].rust, json!("00112234"));
    }

    #[test]
    fn a_fodid_field_only_one_side_writes_names_that_side() {
        let mut dotnet = fodid_line();
        dotnet["fodid"]
            .as_object_mut()
            .unwrap()
            .remove("usageIsIndirect");
        let mut rust = fodid_line();
        rust["fodid"]
            .as_object_mut()
            .unwrap()
            .remove("signatureStatus");
        let differences = compare_case(&data_case("a"), &dotnet, &rust);
        let found: Vec<(&str, Kind)> = differences
            .iter()
            .map(|d| (d.path.as_str(), d.kind))
            .collect();
        assert_eq!(
            found,
            [
                ("fodid.signatureStatus", Kind::OnlyInDotnet),
                ("fodid.usageIsIndirect", Kind::OnlyInRust),
            ]
        );
    }

    #[test]
    fn a_null_fodid_field_on_one_side_is_a_presence_difference() {
        let mut rust = fodid_line();
        rust["fodid"]["matchKey"] = Value::Null;
        let differences = compare_case(&data_case("a"), &fodid_line(), &rust);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].path, "fodid.matchKey");
        assert_eq!(differences[0].kind, Kind::Presence);
    }

    #[test]
    fn a_fodid_on_one_side_only_is_a_presence_difference() {
        let rust = json!({"id": "a"});
        let differences = compare_case(&data_case("a"), &fodid_line(), &rust);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].path, "fodid");
        assert_eq!(differences[0].kind, Kind::Presence);
    }

    #[test]
    fn metadata_lists_compare_item_by_item() {
        let mut rust = metadata_line(7);
        rust["metadata"]["properties"][2]["description"] = json!("Changed");
        let differences = compare_case(&data_case("metadata"), &metadata_line(6), &rust);
        let found: Vec<(&str, Kind)> = differences
            .iter()
            .map(|d| (d.path.as_str(), d.kind))
            .collect();
        assert_eq!(
            found,
            [
                ("metadata.properties[2].description", Kind::Value),
                ("metadata.properties[6]", Kind::OnlyInRust),
            ]
        );
    }

    /// A line for an engine's metadata, with one property and one component.
    fn engine_metadata_line() -> Value {
        json!({
            "id": "metadata-device",
            "engineMetadata": {
                "dataSourceTier": "Lite",
                "propertyNames": ["IsMobile"],
                "properties": {
                    "IsMobile": {
                        "type": "bool",
                        "description": "Mobile",
                        "values": [{"name": "True"}, {"name": "False"}]
                    }
                },
                "componentNames": ["HardwarePlatform"],
                "components": {"HardwarePlatform": {"id": 1, "properties": ["IsMobile"]}}
            }
        })
    }

    #[test]
    fn engine_metadata_compares_by_name_and_item() {
        let mut rust = engine_metadata_line();
        let metadata = &mut rust["engineMetadata"];
        metadata["properties"]["IsMobile"]["values"][1]["name"] = json!("Changed");
        metadata["properties"]["IsMobile"]["category"] = json!("Device");
        metadata["components"]["HardwarePlatform"]["id"] = json!(2);
        let differences = compare_case(
            &data_case("metadata-device"),
            &engine_metadata_line(),
            &rust,
        );
        let found: Vec<(&str, Kind)> = differences
            .iter()
            .map(|d| (d.path.as_str(), d.kind))
            .collect();
        assert_eq!(
            found,
            [
                (
                    "engineMetadata.properties.IsMobile.values[1].name",
                    Kind::Value
                ),
                (
                    "engineMetadata.properties.IsMobile.category",
                    Kind::OnlyInRust
                ),
                ("engineMetadata.components.HardwarePlatform.id", Kind::Value),
            ]
        );
    }

    /// The error .NET gave for an unknown usage sharing setting, cut to the
    /// lines that differ by design, with CRLF endings.
    const DOTNET_UNKNOWN_SETTING: &str = "Configuration failed to build.\r\n\
        BuilderName: ShareUsageBuilder\r\n\
        Builder 'FiftyOne.Pipeline.Engines.FiftyOne.FlowElements.ShareUsageBuilder' for \
        element 3 has no 'Set' methods or 'Build' methods that match (case-insensitively) \
        the following parameters: NOSUCHSETTING. The available configuration methods on \
        this builder are: \r\n\
        SetIncludedQueryStringParameters (RuntimeParameterInfo)\r\n\
        SetIncludedQueryStringParameters (RuntimeParameterInfo)\r\n\
        Build (RuntimeParameterInfo)\r\n\
        SetSharePercentage (RuntimeParameterInfo)\r\n\
        The available 'Build' methods have the following signatures: \r\n\
        datafile (String),createTempDataCopy (Boolean)\r\n\
        No Inner Exception.\r\n";

    /// The same error from the crates.
    const RUST_UNKNOWN_SETTING: &str = "Configuration failed to build.\n\
        BuilderName: ShareUsageBuilder\n\
        Builder 'fiftyone_pipeline_engines_fiftyone::configuration::ShareUsageBuilder' for \
        element 3 has no 'Set' methods or 'Build' methods that match (case-insensitively) \
        the following parameters: NOSUCHSETTING. The available configuration methods on \
        this builder are: \n\
        SetIncludedQueryStringParameters (List`1)\n\
        SetSharePercentage (Double)\n\
        The available 'Build' methods have the following signatures: \n\
        datafile (String),createTempDataCopy (Boolean)\n\
        No Inner Exception.\n";

    fn configuration_line(error: &str) -> Value {
        json!({"id": "configuration", "configuration": {"error": error}})
    }

    #[test]
    fn configuration_errors_differing_only_by_design_are_the_same() {
        let differences = compare_case(
            &data_case("configuration"),
            &configuration_line(DOTNET_UNKNOWN_SETTING),
            &configuration_line(RUST_UNKNOWN_SETTING),
        );
        assert!(differences.is_empty(), "{differences:?}");
        assert_eq!(
            normalise_configuration_error(RUST_UNKNOWN_SETTING)
                .lines()
                .nth(2)
                .unwrap(),
            "Builder '{type}' for element 3 has no 'Set' methods or 'Build' methods that \
             match (case-insensitively) the following parameters: NOSUCHSETTING. The \
             available configuration methods on this builder are: "
        );
        let dotnet = "Method 'SetSharePercentage' on builder 'FiftyOne.X.ShareUsageBuilder' \
                      for element 3 expects a parameter of type 'Double'.";
        let rust = "Method 'SetSharePercentage' on builder 'fiftyone::ShareUsageBuilder' \
                    for element 3 expects a parameter of type 'Double'.";
        assert_eq!(
            normalise_configuration_error(dotnet),
            normalise_configuration_error(rust)
        );
    }

    #[test]
    fn the_pipeline_builders_type_name_is_the_same_in_both() {
        let dotnet = "Method 'SetSuppressProcessException' on builder \
                      'FiftyOne.Pipeline.Engines.FiftyOne.FlowElements.FiftyOnePipelineBuilder' \
                      for pipeline expects a parameter of type 'Boolean'.";
        let rust = "Method 'SetSuppressProcessException' on builder \
                    'fiftyone_pipeline_engines_fiftyone::configuration::FiftyOnePipelineBuilder' \
                    for pipeline expects a parameter of type 'Boolean'.";
        assert_eq!(
            normalise_configuration_error(rust),
            "Method 'SetSuppressProcessException' on builder '{type}' for pipeline expects a \
             parameter of type 'Boolean'."
        );
        assert_eq!(
            normalise_configuration_error(dotnet),
            normalise_configuration_error(rust)
        );
        // Quoted text followed by neither ending is kept.
        let other = "Builder 'a' for something else, and builder 'b' for the pipeline";
        assert_eq!(normalise_configuration_error(other), other);
    }

    #[test]
    fn generic_builders_are_left_out_of_the_available_builders() {
        assert_eq!(
            normalise_configuration_error(
                "Could not find builder matching 'X' for element 2. Available builders are: \
                 A`2,SequenceElementBuilder,B`1,ShareUsageBuilder"
            ),
            "Could not find builder matching 'X' for element 2. Available builders are: \
             SequenceElementBuilder,ShareUsageBuilder"
        );
    }

    #[test]
    fn configuration_errors_differing_otherwise_are_a_value_difference() {
        for (from, to) in [
            ("element 3", "element 2"),
            ("NOSUCHSETTING", "NOSUCH"),
            (
                "SetSharePercentage (Double)",
                "SetSharePercentages (Double)",
            ),
            (
                "createTempDataCopy (Boolean)",
                "createTempDataCopy (String)",
            ),
            ("SetIncludedQueryStringParameters (List`1)\n", ""),
        ] {
            let rust = RUST_UNKNOWN_SETTING.replacen(from, to, 1);
            assert_ne!(rust, RUST_UNKNOWN_SETTING, "{from}");
            let differences = compare_case(
                &data_case("configuration"),
                &configuration_line(DOTNET_UNKNOWN_SETTING),
                &configuration_line(&rust),
            );
            let found: Vec<(&str, Kind)> = differences
                .iter()
                .map(|d| (d.path.as_str(), d.kind))
                .collect();
            assert_eq!(found, [("configuration.error", Kind::Value)], "{from}");
        }
    }

    #[test]
    fn a_changed_configuration_setting_is_a_difference_at_its_path() {
        let line = |share: f64| {
            json!({"id": "configuration", "configuration": {
                "error": null,
                "elements": [{"dataKey": "sequence"},
                    {"dataKey": "shareusage", "settings": {"sharePercentage": share}}]
            }})
        };
        assert!(compare_case(&data_case("configuration"), &line(0.5), &line(0.5)).is_empty());
        let differences = compare_case(&data_case("configuration"), &line(0.5), &line(1.0));
        let found: Vec<(&str, Kind)> = differences
            .iter()
            .map(|d| (d.path.as_str(), d.kind))
            .collect();
        assert_eq!(
            found,
            [(
                "configuration.elements[1].settings.sharePercentage",
                Kind::Value
            )]
        );
    }

    /// A line for a derived property case, as both producers write it.
    fn derived_line(value: Option<&str>, missing: &[&str]) -> Value {
        json!({
            "id": "derived-humanconfidence-01",
            "derived": {
                "script": "HumanConfidence",
                "sourceProperties": ["device.IsCrawler", "ip.ConnectionType"],
                "processError": null,
                "type": value.map(|_| "string"),
                "value": value,
                "noValue": value.is_none().then_some("No value."),
                "missing": missing
            }
        })
    }

    #[test]
    fn the_same_derived_answer_is_no_difference() {
        let case = data_case("derived-humanconfidence-01");
        let line = derived_line(Some("High"), &[]);
        assert!(compare_case(&case, &line, &line).is_empty());
        let line = derived_line(None, &["ip.ConnectionType"]);
        assert!(compare_case(&case, &line, &line).is_empty());
    }

    #[test]
    fn a_changed_derived_answer_is_named_by_its_field() {
        let case = data_case("derived-humanconfidence-01");
        let differences = compare_case(
            &case,
            &derived_line(Some("High"), &[]),
            &derived_line(Some("Low"), &[]),
        );
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].path, "derived.value");
        assert_eq!(differences[0].kind, Kind::Value);

        let differences = compare_case(
            &case,
            &derived_line(None, &["ip.ConnectionType"]),
            &derived_line(None, &["device.IsCrawler", "ip.ConnectionType"]),
        );
        let paths: Vec<&str> = differences.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(paths, ["derived.missing[0]", "derived.missing[1]"]);

        let mut rust = derived_line(Some("High"), &[]);
        rust["derived"]["sourceProperties"][1] = json!("ip.HumanProbability");
        let differences = compare_case(&case, &derived_line(Some("High"), &[]), &rust);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].path, "derived.sourceProperties[1]");
    }

    /// A robots.txt text, and the same text with its second line changed.
    const ROBOTS_TXT: &str = "User-Agent: *\nDisallow: /\n";
    const ROBOTS_TXT_CHANGED: &str = "User-Agent: *\nAllow: /\n";

    /// A line for a robots.txt case, as both producers write it.
    fn robots_txt_line(text: Option<&str>) -> Value {
        json!({
            "id": "robots-txt-none",
            "processError": null,
            "errors": [],
            "robotsTxtEngine": {
                "dataKey": "robotstxt",
                "dataSourceTier": "TAC",
                "evidenceKeys": ["query.robotstxt.search", "query.robotstxt.tdl"],
                "properties": [{"name": "AnnotatedText", "category": "Bots"}]
            },
            "plainText": text,
            "annotatedText": text.map(|text| format!("# Notes\n{text}"))
        })
    }

    #[test]
    fn the_same_robots_txt_answer_is_no_difference() {
        let case = data_case("robots-txt-none");
        let line = robots_txt_line(Some(ROBOTS_TXT));
        assert!(compare_case(&case, &line, &line).is_empty());
        let line = robots_txt_line(None);
        assert!(compare_case(&case, &line, &line).is_empty());
    }

    #[test]
    fn a_changed_robots_txt_answer_is_named_by_its_field_and_line() {
        let case = data_case("robots-txt-none");
        let differences = compare_case(
            &case,
            &robots_txt_line(Some(ROBOTS_TXT)),
            &robots_txt_line(Some(ROBOTS_TXT_CHANGED)),
        );
        let paths: Vec<(&str, Kind)> = differences
            .iter()
            .map(|d| (d.path.as_str(), d.kind))
            .collect();
        assert_eq!(
            paths,
            [("plainText", Kind::Text), ("annotatedText", Kind::Text)]
        );
        assert_eq!(differences[0].dotnet, json!("line 2: Disallow: /"));
        assert_eq!(differences[0].rust, json!("line 2: Allow: /"));
        assert_eq!(differences[1].dotnet, json!("line 3: Disallow: /"));

        let differences = compare_case(
            &case,
            &robots_txt_line(Some(ROBOTS_TXT)),
            &robots_txt_line(None),
        );
        assert_eq!(differences.len(), 2);
        assert!(differences.iter().all(|d| d.kind == Kind::Presence));

        let mut rust = robots_txt_line(None);
        rust["robotsTxtEngine"]["evidenceKeys"][0] = json!("query.robotstxt.training");
        let differences = compare_case(&case, &robots_txt_line(None), &rust);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].path, "robotsTxtEngine.evidenceKeys[0]");
    }

    /// A line for a TAC and Native case, as both producers write it.
    fn property_keyed_line(ids: Option<Vec<u64>>) -> Value {
        json!({
            "id": "property-keyed-tac-value-000001",
            "processError": null,
            "errors": [],
            "warnings": [],
            "profileIds": ids,
            "properties": {"device": {
                "profileid": {"name": "ProfileId", "type": "string", "value": "17"}
            }},
            "hardwareJson": {"profiles": []},
            "propertyKeyedEngines": null,
            "indexedValues": {"TAC": {"00000000": 7}}
        })
    }

    #[test]
    fn a_changed_property_keyed_answer_is_named_by_its_field() {
        let case = data_case("property-keyed-tac-value-000001");
        let line = property_keyed_line(Some(vec![17, 18]));
        assert!(compare_case(&case, &line, &line).is_empty());

        let differences = compare_case(&case, &line, &property_keyed_line(Some(vec![18, 17])));
        let paths: Vec<&str> = differences.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(paths, ["profileIds[0]", "profileIds[1]"]);

        let differences = compare_case(&case, &line, &property_keyed_line(None));
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].path, "profileIds");
        assert_eq!(differences[0].kind, Kind::Presence);

        let mut rust = line.clone();
        rust["properties"]["device"]["profileid"]["value"] = json!("18");
        rust["indexedValues"]["TAC"]["00000001"] = json!(1);
        rust["warnings"] = json!([{"element": "hardware", "message": "The warning."}]);
        rust["hardwareJson"]["profiles"] = json!([{"profileid": "17"}]);
        let differences = compare_case(&case, &line, &rust);
        let paths: Vec<(&str, Kind)> = differences
            .iter()
            .map(|d| (d.path.as_str(), d.kind))
            .collect();
        assert_eq!(
            paths,
            [
                ("properties.device.profileid", Kind::Value),
                ("indexedValues.TAC.00000001", Kind::OnlyInRust),
                ("warnings[0]", Kind::OnlyInRust),
                ("hardwareJson.profiles[0]", Kind::OnlyInRust),
            ]
        );
    }

    #[test]
    fn an_entry_can_allow_one_metadata_field_in_every_property() {
        let mut rust = metadata_line(6);
        rust["metadata"]["properties"][4]["description"] = json!("Changed");
        rust["metadata"]["componentId"] = json!(0);
        let differences = compare_case(&data_case("metadata"), &metadata_line(6), &rust);
        let allowed = entries(json!([{
            "path": "metadata.properties[*].description", "kind": "value",
            "reason": "test"
        }]));
        let outcome = check(&differences, &allowed, "lite");
        assert_eq!(outcome.allowed_counts[&0], 1);
        assert_eq!(outcome.failures.len(), 1);
        assert_eq!(outcome.failures[0].path, "metadata.componentId");
    }

    #[test]
    fn a_side_pattern_allows_a_line_wherever_it_falls() {
        let differences = compare_case(
            &data_case("robots-txt-none"),
            &robots_txt_line(Some(ROBOTS_TXT)),
            &robots_txt_line(Some(ROBOTS_TXT_CHANGED)),
        );
        let allowed = entries(json!([{
            "path": "*Text", "kind": "text",
            "dotnet": {"pattern": ["line *: Disallow: /"]},
            "rust": {"pattern": ["line *: Allow: /"]},
            "reason": "test"
        }]));
        assert!(allowed[0].problem().is_none());
        let outcome = check(&differences, &allowed, "lite");
        assert!(outcome.failures.is_empty());
        assert_eq!(outcome.allowed_counts[&0], 2);

        let allowed = entries(json!([{
            "path": "*Text", "kind": "text",
            "dotnet": {"pattern": ["line *: Disallow: /"]},
            "rust": {"pattern": ["line 1: *", "line *: Allow: /private"]},
            "reason": "test"
        }]));
        assert_eq!(check(&differences, &allowed, "lite").failures.len(), 2);
    }

    #[test]
    fn a_side_pattern_matches_only_text() {
        let differences = compare_case(
            &data_case("robots-txt-none"),
            &robots_txt_line(Some(ROBOTS_TXT)),
            &robots_txt_line(None),
        );
        let allowed = entries(json!([{
            "path": "*Text", "rust": {"pattern": ["*"]}, "reason": "test"
        }]));
        assert_eq!(check(&differences, &allowed, "lite").failures.len(), 2);
    }
}
