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

//! Turning the text of a script into a value tree.
//!
//! YAML and JSON both arrive here and both leave as the same tree, which is
//! what makes the two forms of a script interchangeable. JSON is read as the
//! JSON subset of YAML rather than by a second parser, which is what format 1
//! asks for, so text a strict JSON reader would refuse (a trailing comma, for
//! example) may be read here. Plain JSON raises no such question.

use serde_norway::{Mapping, Value};

/// The text of a script could not be read at all.
pub(crate) struct ParseFault {
    /// What is wrong, in plain words.
    pub message: String,
    /// The one-based line, where the parser supplies one.
    pub line: Option<usize>,
}

/// Read the text of a script.
pub(crate) fn parse(text: &str) -> Result<Value, ParseFault> {
    // A script that opens with a brace is JSON, which only changes the wording
    // of a fault because one parser reads both forms.
    let is_json = text.trim_start().starts_with('{');
    let document: Value = match serde_norway::from_str(text) {
        Ok(value) => value,
        Err(error) => {
            let form = if is_json { "JSON" } else { "YAML" };
            return Err(ParseFault {
                message: format!("the text is not valid {form}: {error}"),
                line: error.location().map(|location| location.line()),
            });
        }
    };
    require_mapping(&document)?;
    require_distinct_keys(&document)?;
    Ok(document)
}

fn require_mapping(document: &Value) -> Result<(), ParseFault> {
    match document {
        Value::Null => Err(ParseFault {
            message: "the script is empty".to_owned(),
            line: None,
        }),
        Value::Mapping(_) => Ok(()),
        _ => Err(ParseFault {
            message: "the script must be a mapping of keys to values at the top level".to_owned(),
            line: None,
        }),
    }
}

/// Every key in the format is matched without regard to case, so two keys in
/// one mapping that differ only in case are one key written twice. A reader
/// that then matched without regard to case would quietly take one and drop
/// the other, which is the failure this rule exists to stop, so both are
/// refused.
///
/// An exact duplicate is refused by the YAML reader itself before the tree
/// reaches here.
fn require_distinct_keys(node: &Value) -> Result<(), ParseFault> {
    match node {
        Value::Sequence(items) => {
            for item in items {
                require_distinct_keys(item)?;
            }
            Ok(())
        }
        Value::Mapping(mapping) => {
            let mut seen: Vec<(String, String)> = Vec::new();
            for (key, value) in mapping {
                let written = key_text(key);
                let folded = written.to_lowercase();
                if let Some((_, first)) = seen.iter().find(|(lower, _)| *lower == folded) {
                    return Err(ParseFault {
                        message: format!(
                            "the keys '{first}' and '{written}' differ only in case, and keys \
                             are matched without regard to case, so one of the two would be \
                             dropped"
                        ),
                        line: None,
                    });
                }
                seen.push((folded, written));
                require_distinct_keys(value)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// The text form of a mapping key, which is how a key is written in a fault
/// message and how two keys are compared.
pub(crate) fn key_text(key: &Value) -> String {
    match key {
        Value::String(text) => text.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(number) => number.to_string(),
        Value::Null => "null".to_owned(),
        _ => "(a key that is not a scalar)".to_owned(),
    }
}

/// Read a key from a mapping without regard to case.
pub(crate) fn get_ci<'a>(mapping: &'a Mapping, name: &str) -> Option<&'a Value> {
    let wanted = name.to_lowercase();
    mapping
        .iter()
        .find(|(key, _)| key_text(key).to_lowercase() == wanted)
        .map(|(_, value)| value)
}

/// Whether a mapping carries a key, without regard to case.
pub(crate) fn has_ci(mapping: &Mapping, name: &str) -> bool {
    get_ci(mapping, name).is_some()
}

/// The keys of a mapping as they were written, in the order they were written.
pub(crate) fn keys_of(mapping: &Mapping) -> Vec<String> {
    mapping.keys().map(key_text).collect()
}

/// The keys of a mapping that are not in the allowed list, matched without
/// regard to case.
pub(crate) fn unknown_keys(mapping: &Mapping, allowed: &[&str]) -> Vec<String> {
    keys_of(mapping)
        .into_iter()
        .filter(|key| {
            let folded = key.to_lowercase();
            !allowed.iter().any(|name| name.to_lowercase() == folded)
        })
        .collect()
}

/// How a fault message names the kind of thing it found.
pub(crate) fn type_of(value: Option<&Value>) -> String {
    match value {
        None => "nothing".to_owned(),
        Some(Value::Null) => "a null literal".to_owned(),
        Some(Value::Sequence(_)) => "a list".to_owned(),
        Some(Value::Bool(_)) => "a boolean".to_owned(),
        Some(Value::Number(number)) => {
            let whole = number
                .as_f64()
                .map(|value| value.is_finite() && value.fract() == 0.0)
                .unwrap_or(false);
            if whole {
                "an integer".to_owned()
            } else {
                "a number".to_owned()
            }
        }
        Some(Value::String(text)) => format!("a string ({})", quote(text)),
        Some(_) => "a mapping".to_owned(),
    }
}

/// A value written the way JSON would write it, for the fault messages that
/// quote what they found.
pub(crate) fn json_like(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => quote(text),
        Value::Sequence(items) => {
            let members: Vec<String> = items.iter().map(json_like).collect();
            format!("[{}]", members.join(","))
        }
        Value::Mapping(mapping) => {
            let members: Vec<String> = mapping
                .iter()
                .map(|(key, value)| format!("{}:{}", quote(&key_text(key)), json_like(value)))
                .collect();
            format!("{{{}}}", members.join(","))
        }
        // A tagged value, such as `!Something value`, has no JSON form. It is
        // never valid in a script, and naming the tag is what an author needs.
        Value::Tagged(tagged) => format!("{} {}", tagged.tag, json_like(&tagged.value)),
    }
}

fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            _ => out.push(character),
        }
    }
    out.push('"');
    out
}
