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
//! ```
//!
//! The pipelines and the line format are the same as the .NET producer's, as
//! `parity/README.md` describes. Usage sharing is left out of the pipeline,
//! because the crates have no public sink for its records yet, so `usage` is
//! always null.

mod agent_signature;
mod values;

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Map, Value};

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
use fiftyone_pipeline_engines_fiftyone::SequenceElement;

use values::ValuesElement;

/// The element whose data also gets the match metrics read.
const DEVICE_DATA_KEY: &str = "device";

/// Match metrics, read whether or not the engine lists them as properties.
const MATCH_METRICS: &[&str] = &[
    "DeviceId",
    "Difference",
    "Drift",
    "Iterations",
    "MatchedNodes",
    "Method",
    "UserAgents",
];

/// The evidence the Sequence element reads, and in .NET writes back.
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
    let args = parse_args()?;
    let text = std::fs::read_to_string(&args.corpus)
        .with_context(|| format!("reading {}", args.corpus.display()))?;
    let cases: Vec<Case> = serde_json::from_str(&text).context("parsing the corpus")?;
    let pipeline = build_pipeline(&args)?;
    let values = build_values_pipeline()?;
    let signatures = agent_signature::build_pipeline()?;

    let mut out = BufWriter::new(
        File::create(&args.out).with_context(|| format!("creating {}", args.out.display()))?,
    );
    for case in &cases {
        let chosen = match case.pipeline.as_deref() {
            None => &pipeline,
            Some(VALUES_PIPELINE) => &values,
            Some(agent_signature::PIPELINE) => &signatures,
            Some(other) => bail!("{}: unknown pipeline {other}", case.id),
        };
        let line = run_case(chosen, case)?;
        serde_json::to_writer(&mut out, &line)?;
        out.write_all(b"\n")?;
    }
    out.flush()?;
    eprintln!("{} cases written to {}", cases.len(), args.out.display());
    Ok(())
}

fn parse_args() -> Result<Args> {
    let mut corpus = None;
    let mut dd = None;
    let mut ipi = None;
    let mut out = None;
    let mut args = std::env::args().skip(1);
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

/// Sequence, Device Detection, IP Intelligence, then the JSON and JavaScript
/// builders, with process errors collected rather than returned, as the .NET
/// producer builds it.
fn build_pipeline(args: &Args) -> Result<Arc<Pipeline>> {
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
    Ok(Pipeline::builder()
        .add_element(Arc::new(SequenceElement::new()))
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

fn run_case(pipeline: &Arc<Pipeline>, case: &Case) -> Result<Value> {
    let mut evidence = Evidence::builder();
    for (key, value) in &case.evidence {
        let value = value
            .as_str()
            .with_context(|| format!("{}: evidence {key} is not a string", case.id))?;
        evidence = evidence.add(key, value);
    }
    let mut data = pipeline.create_flow_data_with(evidence.build());
    let process_error = data.process().err().map(|error| error.to_string());

    let session = data.get(SequenceElement::KEY);
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
        "properties": properties(&data),
        "session": {
            "id": session.and_then(|s| s.session_id()),
            "sequence": session.and_then(|s| s.sequence()),
        },
        "evidenceAfter": {
            SESSION_ID_KEY: data.evidence().get(SESSION_ID_KEY),
            SEQUENCE_KEY: data.evidence().get(SEQUENCE_KEY),
        },
        "errors": errors,
        "json": data.get(JSON_BUILDER_DATA_KEY).map(|d| d.json()),
        "javascript": data.get(JAVASCRIPT_BUILDER_DATA_KEY).map(|d| d.javascript()),
        "usage": Value::Null,
    }))
}

/// Every property of every element that lists properties, keyed by the
/// element's data key and the lowercase property name. The two builders are
/// left out because their whole output is recorded instead.
fn properties(data: &FlowData) -> Value {
    let mut elements = Map::new();
    for element in data.pipeline().flow_elements() {
        let key = element.data_key();
        if key == JSON_BUILDER_ELEMENT_DATA_KEY || key == JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY {
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
        if key == DEVICE_DATA_KEY {
            for metric in MATCH_METRICS {
                if !names.iter().any(|name| name.eq_ignore_ascii_case(metric)) {
                    names.push((*metric).to_owned());
                }
            }
        }
        let Some(element_data) = data.get_str(key) else {
            elements.insert(key.to_owned(), Value::Null);
            continue;
        };
        let mut values = Map::new();
        for name in names {
            values.insert(name.to_lowercase(), read(element_data, &name));
        }
        elements.insert(key.to_owned(), Value::Object(values));
    }
    Value::Object(elements)
}

fn read(data: &dyn ElementData, name: &str) -> Value {
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
        Err(error) => json!({"name": name, "noValue": error.message}),
    }
}

/// An IP Intelligence weighted list with its raw weightings, which the
/// property bag only carries as fractions.
fn weighted(data: &dyn ElementData, name: &str) -> Option<Value> {
    let ip = data.as_any().downcast_ref::<IpIntelligenceDataBase>()?;
    let list = ip.weighted_string(name);
    let list = list.value().ok()?;
    Some(Value::Array(
        list.iter()
            .map(|item| json!({"rawWeighting": item.raw_weighting, "value": item.value}))
            .collect(),
    ))
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
