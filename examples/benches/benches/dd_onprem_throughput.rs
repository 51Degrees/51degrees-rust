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

//! On-premise Device Detection detection-throughput benchmark. See the
//! descriptive block at the bottom of this file for the full write-up.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use fiftyone_device_detection::{
    DeviceData, DeviceDetectionPipelineBuilder, Evidence, PerformanceProfile, Pipeline,
    DEVICE_DATA_KEY,
};

/// The relative path to the bundled evidence file inside the
/// data in the `cxx` submodule. `examples_shared::find_file` walks up
/// the tree to locate it, so it resolves from a bench run, an IDE or CI alike.
const EVIDENCE_RELATIVE_PATH: &str =
    "cxx/device-detection-data/20000 Evidence Records.yml";

/// How many evidence records to load and replay per benchmark iteration. The
/// bundled file holds 20,000. A few hundred keeps each iteration short enough for
/// Criterion to gather a stable sample while still exercising a realistic mix of
/// User-Agents and User-Agent Client Hints.
const RECORD_COUNT: usize = 500;

/// Build the on-premise pipeline the benchmark detects through.
///
/// `HighPerformance` holds the data set in memory for the fastest detections,
/// which is what a throughput measurement wants, and a single requested property
/// (`IsMobile`) keeps detection resolving one component, the fastest realistic
/// configuration. Usage sharing is turned off, as it is for console and
/// off-line tools, because it would add work that has nothing to do with the
/// figure being measured. In normal operation leave it on, which is the
/// builder's default.
fn build_pipeline(data_file: &Path) -> Arc<Pipeline> {
    DeviceDetectionPipelineBuilder::on_premise(data_file)
        .performance_profile(PerformanceProfile::InMemory)
        .property("IsMobile")
        .share_usage(false)
        // Off so no licence key is needed.
        .auto_update(false)
        .build()
        .expect("the benchmark on-premise pipeline should build from the Lite .hash file")
}

/// Load up to `max` evidence records from the multi-document YAML evidence file.
///
/// The file is a `---`-separated stream, one document per record, each a map of
/// evidence key (for example `header.user-agent`) to a string value. Each chunk
/// is parsed on its own into a `BTreeMap`, mirroring the offline-processing
/// example's reader. Records that fail to parse are skipped so one odd document
/// cannot abort the load.
fn load_records(path: &Path, max: usize) -> Vec<BTreeMap<String, String>> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut records = Vec::new();
    for chunk in text.split("\n---") {
        if records.len() >= max {
            break;
        }
        let body = chunk.trim().trim_start_matches("---").trim();
        if body.is_empty() {
            continue;
        }
        if let Ok(record) = serde_norway::from_str::<BTreeMap<String, String>>(body) {
            if !record.is_empty() {
                records.push(record);
            }
        }
    }
    records
}

/// Run one detection per record, reading `IsMobile` so the optimizer cannot elide
/// the work being measured, and return how many detections succeeded.
///
/// Per-detection errors are ignored: a single malformed record must not abort the
/// benchmark, and only successful detections are counted.
fn detect_all(pipeline: &Arc<Pipeline>, records: &[BTreeMap<String, String>]) -> u64 {
    let mut detections = 0u64;
    for record in records {
        let mut data = pipeline.create_flow_data_with(
            Evidence::builder()
                .add_all(record.iter().map(|(k, v)| (k.as_str(), v.clone())))
                .build(),
        );
        if data.process().is_ok() {
            if let Some(device) = data.get(DEVICE_DATA_KEY) {
                // Read a property so the detection result is observed. black_box
                // stops the compiler optimizing the read (and so the detection)
                // away.
                let is_mobile = device.is_mobile().as_option().copied().unwrap_or(false);
                black_box(is_mobile);
                detections += 1;
            }
        }
    }
    detections
}

/// Register the Device Detection throughput benchmark, or skip cleanly when the
/// data file or evidence file is not present in this checkout.
fn dd_onprem_throughput(c: &mut Criterion) {
    // Resolve the Lite .hash data file and the bundled evidence records. With
    // either absent (no submodule, no 51DEGREES_DD_PATH) the bench registers
    // nothing and `cargo bench` stays green offline.
    let Some(data_file) = examples_shared::dd_data_path() else {
        eprintln!(
            "skipping dd_onprem_throughput: no Device Detection data file found \
             (set {} or check out the cxx submodule)",
            examples_shared::DD_PATH_ENV_VAR
        );
        return;
    };
    let Some(evidence_file) = examples_shared::find_file(EVIDENCE_RELATIVE_PATH) else {
        eprintln!(
            "skipping dd_onprem_throughput: '{EVIDENCE_RELATIVE_PATH}' not found \
             (check out the cxx submodule)"
        );
        return;
    };

    let records = load_records(&evidence_file, RECORD_COUNT);
    if records.is_empty() {
        eprintln!("skipping dd_onprem_throughput: no evidence records were loaded");
        return;
    }

    let pipeline = build_pipeline(&data_file);

    // A warm-up detection primes the data set and caches so the first measured
    // iteration is not skewed by one-off initialization.
    let _ = detect_all(&pipeline, &records);

    let mut group = c.benchmark_group("dd_onprem");
    // Report throughput as detections per second by telling Criterion how many
    // elements one iteration processes.
    group.throughput(Throughput::Elements(records.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("detect", records.len()),
        &records,
        |b, records| {
            b.iter(|| {
                let detections = detect_all(&pipeline, records);
                black_box(detections);
            });
        },
    );
    group.finish();
}

criterion_group!(benches, dd_onprem_throughput);
criterion_main!(benches);

/*
 * @example dd_onprem_throughput.rs
 *
 * The on-premise Device Detection detection-throughput benchmark. It is the
 * Criterion counterpart of the `dd-onprem-performance` console example,
 * expressed as a repeatable benchmark so a throughput regression shows up as a
 * measured slowdown.
 *
 * What it measures
 *
 * It loads a few hundred evidence records from the bundled
 * "20000 Evidence Records.yml" (a mix of plain User-Agents and User-Agent Client
 * Hints), builds one on-premise pipeline tuned for speed, then times running a
 * detection over every record. Criterion is told the element count per iteration
 * via `Throughput::Elements`, so it reports detections per second alongside the
 * per-iteration time.
 *
 * Two deliberate choices maximise and stabilise the figure:
 *
 * - The `HighPerformance` profile holds the data set in memory for the fastest
 *   detections. The other profiles (Balanced, LowMemory and so on) trade memory
 *   for speed. Swap the profile in `build_pipeline` to compare the options.
 * - A single requested property (`IsMobile`) means detection resolves one
 *   component, the fastest realistic configuration. A property is read on each
 *   detection, behind `black_box`, so the optimizer cannot remove the work.
 *
 * Usage sharing is turned off with `.share_usage(false)`, as it is for console
 * and off-line tools, because it would add work unrelated to the measurement.
 * In normal operation leave it on, which is the builder's default.
 *
 * Data is resolved through `examples_shared` (the Lite .hash via `dd_data_path`,
 * the evidence file via `find_file`). When either is missing the benchmark
 * registers nothing and exits cleanly, so `cargo bench` is safe to run without
 * the data submodules. See
 * https://51degrees.com/documentation/_device_detection__features__performance_options.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-benches-benches-dd_onprem_throughput.rs&utm_term=header
 */
