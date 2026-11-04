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

//! @page dd-onprem-offline-processing-example Offline Processing (Device Detection, On-premise)
//!
//! On-premise offline-processing console example. See the descriptive block at
//! the bottom of this file for the full write-up.
//!
//! @snippet dd-onprem-offline-processing.rs example

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use serde::Deserialize;

use fiftyone_device_detection::{
    DeviceData, DeviceDetectionPipelineBuilder, Evidence, PerformanceProfile, Pipeline,
    DEVICE_DATA_KEY,
};

/// The relative path to the evidence file in the data repository inside the
/// `cxx` submodule, used when no path is supplied.
const EVIDENCE_RELATIVE_PATH: &str = "cxx/data/device-detection/20000 Evidence Records.yml";

/// Options the example runs with, so `main` and the test share one entry point.
pub struct ExampleOptions {
    /// The path to the on-premise Hash data file the engine loads.
    pub data_file: PathBuf,
    /// The path to the YAML evidence file to read records from.
    pub evidence_file: PathBuf,
    /// The path to write the YAML results to.
    pub output_file: PathBuf,
    /// The maximum number of records to process. The bundled file holds 20,000;
    /// the test caps this so it stays fast.
    pub max_records: usize,
}

// [example]
/// Run the on-premise offline-processing example.
///
/// Builds an on-premise pipeline, streams evidence records from the YAML file,
/// runs device detection on each, and writes the input evidence together with the
/// detected properties to a YAML output file.
pub fn run(options: ExampleOptions) -> Result<()> {
    // Build the on-premise pipeline with usage sharing off. Off-line processing
    // should not share usage with 51Degrees, because it lacks the full evidence
    // the data back-end needs and would be discarded. In normal operation, on
    // a web server, leave usage sharing on, which is the builder's default.
    let pipeline = DeviceDetectionPipelineBuilder::on_premise(&options.data_file)
        .performance_profile(PerformanceProfile::LowMemory)
        .share_usage(false)
        // Off so no licence key is needed.
        .auto_update(false)
        .build()
        .context("failed to build the on-premise device-detection pipeline")?;

    // Read the whole evidence file. It is a multi-document YAML stream, one
    // document per record, each a map of evidence key to value.
    let yaml = std::fs::read_to_string(&options.evidence_file).with_context(|| {
        format!(
            "failed to read the evidence file '{}'",
            options.evidence_file.display()
        )
    })?;

    // Open the output file for writing. Each processed record is appended as one
    // YAML document, separated by the `---` document marker.
    let mut output = std::fs::File::create(&options.output_file).with_context(|| {
        format!(
            "failed to create the output file '{}'",
            options.output_file.display()
        )
    })?;

    let mut processed = 0usize;
    for record in evidence_records(&yaml) {
        if processed >= options.max_records {
            break;
        }
        let record = record.context("failed to parse an evidence record")?;
        let result = analyse_evidence(&pipeline, &record)?;
        write_record(&mut output, &result)?;
        processed += 1;
    }

    println!(
        "Processed {processed} record(s). Results written to '{}'.",
        options.output_file.display()
    );

    // Print the standard data-file warnings, as the other examples do.
    device_detection_examples::print_data_file_warnings(
        &options.data_file,
        PerformanceProfile::LowMemory,
    )?;

    Ok(())
}
// [example]

/// Iterate the records in a multi-document YAML evidence stream.
///
/// Each document is a map of evidence key to (string) value. The native evidence
/// values in the bundled file are all scalars, so a `BTreeMap<String, String>`
/// captures them while keeping a stable key order for reproducible output. A
/// `serde_norway::Deserializer` over the whole stream yields one sub-deserializer
/// per `---`-separated document, so the records are read straight from the parser
/// without any manual splitting on the document marker.
fn evidence_records(yaml: &str) -> impl Iterator<Item = Result<BTreeMap<String, String>>> + '_ {
    serde_norway::Deserializer::from_str(yaml).map(|document| {
        BTreeMap::<String, String>::deserialize(document)
            .context("a YAML evidence document was not a string-to-string map")
    })
}

/// Process one evidence record and return the input plus the detected properties
/// as an ordered map ready to serialize.
fn analyse_evidence(
    pipeline: &Arc<Pipeline>,
    evidence: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>> {
    // Build a flow data carrying this record's evidence and process it. The flow
    // data owns the native results and frees them when dropped at the end of this
    // function, so no manual cleanup is needed.
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add_all(evidence.iter().map(|(k, v)| (k.as_str(), v.clone())))
            .build(),
    );
    data.process()
        .context("on-premise pipeline processing failed")?;
    let device = data
        .get(DEVICE_DATA_KEY)
        .context("the Hash engine should have produced device data")?;

    // Start the output with the input evidence so each record is self-describing.
    let mut out: BTreeMap<String, String> = evidence.clone();

    // Add the detected properties under their conventional `device.*` keys, using
    // the marker text for any property the data file did not populate. Only the
    // properties present in the free Lite tier are read here; further fields
    // (hardware vendor/model/name) are available with a paid Enterprise file.
    out.insert(
        "device.ismobile".to_owned(),
        examples_shared::get_property_as_string(device, "IsMobile"),
    );
    out.insert(
        "device.platformname".to_owned(),
        examples_shared::get_property_as_string(device, "PlatformName"),
    );
    out.insert(
        "device.platformversion".to_owned(),
        examples_shared::get_property_as_string(device, "PlatformVersion"),
    );
    out.insert(
        "device.browsername".to_owned(),
        examples_shared::get_property_as_string(device, "BrowserName"),
    );
    out.insert(
        "device.browserversion".to_owned(),
        examples_shared::get_property_as_string(device, "BrowserVersion"),
    );
    // The device id is a unique identifier for the detected combination of
    // hardware, OS, browser and crawler. Storing it lets a later run skip
    // detection by looking the id up (supply it under query.51D_deviceId), which
    // is faster and guarantees the same result over time.
    out.insert(
        "device.deviceid".to_owned(),
        device.device_id().into_option().unwrap_or_default(),
    );

    Ok(out)
}

/// Serialize one record as a YAML document and append it to `output`, preceded by
/// the `---` document separator so the file is a valid multi-document stream.
fn write_record(output: &mut std::fs::File, record: &BTreeMap<String, String>) -> Result<()> {
    let mut document = String::from("---\n");
    let body = serde_norway::to_string(record).context("failed to serialize a result record")?;
    document.push_str(&body);
    output
        .write_all(document.as_bytes())
        .context("failed to write a result record")?;
    Ok(())
}

/// Locate the evidence file, walking up the tree from the crate so it resolves
/// whether the example runs from an IDE, its crate directory or CI, and
/// downloading it when it is missing.
fn default_evidence_file() -> Option<PathBuf> {
    examples_shared::find_data_file(EVIDENCE_RELATIVE_PATH)
}

/// Resolve the data, evidence and output files, then run the example. The data
/// and evidence files use the usual fallbacks; the output file defaults to a file
/// next to the evidence file. With the data or evidence file missing the example
/// fails, after the resolver has printed the command that gets the file.
fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let data_file = args
        .next()
        .map(PathBuf::from)
        .or_else(examples_shared::dd_data_path);
    let evidence_file = args
        .next()
        .map(PathBuf::from)
        .or_else(default_evidence_file);

    let (Some(data_file), Some(evidence_file)) = (data_file, evidence_file) else {
        anyhow::bail!(
            "Missing the data file or the evidence file. Set 51DEGREES_DD_PATH, or \
             pass the data-file and evidence-file paths as the first two arguments."
        );
    };

    // The output goes next to the evidence file by default, or to the third
    // argument when one is supplied.
    let output_file = args.next().map(PathBuf::from).unwrap_or_else(|| {
        evidence_file
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("dd-offline-processing-output.yml")
    });

    run(ExampleOptions {
        data_file,
        evidence_file,
        output_file,
        // Process the whole file from the command line.
        max_records: usize::MAX,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run the example against the Lite Hash file and the bundled evidence file,
    /// writing to a temporary output, and skip when either input is missing.
    #[test]
    fn runs_against_the_lite_data_file() {
        let Some(data_file) = examples_shared::dd_data_path() else {
            eprintln!("skipping: no on-premise data file found (set 51DEGREES_DD_PATH)");
            return;
        };
        let Some(evidence_file) = default_evidence_file() else {
            eprintln!("skipping: bundled evidence file not found");
            return;
        };

        // Write to a unique temporary file so the test does not clobber a real
        // output and cleans up after itself.
        let output_file = std::env::temp_dir().join(format!(
            "dd-offline-processing-test-{}.yml",
            std::process::id()
        ));

        run(ExampleOptions {
            data_file,
            evidence_file,
            output_file: output_file.clone(),
            // A small cap keeps the test fast while still exercising the batch
            // read/process/write loop end to end.
            max_records: 50,
        })
        .expect("the on-premise offline-processing example should complete");

        // The output file must exist and be non-empty.
        let written = std::fs::read_to_string(&output_file).expect("output file should exist");
        assert!(written.contains("device.ismobile"), "results were written");
        let _ = std::fs::remove_file(&output_file);
    }

    /// Process a capped batch and assert every input record produced a result
    /// document carrying the detected properties: the count of output documents
    /// matches the count of input records, and a known property resolves for each.
    #[test]
    fn every_record_in_a_batch_produces_a_result() {
        let Some(data_file) = examples_shared::dd_data_path() else {
            eprintln!("skipping: no on-premise data file found (set 51DEGREES_DD_PATH)");
            return;
        };
        let Some(evidence_file) = default_evidence_file() else {
            eprintln!("skipping: bundled evidence file not found");
            return;
        };

        // Cap the batch so the test is fast, then learn how many records the cap
        // actually yields from the source evidence stream (the file may hold
        // fewer than the cap, though here it holds many more).
        let cap = 25usize;
        let yaml = std::fs::read_to_string(&evidence_file).expect("evidence file should read");
        let input_records = evidence_records(&yaml)
            .take(cap)
            .filter(|record| record.is_ok())
            .count();
        assert!(input_records > 0, "the evidence file should hold records");

        let output_file = std::env::temp_dir().join(format!(
            "dd-offline-processing-batch-{}.yml",
            std::process::id()
        ));

        run(ExampleOptions {
            data_file,
            evidence_file,
            output_file: output_file.clone(),
            max_records: cap,
        })
        .expect("the on-premise offline-processing example should complete");

        let written = std::fs::read_to_string(&output_file).expect("output file should exist");
        let _ = std::fs::remove_file(&output_file);

        // One YAML document is written per processed record, each opening with a
        // `---` marker. The number of result documents must equal the number of
        // input records, proving the batch lost none.
        let result_documents = written.matches("---").count();
        assert_eq!(
            result_documents, input_records,
            "each input record should produce exactly one result document"
        );

        // Every result document carries the IsMobile property, so the known
        // property resolved (to a value or the no-value marker) for every record.
        let ismobile_lines = written.matches("device.ismobile:").count();
        assert_eq!(
            ismobile_lines, input_records,
            "every record's result should report device.ismobile"
        );
    }
}

/*
 * @example dd-onprem-offline-processing.rs
 *
 * The device-detection on-premise offline-processing console example. It shows
 * batch processing of a YAML file of evidence.
 *
 * The "20000 Evidence Records.yml" file holds 20,000 records of evidence
 * representing HTTP headers, for example:
 *
 *   header.user-agent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) ...'
 *   header.sec-ch-ua-mobile: '?0'
 *   header.sec-ch-ua-platform: '"Windows"'
 *
 * The example builds an on-premise pipeline once, then streams the multi-document
 * YAML file one record at a time. For each record it creates a `FlowData`, adds
 * the record's evidence, processes it, and reads the detected properties back
 * through the shared helpers. It writes the input evidence together with the
 * detected `device.*` properties (IsMobile, PlatformName, PlatformVersion,
 * BrowserName, BrowserVersion and DeviceId) to a YAML output file, one document
 * per record.
 *
 * Storing the `DeviceId` alongside each record is deliberate: a device id is a
 * stable identifier for the detected combination of hardware, OS, browser and
 * crawler, so a later run can skip detection by supplying the id under the
 * `query.51D_deviceId` evidence key, which is faster and yields the same result
 * even as detection improves over time.
 *
 * Only the properties available in the free Lite data tier are read here. Further
 * fields (hardware vendor, model and name) are available with a paid Enterprise
 * data file and can be added in the same way.
 *
 * Usage sharing is turned off with `.share_usage(false)`. Off-line processing
 * should not share usage with 51Degrees, because it lacks the full evidence the
 * data back-end needs and would be discarded. In normal operation, on a web
 * server, leave it on, which is the builder's default.
 *
 * The data and evidence files are read from the first two command-line arguments,
 * the `51DEGREES_DD_PATH` environment variable, or the files in the data
 * repository inside the cxx submodule, which the example downloads when they
 * are missing. The output file defaults to
 * `dd-offline-processing-output.yml` next to the evidence file, or the third
 * command-line argument.
 *
 * For experimenting with the performance/accuracy trade-off, change the
 * `PerformanceProfile` passed to the builder (for example MaxPerformance,
 * LowMemory or Balanced); the performance example explores these in depth.
 */
