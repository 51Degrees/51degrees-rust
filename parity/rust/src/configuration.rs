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

//! The configuration cases: each builds a pipeline from a configuration
//! file in .NET's shape and writes what was built, or the error.
//!
//! ```text
//! parity-rust configuration --cases <json> --dd <hash file> --ipi <ipi file> --out <jsonl>
//! ```
//!
//! `parity/README.md` ("The configuration comparison") describes the cases
//! and the line. Nothing is processed, so the usage sharing element sends
//! nothing.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};

use fiftyone_device_detection_onpremise::DeviceDetectionOnPremiseEngine;
use fiftyone_ip_intelligence_onpremise::IpIntelligenceOnPremiseEngine;
use fiftyone_pipeline_core::configuration::{BuilderRegistry, PipelineOptions};
use fiftyone_pipeline_core::{Error, EvidenceKeyFilter, FlowElement, Pipeline, PipelineBuilder};
use fiftyone_pipeline_engines::{AspectEngineDataFile, OnPremiseAspectEngine};
use fiftyone_pipeline_engines_fiftyone::constants::SHARE_USAGE_DEFAULT_ELEMENT_DATA_KEY;
use fiftyone_pipeline_engines_fiftyone::{ShareUsageConfig, ShareUsageElement};

/// The first argument that selects this form.
pub const RUN: &str = "configuration";

/// The evidence keys given to the usage sharing element's filter, whose
/// answers show the blocked headers, the query string parameters shared and
/// sharing all evidence as the element applies them, because .NET holds
/// those settings only in that filter.
const USAGE_FILTER_KEYS: &[&str] = &[
    "header.user-agent",
    "header.referer",
    "header.x-forwarded-for",
    "header.accept-language",
    "query.utm_source",
    "query.utm_medium",
    "query.utm_campaign",
    "cookie.other",
];

/// The text in a case's configuration that stands for each data file.
const DD_PLACEHOLDER: &str = "{dd}";
const IPI_PLACEHOLDER: &str = "{ipi}";

/// One configuration case, given as JSON or as the text of a file.
#[derive(Deserialize)]
struct Case {
    id: String,
    #[serde(default)]
    configuration: Value,
    text: Option<String>,
}

/// Writes one line for each case in `--cases`.
pub fn run(args: &[String]) -> Result<()> {
    let mut cases = None;
    let mut dd = None;
    let mut ipi = None;
    let mut out = None;
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let value = rest
            .next()
            .with_context(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--cases" => cases = Some(PathBuf::from(value)),
            "--dd" => dd = Some(absolute(value)?),
            "--ipi" => ipi = Some(absolute(value)?),
            "--out" => out = Some(PathBuf::from(value)),
            _ => bail!("unknown argument {flag}"),
        }
    }
    let cases = cases.context("--cases is required")?;
    let files = [
        (DD_PLACEHOLDER, dd.context("--dd is required")?),
        (IPI_PLACEHOLDER, ipi.context("--ipi is required")?),
    ];
    let out = out.context("--out is required")?;

    let text =
        std::fs::read_to_string(&cases).with_context(|| format!("reading {}", cases.display()))?;
    let cases: Vec<Case> =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", cases.display()))?;
    let registry = registry();
    let mut writer =
        BufWriter::new(File::create(&out).with_context(|| format!("creating {}", out.display()))?);
    for case in cases {
        let text = match &case.text {
            Some(text) => text_with_files(text, &files)?,
            None => serde_json::to_string(&with_files(&case.configuration, &files))?,
        };
        let line = json!({
            "id": case.id,
            "configuration": describe_case(&registry, &text, &files)?,
        });
        serde_json::to_writer(&mut writer, &line)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    Ok(())
}

fn absolute(path: &str) -> Result<String> {
    let path = std::path::absolute(path).with_context(|| format!("resolving {path}"))?;
    Ok(path.to_string_lossy().into_owned())
}

/// The builders of the crates this program uses, in the order .NET lists
/// those of the packages the .NET program loads, making pipelines as .NET's
/// `FiftyOnePipelineBuilder` does.
fn registry() -> BuilderRegistry {
    let mut registry = BuilderRegistry::new();
    fiftyone_pipeline_engines_fiftyone::configuration::add_builders(&mut registry);
    fiftyone_device_detection_onpremise::configuration::add_builders(&mut registry);
    fiftyone_ip_intelligence_onpremise::configuration::add_builders(&mut registry);
    fiftyone_json_builder::configuration::add_builders(&mut registry);
    fiftyone_javascript_builder::configuration::add_builders(&mut registry);
    fiftyone_cloud_request_engine::configuration::add_builders(&mut registry);
    fiftyone_agent_signature::configuration::add_builders(&mut registry);
    fiftyone_ip_intelligence_translation::configuration::add_builders(&mut registry);
    fiftyone_ip_intelligence_cloud::configuration::add_builders(&mut registry);
    fiftyone_device_detection_robots_txt::configuration::add_builders(&mut registry);
    fiftyone_pipeline_engines_fiftyone::configuration::use_fiftyone_pipeline_builder(&mut registry);
    registry
}

/// What building the configuration file `text` gave: the pipeline's
/// settings and its elements, or the error.
fn describe_case(
    registry: &BuilderRegistry,
    text: &str,
    files: &[(&str, String)],
) -> Result<Value> {
    let built = PipelineOptions::from_json(text)
        .and_then(|options| PipelineBuilder::new().build_from_configuration(registry, &options));
    let pipeline = match built {
        Ok(pipeline) => pipeline,
        Err(Error::PipelineConfiguration { message }) => return Ok(json!({ "error": message })),
        Err(other) => bail!("not a configuration error: {other}"),
    };
    let elements = pipeline
        .flow_elements()
        .iter()
        .map(|element| describe_element(&pipeline, element, files))
        .collect::<Vec<_>>();
    Ok(json!({
        "error": null,
        "suppressProcessExceptions": pipeline.is_suppress_process_exceptions(),
        "concurrent": pipeline.is_concurrent(),
        "elements": elements,
    }))
}

/// A configuration file given as text, with each placeholder written as a
/// JSON string replaced by its data file's path.
fn text_with_files(text: &str, files: &[(&str, String)]) -> Result<String> {
    let mut text = text.to_owned();
    for (placeholder, path) in files {
        text = text.replace(
            &serde_json::to_string(placeholder)?,
            &serde_json::to_string(path)?,
        );
    }
    Ok(text)
}

/// `configuration` with each string that is a placeholder replaced by its
/// data file's path.
fn with_files(configuration: &Value, files: &[(&str, String)]) -> Value {
    match configuration {
        Value::String(text) => files
            .iter()
            .find(|(placeholder, _)| placeholder == text)
            .map_or_else(|| configuration.clone(), |(_, path)| json!(path)),
        Value::Array(items) => items.iter().map(|v| with_files(v, files)).collect(),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), with_files(v, files)))
                .collect(),
        ),
        _ => configuration.clone(),
    }
}

/// An element's data key and the settings read back from it.
fn describe_element(
    pipeline: &Pipeline,
    element: &Arc<dyn FlowElement>,
    files: &[(&str, String)],
) -> Value {
    let key = element.data_key();
    let mut out = json!({ "dataKey": key });
    let data_file = pipeline
        .resource::<DeviceDetectionOnPremiseEngine>()
        .filter(|engine| engine.data_key() == key)
        .map(|engine| Arc::clone(&engine.data_files()[0]))
        .or_else(|| {
            pipeline
                .resource::<IpIntelligenceOnPremiseEngine>()
                .filter(|engine| engine.data_key() == key)
                .map(|engine| Arc::clone(&engine.data_files()[0]))
        });
    if let Some(file) = data_file {
        out["dataFile"] = describe_data_file(&file, files);
    }
    if key == SHARE_USAGE_DEFAULT_ELEMENT_DATA_KEY {
        // The element's own settings, as .NET reads its element's fields.
        if let Some(share) = Arc::clone(element)
            .into_any()
            .and_then(|element| element.downcast::<ShareUsageElement>().ok())
        {
            out["settings"] = describe_share_usage(share.config());
            // The rules for what is shared. The element accepts no evidence,
            // and .NET's element holds these rules as its evidence filter.
            let filter = share.shared_evidence_filter();
            out["evidenceFilter"] = USAGE_FILTER_KEYS
                .iter()
                .map(|key| (key.to_string(), json!(filter.include(key))))
                .collect::<serde_json::Map<_, _>>()
                .into();
        }
    }
    out
}

/// The data file settings, with the data file's path given as its
/// placeholder.
fn describe_data_file(file: &AspectEngineDataFile, files: &[(&str, String)]) -> Value {
    let configuration = file.configuration();
    let path = configuration
        .data_file_path
        .as_deref()
        .map(|path| placeholder(path, files));
    json!({
        "dataFilePath": path,
        "automaticUpdatesEnabled": configuration.automatic_updates_enabled,
        "fileSystemWatcherEnabled": configuration.file_system_watcher_enabled,
        "updateOnStartup": configuration.update_on_startup,
        "pollingIntervalSeconds": configuration.polling_interval_seconds,
        "maxRandomisationSeconds": configuration.max_randomisation_seconds,
        "dataUpdateUrl": configuration.data_update_url,
        "dataUpdateLicenseKeys": configuration.data_update_license_keys,
        "verifyMd5": configuration.verify_md5,
        "decompressContent": configuration.decompress_content,
        "verifyModifiedSince": configuration.verify_modified_since,
        "formattedUrl": file.formatted_update_url(),
    })
}

fn placeholder(path: &Path, files: &[(&str, String)]) -> String {
    let text = path.to_string_lossy();
    files.iter().find(|(_, file)| *file == text).map_or_else(
        || text.into_owned(),
        |(placeholder, _)| placeholder.to_string(),
    )
}

/// The usage sharing settings. The blocked headers, the query string
/// parameters and sharing all evidence are read back through the element's
/// evidence filter instead, as .NET holds them only there.
fn describe_share_usage(config: &ShareUsageConfig) -> Value {
    json!({
        "sharePercentage": config.share_percentage(),
        "minimumEntriesPerMessage": config.minimum_entries_per_message(),
        "maximumQueueSize": config.maximum_queue_size(),
        "addTimeoutMilliseconds": config.add_timeout().as_millis() as u64,
        "takeTimeoutMilliseconds": config.take_timeout().as_millis() as u64,
        "shareUsageUrl": config.share_usage_url(),
        "repeatEvidenceIntervalMinutes": config.repeat_evidence_interval().as_secs_f64() / 60.0,
        "ignoreFlowDataEvidenceFilter": config
            .ignore_flow_data_evidence_filter()
            .iter()
            .map(|(key, value)| json!([key, value]))
            .collect::<Vec<_>>(),
    })
}
