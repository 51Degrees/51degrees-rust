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

//! Writes one JSON line per corpus case from the crates in this checkout.
//!
//! ```text
//! parity-rust --corpus <json> --dd <hash file> --ipi <ipi file> --out <jsonl>
//! parity-rust fodid-sign --cases <json> --prefix <text> --out <json>
//! parity-rust fodid --corpus <json> [--corpus <json>] --out <jsonl>
//! parity-rust configuration --cases <json> --dd <hash file> --ipi <ipi file> --out <jsonl>
//! parity-rust derived-cases --cases <folder> --out <json>
//! parity-rust derived --cases <folder> --out <jsonl>
//! parity-rust robots-txt-cases --corpus <json> --dd <hash file> --out <json>
//! parity-rust robots-txt --corpus <json> --dd <hash file> --out <jsonl>
//! parity-rust property-keyed-cases --corpus <json> --dd <hash file> --out <json>
//! parity-rust property-keyed --corpus <json> --dd <hash file> --out <jsonl>
//! parity-rust metadata --cases <json> --dd <hash file> --ipi <ipi file> --out <jsonl>
//! ```
//!
//! The first form runs the evidence cases through the pipelines. The
//! pipelines and the line format are the same as the .NET producer's, as
//! `parity/README.md` describes. Usage records go to sinks inside this
//! program, so nothing is sent anywhere. The next two forms are the 51Did
//! cases, described in `fodid_cases`, the fourth builds pipelines from
//! configuration files, described in `configuration`, the next two are the
//! derived property cases, described in `derived`, the next two are the
//! robots.txt cases, described in `robots_txt`, the next two are the TAC
//! and Native cases, described in `property_keyed`, and the last is the
//! engines' metadata, described in `metadata`.

mod agent_signature;
mod cloud_translation;
mod configuration;
mod derived;
mod fodid_cases;
mod metadata;
mod property_keyed;
mod robots_txt;
mod translation;
mod usage;
mod values;

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use fiftyone_cloud_request_engine::ELEMENT_DATA_KEY as CLOUD_REQUEST_DATA_KEY;
use fiftyone_device_detection_onpremise::DeviceDetectionOnPremiseEngineBuilder;
use fiftyone_ip_intelligence_onpremise::IpIntelligenceOnPremiseEngineBuilder;
use fiftyone_ip_intelligence_shared::IpIntelligenceDataBase;
use fiftyone_javascript_builder::{
    JavaScriptBuilderElementBuilder, JAVASCRIPT_BUILDER_DATA_KEY,
    JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY,
};
use fiftyone_json_builder::{
    JsonBuilderElementBuilder, JSON_BUILDER_DATA_KEY, JSON_BUILDER_ELEMENT_DATA_KEY,
};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{
    ElementData, Evidence, FlowData, FlowElement, Pipeline, PropertyValue,
};
use fiftyone_pipeline_engines_fiftyone::{SequenceElement, ShareUsageElement};
use fiftyone_pipeline_translation::{TranslationData, TranslationValue};

use usage::{CustomUsageCapture, UsageCapture};
use values::ValuesElement;

/// The values read beside an element's listed properties, by the element's
/// data key. Device Detection's match metrics, and the culture the countries
/// translation engine sorted with, which it writes without listing.
const EXTRA_PROPERTIES: &[(&str, &[&str])] = &[
    (
        "device",
        &[
            "DeviceId",
            "Difference",
            "Drift",
            "Iterations",
            "MatchedNodes",
            "Method",
            "UserAgents",
        ],
    ),
    ("countrynamestranslated", &["SortingCultureUsed"]),
];

/// The elements whose properties the line leaves out, by data key.
const SKIPPED_ELEMENTS: &[&str] = &[
    JSON_BUILDER_ELEMENT_DATA_KEY,
    JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY,
    CLOUD_REQUEST_DATA_KEY,
];

/// The evidence the Sequence element reads and writes back.
const SESSION_ID_KEY: &str = "query.session-id";
const SEQUENCE_KEY: &str = "query.sequence";

/// The `pipeline` of a case that runs through the values pipeline.
const VALUES_PIPELINE: &str = "values";

/// The JavaScript builder's settings in the values pipeline.
const VALUES_ENDPOINT: &str = "json";
const VALUES_OBJECT_NAME: &str = "fod";

/// One corpus case. `serde_json` keeps the evidence in file order.
#[derive(Deserialize)]
struct Case {
    id: String,
    evidence: Map<String, Value>,
    /// The pipeline the case runs through, absent for the engines' one.
    #[serde(default)]
    pipeline: Option<String>,
}

struct Args {
    corpus: PathBuf,
    dd: PathBuf,
    ipi: PathBuf,
    out: PathBuf,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some(fodid_cases::SIGN) => return fodid_cases::sign(&args[1..]),
        Some(fodid_cases::READ) => return fodid_cases::read(&args[1..]),
        Some(configuration::RUN) => return configuration::run(&args[1..]),
        Some(derived::CASES) => return derived::cases(&args[1..]),
        Some(derived::RUN) => return derived::run(&args[1..]),
        Some(robots_txt::CASES) => return robots_txt::cases(&args[1..]),
        Some(robots_txt::RUN) => return robots_txt::run(&args[1..]),
        Some(property_keyed::CASES) => return property_keyed::cases(&args[1..]),
        Some(property_keyed::RUN) => return property_keyed::run(&args[1..]),
        Some(metadata::RUN) => return metadata::run(&args[1..]),
        _ => {}
    }
    let args = parse_args(&args)?;
    let text = std::fs::read_to_string(&args.corpus)
        .with_context(|| format!("reading {}", args.corpus.display()))?;
    let cases: Vec<Case> = serde_json::from_str(&text).context("parsing the corpus")?;
    let capture = Arc::new(UsageCapture::default());
    let custom_capture = Arc::new(CustomUsageCapture::default());
    let pipeline = build_pipeline(&args, &capture, &custom_capture)?;
    let values = build_values_pipeline()?;
    let signatures = agent_signature::build_pipeline()?;
    let translation = translation::build_pipeline()?;
    let cloud_translation = cloud_translation::build_pipeline()?;

    let mut lines = Vec::with_capacity(cases.len());
    // The lines whose case ran through usage sharing. A values case may give
    // a session id an engines' case also gives, and must not be given that
    // case's record.
    let mut shared = Vec::new();
    for case in &cases {
        let chosen = match case.pipeline.as_deref() {
            None => &pipeline,
            Some(VALUES_PIPELINE) => &values,
            Some(agent_signature::PIPELINE) => &signatures,
            Some(translation::PIPELINE) => &translation,
            Some(cloud_translation::PIPELINE) => &cloud_translation,
            Some(other) => bail!("{}: unknown pipeline {other}", case.id),
        };
        if Arc::ptr_eq(chosen, &pipeline) {
            shared.push(lines.len());
        }
        let missing_fails = case.pipeline.as_deref() == Some(cloud_translation::PIPELINE);
        lines.push(run_case(chosen, case, missing_fails)?);
    }

    // Dropping the pipeline drops usage sharing, which delivers what it holds
    // before the drop returns.
    let pipeline = Arc::try_unwrap(pipeline).map_err(|_| {
        anyhow!(
            "the engines' pipeline is still held, so usage sharing has not delivered its records"
        )
    })?;
    drop(pipeline);
    let records = capture.records_by_session();
    let custom_records = custom_capture.records_by_session();
    for index in shared {
        let line = &mut lines[index];
        let session_id = line["session"]["id"].as_str().map(str::to_owned);
        line["usage"] = record_for(&records, session_id.as_deref());
        line["usageCustom"] = record_for(&custom_records, session_id.as_deref());
    }

    let mut out = BufWriter::new(
        File::create(&args.out).with_context(|| format!("creating {}", args.out.display()))?,
    );
    for line in &lines {
        serde_json::to_writer(&mut out, line)?;
        out.write_all(b"\n")?;
    }
    out.flush()?;
    eprintln!(
        "{} cases written to {}, {} usage records and {} custom usage records captured",
        cases.len(),
        args.out.display(),
        records.len(),
        custom_records.len()
    );
    Ok(())
}

/// The record usage sharing wrote with `session_id`, or null.
fn record_for(records: &HashMap<String, String>, session_id: Option<&str>) -> Value {
    session_id
        .and_then(|id| records.get(id))
        .map_or(Value::Null, |record| Value::String(record.clone()))
}

fn parse_args(args: &[String]) -> Result<Args> {
    let mut corpus = None;
    let mut dd = None;
    let mut ipi = None;
    let mut out = None;
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .with_context(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--corpus" => corpus = Some(PathBuf::from(value)),
            "--dd" => dd = Some(PathBuf::from(value)),
            "--ipi" => ipi = Some(PathBuf::from(value)),
            "--out" => out = Some(PathBuf::from(value)),
            _ => bail!("unknown argument {flag}"),
        }
    }
    Ok(Args {
        corpus: corpus.context("--corpus is required")?,
        dd: dd.context("--dd is required")?,
        ipi: ipi.context("--ipi is required")?,
        out: out.context("--out is required")?,
    })
}

/// Sequence, usage sharing into each capture, Device Detection, IP
/// Intelligence, then the JSON and JavaScript builders, with process errors
/// collected rather than returned, as the .NET producer builds it.
fn build_pipeline(
    args: &Args,
    capture: &Arc<UsageCapture>,
    custom_capture: &Arc<CustomUsageCapture>,
) -> Result<Arc<Pipeline>> {
    let device = DeviceDetectionOnPremiseEngineBuilder::new(&args.dd)
        .performance_profile(PerformanceProfile::HighPerformance)
        .build()
        .context("building the Device Detection engine")?;
    let ip = IpIntelligenceOnPremiseEngineBuilder::new(&args.ipi)
        .performance_profile(PerformanceProfile::LowMemory)
        .auto_update(false)
        .file_system_watcher(false)
        .update_on_startup(false)
        .build_shared()
        .context("building the IP Intelligence engine")?;
    let javascript = JavaScriptBuilderElementBuilder::new()
        .set_minify(false)
        .build();
    let shared = ShareUsageElement::with_sink(usage::config(), Arc::clone(capture));
    let custom = ShareUsageElement::with_sink(usage::config(), Arc::clone(custom_capture));
    Ok(Pipeline::builder()
        .add_element(Arc::new(SequenceElement::new()))
        .add_element(Arc::new(shared))
        .add_element(Arc::new(custom))
        .add_element(device)
        .add_element(ip)
        .add_element(Arc::new(JsonBuilderElementBuilder::new().build()))
        .add_element(Arc::new(javascript))
        .suppress_process_exceptions(true)
        .build()?)
}

/// Sequence, the values element and the two builders, for the cases that
/// compare the JSON and the JavaScript byte for byte. The crates' Sequence
/// element has data of its own, which .NET's does not, so the JSON builder
/// here leaves that data out.
fn build_values_pipeline() -> Result<Arc<Pipeline>> {
    let sequence = Arc::new(SequenceElement::new());
    let json = JsonBuilderElementBuilder::new()
        .exclude_element(sequence.data_key())
        .build();
    let javascript = JavaScriptBuilderElementBuilder::new()
        .set_endpoint(VALUES_ENDPOINT)
        .set_object_name(VALUES_OBJECT_NAME)?
        .set_enable_cookies(false)
        .set_minify(false)
        .build();
    Ok(Pipeline::builder()
        .add_element(sequence)
        .add_element(Arc::new(ValuesElement::new()))
        .add_element(Arc::new(json))
        .add_element(Arc::new(javascript))
        .suppress_process_exceptions(true)
        .build()?)
}

/// Run one case. With `missing_fails`, a property the data does not hold is
/// written as missing, as .NET's cloud data fails such a read where its
/// on-premise data gives null.
fn run_case(pipeline: &Arc<Pipeline>, case: &Case, missing_fails: bool) -> Result<Value> {
    let mut evidence = Evidence::builder();
    for (key, value) in &case.evidence {
        let value = value
            .as_str()
            .with_context(|| format!("{}: evidence {key} is not a string", case.id))?;
        evidence = evidence.add(key, value);
    }
    let mut data = pipeline.create_flow_data_with(evidence.build());
    let process_error = data.process().err().map(|error| error.to_string());

    let evidence_after = data.evidence();
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
    Ok(json!({
        "id": case.id,
        "processError": process_error,
        "properties": properties(&data, missing_fails),
        "session": {
            "id": evidence_after.get(SESSION_ID_KEY),
            "sequence": evidence_after.get(SEQUENCE_KEY).and_then(read_long),
        },
        "evidenceAfter": {
            SESSION_ID_KEY: evidence_after.get(SESSION_ID_KEY),
            SEQUENCE_KEY: evidence_after.get(SEQUENCE_KEY),
        },
        "errors": errors,
        "json": data.get(JSON_BUILDER_DATA_KEY).map(|d| d.json()),
        "javascript": data.get(JAVASCRIPT_BUILDER_DATA_KEY).map(|d| d.javascript()),
        "usage": Value::Null,
        "usageCustom": Value::Null,
    }))
}

/// `text` as a 64-bit integer, read as the .NET producer's `long.TryParse`
/// reads it, allowing white space either side and a sign.
fn read_long(text: &str) -> Option<i64> {
    text.trim_matches(|c| matches!(c, ' ' | '\t'..='\r'))
        .parse()
        .ok()
}

/// Every property of every element that lists properties, keyed by the
/// element's data key and the lowercase property name. The two builders are
/// left out because their whole output is recorded instead, and the cloud
/// request engine because its data is the case's own response.
fn properties(data: &FlowData, missing_fails: bool) -> Value {
    let mut elements = Map::new();
    for element in data.pipeline().flow_elements() {
        let key = element.data_key();
        if SKIPPED_ELEMENTS.contains(&key) {
            continue;
        }
        let mut names: Vec<String> = element
            .properties()
            .iter()
            .map(|p| p.name.clone())
            .collect();
        if names.is_empty() {
            continue;
        }
        let extra = EXTRA_PROPERTIES
            .iter()
            .filter(|(element, _)| *element == key)
            .flat_map(|(_, extra)| extra.iter());
        for name in extra {
            if !names.iter().any(|listed| listed.eq_ignore_ascii_case(name)) {
                names.push((*name).to_owned());
            }
        }
        let Some(element_data) = data.get_str(key) else {
            elements.insert(key.to_owned(), Value::Null);
            continue;
        };
        let mut values = Map::new();
        for name in names {
            values.insert(
                name.to_lowercase(),
                read(element_data, &name, missing_fails),
            );
        }
        elements.insert(key.to_owned(), Value::Object(values));
    }
    Value::Object(elements)
}

fn read(data: &dyn ElementData, name: &str, missing_fails: bool) -> Value {
    match data.get(name) {
        Ok(PropertyValue::KeyValueList(records)) => match weighted(data, name) {
            Some(list) => json!({"name": name, "type": "weightedList", "value": list}),
            None => {
                let (kind, value) = typed(PropertyValue::KeyValueList(records));
                json!({"name": name, "type": kind, "value": value})
            }
        },
        Ok(value) => {
            let (kind, value) = typed(value);
            json!({"name": name, "type": kind, "value": value})
        }
        Err(error) if missing_fails && !data.has_property(name) => {
            json!({"name": name, "missing": error.message})
        }
        Err(error) => json!({"name": name, "noValue": error.message}),
    }
}

/// A weighted list with its raw weightings. The IP Intelligence property bag
/// only carries them as fractions, and a translation engine's leaves out an
/// item with no translation, so each is read from its own data.
fn weighted(data: &dyn ElementData, name: &str) -> Option<Value> {
    if let Some(ip) = data.as_any().downcast_ref::<IpIntelligenceDataBase>() {
        let list = ip.weighted_string(name);
        let list = list.value().ok()?;
        return Some(Value::Array(
            list.iter()
                .map(|item| json!({"rawWeighting": item.raw_weighting, "value": item.value}))
                .collect(),
        ));
    }
    let translation = data.as_any().downcast_ref::<TranslationData>()?;
    match translation.value(name)?.value().ok()? {
        TranslationValue::WeightedList(list) => Some(Value::Array(
            list.iter()
                .map(|item| json!({"rawWeighting": item.raw_weighting, "value": item.value}))
                .collect(),
        )),
        _ => None,
    }
}

fn typed(value: PropertyValue) -> (&'static str, Value) {
    match value {
        PropertyValue::String(s) => ("string", json!(s)),
        PropertyValue::Bool(b) => ("bool", json!(b)),
        PropertyValue::Integer(i) => ("integer", json!(i)),
        PropertyValue::Double(d) => ("double", json!(d)),
        PropertyValue::Float(f) => ("double", json!(f64::from(f))),
        PropertyValue::StringList(list) => ("stringList", json!(list)),
        PropertyValue::JavaScript(s) => ("javascript", json!(s)),
        PropertyValue::WktString(text) => ("wktString", json!(text.as_str())),
        PropertyValue::KeyValueList(records) => (
            "keyValueList",
            Value::Array(
                records
                    .into_iter()
                    .map(|record| {
                        let mut entry = Map::new();
                        for (key, value) in record {
                            let (kind, value) = typed(value);
                            entry.insert(key, json!({"type": kind, "value": value}));
                        }
                        Value::Object(entry)
                    })
                    .collect(),
            ),
        ),
        // The enum is non-exhaustive, so a type added later still shows.
        other => ("other", json!(format!("{other:?}"))),
    }
}
