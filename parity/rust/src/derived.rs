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

//! The derived property cases: every case of each case file derived-properties
//! ships, run through a pipeline of one source element for each data key and
//! the derived property element with the built-in script the file names.
//!
//! ```text
//! parity-rust derived-cases --cases <folder> --out <json>
//! parity-rust derived --cases <folder> --out <jsonl>
//! ```
//!
//! The first writes the cases as a corpus for the comparer, the second writes
//! one line for each case. `parity/README.md` ("The derived property
//! comparison") describes both. The .NET producer's `DerivedCases` builds the
//! same pipelines.

use std::any::Any;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use serde_norway::Value as Yaml;

use fiftyone_derived_properties::{BuiltInScript, DerivedPropertyElement};
use fiftyone_pipeline_core::{
    ElementData, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    NoValueError, Pipeline, PropertyMetaData, PropertyValue, PropertyValueType, TypedKey,
};

/// The first argument that writes the corpus.
pub const CASES: &str = "derived-cases";

/// The first argument that writes the lines.
pub const RUN: &str = "derived";

/// The end of a case file's name.
const CASE_FILE_SUFFIX: &str = ".cases.yaml";

/// The names of a weighted value's two parts, as the IP Intelligence data
/// gives them and the derived property element reads them.
const WEIGHTED_VALUE: &str = "value";
const WEIGHTED_RAW_WEIGHTING: &str = "rawweighting";

/// One case, with its id and the script its file names.
struct Case {
    id: String,
    about: String,
    script: String,
    case: Yaml,
}

/// Writes the corpus for `--cases`.
pub fn cases(args: &[String]) -> Result<()> {
    let (folder, out) = parse_args(args)?;
    let corpus: Vec<Value> = read_cases(&folder)?
        .iter()
        .map(|case| {
            json!({
                "id": case.id,
                "about": case.about,
                "expect": expect_of(&case.case),
            })
        })
        .collect();
    let mut writer = writer(&out)?;
    serde_json::to_writer_pretty(&mut writer, &corpus)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

/// Writes one line for each case in `--cases`.
pub fn run(args: &[String]) -> Result<()> {
    let (folder, out) = parse_args(args)?;
    let mut writer = writer(&out)?;
    for case in read_cases(&folder)? {
        let line = json!({
            "id": case.id,
            "derived": run_case(&case)?,
        });
        serde_json::to_writer(&mut writer, &line)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    Ok(())
}

fn parse_args(args: &[String]) -> Result<(PathBuf, PathBuf)> {
    let mut folder = None;
    let mut out = None;
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let value = rest
            .next()
            .with_context(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--cases" => folder = Some(PathBuf::from(value)),
            "--out" => out = Some(PathBuf::from(value)),
            _ => bail!("unknown argument {flag}"),
        }
    }
    Ok((
        folder.context("--cases is required")?,
        out.context("--out is required")?,
    ))
}

fn writer(out: &Path) -> Result<BufWriter<File>> {
    Ok(BufWriter::new(
        File::create(out).with_context(|| format!("creating {}", out.display()))?,
    ))
}

fn get<'a>(value: &'a Yaml, name: &str) -> Option<&'a Yaml> {
    value.as_mapping()?.get(Yaml::String(name.to_owned()))
}

/// Every case of every case file in the folder, the files in the ordinal
/// order of their names. A case's id is `derived-`, the script's name in
/// lower case and the case's place in its file, counting from 1.
fn read_cases(folder: &Path) -> Result<Vec<Case>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(folder)
        .with_context(|| format!("reading {}", folder.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<_>>()?;
    files.retain(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(CASE_FILE_SUFFIX))
    });
    files.sort();
    if files.is_empty() {
        bail!("{} holds no case files", folder.display());
    }
    let mut cases = Vec::new();
    for path in files {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let document: Yaml =
            serde_norway::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        let script = get(&document, "Script")
            .and_then(Yaml::as_str)
            .with_context(|| format!("{} names no Script", path.display()))?
            .to_owned();
        let list = get(&document, "Cases")
            .and_then(Yaml::as_sequence)
            .with_context(|| format!("{} has no Cases list", path.display()))?;
        for (index, case) in list.iter().enumerate() {
            let name = get(case, "Name").and_then(Yaml::as_str).unwrap_or_default();
            cases.push(Case {
                id: format!("derived-{}-{:02}", script.to_lowercase(), index + 1),
                about: format!("{script}: {name}"),
                script: script.clone(),
                case: case.clone(),
            });
        }
    }
    Ok(cases)
}

/// What the case file expects, as `value` or as `missing`, for the workflow
/// step that checks each side gives it.
fn expect_of(case: &Yaml) -> Value {
    let expect = get(case, "Expect");
    if let Some(value) = expect.and_then(|expect| get(expect, "Value")) {
        return json!({ "value": json_of_yaml(value) });
    }
    let missing: Vec<Value> = expect
        .and_then(|expect| get(expect, "Missing"))
        .and_then(Yaml::as_sequence)
        .map(|items| items.iter().map(json_of_yaml).collect())
        .unwrap_or_default();
    json!({ "missing": missing })
}

fn json_of_yaml(value: &Yaml) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// Runs one case. The line holds the script's source properties as it names
/// them, then the output's value and type, or its no value message and the
/// source properties that message names.
fn run_case(case: &Case) -> Result<Value> {
    let built_in = BuiltInScript::named(&case.script)
        .with_context(|| format!("{}: no built-in script is named {}", case.id, case.script))?;
    let element = DerivedPropertyElement::builder()
        .add_built_in(built_in)
        .build()
        .map_err(|faults| anyhow::anyhow!("{}: {faults}", case.id))?;
    let script = &element.scripts()[0];
    let sources: Vec<(String, String, String)> = script
        .source_properties()
        .iter()
        .map(|property| {
            (
                property.name.clone(),
                property.element_key.clone(),
                property.property_name.clone(),
            )
        })
        .collect();
    let output = script.output().name.clone();
    let derived_key = element.data_key().to_owned();
    let names: Vec<&str> = sources.iter().map(|(name, _, _)| name.as_str()).collect();

    let mut builder = Pipeline::builder();
    for source in case_sources(&sources, &case.case)? {
        builder = builder.add_element(Arc::new(source));
    }
    let pipeline = match builder.add_element(Arc::new(element)).build() {
        Ok(pipeline) => pipeline,
        Err(error) => {
            return Ok(json!({
                "script": case.script,
                "sourceProperties": names,
                "error": error.to_string(),
            }))
        }
    };
    let mut data = pipeline.create_flow_data();
    let process_error = data.process().err().map(|error| error.to_string());
    let answer = data
        .get_str(&derived_key)
        .map(|derived| derived.get(&output));
    let (value, value_type, no_value) = match answer {
        Some(Ok(value)) => (
            value_json(&value),
            Value::from(type_name(&value)),
            Value::Null,
        ),
        Some(Err(no_value)) => (Value::Null, Value::Null, Value::from(no_value.message)),
        None => (Value::Null, Value::Null, Value::Null),
    };
    let missing: Vec<&str> = match no_value.as_str() {
        Some(message) => named_in(message, &names),
        None => Vec::new(),
    };
    Ok(json!({
        "script": case.script,
        "sourceProperties": names,
        "processError": process_error,
        "type": value_type,
        "value": value,
        "noValue": no_value,
        "missing": missing,
    }))
}

/// The source properties a no value message names, in the script's order. A
/// name counts when the message holds it between single quotes, ignoring
/// case, as .NET's own conformance tests look for it.
fn named_in<'a>(message: &str, names: &[&'a str]) -> Vec<&'a str> {
    let message = message.to_lowercase();
    names
        .iter()
        .filter(|name| message.contains(&format!("'{}'", name.to_lowercase())))
        .copied()
        .collect()
}

fn value_json(value: &PropertyValue) -> Value {
    match value {
        PropertyValue::String(text) => Value::from(text.as_str()),
        PropertyValue::Bool(flag) => Value::from(*flag),
        PropertyValue::Integer(number) => Value::from(*number),
        PropertyValue::Double(number) => Value::from(*number),
        other => Value::from(format!("{other:?}")),
    }
}

fn type_name(value: &PropertyValue) -> &'static str {
    match value {
        PropertyValue::String(_) => "string",
        PropertyValue::Bool(_) => "bool",
        PropertyValue::Integer(_) => "integer",
        PropertyValue::Double(_) => "double",
        _ => "other",
    }
}

/// One source element for each data key the script or the case names, in the
/// ordinal order of the keys, as .NET's conformance tests build them. Each
/// lists every source property of the script under its key, and the case's
/// own properties, so the pipeline check passes, and writes only the values
/// the case gives, so a case that leaves one out has it absent.
fn case_sources(sources: &[(String, String, String)], case: &Yaml) -> Result<Vec<CaseSource>> {
    // Keyed by the lower case data key, holding the key as first written, the
    // properties listed and the values written.
    type Entries = Vec<(String, Entry)>;
    let mut by_key: BTreeMap<String, (String, Vec<String>, Entries)> = BTreeMap::new();
    for (_, key, property) in sources {
        let entry = by_key
            .entry(key.to_lowercase())
            .or_insert_with(|| (key.clone(), Vec::new(), Vec::new()));
        entry.1.push(property.clone());
    }
    if let Some(properties) = get(case, "Properties").and_then(Yaml::as_mapping) {
        for (name, value) in properties {
            let name = name.as_str().context("a property name is not text")?;
            let (key, property) = name
                .split_once('.')
                .with_context(|| format!("'{name}' is not written as elementKey.PropertyName"))?;
            let entry = by_key
                .entry(key.to_lowercase())
                .or_insert_with(|| (key.to_owned(), Vec::new(), Vec::new()));
            if !entry
                .1
                .iter()
                .any(|listed| listed.eq_ignore_ascii_case(property))
            {
                entry.1.push(property.to_owned());
            }
            if let Some(value) = entry_of(name, value)? {
                entry.2.push((property.to_owned(), value));
            }
        }
    }
    Ok(by_key
        .into_values()
        .map(|(key, listed, values)| CaseSource::new(&key, &listed, values))
        .collect())
}

/// A property's value, or the message it gives instead of one.
#[derive(Clone)]
enum Entry {
    Value(PropertyValue),
    NoValue(String),
}

/// A case writes a value one of four ways, which this reads as .NET's
/// conformance tests do. A plain value arrives as its own type, being a whole
/// number in the range of a 32-bit integer as an integer and any other number
/// as a double. `{ String: ... }` arrives as text, `{ NoValue: ... }` as a
/// property with that no value message, and a list of `Value` and `Weight`
/// pairs as weighted values, each value as text and each weight as a whole
/// number. A null leaves the property out.
fn entry_of(name: &str, value: &Yaml) -> Result<Option<Entry>> {
    Ok(Some(match value {
        Yaml::Null => return Ok(None),
        Yaml::Mapping(_) => {
            if let Some(text) = get(value, "String") {
                Entry::Value(PropertyValue::String(text_of(text)))
            } else if let Some(message) = get(value, "NoValue") {
                Entry::NoValue(text_of(message))
            } else {
                bail!("'{name}' is written as a mapping that is neither String nor NoValue")
            }
        }
        Yaml::Sequence(items) => {
            let mut records = Vec::with_capacity(items.len());
            for item in items {
                let (Some(value), Some(weight)) = (get(item, "Value"), get(item, "Weight")) else {
                    bail!(
                        "'{name}' is written as a list, so every member needs a Value and a Weight"
                    )
                };
                let weight = weight
                    .as_u64()
                    .and_then(|weight| u16::try_from(weight).ok())
                    .with_context(|| format!("'{name}' has a weight that is not 0 to 65535"))?;
                records.push(BTreeMap::from([
                    (
                        WEIGHTED_VALUE.to_owned(),
                        PropertyValue::String(text_of(value)),
                    ),
                    (
                        WEIGHTED_RAW_WEIGHTING.to_owned(),
                        PropertyValue::Integer(i64::from(weight)),
                    ),
                ]));
            }
            Entry::Value(PropertyValue::KeyValueList(records))
        }
        scalar => Entry::Value(scalar_value(scalar)),
    }))
}

fn scalar_value(value: &Yaml) -> PropertyValue {
    match value {
        Yaml::Bool(flag) => PropertyValue::Bool(*flag),
        Yaml::Number(number) => match number.as_f64() {
            Some(real)
                if real.fract() == 0.0
                    && (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&real) =>
            {
                PropertyValue::Integer(real as i64)
            }
            Some(real) => PropertyValue::Double(real),
            None => PropertyValue::String(number.to_string()),
        },
        other => PropertyValue::String(text_of(other)),
    }
}

/// A single value as .NET writes it as text, so a boolean is `True` or
/// `False`.
fn text_of(value: &Yaml) -> String {
    match value {
        Yaml::String(text) => text.clone(),
        Yaml::Bool(flag) => if *flag { "True" } else { "False" }.to_owned(),
        Yaml::Number(number) => number.to_string(),
        other => format!("{other:?}"),
    }
}

/// A data key as the `'static` text a [`TypedKey`] holds. The cases name only
/// a few keys, so each is kept once for the life of the program.
fn static_key(key: &str) -> &'static str {
    static KEYS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let mut keys = KEYS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(found) = keys.iter().copied().find(|known| *known == key) {
        return found;
    }
    let leaked: &'static str = Box::leak(key.to_owned().into_boxed_str());
    keys.push(leaked);
    leaked
}

/// Writes one case's values under one data key.
struct CaseSource {
    key: &'static str,
    values: Vec<(String, Entry)>,
    properties: Vec<PropertyMetaData>,
    filter: EvidenceKeyFilterWhitelist,
}

impl CaseSource {
    fn new(key: &str, listed: &[String], values: Vec<(String, Entry)>) -> Self {
        let key = static_key(key);
        CaseSource {
            key,
            values,
            properties: listed
                .iter()
                .map(|name| PropertyMetaData::new(name, key, PropertyValueType::String))
                .collect(),
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        }
    }
}

impl FlowElement for CaseSource {
    fn process(&self, data: &mut FlowData) -> fiftyone_pipeline_core::Result<()> {
        let values = self.values.clone();
        data.get_or_add(TypedKey::<CaseSourceData>::new(self.key), || {
            CaseSourceData { entries: values }
        })?;
        Ok(())
    }

    fn data_key(&self) -> &str {
        self.key
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

/// One case's values under one data key, in the order the case gives them.
struct CaseSourceData {
    entries: Vec<(String, Entry)>,
}

impl CaseSourceData {
    fn entry(&self, name: &str) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, entry)| entry)
    }
}

impl ElementData for CaseSourceData {
    fn get(&self, name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        match self.entry(name) {
            Some(Entry::Value(value)) => Ok(value.clone()),
            Some(Entry::NoValue(message)) => Err(NoValueError::new(message.clone())),
            None => Err(NoValueError::new(format!(
                "No value for property '{name}'."
            ))),
        }
    }

    fn has_property(&self, name: &str) -> bool {
        self.entry(name).is_some()
    }

    fn keys(&self) -> Vec<String> {
        self.entries.iter().map(|(name, _)| name.clone()).collect()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
