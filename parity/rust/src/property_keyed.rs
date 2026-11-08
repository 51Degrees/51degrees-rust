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

//! The TAC and Native cases: requests to the TAC and Native engines, in a
//! pipeline with the on-premise Device Detection engine, whose data file
//! holds the profiles.
//!
//! ```text
//! parity-rust property-keyed-cases --corpus <json> --dd <hash file> --out <json>
//! parity-rust property-keyed --corpus <json> --dd <hash file> --out <jsonl>
//! ```
//!
//! The first writes the fixed cases of `--corpus` and, after them, the cases
//! for the TAC and NativeModel values the data file holds, as a corpus for
//! the comparer and for both producers. The second writes one line for each
//! case. `parity/README.md` ("The TAC and Native comparison") describes both.
//! The .NET producer's `PropertyKeyedCases` builds the same pipelines.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use fiftyone_device_detection_onpremise::{
    DeviceDetectionOnPremiseEngine, DeviceDetectionOnPremiseEngineBuilder, PROFILE_ID_PROPERTY_NAME,
};
use fiftyone_device_detection_property_keyed::constants::{
    NATIVE_MODEL_PROPERTY_NAME, TAC_PROPERTY_NAME,
};
use fiftyone_device_detection_property_keyed::{
    NativeEngineBuilder, PropertyKeyedDeviceEngine, TacEngineBuilder,
};
use fiftyone_device_detection_shared::{
    DeviceDataBase, HARDWARE_DATA_KEY, HARDWARE_ELEMENT_DATA_KEY,
};
use fiftyone_json_builder::{JsonBuilderElementBuilder, JSON_BUILDER_DATA_KEY};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{Evidence, FlowElement, Pipeline};
use fiftyone_pipeline_engines::AspectEngine;
use fiftyone_pipeline_engines_fiftyone::SequenceElement;

use crate::fodid_cases::value_type_name;

/// The first argument that writes the corpus.
pub const CASES: &str = "property-keyed-cases";

/// The first argument that writes the lines.
pub const RUN: &str = "property-keyed";

/// The profiles whose values a line holds are every this many distinct
/// profiles met over the value cases, counting from the first, so the lines
/// stay small enough for the job.
const PROFILE_EVERY: usize = 50;

/// One case. `serde_json` keeps the evidence in file order.
#[derive(Deserialize)]
struct Case {
    id: String,
    evidence: Map<String, Value>,
    /// Whether the line holds the engines' metadata and indexed values.
    #[serde(default)]
    engine: bool,
    /// Whether the case runs through the pipeline with the JSON builder, and
    /// the line holds the JSON's `hardware`.
    #[serde(default)]
    json: bool,
    /// The profile whose values the line holds, from among those the case
    /// finds.
    #[serde(default, rename = "profileId")]
    profile_id: Option<u64>,
}

struct Args {
    corpus: PathBuf,
    dd: PathBuf,
    out: PathBuf,
}

/// The Device Detection engine, then the TAC and Native engines.
struct Engines {
    device: Arc<DeviceDetectionOnPremiseEngine>,
    tac: Arc<PropertyKeyedDeviceEngine>,
    native: Arc<PropertyKeyedDeviceEngine>,
    pipeline: Arc<Pipeline>,
}

impl Engines {
    /// The engines after `device`, with the Sequence element first and the
    /// JSON builder last when `json` is set.
    fn build(device: &Arc<DeviceDetectionOnPremiseEngine>, json: bool) -> Result<Self> {
        let tac = TacEngineBuilder::new()
            .build()
            .context("building the TAC engine")?;
        let native = NativeEngineBuilder::new()
            .build()
            .context("building the Native engine")?;
        let mut builder = Pipeline::builder();
        if json {
            // The JSON builder reads the sequence this element writes.
            builder = builder.add_element(Arc::new(SequenceElement::new()) as Arc<dyn FlowElement>);
        }
        builder = builder
            .add_element(Arc::clone(device) as Arc<dyn FlowElement>)
            .add_element(Arc::clone(&tac) as Arc<dyn FlowElement>)
            .add_element(Arc::clone(&native) as Arc<dyn FlowElement>);
        if json {
            builder = builder.add_element(
                Arc::new(JsonBuilderElementBuilder::new().build()) as Arc<dyn FlowElement>
            );
        }
        let pipeline = builder
            .suppress_process_exceptions(true)
            .build()
            .context("building the TAC and Native pipeline")?;
        Ok(Engines {
            device: Arc::clone(device),
            tac,
            native,
            pipeline,
        })
    }

    /// The two engines, the TAC engine first.
    fn both(&self) -> [&Arc<PropertyKeyedDeviceEngine>; 2] {
        [&self.tac, &self.native]
    }
}

/// Writes the fixed cases of `--corpus`, then the cases for each TAC and
/// NativeModel value the data file `--dd` holds.
pub fn cases(args: &[String]) -> Result<()> {
    let args = parse_args(args)?;
    let mut corpus: Vec<Value> = serde_json::from_str(&read(&args.corpus)?)
        .with_context(|| format!("parsing {}", args.corpus.display()))?;
    let device = device_engine(&args.dd)?;
    let engines = Engines::build(&device, false)?;
    corpus.extend(value_cases(&engines)?);
    let mut writer = writer(&args.out)?;
    serde_json::to_writer(&mut writer, &corpus)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

/// Writes one line for each case in `--corpus`.
pub fn run(args: &[String]) -> Result<()> {
    let args = parse_args(args)?;
    let cases: Vec<Case> = serde_json::from_str(&read(&args.corpus)?)
        .with_context(|| format!("parsing {}", args.corpus.display()))?;
    let device = device_engine(&args.dd)?;
    let engines = Engines::build(&device, false)?;
    let with_json = Engines::build(&device, true)?;
    let mut writer = writer(&args.out)?;
    let mut found = 0;
    for case in &cases {
        let line = run_case(
            if case.json { &with_json } else { &engines },
            &engines,
            case,
        )?;
        if line["profileIds"]
            .as_array()
            .is_some_and(|ids| !ids.is_empty())
        {
            found += 1;
        }
        serde_json::to_writer(&mut writer, &line)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    eprintln!(
        "{} TAC and Native cases written to {}, {found} of them finding profiles",
        cases.len(),
        args.out.display()
    );
    Ok(())
}

/// The cases for the values each engine's key property has in the data
/// file, none when it has no such property. First the key in capitals,
/// a native model name in lower case and in upper case, and both keys in
/// one request, each with the first value a profile holds that the engine
/// accepts. Then `property-keyed-tac-value-NNNNNN` for each TAC and
/// `property-keyed-native-value-NNNNNN` for each NativeModel, in the data
/// file's order counting from 1. Last, `property-keyed-profile-<id>` for
/// every [`PROFILE_EVERY`]th distinct profile those cases find through a
/// value the engine accepts, with the evidence of the first case that finds
/// it.
fn value_cases(engines: &Engines) -> Result<Vec<Value>> {
    let device = &engines.device;
    let mut values = Vec::new();
    let mut profiles = Vec::new();
    let mut seen = HashSet::new();
    let mut first_held: Vec<(String, String)> = Vec::new();
    let mut lower = None;
    let mut upper = None;
    for (engine, kind) in engines.both().into_iter().zip(["tac", "native"]) {
        let Some(key) = engine.evidence_key() else {
            continue;
        };
        let property = engine.key_property();
        let all = device
            .property_values(property)
            .with_context(|| format!("reading the {property} values"))?
            .unwrap_or_default();
        let mut held = None;
        for (index, value) in all.into_iter().enumerate() {
            let value = value.name;
            let ids = device
                .profile_ids_for_property_value(property, &value)
                .with_context(|| format!("looking up {property} {value:?}"))?;
            // A value the engine refuses finds no profiles, so the cases
            // below take only values it accepts.
            if !ids.is_empty() && accepted(kind, &value) {
                if held.is_none() {
                    held = Some(value.clone());
                }
                if kind == "native" {
                    if lower.is_none() && value.to_lowercase() != value {
                        lower = Some(value.to_lowercase());
                    }
                    if upper.is_none() && value.to_uppercase() != value {
                        upper = Some(value.to_uppercase());
                    }
                }
                for id in ids {
                    if seen.insert(id) && (seen.len() - 1).is_multiple_of(PROFILE_EVERY) {
                        profiles.push(json!({
                            "id": format!("property-keyed-profile-{id}"),
                            "about": format!("The values of profile {id}, which {property} {value:?} finds."),
                            "evidence": {key: value.clone()},
                            "profileId": id,
                        }));
                    }
                }
            }
            values.push(case(
                format!("property-keyed-{kind}-value-{:06}", index + 1),
                format!("The {property} value {value:?}."),
                [(key.to_owned(), value)],
            ));
        }
        if let Some(value) = held {
            first_held.push((key.to_owned(), value));
        }
    }
    let mut cases = Vec::new();
    for (key, value) in &first_held {
        cases.push(case(
            format!(
                "property-keyed-key-case-{}",
                key.trim_start_matches("query.")
            ),
            format!("The key {key} in capitals, with {value:?}."),
            [(key.to_uppercase(), value.clone())],
        ));
    }
    if let Some((key, _)) = first_held
        .iter()
        .find(|(key, _)| key.ends_with("nativemodel"))
    {
        for (id, about, model) in [
            ("property-keyed-native-lower", "in lower case", lower),
            ("property-keyed-native-upper", "in upper case", upper),
        ] {
            if let Some(model) = model {
                cases.push(case(
                    id.to_owned(),
                    format!("A native model name a profile holds, {about}, {model:?}."),
                    [(key.clone(), model)],
                ));
            }
        }
    }
    if first_held.len() == 2 {
        let mut both = case(
            "property-keyed-both".to_owned(),
            "A TAC and a native model name profiles hold, so both engines add to one data."
                .to_owned(),
            first_held.clone(),
        );
        both["json"] = Value::Bool(true);
        cases.push(both);
    }
    cases.extend(values);
    cases.extend(profiles);
    Ok(cases)
}

/// Whether the engine of `kind` accepts `value` without a warning.
fn accepted(kind: &str, value: &str) -> bool {
    if kind == "tac" {
        value.len() == 8 && value.bytes().all(|byte| byte.is_ascii_digit())
    } else {
        value.encode_utf16().count() >= 2
    }
}

fn case(id: String, about: String, evidence: impl IntoIterator<Item = (String, String)>) -> Value {
    let evidence: Map<String, Value> = evidence
        .into_iter()
        .map(|(key, value)| (key, Value::String(value)))
        .collect();
    json!({"id": id, "about": about, "evidence": evidence})
}

/// The Device Detection engine reading `dd` in LowMemory, indexing `TAC` and
/// `NativeModel`.
fn device_engine(dd: &Path) -> Result<Arc<DeviceDetectionOnPremiseEngine>> {
    DeviceDetectionOnPremiseEngineBuilder::new(dd)
        .performance_profile(PerformanceProfile::LowMemory)
        .indexed_properties([TAC_PROPERTY_NAME, NATIVE_MODEL_PROPERTY_NAME])
        .build()
        .context("building the Device Detection engine")
}

/// Runs one case through `chosen`'s pipeline. The metadata comes from
/// `engines`, the pipeline without the JSON builder.
fn run_case(chosen: &Engines, engines: &Engines, case: &Case) -> Result<Value> {
    let mut evidence = Evidence::builder();
    for (key, value) in &case.evidence {
        let value = value
            .as_str()
            .with_context(|| format!("{}: evidence {key} is not a string", case.id))?;
        evidence = evidence.add(key, value);
    }
    let mut data = chosen.pipeline.create_flow_data_with(evidence.build());
    let process_error = data.process().err().map(|error| error.to_string());
    let errors: Vec<Value> = data
        .errors()
        .iter()
        .map(|error| {
            json!({
                "element": error.element_data_key,
                "message": error.source.message(),
            })
        })
        .collect();
    let warnings: Vec<Value> = data
        .warnings()
        .iter()
        .map(|warning| {
            json!({
                "element": warning.element_data_key,
                "message": warning.message,
            })
        })
        .collect();
    let hardware = data.get(HARDWARE_DATA_KEY);
    let profile_ids = hardware.map(|hardware| {
        hardware
            .profiles()
            .iter()
            .map(profile_id)
            .collect::<Vec<Value>>()
    });
    let properties = case.profile_id.map(|wanted| {
        let wanted = json!(wanted);
        let profile = hardware.and_then(|hardware| {
            hardware
                .profiles()
                .iter()
                .find(|profile| profile_id(profile) == wanted)
        });
        json!({"device": profile.map(|profile| profile_values(profile, engines))})
    });
    let hardware_json = if case.json {
        data.get(JSON_BUILDER_DATA_KEY)
            .and_then(|output| serde_json::from_str::<Value>(output.json()).ok())
            .and_then(|output| output.get(HARDWARE_ELEMENT_DATA_KEY).cloned())
    } else {
        None
    };
    let (metadata, indexed) = if case.engine {
        (
            Value::Array(engines.both().into_iter().map(engine_metadata).collect()),
            indexed_values(engines)?,
        )
    } else {
        (Value::Null, Value::Null)
    };
    Ok(json!({
        "id": case.id,
        "processError": process_error,
        "errors": errors,
        "warnings": warnings,
        "profileIds": profile_ids,
        "properties": properties,
        "hardwareJson": hardware_json,
        "propertyKeyedEngines": metadata,
        "indexedValues": indexed,
    }))
}

/// The id a profile holds, as a number, or as text when it is not one.
fn profile_id(profile: &DeviceDataBase) -> Value {
    match profile.string_property(PROFILE_ID_PROPERTY_NAME).value() {
        Ok(text) => text
            .parse::<u64>()
            .map_or_else(|_| json!(text), |id| json!(id)),
        Err(_) => Value::Null,
    }
}

/// Each property the engines list for a profile, keyed by its lowercase
/// name, read as the engines' pipeline lines read a property.
fn profile_values(profile: &DeviceDataBase, engines: &Engines) -> Value {
    let mut values = Map::new();
    for name in item_names(engines) {
        values.insert(name.to_lowercase(), crate::read(profile, &name, false));
    }
    Value::Object(values)
}

/// The properties the TAC engine lists for each profile, or the Native
/// engine's when the TAC engine lists none.
fn item_names(engines: &Engines) -> Vec<String> {
    engines
        .both()
        .into_iter()
        .map(|engine| {
            engine
                .properties()
                .iter()
                .flat_map(|property| property.item_properties.iter())
                .map(|item| item.name.clone())
                .collect::<Vec<String>>()
        })
        .find(|names| !names.is_empty())
        .unwrap_or_default()
}

/// The engine's data key, data tier, evidence keys in ordinal order and
/// properties in list order, each with its item properties.
fn engine_metadata(engine: &Arc<PropertyKeyedDeviceEngine>) -> Value {
    let mut keys = engine
        .evidence_key_filter()
        .named_keys()
        .unwrap_or_default();
    keys.sort();
    let properties: Vec<Value> = engine
        .aspect_properties()
        .iter()
        .map(|property| {
            let core = property.core();
            let items: Vec<Value> = core
                .item_properties
                .iter()
                .map(|item| {
                    json!({
                        "name": item.name,
                        "category": item.category,
                        "type": value_type_name(item.value_type),
                        "available": item.available,
                    })
                })
                .collect();
            json!({
                "name": property.name(),
                "category": core.category,
                "type": value_type_name(core.value_type),
                "available": property.available(),
                "dataTiers": property.data_tiers_where_present(),
                "description": property.description(),
                "itemProperties": items,
            })
        })
        .collect();
    json!({
        "dataKey": engine.data_key(),
        "dataSourceTier": engine.data_source_tier(),
        "evidenceKeys": keys,
        "properties": properties,
    })
}

/// For each engine whose key property the data file has, each value of that
/// property some profile holds, with how many profiles hold it.
fn indexed_values(engines: &Engines) -> Result<Value> {
    let mut all = Map::new();
    for engine in engines.both() {
        if engine.evidence_key().is_none() {
            continue;
        }
        let property = engine.key_property();
        let mut counts = Map::new();
        let values = engines
            .device
            .property_values(property)
            .with_context(|| format!("reading the {property} values"))?
            .unwrap_or_default();
        for value in values {
            let ids = engines
                .device
                .profile_ids_for_property_value(property, &value.name)
                .with_context(|| format!("looking up {property} {:?}", value.name))?;
            if !ids.is_empty() {
                counts.insert(value.name, json!(ids.len()));
            }
        }
        all.insert(property.to_owned(), Value::Object(counts));
    }
    Ok(Value::Object(all))
}

fn parse_args(args: &[String]) -> Result<Args> {
    let mut corpus = None;
    let mut dd = None;
    let mut out = None;
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let value = rest
            .next()
            .with_context(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--corpus" => corpus = Some(PathBuf::from(value)),
            "--dd" => dd = Some(PathBuf::from(value)),
            "--out" => out = Some(PathBuf::from(value)),
            _ => bail!("unknown argument {flag}"),
        }
    }
    Ok(Args {
        corpus: corpus.context("--corpus is required")?,
        dd: dd.context("--dd is required")?,
        out: out.context("--out is required")?,
    })
}

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

fn writer(out: &Path) -> Result<BufWriter<File>> {
    Ok(BufWriter::new(
        File::create(out).with_context(|| format!("creating {}", out.display()))?,
    ))
}
