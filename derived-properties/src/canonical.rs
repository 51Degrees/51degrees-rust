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

//! The canonical JSON form of a script, which the element writes to its debug
//! log at build so that anyone holding the log can see exactly what was
//! evaluated without the script file.
//!
//! The text matches `tools/canonical.mjs` in 51Degrees/derived-properties
//! character for character. Keys are PascalCase in the order of the format
//! reference, the indent is two spaces, there is no trailing newline, literals
//! keep their types, and the inferred type of each source property and the
//! computed `Dependencies` are included. A YAML script and the JSON script that
//! mirrors it print the same text.

use std::fmt::Write;

use crate::model::{Condition, Literal, Operand, Output, Script};

/// The only script format this crate reads, which validation has checked.
pub(crate) const FORMAT: i64 = 1;

impl Script {
    /// The script as canonical JSON, without a trailing newline.
    ///
    /// This is the text the element writes to its debug log at build. It
    /// matches the reference printer in 51Degrees/derived-properties character
    /// for character, so the same script gives the same text in every
    /// language.
    pub fn to_canonical_json(&self) -> String {
        let mut text = String::new();
        print(&build(self), &mut text, 0);
        text
    }
}

/// The canonical form as a tree, before it is printed.
enum Node {
    Object(Vec<(&'static str, Node)>),
    /// An object whose keys come from the script, such as check names.
    Named(Vec<(String, Node)>),
    Array(Vec<Node>),
    Text(String),
    Bool(bool),
    Whole(i64),
    Number(f64),
}

fn text(value: &str) -> Node {
    Node::Text(value.to_owned())
}

fn texts(values: &[String]) -> Node {
    Node::Array(values.iter().map(|value| text(value)).collect())
}

fn build(script: &Script) -> Node {
    let mut result = vec![
        ("Format", Node::Whole(FORMAT)),
        ("Name", text(&script.name)),
        ("Version", text(&script.version)),
    ];
    if script.deprecated {
        result.push(("Deprecated", Node::Bool(true)));
        result.push((
            "DeprecationNote",
            text(script.deprecation_note.as_deref().unwrap_or_default()),
        ));
    }
    result.push(("Output", output(&script.output, script.vendor_ids_written)));

    // The inferred type of every source property, which the script does not
    // carry because it is worked out from the literals the property is
    // compared against.
    result.push((
        "Properties",
        Node::Named(
            script
                .properties
                .iter()
                .map(|property| {
                    (
                        property.name.clone(),
                        Node::Object(vec![("Type", text(property.value_type.as_str()))]),
                    )
                })
                .collect(),
        ),
    ));

    if !script.checks.is_empty() {
        result.push((
            "Checks",
            Node::Named(
                script
                    .checks
                    .iter()
                    .map(|check| (check.name.clone(), condition(&check.condition, script)))
                    .collect(),
            ),
        ));
    }

    result.push((
        "Rules",
        Node::Array(
            script
                .rules
                .iter()
                .map(|rule| match &rule.when {
                    None => Node::Object(vec![("Else", literal(&rule.value))]),
                    Some(when) => Node::Object(vec![
                        ("When", condition(when, script)),
                        ("Then", literal(&rule.value)),
                    ]),
                })
                .collect(),
        ),
    ));

    Node::Object(result)
}

/// The output block in the order of the format reference, leaving out every
/// field the script did not give and printing `Values` last.
fn output(output: &Output, vendor_ids_written: bool) -> Node {
    let mut result = vec![
        ("Name", text(&output.name)),
        ("Description", text(&output.description)),
        ("ValueType", text(output.value_type.as_str())),
    ];
    let mut add = |name: &'static str, value: Option<Node>| {
        if let Some(value) = value {
            result.push((name, value));
        }
    };
    add(
        "StoredValueType",
        output.stored_value_type.as_deref().map(text),
    );
    add("DefaultValue", output.default_value.as_deref().map(text));
    add("IsList", Some(Node::Bool(output.is_list)));
    add("IsMandatory", output.is_mandatory.map(Node::Bool));
    add("IsObsolete", output.is_obsolete.map(Node::Bool));
    add("Category", output.category.as_deref().map(text));
    add("IsPopular", output.is_popular.map(Node::Bool));
    add("ExportValues", output.export_values.map(Node::Bool));
    add("Url", output.url.as_deref().map(text));
    add("DisplayOrder", output.display_order.map(Node::Whole));
    add("PropertyId", output.property_id.map(Node::Whole));
    // A script that writes an empty list prints it, so the list is printed
    // where the script wrote it rather than where it has members.
    add(
        "VendorIds",
        vendor_ids_written.then(|| texts(&output.vendor_ids)),
    );
    add("Dependencies", Some(texts(&output.dependencies)));
    add(
        "Values",
        output.values.as_ref().map(|values| {
            Node::Array(
                values
                    .iter()
                    .map(|value| {
                        let mut entry = vec![("Name", text(&value.name))];
                        if let Some(description) = &value.description {
                            entry.push(("Description", text(description)));
                        }
                        Node::Object(entry)
                    })
                    .collect(),
            )
        }),
    );
    Node::Object(result)
}

/// A condition, printed back in the shape the script wrote it.
fn condition(value: &Condition, script: &Script) -> Node {
    match value {
        Condition::Compare {
            property,
            op,
            operand,
            ..
        } => Node::Object(vec![
            ("Property", text(&script.properties[*property].name)),
            (
                op.as_str(),
                match operand {
                    Operand::One(value) => literal(value),
                    Operand::Many(values) => Node::Array(values.iter().map(literal).collect()),
                },
            ),
        ]),
        Condition::Check(index) => Node::Object(vec![("Check", text(&script.checks[*index].name))]),
        Condition::Aggregate {
            aggregate,
            group,
            op,
            operand,
        } => Node::Object(vec![
            // A group is the word Checks where it covers every check, and the
            // list of check names otherwise.
            (
                aggregate.as_str(),
                match group {
                    None => text("Checks"),
                    Some(indexes) => Node::Array(
                        indexes
                            .iter()
                            .map(|index| text(&script.checks[*index].name))
                            .collect(),
                    ),
                },
            ),
            (op.as_str(), Node::Whole(*operand)),
        ]),
        Condition::All(items) => Node::Object(vec![("All", conditions(items, script))]),
        Condition::Any(items) => Node::Object(vec![("Any", conditions(items, script))]),
        Condition::Not(item) => Node::Object(vec![("Not", condition(item, script))]),
    }
}

fn conditions(items: &[Condition], script: &Script) -> Node {
    Node::Array(items.iter().map(|item| condition(item, script)).collect())
}

fn literal(value: &Literal) -> Node {
    match value {
        Literal::Bool(value) => Node::Bool(*value),
        Literal::Int(value) => Node::Whole(i64::from(*value)),
        Literal::Double(value) => Node::Number(*value),
        Literal::Text(value) => text(value),
    }
}

// -------------------------------------------------------------------------
// Printing, following JSON.stringify(value, null, 2).
// -------------------------------------------------------------------------

fn print(node: &Node, text: &mut String, depth: usize) {
    match node {
        Node::Object(members) => print_members(
            members.iter().map(|(key, value)| (*key, value)),
            members.len(),
            text,
            depth,
        ),
        Node::Named(members) => print_members(
            members.iter().map(|(key, value)| (key.as_str(), value)),
            members.len(),
            text,
            depth,
        ),
        Node::Array(items) => {
            if items.is_empty() {
                text.push_str("[]");
                return;
            }
            text.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    text.push(',');
                }
                new_line(text, depth + 1);
                print(item, text, depth + 1);
            }
            new_line(text, depth);
            text.push(']');
        }
        Node::Text(value) => print_text(value, text),
        Node::Bool(value) => text.push_str(if *value { "true" } else { "false" }),
        Node::Whole(value) => {
            let _ = write!(text, "{value}");
        }
        Node::Number(value) => text.push_str(&number(*value)),
    }
}

fn print_members<'a>(
    members: impl Iterator<Item = (&'a str, &'a Node)>,
    count: usize,
    text: &mut String,
    depth: usize,
) {
    if count == 0 {
        text.push_str("{}");
        return;
    }
    text.push('{');
    for (index, (key, value)) in members.enumerate() {
        if index > 0 {
            text.push(',');
        }
        new_line(text, depth + 1);
        print_text(key, text);
        text.push_str(": ");
        print(value, text, depth + 1);
    }
    new_line(text, depth);
    text.push('}');
}

fn new_line(text: &mut String, depth: usize) {
    text.push('\n');
    for _ in 0..depth {
        text.push_str("  ");
    }
}

/// Writes text the way JSON requires. Characters outside ASCII are not
/// escaped, because JSON.stringify does not escape them either.
fn print_text(value: &str, text: &mut String) {
    text.push('"');
    for character in value.chars() {
        match character {
            '"' => text.push_str("\\\""),
            '\\' => text.push_str("\\\\"),
            '\u{8}' => text.push_str("\\b"),
            '\u{c}' => text.push_str("\\f"),
            '\n' => text.push_str("\\n"),
            '\r' => text.push_str("\\r"),
            '\t' => text.push_str("\\t"),
            control if control < ' ' => {
                let _ = write!(text, "\\u{:04x}", u32::from(control));
            }
            other => text.push(other),
        }
    }
    text.push('"');
}

/// Writes a number the way JavaScript writes one, so a whole valued double
/// prints with no decimal point and 2.0 prints as 2.
///
/// The rule is the one in the JavaScript language standard (Number::toString),
/// where a number is written in plain form while its decimal exponent sits
/// between -6 and 21 and in exponent form outside that range, always with the
/// fewest digits that read back as the same number.
fn number(value: f64) -> String {
    // JSON has no way to write these, and JSON.stringify writes null.
    if !value.is_finite() {
        return "null".to_owned();
    }
    // JavaScript writes minus zero as zero.
    if value == 0.0 {
        return "0".to_owned();
    }

    // The shortest digits that read back as the value, in exponent form such
    // as 1.25e5.
    let shortest = format!("{:e}", value.abs());
    let (mantissa, exponent) = shortest
        .split_once('e')
        .expect("exponent formatting always writes an exponent");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    // The value is 0.digits times ten to the power `point`.
    let point = exponent
        .parse::<i32>()
        .expect("exponent formatting writes a whole exponent")
        + 1;
    let count = digits.len() as i32;

    let mut text = String::new();
    if value < 0.0 {
        text.push('-');
    }
    if count <= point && point <= 21 {
        text.push_str(&digits);
        text.push_str(&"0".repeat((point - count) as usize));
    } else if 0 < point && point <= 21 {
        let (whole, fraction) = digits.split_at(point as usize);
        text.push_str(whole);
        text.push('.');
        text.push_str(fraction);
    } else if -6 < point && point <= 0 {
        text.push_str("0.");
        text.push_str(&"0".repeat((-point) as usize));
        text.push_str(&digits);
    } else {
        let (first, rest) = digits.split_at(1);
        text.push_str(first);
        if !rest.is_empty() {
            text.push('.');
            text.push_str(rest);
        }
        let _ = write!(
            text,
            "e{}{}",
            if point > 0 { '+' } else { '-' },
            (point - 1).abs()
        );
    }
    text
}

#[cfg(test)]
mod tests {
    use super::number;

    /// Each value against the text `String(value)` gives in JavaScript.
    #[test]
    fn numbers_print_as_javascript_prints_them() {
        for (value, expected) in [
            (2.0, "2"),
            (-2.0, "-2"),
            (0.25, "0.25"),
            (-0.0, "0"),
            (0.1, "0.1"),
            (1.5e-7, "1.5e-7"),
            (1e-7, "1e-7"),
            (0.000001, "0.000001"),
            (0.0000012, "0.0000012"),
            (123456.789, "123456.789"),
            (1e21, "1e+21"),
            (1.5e21, "1.5e+21"),
            (1e20, "100000000000000000000"),
            (123456789012345680000.0, "123456789012345680000"),
            (3000000000.0, "3000000000"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (5e-324, "5e-324"),
            (f64::NAN, "null"),
            (f64::INFINITY, "null"),
        ] {
            assert_eq!(number(value), expected, "{value:e}");
        }
    }
}
