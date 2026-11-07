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

//! The robots.txt cases: requests to the robots.txt engine, in a pipeline
//! with the on-premise Device Detection engine, whose data file holds the
//! crawlers and the usages.
//!
//! ```text
//! parity-rust robots-txt-cases --corpus <json> --dd <hash file> --out <json>
//! parity-rust robots-txt --corpus <json> --dd <hash file> --out <jsonl>
//! ```
//!
//! The first writes the fixed cases of `--corpus` and, after them, the cases
//! for the usages the data file holds, as a corpus for the comparer and for
//! both producers. The second writes one line for each case.
//! `parity/README.md` ("The robots.txt comparison") describes both. The .NET
//! producer's `RobotsTxtCases` builds the same pipeline.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use url::Url;

use fiftyone_device_detection_onpremise::DeviceDetectionOnPremiseEngineBuilder;
use fiftyone_device_detection_robots_txt::{
    RobotsTxtEngine, TdlSourceResolver, ROBOTS_TXT_DATA_KEY,
};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{Evidence, EvidenceKeyFilter, FlowElement, Pipeline};
use fiftyone_pipeline_engines::AspectEngine;

use crate::fodid_cases::value_type_name;

/// The first argument that writes the corpus.
pub const CASES: &str = "robots-txt-cases";

/// The first argument that writes the lines.
pub const RUN: &str = "robots-txt";

/// The source id the resolver resolves, compared ignoring case.
const KNOWN_ID: &str = "parity-tdl";

/// The URI [`KNOWN_ID`] resolves to.
const KNOWN_URI: &str = "https://tdl.parity.test/terms/3.txt";

/// The source id the resolver knows but cannot resolve, as when a lookup
/// fails with no earlier answer.
const UNRESOLVED_ID: &str = "parity-unresolved";

/// The values the usage cases give a usage's evidence key.
const ALLOW: &str = "allow";
const DISALLOW: &str = "disallow";

/// The values given to the first usage's key, one case each. Every value the
/// engine accepts in a different case, two values it does not accept, and
/// one with spaces around it.
const SPELLINGS: &[&str] = &[
    "Allow", "TRUE", "Yes", "Enabled", "ON", "Disallow", "FALSE", "No", "Disabled", "OFF", "maybe",
    "", " allow ",
];

/// The URI of the case that gives every usage and a Terms Document Locator.
const TDL_URI: &str = "https://m4ow.uk/socw/1.txt";

/// The key Terms Document Locator URIs are given under.
const TDL_KEY: &str = "query.robotstxt.tdl";

/// One case. `serde_json` keeps the evidence in file order.
#[derive(Deserialize)]
struct Case {
    id: String,
    evidence: Map<String, Value>,
    /// Whether the line holds the engine's evidence keys and properties.
    #[serde(default)]
    engine: bool,
}

/// Resolves [`KNOWN_ID`] and knows [`UNRESOLVED_ID`], so no case needs the
/// network. The .NET producer's resolver answers the same.
struct ParityResolver;

impl TdlSourceResolver for ParityResolver {
    fn is_known(&self, id: &str) -> bool {
        id.eq_ignore_ascii_case(KNOWN_ID) || id.eq_ignore_ascii_case(UNRESOLVED_ID)
    }

    fn resolve(&self, id: &str) -> Option<Url> {
        if id.eq_ignore_ascii_case(KNOWN_ID) {
            Url::parse(KNOWN_URI).ok()
        } else {
            None
        }
    }
}

struct Args {
    corpus: PathBuf,
    dd: PathBuf,
    out: PathBuf,
}

/// Writes the fixed cases of `--corpus`, then the cases for each usage the
/// engine reads from `--dd`.
pub fn cases(args: &[String]) -> Result<()> {
    let args = parse_args(args)?;
    let mut corpus: Vec<Value> = serde_json::from_str(&read(&args.corpus)?)
        .with_context(|| format!("parsing {}", args.corpus.display()))?;
    let (_pipeline, engine) = build_pipeline(&args.dd)?;
    corpus.extend(usage_cases(&engine.usage_evidence_keys()));
    let mut writer = writer(&args.out)?;
    serde_json::to_writer_pretty(&mut writer, &corpus)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

/// Writes one line for each case in `--corpus`.
pub fn run(args: &[String]) -> Result<()> {
    let args = parse_args(args)?;
    let cases: Vec<Case> = serde_json::from_str(&read(&args.corpus)?)
        .with_context(|| format!("parsing {}", args.corpus.display()))?;
    let (pipeline, engine) = build_pipeline(&args.dd)?;
    let mut writer = writer(&args.out)?;
    for case in &cases {
        let line = run_case(&pipeline, &engine, case)?;
        serde_json::to_writer(&mut writer, &line)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    eprintln!(
        "{} robots.txt cases written to {}, with {} usage keys",
        cases.len(),
        args.out.display(),
        engine.usage_evidence_keys().len()
    );
    Ok(())
}

/// The cases for the usages whose evidence keys are `keys`. Each usage
/// allowed alone and refused alone, every usage allowed, every usage but the
/// first allowed, every usage allowed with a Terms Document Locator, the
/// first usage's key in capitals, and each of [`SPELLINGS`] for the first
/// usage. None when the data file holds no usages.
fn usage_cases(keys: &[String]) -> Vec<Value> {
    let mut cases = Vec::new();
    for (index, key) in keys.iter().enumerate() {
        let place = index + 1;
        cases.push(case(
            format!("robots-txt-allow-{place:02}"),
            format!("Allows the usage {key} names, alone."),
            [(key.clone(), ALLOW.to_owned())],
        ));
        cases.push(case(
            format!("robots-txt-disallow-{place:02}"),
            format!("Refuses the usage {key} names, alone."),
            [(key.clone(), DISALLOW.to_owned())],
        ));
    }
    let Some(first) = keys.first() else {
        return cases;
    };
    let every = |first_value: &str| -> Vec<(String, String)> {
        keys.iter()
            .enumerate()
            .map(|(index, key)| {
                let value = if index == 0 { first_value } else { ALLOW };
                (key.clone(), value.to_owned())
            })
            .collect()
    };
    cases.push(case(
        "robots-txt-allow-every".to_owned(),
        "Allows every usage.".to_owned(),
        every(ALLOW),
    ));
    cases.push(case(
        "robots-txt-allow-every-but-first".to_owned(),
        format!("Allows every usage but the one {first} names, which it refuses."),
        every(DISALLOW),
    ));
    let mut with_tdl = every(ALLOW);
    with_tdl.push((TDL_KEY.to_owned(), TDL_URI.to_owned()));
    cases.push(case(
        "robots-txt-allow-every-with-tdl".to_owned(),
        "Allows every usage and gives a Terms Document Locator URI.".to_owned(),
        with_tdl,
    ));
    cases.push(case(
        "robots-txt-key-case".to_owned(),
        format!("Allows the usage {first} names, with the key in capitals."),
        [(first.to_uppercase(), ALLOW.to_owned())],
    ));
    for (index, value) in SPELLINGS.iter().enumerate() {
        let place = index + 1;
        cases.push(case(
            format!("robots-txt-value-{place:02}"),
            format!("Gives {first} the value '{value}'."),
            [(first.clone(), (*value).to_owned())],
        ));
    }
    cases
}

fn case(id: String, about: String, evidence: impl IntoIterator<Item = (String, String)>) -> Value {
    let evidence: Map<String, Value> = evidence
        .into_iter()
        .map(|(key, value)| (key, Value::String(value)))
        .collect();
    json!({"id": id, "about": about, "evidence": evidence})
}

/// The Device Detection engine reading `dd`, then the robots.txt engine with
/// [`ParityResolver`], with process errors collected rather than returned.
fn build_pipeline(dd: &Path) -> Result<(Arc<Pipeline>, Arc<RobotsTxtEngine>)> {
    let device = DeviceDetectionOnPremiseEngineBuilder::new(dd)
        .performance_profile(PerformanceProfile::LowMemory)
        .build()
        .context("building the Device Detection engine")?;
    let robots = RobotsTxtEngine::builder()
        .set_tdl_source_resolver(Some(Arc::new(ParityResolver)))
        .build()
        .context("building the robots.txt engine")?;
    let pipeline = Pipeline::builder()
        .add_element(device as Arc<dyn FlowElement>)
        .add_element(Arc::clone(&robots) as Arc<dyn FlowElement>)
        .suppress_process_exceptions(true)
        .build()
        .context("building the robots.txt pipeline")?;
    Ok((pipeline, robots))
}

/// Runs one case. The texts are null when the engine wrote no data, or the
/// property has no value.
fn run_case(pipeline: &Arc<Pipeline>, engine: &RobotsTxtEngine, case: &Case) -> Result<Value> {
    let mut evidence = Evidence::builder();
    for (key, value) in &case.evidence {
        let value = value
            .as_str()
            .with_context(|| format!("{}: evidence {key} is not a string", case.id))?;
        evidence = evidence.add(key, value);
    }
    let mut data = pipeline.create_flow_data_with(evidence.build());
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
    let robots = data.get(ROBOTS_TXT_DATA_KEY);
    let plain = robots.and_then(|data| data.plain_text().into_value().ok());
    let annotated = robots.and_then(|data| data.annotated_text().into_value().ok());
    let metadata = if case.engine {
        engine_metadata(engine)
    } else {
        Value::Null
    };
    Ok(json!({
        "id": case.id,
        "processError": process_error,
        "errors": errors,
        "robotsTxtEngine": metadata,
        "plainText": plain,
        "annotatedText": annotated,
    }))
}

/// The engine's data key, data tier, evidence keys in ordinal order and
/// properties in list order.
fn engine_metadata(engine: &RobotsTxtEngine) -> Value {
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
            json!({
                "name": property.name(),
                "category": core.category,
                "type": value_type_name(core.value_type),
                "available": property.available(),
                "dataTiers": property.data_tiers_where_present(),
                "description": property.description(),
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
