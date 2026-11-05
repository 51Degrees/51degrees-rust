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

//! @page dd-onprem-match-metrics-example Match Metrics (Device Detection, On-premise)
//!
//! On-premise match-metrics console example. See the descriptive block at the
//! bottom of this file for the full write-up.
//!
//! @snippet dd-onprem-match-metrics.rs example

use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use fiftyone_device_detection::{
    DeviceData, DeviceDetectionPipelineBuilder, Evidence, PerformanceProfile, Pipeline,
    DEVICE_DATA_KEY,
};
use fiftyone_pipeline_engines::AspectPropertyValue;

/// Options the example runs with, so `main` and the test share one entry point.
pub struct ExampleOptions {
    /// The path to the on-premise Hash data file the engine loads.
    pub data_file: PathBuf,
    /// How many detections to run when timing the "fewer properties is faster"
    /// comparison. Kept small in tests, larger from `main` for a steadier signal.
    pub timing_iterations: usize,
}

// [example]
/// Run the on-premise match-metrics example.
///
/// Processes a sample User-Agent and prints the detection match metrics
/// (MatchedNodes, Difference, Drift, Iterations, Method and the DeviceId), then
/// demonstrates that restricting the requested properties to a single component
/// reduces detection time compared with requesting every property.
pub fn run(options: ExampleOptions) -> Result<()> {
    // Build a pipeline that returns every property in the data tier. Requesting
    // no specific property loads them all, so every component is resolved and the
    // full DeviceId (all four profile ids) is produced. Usage sharing is off
    // because this is a console example. In normal operation leave it on,
    // which is the builder's default.
    let pipeline = DeviceDetectionPipelineBuilder::on_premise(&options.data_file)
        .performance_profile(PerformanceProfile::LowMemory)
        .share_usage(false)
        // Off so no licence key is needed.
        .auto_update(false)
        .build()
        .context("failed to build the on-premise device-detection pipeline")?;

    // Print the standard data-file warnings up front.
    device_detection_examples::print_data_file_warnings(
        &options.data_file,
        PerformanceProfile::LowMemory,
    )?;

    // Run a single detection and print the match metrics for it.
    let sample = examples_shared::evidence::mobile_user_agent_evidence();
    print_match_metrics(&pipeline, &sample)?;

    // Demonstrate that requesting fewer properties is faster: time a one-property
    // engine against an all-properties engine over the same evidence.
    compare_property_count_timing(&options)?;

    Ok(())
}
// [example]

/// Process one evidence set and print every match metric the on-premise engine
/// exposes for the detection.
fn print_match_metrics(pipeline: &Arc<Pipeline>, pairs: &[(&str, &str)]) -> Result<()> {
    let mut message = String::new();
    let _ = writeln!(message, "--- Match metrics for the sample detection ---");
    let _ = writeln!(message, "Evidence:");
    for (key, value) in pairs {
        let _ = writeln!(message, "\t{key:<34}: {value}");
    }
    print!("{message}");

    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add_all(pairs.iter().map(|(k, v)| (*k, *v)))
            .build(),
    );
    data.process()
        .context("on-premise pipeline processing failed")?;
    let device = data
        .get(DEVICE_DATA_KEY)
        .context("the Hash engine should have produced device data")?;

    // The match metrics describe how the detection was reached. For a discussion
    // of what they mean see
    // https://51degrees.com/documentation/_device_detection__hash.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-onprem-match-metrics.rs&utm_term=print_match_metrics
    let mut message = String::new();
    let _ = writeln!(message, "Match metrics:");
    // DeviceId is the four hyphen-separated profile ids that uniquely identify
    // the detected combination of hardware, OS, browser and crawler.
    output_value("DeviceId", &device.device_id(), &mut message);
    // MatchedNodes: how many hash nodes were matched within the evidence.
    output_value("MatchedNodes", &device.matched_nodes(), &mut message);
    // Difference: larger means the detector is less confident in the result.
    output_value("Difference", &device.difference(), &mut message);
    // Drift: how far matched substrings were from where they were expected.
    output_value("Drift", &device.drift(), &mut message);
    // Iterations: how many graph nodes were visited to find the match.
    output_value("Iterations", &device.iterations(), &mut message);
    // Method: the algorithm used, for example PERFORMANCE or PREDICTIVE.
    output_value("Method", &device.method(), &mut message);
    let _ = writeln!(message);

    // Each metric is an AspectPropertyValue, so a metric the underlying data set
    // does not supply a value for is reported as a no-value rather than a crash.
    // The DeviceId is always available from a match; the numeric metrics depend
    // on what the loaded data set exposes.
    let _ = writeln!(
        message,
        "(A metric shown as a no-value was not supplied by this data set for this \
         detection; the DeviceId above is always produced for a match.)"
    );
    let _ = writeln!(message);

    // Show a couple of ordinary detected properties alongside the metrics, so the
    // result is self-explanatory.
    let _ = writeln!(message, "Detected device properties:");
    output_value("IsMobile", &device.is_mobile(), &mut message);
    output_value("HardwareName", &device.hardware_name_joined(), &mut message);
    let _ = writeln!(message);
    print!("{message}");

    Ok(())
}

/// Time an engine restricted to one property against an engine returning every
/// property, over the same evidence, and report that fewer properties is faster.
fn compare_property_count_timing(options: &ExampleOptions) -> Result<()> {
    // A single-property engine. IsMobile is a hardware-component property, so the
    // engine need only resolve that one component.
    let few = DeviceDetectionPipelineBuilder::on_premise(&options.data_file)
        .performance_profile(PerformanceProfile::LowMemory)
        .property("IsMobile")
        .share_usage(false)
        .auto_update(false)
        .build()
        .context("failed to build the single-property pipeline")?;

    // An all-properties engine: requesting no specific property loads every
    // property the data tier supports, across all components.
    let all = DeviceDetectionPipelineBuilder::on_premise(&options.data_file)
        .performance_profile(PerformanceProfile::LowMemory)
        .share_usage(false)
        .auto_update(false)
        .build()
        .context("failed to build the all-properties pipeline")?;

    let sample = examples_shared::evidence::mobile_user_agent_evidence();
    let few_time = time_detections(&few, &sample, options.timing_iterations)?;
    let all_time = time_detections(&all, &sample, options.timing_iterations)?;

    let mut message = String::new();
    let _ = writeln!(
        message,
        "--- Effect of requesting fewer properties ({} detections each) ---",
        options.timing_iterations
    );
    let _ = writeln!(
        message,
        "\tOne property  (IsMobile)      : {:?} total, {:?} each",
        few_time,
        per_detection(few_time, options.timing_iterations)
    );
    let _ = writeln!(
        message,
        "\tAll properties (whole tier)   : {:?} total, {:?} each",
        all_time,
        per_detection(all_time, options.timing_iterations)
    );
    let _ = writeln!(
        message,
        "Requesting fewer properties, ideally from a single component, reduces \
         detection time."
    );
    print!("{message}");

    Ok(())
}

/// Run `iterations` detections of `pairs` through `pipeline` and return the total
/// elapsed time, touching a property each time so the work is not optimized away.
fn time_detections(
    pipeline: &Arc<Pipeline>,
    pairs: &[(&str, &str)],
    iterations: usize,
) -> Result<Duration> {
    // One warm-up detection so the first-call data-set initialization is not
    // counted in the timed loop.
    let mut warm = pipeline.create_flow_data_with(
        Evidence::builder()
            .add_all(pairs.iter().map(|(k, v)| (*k, *v)))
            .build(),
    );
    warm.process().context("warm-up detection failed")?;

    let start = Instant::now();
    let mut sink = 0u64;
    for _ in 0..iterations {
        let mut data = pipeline.create_flow_data_with(
            Evidence::builder()
                .add_all(pairs.iter().map(|(k, v)| (*k, *v)))
                .build(),
        );
        data.process().context("timed detection failed")?;
        if let Some(device) = data.get(DEVICE_DATA_KEY) {
            // Read IsMobile to keep the optimizer from eliding the detection.
            if device.is_mobile().as_option().copied().unwrap_or(false) {
                sink = sink.wrapping_add(1);
            }
        }
    }
    // Use the accumulator so it is observable and the loop cannot be removed.
    std::hint::black_box(sink);
    Ok(start.elapsed())
}

/// The average time per detection, guarding against a zero iteration count.
fn per_detection(total: Duration, iterations: usize) -> Duration {
    if iterations == 0 {
        Duration::ZERO
    } else {
        total / iterations as u32
    }
}

/// Append one typed property to the output, handling the no-value case.
fn output_value<T: std::fmt::Display>(
    name: &str,
    value: &AspectPropertyValue<T>,
    message: &mut String,
) {
    match value.value() {
        Ok(value) => {
            let _ = writeln!(message, "\t{name:<14}: {value}");
        }
        Err(_) => {
            let reason = value.no_value_message().unwrap_or("No value");
            let _ = writeln!(message, "\t{name:<14}: {reason}");
        }
    }
}

/// A small helper trait so the example can print a `HardwareName` list value
/// through the same `output_value` path as the scalar properties.
trait HardwareNameJoined {
    /// `HardwareName` joined into a single display string, preserving the
    /// no-value state.
    fn hardware_name_joined(&self) -> AspectPropertyValue<String>;
}

impl HardwareNameJoined for fiftyone_device_detection::DeviceDataBase {
    fn hardware_name_joined(&self) -> AspectPropertyValue<String> {
        self.hardware_name().map(|names| names.join(", "))
    }
}

/// Resolve the data file then run the example, with the same fallback chain as
/// the other on-premise examples (argument, env var, Lite file).
fn main() -> Result<()> {
    let data_file = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .or_else(examples_shared::dd_data_path);

    let Some(data_file) = data_file else {
        anyhow::bail!(
            "No device-detection data file found. Set 51DEGREES_DD_PATH, or pass \
             the path as the first argument."
        );
    };

    run(ExampleOptions {
        data_file,
        // A few thousand detections give a steady timing signal without making
        // the example slow to run from the command line.
        timing_iterations: 5_000,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run the example against the Lite Hash file, skipping when none is present.
    #[test]
    fn runs_against_the_lite_data_file() {
        let Some(data_file) = examples_shared::dd_data_path() else {
            eprintln!("skipping: no on-premise data file found (set 51DEGREES_DD_PATH)");
            return;
        };
        run(ExampleOptions {
            data_file,
            // Keep the timing loop short so the test stays fast.
            timing_iterations: 200,
        })
        .expect("the on-premise match-metrics example should complete");
    }
}

/*
 * @example dd-onprem-match-metrics.rs
 *
 * The device-detection on-premise match-metrics console example.
 *
 * On-premise detection provides metrics that give insight into the detection
 * process and the confidence in the result. After processing a sample User-Agent
 * the example prints, through the strongly-typed accessors:
 *
 * - `DeviceId`     - the four hyphen-separated profile ids identifying the
 *                    detected hardware / platform / browser / crawler combination,
 * - `MatchedNodes` - the number of hash nodes matched within the evidence,
 * - `Difference`   - how far the matched substrings were from the expected values
 *                    (larger means less confident),
 * - `Drift`        - the total difference in character positions where substring
 *                    hashes were found versus expected,
 * - `Iterations`   - the number of graph nodes visited to find the match,
 * - `Method`       - the algorithm used, for example PERFORMANCE or PREDICTIVE.
 *
 * Each metric is an `AspectPropertyValue<T>`, so the example prints the value
 * when present and the engine's no-value explanation otherwise. The match metrics
 * are pseudo-properties the Hash engine advertises in addition to the data-file
 * properties. The DeviceId is always produced for a match; a numeric metric is
 * shown only when the loaded data set supplies a value for it, and otherwise
 * reported as a no-value rather than fabricated.
 *
 * A key point of this example is that reducing the number of properties requested
 * reduces the time taken for detection, because the engine resolves fewer
 * components. To make that concrete the example builds two pipelines over the
 * same data file, one restricted to a single hardware property (`IsMobile`) and
 * one returning every property the data tier supports, then times the same
 * detection through each and reports the per-detection time. The single-property
 * engine is the faster of the two.
 *
 * The example prints the data file's tier and publish date and the standard
 * warnings (Lite tier has a small number of properties and limited accuracy; a
 * file more than 30 days old may miss the latest devices). For a discussion of
 * metrics and data-set production see
 * https://51degrees.com/documentation/_device_detection__hash.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-onprem-match-metrics.rs&utm_term=dd-onprem-match-metrics and the
 * performance-options page.
 *
 * Usage sharing is turned off with `.share_usage(false)`, because this is a
 * console example. In normal operation leave it on, which is the builder's
 * default, so 51Degrees receives evidence that improves detection.
 *
 * The data file is read from the first command-line argument, the
 * `51DEGREES_DD_PATH` environment variable, or the Lite Hash file in the data
 * repository inside the cxx submodule, which the example downloads when it is
 * missing.
 */
