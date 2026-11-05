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

//! @page ipi-onprem-offline-processing-example Offline Processing (IP Intelligence, On-premise)
//!
//! On-premise IP Intelligence offline (batch) processing console example.
//!
//! Streams IP-address evidence records from a YAML file, looks each up against
//! a local `.ipi` data file and writes the results back as a YAML-like
//! document stream. The descriptive block the documentation tooling renders
//! lives at the bottom of the file.
//!
//! @snippet ipi-onprem-offline-processing.rs example

use std::io::Write;
use std::sync::Arc;

use anyhow::Context;
use fiftyone_ip_intelligence::{IpIntelligencePipelineBuilder, PerformanceProfile, IP_DATA_KEY};
use fiftyone_pipeline_core::{Evidence, Pipeline};

/// Where to ask about a copy of the Enterprise data file.
const CONTACT_US_URL: &str = "https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-offline-processing.rs&utm_term=enterprise-data-file";

/// The links the example's output ends with.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("IP Intelligence", "https://51degrees.com/ip-intelligence?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-offline-processing.rs&utm_term=find-out-more"),
    ("Pricing", "https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-offline-processing.rs&utm_term=find-out-more"),
    ("Documentation", "https://51degrees.com/documentation/index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-offline-processing.rs&utm_term=find-out-more"),
];

/// The relative path to the IP-intelligence evidence file in the data submodule.
const IPI_EVIDENCE_RELATIVE_PATH: &str = "cxx/data/ip-intelligence/evidence.yml";

/// The properties reported for each record. The free ASN file carries Asn and
/// AsnName, and only the Enterprise data file carries the location ones.
const REPORTED_PROPERTIES: &[&str] = &["Asn", "AsnName", "Country", "CountryCode", "Town"];

/// The reported properties only the Enterprise data file carries.
const ENTERPRISE_REPORTED: &[&str] = &["Country", "CountryCode", "Town"];

/// The properties the pipeline is asked for. RegisteredName is included so the
/// request holds a property each free file carries, because a data file opens
/// only when it carries at least one property asked for.
const REQUESTED_PROPERTIES: &[&str] = &[
    "Asn",
    "AsnName",
    "RegisteredName",
    "Country",
    "CountryCode",
    "Town",
];

/// The options that drive a run of this example.
pub struct ExampleOptions {
    /// The full path to the `.ipi` data file to open.
    pub data_file: std::path::PathBuf,
    /// The full path to the YAML evidence file to read.
    pub evidence_file: std::path::PathBuf,
    /// The maximum number of records to process (the evidence file holds many).
    pub max_records: usize,
}

impl ExampleOptions {
    /// Default options, using the best-available loadable data-file tier.
    pub fn from_env() -> Option<Self> {
        Self::for_tier(examples_shared::IpiTier::BestAvailable, 20)
    }

    /// Options pinned to a specific data-file tier and record limit. The test
    /// pins the ASN tier so it runs deterministically against the
    /// always-loadable ASN file.
    pub fn for_tier(tier: examples_shared::IpiTier, max_records: usize) -> Option<Self> {
        Some(ExampleOptions {
            data_file: examples_shared::ipi_data_path(tier)?,
            evidence_file: examples_shared::find_file(IPI_EVIDENCE_RELATIVE_PATH)?,
            max_records,
        })
    }
}

/// One evidence record read from the YAML file: client-IP key/value pairs.
type EvidenceRecord = std::collections::BTreeMap<String, String>;

// [example]
/// Run the offline-processing example, writing all output to `out`.
///
/// Usage sharing is not enabled, as this is a console example.
pub fn run(options: &ExampleOptions, out: &mut dyn Write) -> anyhow::Result<()> {
    // The data file's own list of properties says which reported properties it
    // can answer. The free files carry none of the location ones.
    let available = ip_intelligence_examples::data_file_properties(&options.data_file)?;

    // LowMemory keeps the data file on disk, which suits a long batch run that
    // does not want to pay the memory cost of loading the whole file.
    let pipeline: Arc<Pipeline> = IpIntelligencePipelineBuilder::on_premise(&options.data_file)
        .performance_profile(PerformanceProfile::LowMemory)
        .properties(REQUESTED_PROPERTIES.iter().copied())
        .auto_update(false)
        .file_system_watcher(false)
        .build()
        .context("failed to build the on-premise IP Intelligence pipeline")?;

    let records =
        read_evidence(&options.evidence_file, options.max_records).with_context(|| {
            format!(
                "failed to read evidence from {}",
                options.evidence_file.display()
            )
        })?;

    writeln!(
        out,
        "# Processing {} evidence record(s) from {}",
        records.len(),
        options.evidence_file.display()
    )?;

    for record in &records {
        analyse_record(&pipeline, record, &available, out)?;
    }
    writeln!(out, "...")?;

    writeln!(out)?;
    ip_intelligence_examples::write_data_file_warnings(&options.data_file, out)?;
    let missing = examples_shared::missing_properties(ENTERPRISE_REPORTED, &available);
    if let Some(message) = examples_shared::enterprise_data_file_message(&missing, CONTACT_US_URL) {
        writeln!(out)?;
        writeln!(out, "{message}")?;
    }
    examples_shared::write_find_out_more(
        out,
        FIND_OUT_MORE,
        examples_shared::ON_PREMISE_REPOSITORIES,
    )?;
    Ok(())
}
// [example]

/// Read up to `max_records` evidence records from a multi-document YAML file.
///
/// The IP-intelligence evidence file is a stream of `---`-separated single-key
/// maps, for example `server.client-ip: 1.2.3.4`. Each document is parsed into
/// a generic [`serde_norway::Value`], then its string key/value pairs are
/// pulled into an [`EvidenceRecord`]. A document that is not a map (a blank or
/// comment-only document parses to null) is skipped rather than failing the
/// whole batch.
fn read_evidence(
    path: &std::path::Path,
    max_records: usize,
) -> anyhow::Result<Vec<EvidenceRecord>> {
    let text = std::fs::read_to_string(path)?;
    let mut records = Vec::new();
    // Each YAML document in the stream is delimited by a `---` line. Splitting
    // on it and parsing each chunk into a generic value keeps the reader simple
    // and avoids pulling in serde derive: the evidence file is just single-key
    // IP maps.
    for chunk in text.split("\n---") {
        if records.len() >= max_records {
            break;
        }
        let chunk = chunk.trim();
        if chunk.is_empty() || chunk == "---" {
            continue;
        }
        let value: serde_norway::Value = match serde_norway::from_str(chunk) {
            Ok(value) => value,
            // A malformed document is skipped rather than failing the whole
            // batch, so one odd record cannot abort the run.
            Err(_) => continue,
        };
        if let serde_norway::Value::Mapping(mapping) = value {
            let mut record = EvidenceRecord::new();
            for (key, val) in mapping {
                if let (Some(key), Some(val)) = (key.as_str(), val.as_str()) {
                    record.insert(key.to_owned(), val.to_owned());
                }
            }
            if !record.is_empty() {
                records.push(record);
            }
        }
    }
    Ok(records)
}

/// Look one evidence record up and write its inputs and results. `available`
/// holds the properties the data file carries.
fn analyse_record(
    pipeline: &Arc<Pipeline>,
    record: &EvidenceRecord,
    available: &[String],
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    writeln!(out, "---")?;

    let mut builder = Evidence::builder();
    for (key, value) in record {
        writeln!(out, "{key}: {value}")?;
        builder = builder.add(key, value);
    }

    let mut data = pipeline.create_flow_data_with(builder.build());
    data.process()
        .context("processing an evidence record failed")?;

    let ip_data = data
        .get(IP_DATA_KEY)
        .context("the pipeline produced no IP Intelligence data")?;

    for property in REPORTED_PROPERTIES {
        let needs_enterprise = ENTERPRISE_REPORTED.contains(property)
            && !examples_shared::is_available(property, available);
        if needs_enterprise {
            let note = examples_shared::NEEDS_ENTERPRISE_DATA_FILE;
            writeln!(out, "{property}: {note}")?;
            continue;
        }
        let value = ip_data.string(property);
        let rendered = match value.value() {
            Ok(value) => value.clone(),
            Err(no_value) => no_value.to_string(),
        };
        writeln!(out, "{property}: {rendered}")?;
    }

    Ok(())
}

/// Read the data file and evidence file, then run the example.
fn main() -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let options = match ExampleOptions::from_env() {
        Some(options) => options,
        None => anyhow::bail!(
            "Could not locate both an IP Intelligence data file and the \
             evidence.yml file. Set {} to an .ipi file, and run git submodule \
             update --init --recursive so cxx/data/ip-intelligence/evidence.yml \
             is present.",
            examples_shared::IPI_PATH_ENV_VAR
        ),
    };

    run(&options, &mut out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_processing_streams_evidence() {
        let Some(options) = ExampleOptions::for_tier(examples_shared::IpiTier::Asn, 10) else {
            eprintln!("no usable IP Intelligence data file or evidence file; skipping offline run");
            return;
        };

        let mut buffer = Vec::new();
        run(&options, &mut buffer).expect("the offline-processing example should run");

        let printed = String::from_utf8(buffer).expect("output should be valid UTF-8");
        // The document stream markers and at least one record must appear.
        assert!(printed.contains("---"));
        assert!(printed.contains("..."));
        assert!(printed.contains("client-ip"));
        assert!(printed.contains("Asn:"));
        assert!(printed.contains(examples_shared::FIND_OUT_MORE_HEADING));
        // The ASN file carries no location properties, so each record says so.
        // 51DEGREES_IPI_PATH can name another file.
        let asn_file = examples_shared::IPI_ASN_RELATIVE_PATH.rsplit('/').next();
        if options.data_file.file_name().and_then(|n| n.to_str()) == asn_file {
            assert!(printed.contains("Country: needs the Enterprise data file"));
            assert!(printed.contains("utm_term=enterprise-data-file"));
        }
    }
}

/*
 * @example ipi-onprem-offline-processing.rs
 *
 * The on-premise IP Intelligence offline (batch) processing example.
 *
 * It shows how to:
 *
 * 1. Build an on-premise IP Intelligence pipeline restricted to a few
 *    properties, using the `LowMemory` profile that suits a long batch run.
 * 2. Stream IP-address evidence records from a multi-document YAML file
 *    (`evidence.yml` in the data submodule), where each `---`
 *    separated document is a single client-IP key/value map.
 * 3. Process each record and write the inputs and the results back as a
 *    YAML-like document stream, suitable for piping to a file for later
 *    analysis.
 *
 * Each record reports `Asn` and `AsnName`, which the free ASN file carries, and
 * `Country`, `CountryCode` and `Town`, which only the Enterprise data file
 * carries. The data file's own list of properties says which it can answer,
 * so with a free file the location properties each say "needs the Enterprise
 * data file". After the stream the run prints the standard data-file
 * warnings, names the properties the data file lacks with a link to contact
 * 51Degrees about a copy of the Enterprise data file, and ends with a "Find
 * out more" section.
 *
 * The evidence file is located with `examples_shared::find_file`, and the data
 * file with `examples_shared::ipi_data_path`. By default only the first 20
 * records are processed so the example finishes quickly; pass a different limit
 * by constructing `ExampleOptions::for_tier` directly.
 *
 * # Data file
 *
 * Contact 51Degrees for an Enterprise file with the full property set:
 * <https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-offline-processing.rs&utm_term=ipi-onprem-offline-processing>.
 *
 * # Usage sharing
 *
 * Not enabled, because this is a console example. Production deployments are
 * encouraged to enable usage sharing.
 */
