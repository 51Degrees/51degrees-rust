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

//! Checking a script against format 1 and turning it into the model.
//!
//! Every fault is collected rather than stopping at the first, so an author
//! sees everything wrong with a file in one go.

use serde_norway::{Mapping, Value};

use crate::fault::{Fault, Faults};
use crate::model::{
    format_number, Aggregate, Check, CompareOp, Condition, Literal, Operand, Output, OutputValue,
    Rule, Script, SourceProperty, ValueType, INT_MAX, INT_MIN,
};
use crate::parse::{get_ci, has_ci, json_like, key_text, keys_of, parse, type_of, unknown_keys};

/// The keys format 1 knows at the top level.
const TOP_LEVEL_KEYS: &[&str] = &[
    "Format",
    "Name",
    "Version",
    "Deprecated",
    "DeprecationNote",
    "Output",
    "Checks",
    "Rules",
];

/// The `Output` keys, in the order the format prints them. The set is the one
/// 51Degrees uses for the metadata of every property, so an `Output` block is
/// a complete property definition rather than a cut-down one.
const OUTPUT_KEYS: &[&str] = &[
    "Name",
    "Description",
    "ValueType",
    "StoredValueType",
    "DefaultValue",
    "IsList",
    "IsMandatory",
    "IsObsolete",
    "Category",
    "IsPopular",
    "ExportValues",
    "Url",
    "DisplayOrder",
    "PropertyId",
    "VendorIds",
    "Dependencies",
    "Values",
];

const VALUE_TYPE_NAMES: &str = "string, bool, int, double";
const AGGREGATE_OPERATORS: &str = "Eq, Ne, Gt, Ge, Lt, Le";
const IDENTIFIER_PATTERN: &str = "^[A-Za-z][A-Za-z0-9]*$";

/// Read the text of a script and check it against format 1.
///
/// `name` is the file name without its extension, which the script's `Name`
/// must equal. `source` says where the script came from, for the faults.
pub(crate) fn validate_text(
    text: &str,
    name: Option<&str>,
    source: &str,
) -> Result<Script, Faults> {
    let document = match parse(text) {
        Ok(document) => document,
        Err(fault) => {
            return Err(Faults::new(vec![Fault {
                script: name.map(str::to_owned),
                source: source.to_owned(),
                path: String::new(),
                line: fault.line,
                message: fault.message,
            }]));
        }
    };
    let mut validator = Validator::new(name, source);
    let script = validator.build(&document);
    if validator.faults.is_empty() {
        match script {
            Some(script) => Ok(script),
            // Only reachable if a required field was missing without a fault
            // being recorded, which would be a defect in this file rather than
            // in the script being read. Saying so beats a panic in a library.
            None => Err(Faults::new(vec![Fault {
                script: name.map(str::to_owned),
                source: source.to_owned(),
                path: String::new(),
                line: None,
                message: "the script could not be built and no fault was recorded".to_owned(),
            }])),
        }
    } else {
        Err(Faults::new(validator.faults))
    }
}

struct Validator<'a> {
    script: Option<&'a str>,
    source: &'a str,
    faults: Vec<Fault>,
    properties: Vec<SourceProperty>,
    /// Where each property's type was first inferred, so a conflict can name
    /// both places. Runs in step with `properties`.
    inferred_at: Vec<String>,
}

impl<'a> Validator<'a> {
    fn new(script: Option<&'a str>, source: &'a str) -> Self {
        Validator {
            script,
            source,
            faults: Vec::new(),
            properties: Vec::new(),
            inferred_at: Vec::new(),
        }
    }

    fn fault(&mut self, path: &str, message: impl Into<String>) {
        self.faults.push(Fault {
            script: self.script.map(str::to_owned),
            source: self.source.to_owned(),
            path: path.to_owned(),
            line: None,
            message: message.into(),
        });
    }

    fn build(&mut self, document: &Value) -> Option<Script> {
        let root = document.as_mapping()?;

        for key in unknown_keys(root, TOP_LEVEL_KEYS) {
            self.fault(
                &key,
                format!(
                    "unknown key '{key}' at the top level. Expected one of {}",
                    TOP_LEVEL_KEYS.join(", ")
                ),
            );
        }

        self.read_format(root);
        let name = self.read_name(root);
        let version = self.read_version(root);
        let (deprecated, deprecation_note) = self.read_deprecation(root);
        let mut output = self.read_output(root);

        // Checks are read before the rules so that a rule can reference one,
        // and so that a property is first named where the script first names
        // it.
        let check_names = self.read_check_names(root);
        let checks = self.read_checks(root, &check_names);
        let rules = self.read_rules(root, &check_names, &output);

        if output.dependencies.is_none() {
            output.dependencies = Some(
                self.properties
                    .iter()
                    .map(|property| property.name.clone())
                    .collect(),
            );
        }

        let vendor_ids_written = output.vendor_ids.is_some();
        Some(Script {
            name: name?,
            version: version?,
            deprecated,
            deprecation_note,
            source: self.source.to_owned(),
            output: output.finish()?,
            vendor_ids_written,
            properties: std::mem::take(&mut self.properties),
            checks,
            rules,
        })
    }

    // -----------------------------------------------------------------
    // The top level keys.
    // -----------------------------------------------------------------

    fn read_format(&mut self, root: &Mapping) {
        match get_ci(root, "Format") {
            None => self.fault("Format", "required key 'Format' is missing"),
            Some(value) => {
                let is_one = value
                    .as_i64()
                    .map(|number| number == 1)
                    .unwrap_or(false)
                    // A YAML reader may hand back 1.0 for a value written as
                    // `1.0`, which is the same number, so the value decides
                    // rather than the way it was written.
                    || value.as_f64().map(|number| number == 1.0).unwrap_or(false);
                if !is_one {
                    self.fault(
                        "Format",
                        format!("Format must be 1, found {}", json_like(value)),
                    );
                }
            }
        }
    }

    fn read_name(&mut self, root: &Mapping) -> Option<String> {
        let value = match get_ci(root, "Name") {
            None => {
                self.fault("Name", "required key 'Name' is missing");
                return None;
            }
            Some(value) => value,
        };
        let text = match value.as_str() {
            None => {
                self.fault(
                    "Name",
                    format!("Name expected a string, found {}", type_of(Some(value))),
                );
                return None;
            }
            Some(text) => text,
        };
        if !is_identifier(text) {
            self.fault(
                "Name",
                format!("script name '{text}' does not match the pattern {IDENTIFIER_PATTERN}"),
            );
            return Some(text.to_owned());
        }
        if let Some(file_name) = self.script {
            if file_name != text {
                self.fault(
                    "Name",
                    format!("script name '{text}' must equal the file name '{file_name}'"),
                );
            }
        }
        Some(text.to_owned())
    }

    fn read_version(&mut self, root: &Mapping) -> Option<String> {
        let value = match get_ci(root, "Version") {
            None => {
                self.fault("Version", "required key 'Version' is missing");
                return None;
            }
            Some(value) => value,
        };
        match value.as_str() {
            Some(text) if is_semantic_version(text) => Some(text.to_owned()),
            _ => {
                self.fault(
                    "Version",
                    format!(
                        "Version expected a semantic version such as 1.0.0, found {}",
                        json_like(value)
                    ),
                );
                None
            }
        }
    }

    fn read_deprecation(&mut self, root: &Mapping) -> (bool, Option<String>) {
        let mut deprecated = false;
        if let Some(value) = get_ci(root, "Deprecated") {
            match value.as_bool() {
                Some(flag) => deprecated = flag,
                None => self.fault(
                    "Deprecated",
                    format!(
                        "Deprecated expected a boolean, found {}",
                        type_of(Some(value))
                    ),
                ),
            }
        }
        let mut note = None;
        if let Some(value) = get_ci(root, "DeprecationNote") {
            match value.as_str() {
                Some(text) => note = Some(text.to_owned()),
                None => self.fault(
                    "DeprecationNote",
                    format!(
                        "DeprecationNote expected a string, found {}",
                        type_of(Some(value))
                    ),
                ),
            }
        }
        // An empty note says nothing, so it counts as no note, which is what
        // the reference implementation's truthiness test does.
        let has_note = note
            .as_deref()
            .map(|text| !text.is_empty())
            .unwrap_or(false);
        if deprecated && !has_note {
            self.fault(
                "DeprecationNote",
                "a deprecated script must say what to use instead in DeprecationNote",
            );
        }
        if !deprecated && has_note {
            self.fault(
                "DeprecationNote",
                "DeprecationNote is only allowed when Deprecated is true",
            );
        }
        (deprecated, note)
    }

    // -----------------------------------------------------------------
    // Output, the property definition.
    // -----------------------------------------------------------------

    fn read_output(&mut self, root: &Mapping) -> DraftOutput {
        let mut draft = DraftOutput::default();
        let raw = match get_ci(root, "Output") {
            None => {
                self.fault("Output", "required key 'Output' is missing");
                return draft;
            }
            Some(value) => value,
        };
        let raw = match raw.as_mapping() {
            None => {
                self.fault(
                    "Output",
                    format!("Output expected a mapping, found {}", type_of(Some(raw))),
                );
                return draft;
            }
            Some(mapping) => mapping,
        };

        for key in unknown_keys(raw, OUTPUT_KEYS) {
            self.fault(
                &format!("Output.{key}"),
                format!(
                    "unknown key '{key}' under Output. A typo in a metadata field is a fault \
                     rather than a value that is quietly dropped"
                ),
            );
        }

        match get_ci(raw, "Name") {
            None => self.fault("Output.Name", "required key 'Name' is missing"),
            Some(value) => match value.as_str() {
                Some(text) if is_identifier(text) => draft.name = Some(text.to_owned()),
                _ => self.fault(
                    "Output.Name",
                    format!(
                        "Output.Name {} does not match the pattern {IDENTIFIER_PATTERN}",
                        json_like(value)
                    ),
                ),
            },
        }
        if draft.name.is_none() {
            draft.name = Some(self.script.unwrap_or("unknown").to_owned());
        }

        match get_ci(raw, "Description") {
            None => self.fault(
                "Output.Description",
                "required key 'Description' is missing. Say what the property asserts, not how \
                 far to trust it",
            ),
            Some(value) => match value.as_str() {
                Some(text) if !text.trim().is_empty() => {
                    draft.description = Some(text.to_owned());
                }
                _ => self.fault(
                    "Output.Description",
                    format!(
                        "Output.Description expected a non empty string, found {}",
                        type_of(Some(value))
                    ),
                ),
            },
        }

        match get_ci(raw, "ValueType") {
            None => self.fault("Output.ValueType", "required key 'ValueType' is missing"),
            Some(value) => match value.as_str().and_then(value_type_named) {
                Some(value_type) => draft.value_type = Some(value_type),
                None => self.fault(
                    "Output.ValueType",
                    format!(
                        "Output.ValueType '{}' is not allowed in format 1. Expected one of \
                         {VALUE_TYPE_NAMES}",
                        scalar_text(value)
                    ),
                ),
            },
        }

        match get_ci(raw, "IsList") {
            None => self.fault("Output.IsList", "required key 'IsList' is missing"),
            Some(value) => match value.as_bool() {
                None => self.fault(
                    "Output.IsList",
                    format!(
                        "Output.IsList expected a boolean, found {}",
                        type_of(Some(value))
                    ),
                ),
                Some(true) => self.fault(
                    "Output.IsList",
                    "Output.IsList must be false in format 1. List outputs are deferred",
                ),
                Some(false) => {}
            },
        }

        self.read_output_values(raw, &mut draft);
        self.read_output_extras(raw, &mut draft);
        self.read_default_value(raw, &mut draft);
        draft
    }

    fn read_output_values(&mut self, raw: &Mapping, draft: &mut DraftOutput) {
        let values = match get_ci(raw, "Values") {
            None => return,
            Some(value) => value,
        };
        let items = match values.as_sequence() {
            None => {
                self.fault(
                    "Output.Values",
                    format!(
                        "Output.Values expected a list, found {}",
                        type_of(Some(values))
                    ),
                );
                return;
            }
            Some(items) => items,
        };
        if let Some(value_type) = draft.value_type {
            if value_type != ValueType::String && value_type != ValueType::Int {
                self.fault(
                    "Output.Values",
                    format!(
                        "Output.Values is only allowed where ValueType is string or int, not \
                         {value_type}"
                    ),
                );
                return;
            }
        }
        let mut list: Vec<OutputValue> = Vec::new();
        for (index, entry) in items.iter().enumerate() {
            let path = format!("Output.Values[{index}]");
            let entry = match entry.as_mapping() {
                None => {
                    self.fault(
                        &path,
                        format!(
                            "a value expected a mapping of Name and Description, found {}",
                            type_of(Some(entry))
                        ),
                    );
                    continue;
                }
                Some(mapping) => mapping,
            };
            for key in unknown_keys(entry, &["Name", "Description"]) {
                self.fault(
                    &format!("{path}.{key}"),
                    format!("unknown key '{key}' in a value. Expected Name and Description"),
                );
            }
            // A value name is a string or a whole number, and is compared as
            // text wherever it is used, so it is held as text here.
            let name = match get_ci(entry, "Name") {
                Some(Value::String(text)) => Some(text.clone()),
                Some(Value::Number(number)) => Some(
                    number
                        .as_f64()
                        .map(format_number)
                        .unwrap_or_else(|| number.to_string()),
                ),
                _ => None,
            };
            let name = match name {
                None => {
                    self.fault(&format!("{path}.Name"), "a value must have a Name");
                    continue;
                }
                Some(name) => name,
            };
            let mut description = None;
            if let Some(value) = get_ci(entry, "Description") {
                match value.as_str() {
                    Some(text) => description = Some(text.to_owned()),
                    None => self.fault(
                        &format!("{path}.Description"),
                        format!(
                            "a value Description expected a string, found {}",
                            type_of(Some(value))
                        ),
                    ),
                }
            }
            list.push(OutputValue { name, description });
        }
        let mut seen: Vec<String> = Vec::new();
        for entry in &list {
            if seen.contains(&entry.name) {
                self.fault(
                    "Output.Values",
                    format!("the value '{}' is listed more than once", entry.name),
                );
            }
            seen.push(entry.name.clone());
        }
        draft.values = Some(list);
    }

    /// The metadata fields that are carried through unchanged.
    fn read_output_extras(&mut self, raw: &Mapping, draft: &mut DraftOutput) {
        for key in ["IsMandatory", "IsObsolete", "IsPopular", "ExportValues"] {
            let value = match get_ci(raw, key) {
                None => continue,
                Some(value) => value,
            };
            match value.as_bool() {
                None => self.fault(
                    &format!("Output.{key}"),
                    format!(
                        "Output.{key} expected a boolean, found {}",
                        type_of(Some(value))
                    ),
                ),
                Some(flag) => match key {
                    "IsMandatory" => draft.is_mandatory = Some(flag),
                    "IsObsolete" => draft.is_obsolete = Some(flag),
                    "IsPopular" => draft.is_popular = Some(flag),
                    _ => draft.export_values = Some(flag),
                },
            }
        }

        for key in ["Category", "Url", "StoredValueType"] {
            let value = match get_ci(raw, key) {
                None => continue,
                Some(value) => value,
            };
            match value.as_str() {
                None => self.fault(
                    &format!("Output.{key}"),
                    format!(
                        "Output.{key} expected a string, found {}",
                        type_of(Some(value))
                    ),
                ),
                Some(text) => match key {
                    "Category" => draft.category = Some(text.to_owned()),
                    "Url" => draft.url = Some(text.to_owned()),
                    _ => draft.stored_value_type = Some(text.to_owned()),
                },
            }
        }

        for key in ["DisplayOrder", "PropertyId"] {
            let value = match get_ci(raw, key) {
                None => continue,
                Some(value) => value,
            };
            match whole_number(value) {
                None => self.fault(
                    &format!("Output.{key}"),
                    format!(
                        "Output.{key} expected an integer, found {}",
                        type_of(Some(value))
                    ),
                ),
                Some(number) => {
                    if key == "DisplayOrder" {
                        draft.display_order = Some(number);
                    } else {
                        draft.property_id = Some(number);
                    }
                }
            }
        }

        for key in ["VendorIds", "Dependencies"] {
            let value = match get_ci(raw, key) {
                None => continue,
                Some(value) => value,
            };
            match value.as_sequence() {
                None => self.fault(
                    &format!("Output.{key}"),
                    format!(
                        "Output.{key} expected a list, found {}",
                        type_of(Some(value))
                    ),
                ),
                Some(items) => {
                    // Both lists are carried through unchanged, so a member
                    // that is not text is kept as the text it was written as
                    // rather than being refused.
                    let list: Vec<String> = items.iter().map(scalar_text).collect();
                    if key == "VendorIds" {
                        draft.vendor_ids = Some(list);
                    } else {
                        draft.dependencies = Some(list);
                    }
                }
            }
        }
    }

    /// `DefaultValue` is the text form of the value recorded as the default. It
    /// is metadata and nothing more, so it is checked against the value type
    /// and the value list and then carried through.
    fn read_default_value(&mut self, raw: &Mapping, draft: &mut DraftOutput) {
        let value = match get_ci(raw, "DefaultValue") {
            None => return,
            Some(value) => value,
        };
        let text = match value.as_str() {
            None => {
                self.fault(
                    "Output.DefaultValue",
                    format!(
                        "Output.DefaultValue expected a string holding the string form of the \
                         value, found {}",
                        type_of(Some(value))
                    ),
                );
                return;
            }
            Some(text) => text,
        };
        draft.default_value = Some(text.to_owned());
        if let Some(value_type) = draft.value_type {
            if crate::source::convert_text(text, value_type).is_none() {
                self.fault(
                    "Output.DefaultValue",
                    format!("Output.DefaultValue '{text}' cannot be read as {value_type}"),
                );
            }
        }
        if let Some(values) = &draft.values {
            if !values.iter().any(|entry| entry.name == text) {
                self.fault(
                    "Output.DefaultValue",
                    format!(
                        "Output.DefaultValue '{text}' is not one of the values listed under \
                         Output.Values"
                    ),
                );
            }
        }
    }

    // -----------------------------------------------------------------
    // Checks and rules.
    // -----------------------------------------------------------------

    fn read_check_names(&mut self, root: &Mapping) -> Vec<String> {
        let mut names = Vec::new();
        let raw = match get_ci(root, "Checks") {
            None => return names,
            Some(value) => value,
        };
        let raw = match raw.as_mapping() {
            None => {
                self.fault(
                    "Checks",
                    format!(
                        "Checks expected a mapping of names to conditions, found {}",
                        type_of(Some(raw))
                    ),
                );
                return names;
            }
            Some(mapping) => mapping,
        };
        for name in keys_of(raw) {
            if !is_identifier(&name) {
                self.fault(
                    &format!("Checks.{name}"),
                    format!("check name '{name}' does not match the pattern {IDENTIFIER_PATTERN}"),
                );
                continue;
            }
            names.push(name);
        }
        names
    }

    fn read_checks(&mut self, root: &Mapping, check_names: &[String]) -> Vec<Check> {
        let mut checks = Vec::new();
        let raw = match get_ci(root, "Checks").and_then(Value::as_mapping) {
            None => return checks,
            Some(mapping) => mapping,
        };
        for name in check_names {
            // A check name is matched exactly as written, unlike a key, so the
            // condition is read from the key the author wrote.
            let condition = raw
                .iter()
                .find(|(key, _)| key_text(key) == *name)
                .map(|(_, value)| value);
            let path = format!("Checks.{name}");
            let condition = self.read_condition(condition, &path, check_names);
            checks.push(Check {
                name: name.clone(),
                condition,
            });
        }
        checks
    }

    fn read_rules(
        &mut self,
        root: &Mapping,
        check_names: &[String],
        output: &DraftOutput,
    ) -> Vec<Rule> {
        let mut rules = Vec::new();
        let raw = match get_ci(root, "Rules") {
            None => {
                self.fault("Rules", "required key 'Rules' is missing");
                return rules;
            }
            Some(value) => value,
        };
        let items = match raw.as_sequence() {
            None => {
                self.fault(
                    "Rules",
                    format!("Rules expected a list, found {}", type_of(Some(raw))),
                );
                return rules;
            }
            Some(items) => items,
        };
        if items.is_empty() {
            self.fault("Rules", "Rules must hold at least one rule");
            return rules;
        }
        // Every script ends in an Else, so a script always chooses a value once
        // its source properties have been read and there is no path on which no
        // rule matched.
        let last_is_else = items
            .last()
            .and_then(Value::as_mapping)
            .map(|mapping| has_ci(mapping, "Else"))
            .unwrap_or(false);
        if !last_is_else {
            self.fault(
                "Rules",
                "the last rule must be an Else, which is what a script falls back to when no \
                 earlier rule matched",
            );
        }

        let count = items.len();
        for (index, entry) in items.iter().enumerate() {
            let path = format!("Rules[{index}]");
            let entry = match entry.as_mapping() {
                None => {
                    self.fault(
                        &path,
                        format!("a rule expected a mapping, found {}", type_of(Some(entry))),
                    );
                    continue;
                }
                Some(mapping) => mapping,
            };
            for key in unknown_keys(entry, &["When", "Then", "Else"]) {
                self.fault(
                    &format!("{path}.{key}"),
                    format!("unknown key '{key}' in a rule. Expected When and Then, or Else"),
                );
            }
            let has_when = has_ci(entry, "When");
            let has_then = has_ci(entry, "Then");
            let has_else = has_ci(entry, "Else");
            let is_last = index == count - 1;

            if has_else && has_when {
                self.fault(
                    &path,
                    "a rule has both When and Else. A rule is either When with Then, or Else on \
                     its own",
                );
                continue;
            }
            if has_else && !is_last {
                self.fault(&path, "Else is only allowed on the last rule");
                continue;
            }
            if !has_else && !has_when {
                self.fault(&path, "a rule needs a When, or an Else on the last rule");
                continue;
            }
            if has_when && !has_then {
                self.fault(&path, "a rule with When needs a Then");
                continue;
            }

            let value_path = if has_else {
                format!("{path}.Else")
            } else {
                format!("{path}.Then")
            };
            let raw_value = if has_else {
                get_ci(entry, "Else")
            } else {
                get_ci(entry, "Then")
            };
            let value = self.read_rule_value(raw_value, &value_path, output);
            let when = if has_when {
                Some(self.read_condition(
                    get_ci(entry, "When"),
                    &format!("{path}.When"),
                    check_names,
                ))
            } else {
                None
            };
            rules.push(Rule { when, value });
        }
        rules
    }

    /// Read a `Then` or an `Else`, which is a literal of `Output.ValueType` and
    /// must be one of `Output.Values` where that list is given.
    fn read_rule_value(
        &mut self,
        raw: Option<&Value>,
        path: &str,
        output: &DraftOutput,
    ) -> Literal {
        let placeholder = Literal::Bool(false);
        let raw = match raw {
            None | Some(Value::Null) => {
                self.fault(
                    path,
                    "a rule value is a null literal, which format 1 does not allow",
                );
                return placeholder;
            }
            Some(value) => value,
        };
        if matches!(
            raw,
            Value::Mapping(_) | Value::Sequence(_) | Value::Tagged(_)
        ) {
            self.fault(
                path,
                format!(
                    "a rule value is a literal of the output value type, found {}",
                    type_of(Some(raw))
                ),
            );
            return placeholder;
        }
        let literal_type = match infer_type(raw) {
            Some(value_type) => value_type,
            None => {
                self.fault(
                    path,
                    format!(
                        "a rule value is a literal of the output value type, found {}",
                        type_of(Some(raw))
                    ),
                );
                return placeholder;
            }
        };
        if let Some(value_type) = output.value_type {
            // A whole number written without a decimal point reads as a double.
            let matches = literal_type == value_type
                || (literal_type == ValueType::Int && value_type == ValueType::Double);
            if !matches {
                self.fault(
                    path,
                    format!(
                        "expected a {value_type} to match Output.ValueType, found {}",
                        type_of(Some(raw))
                    ),
                );
                return placeholder;
            }
        }
        let literal = literal_of(raw, literal_type);
        if let Some(values) = &output.values {
            let text = literal.to_text();
            if !values.iter().any(|entry| entry.name == text) {
                let names: Vec<&str> = values.iter().map(|entry| entry.name.as_str()).collect();
                self.fault(
                    path,
                    format!(
                        "'{text}' is not one of the values listed under Output.Values ({})",
                        names.join(", ")
                    ),
                );
                return placeholder;
            }
        }
        literal
    }

    // -----------------------------------------------------------------
    // Conditions.
    // -----------------------------------------------------------------

    fn read_condition(
        &mut self,
        raw: Option<&Value>,
        path: &str,
        check_names: &[String],
    ) -> Condition {
        // What a condition that could not be read becomes. A fault has been
        // recorded by the time it is returned, so the model is thrown away and
        // it is never evaluated.
        let faulted = Condition::All(Vec::new());
        let mapping = match raw.and_then(Value::as_mapping) {
            None => {
                self.fault(
                    path,
                    format!("a condition expected a mapping, found {}", type_of(raw)),
                );
                return faulted;
            }
            Some(mapping) => mapping,
        };
        let keys = keys_of(mapping);
        if keys.is_empty() {
            self.fault(path, "a condition is empty");
            return faulted;
        }
        let folded: Vec<String> = keys.iter().map(|key| key.to_lowercase()).collect();
        let holds = |name: &str| folded.iter().any(|key| key == name);

        if holds("property") {
            return self.read_comparison(mapping, &keys, path);
        }
        if holds("check") {
            return self.read_check_reference(mapping, &keys, path, check_names);
        }
        if holds("passed") || holds("failed") {
            return self.read_aggregate(mapping, &keys, path, check_names);
        }
        if holds("all") || holds("any") {
            let which = if holds("all") { "All" } else { "Any" };
            if keys.len() != 1 {
                self.fault(
                    path,
                    format!(
                        "{which} must be the only key of its condition, found {}",
                        keys.join(", ")
                    ),
                );
            }
            let items = match get_ci(mapping, which).and_then(Value::as_sequence) {
                None => {
                    self.fault(
                        &format!("{path}.{which}"),
                        format!(
                            "{which} expected a list of conditions, found {}",
                            type_of(get_ci(mapping, which))
                        ),
                    );
                    return faulted;
                }
                Some(items) => items,
            };
            if items.is_empty() {
                self.fault(
                    &format!("{path}.{which}"),
                    format!("{which} must list at least one condition"),
                );
                return faulted;
            }
            let members: Vec<Condition> = items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    self.read_condition(
                        Some(item),
                        &format!("{path}.{which}[{index}]"),
                        check_names,
                    )
                })
                .collect();
            return if which == "All" {
                Condition::All(members)
            } else {
                Condition::Any(members)
            };
        }
        if holds("not") {
            if keys.len() != 1 {
                self.fault(
                    path,
                    format!(
                        "Not must be the only key of its condition, found {}",
                        keys.join(", ")
                    ),
                );
            }
            let item =
                self.read_condition(get_ci(mapping, "Not"), &format!("{path}.Not"), check_names);
            return Condition::Not(Box::new(item));
        }
        self.fault(
            path,
            format!(
                "a condition must be a comparison, a Check reference, an aggregate, All, Any or \
                 Not. Found the keys {}",
                keys.join(", ")
            ),
        );
        faulted
    }

    fn read_comparison(&mut self, mapping: &Mapping, keys: &[String], path: &str) -> Condition {
        let faulted = Condition::All(Vec::new());
        let property_key = keys
            .iter()
            .find(|key| key.to_lowercase() == "property")
            .cloned()
            .unwrap_or_default();
        let property_value = get_ci(mapping, "Property");
        let property = match property_value.and_then(Value::as_str) {
            Some(text) if is_source_property(text) => text.to_owned(),
            _ => {
                self.fault(
                    &format!("{path}.Property"),
                    format!(
                        "'{}' is not a source property. Write it as elementKey.PropertyName, for \
                         example device.IsCrawler",
                        property_value.map(scalar_text).unwrap_or_default()
                    ),
                );
                return faulted;
            }
        };

        let operator_keys: Vec<&String> = keys.iter().filter(|key| **key != property_key).collect();
        if operator_keys.is_empty() {
            self.fault(
                path,
                format!(
                    "a comparison on '{property}' has no operator. Expected exactly one of {}",
                    operator_names()
                ),
            );
            return faulted;
        }
        let mut known: Vec<(&String, CompareOp)> = Vec::new();
        for key in &operator_keys {
            match operator_named(key) {
                Some(op) => known.push((key, op)),
                None => self.fault(
                    path,
                    format!(
                        "unknown operator '{key}', expected one of {}",
                        operator_names()
                    ),
                ),
            }
        }
        if known.is_empty() {
            return faulted;
        }
        if operator_keys.len() > 1 {
            let written: Vec<&str> = operator_keys.iter().map(|key| key.as_str()).collect();
            self.fault(
                path,
                format!(
                    "a condition takes exactly one operator, found {}",
                    written.join(", ")
                ),
            );
            return faulted;
        }
        let (operator_key, op) = known[0];
        let operand = match mapping
            .iter()
            .find(|(key, _)| key_text(key) == **operator_key)
            .map(|(_, value)| value)
        {
            Some(value) => value,
            None => return faulted,
        };
        if operand.is_null() {
            self.fault(
                &format!("{path}.{}", op.as_str()),
                "a null literal is not allowed. Give the value to compare against",
            );
            return faulted;
        }

        let (value_type, parsed) = if matches!(op, CompareOp::In | CompareOp::NotIn) {
            match self.read_list_operand(operand, path, op) {
                Some(parsed) => parsed,
                None => return faulted,
            }
        } else {
            match infer_type(operand) {
                Some(value_type) => (value_type, Operand::One(literal_of(operand, value_type))),
                None => {
                    self.fault(
                        &format!("{path}.{}", op.as_str()),
                        format!(
                            "the literal {} has no type format 1 knows",
                            json_like(operand)
                        ),
                    );
                    return faulted;
                }
            }
        };

        if !op.allowed_on().contains(&value_type) {
            let allowed: Vec<&str> = op.allowed_on().iter().map(|kind| kind.as_str()).collect();
            self.fault(
                path,
                format!(
                    "operator '{}' is not allowed on type {value_type}. It is allowed on {}",
                    op.as_str(),
                    allowed.join(", ")
                ),
            );
            return faulted;
        }

        let slot = self.use_property(&property, value_type, path);
        Condition::Compare {
            property: slot,
            op,
            operand: parsed,
            value_type,
        }
    }

    /// Read the list `In` and `NotIn` take. Whole numbers and numbers with a
    /// fractional part may sit in one list, and such a mixed list reads as
    /// `double`. Any other mixture of types is a fault.
    fn read_list_operand(
        &mut self,
        operand: &Value,
        path: &str,
        op: CompareOp,
    ) -> Option<(ValueType, Operand)> {
        let operator_path = format!("{path}.{}", op.as_str());
        let items = match operand.as_sequence() {
            None => {
                self.fault(
                    &operator_path,
                    format!(
                        "{} expects a list of values, found {}",
                        op.as_str(),
                        type_of(Some(operand))
                    ),
                );
                return None;
            }
            Some(items) => items,
        };
        if items.is_empty() {
            self.fault(
                &operator_path,
                format!("{} expects a non empty list", op.as_str()),
            );
            return None;
        }
        if items.iter().any(Value::is_null) {
            self.fault(&operator_path, "a null literal is not allowed in a list");
            return None;
        }
        let mut member_types = Vec::with_capacity(items.len());
        for item in items {
            match infer_type(item) {
                Some(value_type) => member_types.push(value_type),
                None => {
                    // A mapping or a list inside the list has no type to
                    // compare with, so naming it is more use to an author than
                    // reporting a mixture of types.
                    self.fault(
                        &operator_path,
                        format!("the literal {} has no type format 1 knows", json_like(item)),
                    );
                    return None;
                }
            }
        }
        let numeric = member_types
            .iter()
            .all(|kind| matches!(kind, ValueType::Int | ValueType::Double));
        let first = member_types[0];
        if !numeric && member_types.iter().any(|kind| *kind != first) {
            let names: Vec<&str> = member_types.iter().map(|kind| kind.as_str()).collect();
            self.fault(
                &operator_path,
                format!(
                    "every member of a list must be of the same type, found {}",
                    names.join(", ")
                ),
            );
            return None;
        }
        let value_type = if numeric {
            if member_types.contains(&ValueType::Double) {
                ValueType::Double
            } else {
                ValueType::Int
            }
        } else {
            first
        };
        let literals = items
            .iter()
            .map(|item| literal_of(item, value_type))
            .collect();
        Some((value_type, Operand::Many(literals)))
    }

    fn read_check_reference(
        &mut self,
        mapping: &Mapping,
        keys: &[String],
        path: &str,
        check_names: &[String],
    ) -> Condition {
        let faulted = Condition::All(Vec::new());
        if keys.len() != 1 {
            self.fault(
                path,
                format!(
                    "Check must be the only key of its condition, found {}",
                    keys.join(", ")
                ),
            );
        }
        let value = get_ci(mapping, "Check");
        let name = match value.and_then(Value::as_str) {
            None => {
                self.fault(
                    &format!("{path}.Check"),
                    format!(
                        "Check expected the name of a check, found {}",
                        type_of(value)
                    ),
                );
                return faulted;
            }
            Some(name) => name,
        };
        match check_names.iter().position(|defined| defined == name) {
            Some(index) => Condition::Check(index),
            None => {
                self.fault(
                    &format!("{path}.Check"),
                    format!(
                        "check '{name}' is not defined. The checks are {}",
                        check_list(check_names)
                    ),
                );
                faulted
            }
        }
    }

    fn read_aggregate(
        &mut self,
        mapping: &Mapping,
        keys: &[String],
        path: &str,
        check_names: &[String],
    ) -> Condition {
        let faulted = Condition::All(Vec::new());
        let aggregate_keys: Vec<&String> = keys
            .iter()
            .filter(|key| {
                let folded = key.to_lowercase();
                folded == "passed" || folded == "failed"
            })
            .collect();
        if aggregate_keys.len() > 1 {
            let written: Vec<&str> = aggregate_keys.iter().map(|key| key.as_str()).collect();
            self.fault(
                path,
                format!(
                    "an aggregate condition takes one of Passed, Failed, found {}",
                    written.join(", ")
                ),
            );
            return faulted;
        }
        let aggregate_key = aggregate_keys[0];
        let aggregate = if aggregate_key.to_lowercase() == "passed" {
            Aggregate::Passed
        } else {
            Aggregate::Failed
        };
        let raw_group = mapping
            .iter()
            .find(|(key, _)| key_text(key) == **aggregate_key)
            .map(|(_, value)| value);
        let group = self.read_group(
            raw_group,
            &format!("{path}.{}", aggregate.as_str()),
            check_names,
        );

        let operator_keys: Vec<&String> = keys.iter().filter(|key| *key != aggregate_key).collect();
        if operator_keys.is_empty() {
            self.fault(
                path,
                format!(
                    "an aggregate condition has no operator. Expected exactly one of \
                     {AGGREGATE_OPERATORS}"
                ),
            );
            return faulted;
        }
        if operator_keys.len() > 1 {
            let written: Vec<&str> = operator_keys.iter().map(|key| key.as_str()).collect();
            self.fault(
                path,
                format!(
                    "a condition takes exactly one operator, found {}",
                    written.join(", ")
                ),
            );
            return faulted;
        }
        let operator_key = operator_keys[0];
        let op = match operator_named(operator_key) {
            Some(op) => op,
            None => {
                self.fault(
                    path,
                    format!(
                        "unknown operator '{operator_key}', expected one of {AGGREGATE_OPERATORS}"
                    ),
                );
                return faulted;
            }
        };
        if !op.allowed_on().contains(&ValueType::Int) {
            self.fault(
                path,
                format!(
                    "operator '{}' is not allowed on a count, which is an int",
                    op.as_str()
                ),
            );
            return faulted;
        }

        // A count is compared against a whole number and against nothing else,
        // an aggregate among them.
        let operand = mapping
            .iter()
            .find(|(key, _)| key_text(key) == **operator_key)
            .map(|(_, value)| value);
        let operand = match operand.and_then(whole_number) {
            Some(number) => number,
            None => {
                self.fault(
                    &format!("{path}.{}", op.as_str()),
                    format!(
                        "an aggregate is compared with a whole number, found {}",
                        type_of(operand)
                    ),
                );
                return faulted;
            }
        };

        Condition::Aggregate {
            aggregate,
            group,
            op,
            operand,
        }
    }

    /// A group is the word `Checks`, meaning every named check, or a list of
    /// check names. It becomes a list of indexes, or `None` for every check.
    fn read_group(
        &mut self,
        raw: Option<&Value>,
        path: &str,
        check_names: &[String],
    ) -> Option<Vec<usize>> {
        match raw {
            Some(Value::String(text)) => {
                if text.to_lowercase() == "checks" {
                    return None;
                }
                self.fault(
                    path,
                    format!(
                        "a group is the word Checks, meaning every check, or a list of check \
                         names. Found '{text}'"
                    ),
                );
                Some(Vec::new())
            }
            Some(Value::Sequence(items)) => {
                let mut indexes = Vec::new();
                for item in items {
                    let name = match item.as_str() {
                        None => {
                            self.fault(
                                path,
                                format!("a group lists check names, found {}", type_of(Some(item))),
                            );
                            continue;
                        }
                        Some(name) => name,
                    };
                    match check_names.iter().position(|defined| defined == name) {
                        Some(index) => indexes.push(index),
                        None => self.fault(
                            path,
                            format!(
                                "check '{name}' is not defined. The checks are {}",
                                check_list(check_names)
                            ),
                        ),
                    }
                }
                Some(indexes)
            }
            other => {
                self.fault(
                    path,
                    format!(
                        "a group is the word Checks or a list of check names, found {}",
                        type_of(other)
                    ),
                );
                Some(Vec::new())
            }
        }
    }

    // -----------------------------------------------------------------
    // Source properties.
    // -----------------------------------------------------------------

    /// Record that a condition names a source property, and that the literal it
    /// is compared against infers a type. Returns the index the evaluator reads
    /// the property from.
    fn use_property(&mut self, name: &str, value_type: ValueType, path: &str) -> usize {
        let key = name.to_lowercase();
        if let Some(index) = self
            .properties
            .iter()
            .position(|property| property.name.to_lowercase() == key)
        {
            let existing = self.properties[index].value_type;
            if existing != value_type {
                let first = self.inferred_at[index].clone();
                let written = self.properties[index].name.clone();
                self.fault(
                    path,
                    format!(
                        "'{written}' is inferred as {value_type} here but was already inferred \
                         as {existing} at {first}. Every use of a property must infer the same \
                         type"
                    ),
                );
            }
            return index;
        }
        let dot = name.find('.').unwrap_or(0);
        self.properties.push(SourceProperty {
            name: name.to_owned(),
            element_key: name[..dot].to_owned(),
            property_name: name[dot + 1..].to_owned(),
            value_type,
        });
        self.inferred_at.push(path.to_owned());
        self.properties.len() - 1
    }
}

/// The `Output` block as it is being read, before it is known to be sound.
#[derive(Default)]
struct DraftOutput {
    name: Option<String>,
    description: Option<String>,
    value_type: Option<ValueType>,
    stored_value_type: Option<String>,
    default_value: Option<String>,
    is_mandatory: Option<bool>,
    is_obsolete: Option<bool>,
    category: Option<String>,
    is_popular: Option<bool>,
    export_values: Option<bool>,
    url: Option<String>,
    display_order: Option<i64>,
    property_id: Option<i64>,
    vendor_ids: Option<Vec<String>>,
    dependencies: Option<Vec<String>>,
    values: Option<Vec<OutputValue>>,
}

impl DraftOutput {
    /// The finished definition, which is only asked for once the script is
    /// known to carry no faults.
    fn finish(self) -> Option<Output> {
        Some(Output {
            name: self.name?,
            description: self.description?,
            value_type: self.value_type?,
            stored_value_type: self.stored_value_type,
            default_value: self.default_value,
            is_list: false,
            is_mandatory: self.is_mandatory,
            is_obsolete: self.is_obsolete,
            category: self.category,
            is_popular: self.is_popular,
            export_values: self.export_values,
            url: self.url,
            display_order: self.display_order,
            property_id: self.property_id,
            vendor_ids: self.vendor_ids.unwrap_or_default(),
            dependencies: self.dependencies.unwrap_or_default(),
            values: self.values,
        })
    }
}

fn check_list(check_names: &[String]) -> String {
    if check_names.is_empty() {
        "(none)".to_owned()
    } else {
        check_names.join(", ")
    }
}

fn operator_names() -> String {
    CompareOp::ALL
        .iter()
        .map(|op| op.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn operator_named(name: &str) -> Option<CompareOp> {
    let folded = name.to_lowercase();
    CompareOp::ALL
        .iter()
        .copied()
        .find(|op| op.as_str().to_lowercase() == folded)
}

fn value_type_named(name: &str) -> Option<ValueType> {
    match name.to_lowercase().as_str() {
        "string" => Some(ValueType::String),
        "bool" => Some(ValueType::Bool),
        "int" => Some(ValueType::Int),
        "double" => Some(ValueType::Double),
        _ => None,
    }
}

/// The text of a scalar as it was written, for the fault messages that quote
/// what they found without JSON quoting it.
fn scalar_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number
            .as_f64()
            .map(format_number)
            .unwrap_or_else(|| number.to_string()),
        Value::Null => "null".to_owned(),
        other => json_like(other),
    }
}

/// A whole number, whichever way it was written. `5` and `5.0` are the same
/// number, so both count.
fn whole_number(value: &Value) -> Option<i64> {
    let number = value.as_f64()?;
    if !number.is_finite() || number.fract() != 0.0 {
        return None;
    }
    if !matches!(value, Value::Number(_)) {
        return None;
    }
    Some(number as i64)
}

/// The type a literal in a script infers.
///
/// A number infers its type from its value rather than from the way the value
/// was written, so 8, 8.0 and 8e0 all infer `int`. A whole number too large for
/// a signed 32 bit integer infers `double` instead, because `int` is fixed at
/// 32 bits so that one script gives one answer in every language.
pub(crate) fn infer_type(value: &Value) -> Option<ValueType> {
    match value {
        Value::Bool(_) => Some(ValueType::Bool),
        Value::String(_) => Some(ValueType::String),
        Value::Number(number) => {
            let number = number.as_f64()?;
            let whole = number.is_finite()
                && number.fract() == 0.0
                && number >= INT_MIN as f64
                && number <= INT_MAX as f64;
            Some(if whole {
                ValueType::Int
            } else {
                ValueType::Double
            })
        }
        _ => None,
    }
}

/// Turn a literal in a script into the model's form of it, read as the type
/// given. A whole number in a list that reads as `double` becomes a `double`
/// here, so that everything in one comparison is of one type.
fn literal_of(value: &Value, value_type: ValueType) -> Literal {
    match value_type {
        ValueType::Bool => Literal::Bool(value.as_bool().unwrap_or(false)),
        ValueType::Int => Literal::Int(value.as_f64().unwrap_or(0.0) as i32),
        ValueType::Double => Literal::Double(value.as_f64().unwrap_or(0.0)),
        ValueType::String => Literal::Text(scalar_text(value)),
    }
}

/// Whether text matches `^[A-Za-z][A-Za-z0-9]*$`.
fn is_identifier(text: &str) -> bool {
    let mut characters = text.chars();
    match characters.next() {
        Some(first) if first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    characters.all(|character| character.is_ascii_alphanumeric())
}

/// Whether text names a source property as `elementKey.PropertyName`, with
/// exactly one dot, an element key and an identifier.
fn is_source_property(text: &str) -> bool {
    match text.split_once('.') {
        Some((element, property)) => is_element_key(element) && is_identifier(property),
        None => false,
    }
}

/// Whether text matches `^[A-Za-z][A-Za-z0-9]*(?:-[A-Za-z0-9]+)*$`, being an
/// identifier that may carry single hyphens, as `agent-signature` does.
fn is_element_key(text: &str) -> bool {
    let mut parts = text.split('-');
    let first = parts.next().unwrap_or_default();
    is_identifier(first)
        && parts.all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric())
        })
}

/// Whether text is a semantic version, being three numbers, then an optional
/// pre-release after a hyphen, then optional build metadata after a plus.
fn is_semantic_version(text: &str) -> bool {
    let (core, build) = match text.split_once('+') {
        Some((core, build)) => (core, Some(build)),
        None => (text, None),
    };
    if let Some(build) = build {
        if build.is_empty() || !build.chars().all(is_version_character) {
            return false;
        }
    }
    let (numbers, pre_release) = match core.split_once('-') {
        Some((numbers, pre_release)) => (numbers, Some(pre_release)),
        None => (core, None),
    };
    if let Some(pre_release) = pre_release {
        if pre_release.is_empty() || !pre_release.chars().all(is_version_character) {
            return false;
        }
    }
    let parts: Vec<&str> = numbers.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

fn is_version_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '.' || character == '-'
}
