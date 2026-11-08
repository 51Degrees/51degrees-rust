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

//! The metadata cases: each on-premise engine's properties, components and
//! values, and what an engine opened with some properties gives when the
//! others are read.
//!
//! ```text
//! parity-rust metadata --cases <json> --dd <hash file> --ipi <ipi file> --out <jsonl>
//! ```
//!
//! The .NET producer writes the same line. `parity/README.md` ("The
//! metadata comparison") describes the cases and the line.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use fiftyone_device_detection_onpremise::{
    DeviceDetectionOnPremiseEngine, DeviceDetectionOnPremiseEngineBuilder,
};
use fiftyone_ip_intelligence_onpremise::{
    IpIntelligenceOnPremiseEngine, IpIntelligenceOnPremiseEngineBuilder,
};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{Evidence, FlowElement, Pipeline};
use fiftyone_pipeline_engines::AspectEngine;
use fiftyone_pipeline_engines_fiftyone::{
    ComponentMetaData, FiftyOneAspectPropertyMetaData, ValueMetaData,
};

use crate::fodid_cases::value_type_name;

/// The first argument that selects this form.
pub const RUN: &str = "metadata";

/// The `engine` of a case for each on-premise engine.
const DEVICE: &str = "device";
const IP: &str = "ip";

/// One metadata case. `about` is for people and is not read.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    id: String,
    /// [`DEVICE`] or [`IP`].
    engine: String,
    /// The properties the engine is opened with, or every one when empty.
    #[serde(default)]
    properties: Vec<String>,
    /// How many of each property's values to write, every one when 0.
    value_limit: usize,
    /// The evidence of the request made before reading [`Case::read`].
    #[serde(default)]
    evidence: Map<String, Value>,
    /// The properties read from the engine's data, with no request made
    /// when empty.
    #[serde(default)]
    read: Vec<String>,
}

/// The metadata both on-premise engines give, under one name for each.
trait EngineMetadata {
    fn tier(&self) -> &str;
    fn property_list(&self) -> Arc<[FiftyOneAspectPropertyMetaData]>;
    fn component_list(&self) -> Arc<[ComponentMetaData]>;
    fn value_count(&self, name: &str) -> Option<usize>;
    fn value_range(
        &self,
        name: &str,
        range: Range<usize>,
    ) -> fiftyone_pipeline_core::Result<Option<Vec<ValueMetaData>>>;
}

impl EngineMetadata for DeviceDetectionOnPremiseEngine {
    fn tier(&self) -> &str {
        AspectEngine::data_source_tier(self)
    }
    fn property_list(&self) -> Arc<[FiftyOneAspectPropertyMetaData]> {
        DeviceDetectionOnPremiseEngine::fiftyone_properties(self)
    }
    fn component_list(&self) -> Arc<[ComponentMetaData]> {
        DeviceDetectionOnPremiseEngine::components(self)
    }
    fn value_count(&self, name: &str) -> Option<usize> {
        DeviceDetectionOnPremiseEngine::property_value_count(self, name)
    }
    fn value_range(
        &self,
        name: &str,
        range: Range<usize>,
    ) -> fiftyone_pipeline_core::Result<Option<Vec<ValueMetaData>>> {
        DeviceDetectionOnPremiseEngine::property_value_range(self, name, range)
    }
}

impl EngineMetadata for IpIntelligenceOnPremiseEngine {
    fn tier(&self) -> &str {
        AspectEngine::data_source_tier(self)
    }
    fn property_list(&self) -> Arc<[FiftyOneAspectPropertyMetaData]> {
        IpIntelligenceOnPremiseEngine::fiftyone_properties(self)
    }
    fn component_list(&self) -> Arc<[ComponentMetaData]> {
        IpIntelligenceOnPremiseEngine::components(self)
    }
    fn value_count(&self, name: &str) -> Option<usize> {
        IpIntelligenceOnPremiseEngine::property_value_count(self, name)
    }
    fn value_range(
        &self,
        name: &str,
        range: Range<usize>,
    ) -> fiftyone_pipeline_core::Result<Option<Vec<ValueMetaData>>> {
        IpIntelligenceOnPremiseEngine::property_value_range(self, name, range)
    }
}

/// Writes one line for each case in the file `--cases` names.
pub fn run(args: &[String]) -> Result<()> {
    let mut cases = None;
    let mut dd = None;
    let mut ipi = None;
    let mut out = None;
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .with_context(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--cases" => cases = Some(PathBuf::from(value)),
            "--dd" => dd = Some(PathBuf::from(value)),
            "--ipi" => ipi = Some(PathBuf::from(value)),
            "--out" => out = Some(PathBuf::from(value)),
            _ => bail!("unknown argument {flag}"),
        }
    }
    let cases = cases.context("--cases is required")?;
    let dd = dd.context("--dd is required")?;
    let ipi = ipi.context("--ipi is required")?;
    let out = out.context("--out is required")?;

    let text =
        std::fs::read_to_string(&cases).with_context(|| format!("reading {}", cases.display()))?;
    let cases: Vec<Case> = serde_json::from_str(&text).context("parsing the metadata cases")?;
    let mut writer =
        BufWriter::new(File::create(&out).with_context(|| format!("creating {}", out.display()))?);
    for case in &cases {
        let line = match case.engine.as_str() {
            DEVICE => device_line(case, &dd)?,
            IP => ip_line(case, &ipi)?,
            other => bail!("{}: unknown engine {other}", case.id),
        };
        serde_json::to_writer(&mut writer, &line)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    eprintln!(
        "{} metadata cases written to {}",
        cases.len(),
        out.display()
    );
    Ok(())
}

/// The Device Detection engine in the HighPerformance profile, as the
/// engines' pipeline has it.
fn device_line(case: &Case, file: &Path) -> Result<Value> {
    let mut builder = DeviceDetectionOnPremiseEngineBuilder::new(file)
        .performance_profile(PerformanceProfile::HighPerformance);
    for name in &case.properties {
        builder = builder.property(name.as_str());
    }
    let engine = builder
        .build()
        .with_context(|| format!("{}: building the Device Detection engine", case.id))?;
    let metadata_of: &DeviceDetectionOnPremiseEngine = &engine;
    let metadata = engine_metadata(metadata_of, case.value_limit)
        .with_context(|| format!("{}: reading the metadata", case.id))?;
    line(case, engine, metadata)
}

/// The IP Intelligence engine in the LowMemory profile, as the engines'
/// pipeline has it.
fn ip_line(case: &Case, file: &Path) -> Result<Value> {
    let mut builder = IpIntelligenceOnPremiseEngineBuilder::new(file)
        .performance_profile(PerformanceProfile::LowMemory)
        .auto_update(false)
        .file_system_watcher(false)
        .update_on_startup(false);
    for name in &case.properties {
        builder = builder.property(name.as_str());
    }
    let engine = builder
        .build_shared()
        .with_context(|| format!("{}: building the IP Intelligence engine", case.id))?;
    let metadata_of: &IpIntelligenceOnPremiseEngine = &engine;
    let metadata = engine_metadata(metadata_of, case.value_limit)
        .with_context(|| format!("{}: reading the metadata", case.id))?;
    line(case, engine, metadata)
}

/// The case's line, with the properties [`Case::read`] names read from the
/// data of a request to `engine` alone. A property the data does not hold is
/// written as missing, as .NET's data fails such a read.
fn line(case: &Case, engine: Arc<dyn FlowElement>, metadata: Value) -> Result<Value> {
    let mut elements = Map::new();
    let mut process_error = None;
    if !case.read.is_empty() {
        let key = engine.data_key().to_owned();
        let pipeline = Pipeline::builder()
            .add_element(engine)
            .suppress_process_exceptions(true)
            .build()?;
        let mut evidence = Evidence::builder();
        for (name, text) in &case.evidence {
            let text = text
                .as_str()
                .with_context(|| format!("{}: evidence {name} is not a string", case.id))?;
            evidence = evidence.add(name, text);
        }
        let mut data = pipeline.create_flow_data_with(evidence.build());
        process_error = data.process().err().map(|error| error.to_string());
        let read = data.get_str(&key).map(|element_data| {
            let mut values = Map::new();
            for name in &case.read {
                values.insert(name.to_lowercase(), crate::read(element_data, name, true));
            }
            Value::Object(values)
        });
        elements.insert(key, read.unwrap_or(Value::Null));
    }
    Ok(json!({
        "id": case.id,
        "processError": process_error,
        "properties": elements,
        "engineMetadata": metadata,
    }))
}

/// The engine's tier, its properties keyed by name and its components keyed
/// by name, each list's order kept beside it, with up to `value_limit`
/// values of each property, every one when it is 0.
fn engine_metadata(engine: &dyn EngineMetadata, value_limit: usize) -> Result<Value> {
    let properties = engine.property_list();
    let mut by_name = Map::new();
    for property in properties.iter() {
        let name = property.name();
        let count = engine
            .value_count(name)
            .with_context(|| format!("{name} is listed with no value count"))?;
        let end = if value_limit == 0 {
            count
        } else {
            value_limit.min(count)
        };
        let values = engine
            .value_range(name, 0..end)
            .with_context(|| format!("reading the values of {name}"))?
            .with_context(|| format!("{name} is listed with no values"))?;
        by_name.insert(name.to_owned(), property_metadata(property, count, &values));
    }
    let components = engine.component_list();
    let mut components_by_name = Map::new();
    for component in components.iter() {
        let names: Vec<&str> = component.properties().iter().map(|p| p.name()).collect();
        components_by_name.insert(
            component.name.clone(),
            json!({
                "id": component.component_id,
                "defaultProfileId": component.default_profile_id,
                "properties": names,
            }),
        );
    }
    let property_names: Vec<&str> = properties.iter().map(|p| p.name()).collect();
    let component_names: Vec<&str> = components.iter().map(|c| c.name.as_str()).collect();
    Ok(json!({
        "dataSourceTier": engine.tier(),
        "propertyNames": property_names,
        "properties": by_name,
        "componentNames": component_names,
        "components": components_by_name,
    }))
}

fn property_metadata(
    property: &FiftyOneAspectPropertyMetaData,
    count: usize,
    values: &[ValueMetaData],
) -> Value {
    let aspect = property.aspect();
    let core = property.core();
    let values: Vec<Value> = values.iter().map(value).collect();
    json!({
        "type": value_type_name(core.value_type),
        "category": core.category,
        "description": aspect.description(),
        "url": property.url(),
        "displayOrder": property.display_order(),
        "mandatory": property.mandatory(),
        "list": property.list(),
        "obsolete": property.obsolete(),
        "show": property.show(),
        "showValues": property.show_values(),
        "available": aspect.available(),
        "dataTiers": aspect.data_tiers_where_present(),
        "component": property
            .component_id()
            .map(|id| json!({"id": id, "name": property.component_name()})),
        "defaultValue": property.default_value().map(value),
        "valueCount": count,
        "values": values,
    })
}

fn value(value: &ValueMetaData) -> Value {
    json!({
        "name": value.name,
        "description": value.description,
        "url": value.url,
    })
}
