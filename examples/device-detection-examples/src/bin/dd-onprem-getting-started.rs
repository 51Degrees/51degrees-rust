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

//! @page dd-onprem-getting-started-example Getting Started (Device Detection, On-premise)
//!
//! Builds an on-premise Device Detection pipeline and reads strongly-typed
//! device properties (`IsMobile`, `PlatformName`, `BrowserName`, `DeviceId`)
//! back from processed User-Agent evidence. The core of the example:
//!
//! @snippet dd-onprem-getting-started.rs example

use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};

use fiftyone_device_detection::{
    DeviceData, DeviceDetectionPipelineBuilder, Evidence, PerformanceProfile, Pipeline,
    DEVICE_DATA_KEY,
};
use fiftyone_pipeline_engines::AspectPropertyValue;

/// Options the example runs with. Holding the inputs in one struct lets `main`
/// and the test feed the same `run` entry point.
pub struct ExampleOptions {
    /// The path to the on-premise Hash data file the engine loads.
    pub data_file: PathBuf,
    /// The performance profile to load the data file with. The example uses the
    /// low-memory profile, whose performance is sufficient here.
    pub profile: PerformanceProfile,
}

// [example]
/// Run the on-premise getting-started example.
///
/// Builds an on-premise device-detection pipeline from the Hash data file, then
/// runs a desktop and a mobile User-Agent through it, printing the strongly-typed
/// `IsMobile`, `PlatformName`, `PlatformVersion`, `BrowserName`, `BrowserVersion`
/// and `DeviceId` results for each. Finishes by printing the standard data-file
/// warnings (Lite tier, more-than-30-days-old) so a limited or stale data file is
/// surfaced rather than silently degrading results.
pub fn run(options: ExampleOptions) -> Result<()> {
    // Build the on-premise pipeline. Usage sharing is off because this is a
    // console example, whose evidence 51Degrees already holds. In normal
    // operation leave it on, which is the builder's default. Automatic
    // data-file updates are off too, so no licence key is needed; see the
    // update-data-file example for them.
    let pipeline = DeviceDetectionPipelineBuilder::on_premise(&options.data_file)
        // The low-memory profile streams data from disk on demand. It is enough
        // for this example; see the performance example for the trade-offs.
        .performance_profile(options.profile)
        .share_usage(false)
        .auto_update(false)
        .build()
        .context("failed to build the on-premise device-detection pipeline")?;

    // One pipeline is built once and reused as a factory for many flow data
    // instances. In a real application it would be a long-lived singleton. Here
    // it processes a desktop and a mobile User-Agent in turn.
    for (label, pairs) in representative_evidence() {
        analyse_evidence(&pipeline, label, &pairs)?;
    }

    // Print the standard data-file warnings. The pipeline holds the engine, but
    // the helper takes the engine directly, so build a sibling engine just to
    // introspect its data file.
    device_detection_examples::print_data_file_warnings(&options.data_file, options.profile)?;

    Ok(())
}
// [example]

/// The representative evidence the example processes: a desktop and a mobile
/// User-Agent, each as a labelled `(key, value)` pair list.
fn representative_evidence() -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    vec![
        (
            "Desktop User-Agent",
            examples_shared::evidence::desktop_user_agent_evidence(),
        ),
        (
            "Mobile User-Agent",
            examples_shared::evidence::mobile_user_agent_evidence(),
        ),
    ]
}

/// Process one evidence set and print the key device properties.
fn analyse_evidence(pipeline: &Arc<Pipeline>, label: &str, pairs: &[(&str, &str)]) -> Result<()> {
    // Echo the inputs so the output is self-explanatory.
    let mut message = String::new();
    let _ = writeln!(message, "--- {label} ---");
    let _ = writeln!(message, "Input values:");
    for (key, value) in pairs {
        let _ = writeln!(message, "\t{key}: {value}");
    }
    print!("{message}");

    // FlowData conveys the evidence in and the results back out. The flow data
    // owns its results and frees them when it is dropped at the end of this
    // function, so the native results are released promptly without any manual
    // cleanup.
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add_all(pairs.iter().map(|(k, v)| (*k, *v)))
            .build(),
    );
    data.process()
        .context("on-premise pipeline processing failed")?;

    // Read the device result back through the shared key. Both the on-premise
    // and cloud engines write a `DeviceDataBase` under `DEVICE_DATA_KEY`, so this
    // code is identical whichever deployment produced the result.
    let mut message = String::new();
    let _ = writeln!(message, "Results:");
    let device = data
        .get(DEVICE_DATA_KEY)
        .context("the Hash engine should have produced device data")?;

    // Each accessor returns an AspectPropertyValue, the no-value-aware wrapper the
    // typed getters use. `output_value` prints either the value or the no-value
    // explanation, never panicking on an absent value.
    output_value("Mobile Device", &device.is_mobile(), &mut message);
    output_value("Platform Name", &device.platform_name(), &mut message);
    output_value("Platform Version", &device.platform_version(), &mut message);
    output_value("Browser Name", &device.browser_name(), &mut message);
    output_value("Browser Version", &device.browser_version(), &mut message);
    output_value("Device Id", &device.device_id(), &mut message);

    // A note on match metrics. The on-premise engine also exposes detection
    // metrics (MatchedNodes, Difference, Drift, Iterations, Method) that describe
    // how the result was reached. They are demonstrated in the match-metrics
    // example; here we only point them out.
    let _ = writeln!(
        message,
        "\t(On-premise detection also exposes match metrics such as Difference \
         and Method. See the dd-onprem-match-metrics example.)"
    );
    let _ = writeln!(message);
    print!("{message}");

    Ok(())
}

/// Append one typed property to the output, handling the no-value case.
///
/// An [`AspectPropertyValue`] behaves like a nullable type: accessing the value
/// of a no-value instance is an error, so the no-value message is printed
/// instead.
fn output_value<T: std::fmt::Display>(
    name: &str,
    value: &AspectPropertyValue<T>,
    message: &mut String,
) {
    match value.value() {
        Ok(value) => {
            let _ = writeln!(message, "\t{name}: {value}");
        }
        Err(_) => {
            let reason = value.no_value_message().unwrap_or("No value");
            let _ = writeln!(message, "\t{name}: {reason}");
        }
    }
}

/// Resolve the data file then run the example. The data file is taken from the
/// first command-line argument, the `51DEGREES_DD_PATH` environment variable, or
/// the Lite Hash file in the data repository inside the `cxx` submodule, which
/// is downloaded when it is missing. With none of those present the example
/// fails, after the resolver has printed the command that gets the file.
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
        profile: PerformanceProfile::LowMemory,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run the example end to end against the Lite Hash data file.
    ///
    /// On-premise tests run for real against the Lite file. The test is
    /// skipped (not failed) only when no data file can be located, so a checkout
    /// without the submodule keeps `cargo test` green while a developer with it
    /// gets genuine coverage.
    #[test]
    fn runs_against_the_lite_data_file() {
        let Some(data_file) = examples_shared::dd_data_path() else {
            eprintln!("skipping: no on-premise data file found (set 51DEGREES_DD_PATH)");
            return;
        };
        run(ExampleOptions {
            data_file,
            // The low-memory profile keeps the test's memory footprint small.
            profile: PerformanceProfile::LowMemory,
        })
        .expect("the on-premise getting-started example should complete");
    }
}

/*
 * @example dd-onprem-getting-started.rs
 *
 * The device-detection on-premise getting-started console example. It shows the
 * simplest end-to-end use of the native Hash engine for device detection.
 *
 * The example builds an on-premise pipeline from a Hash data file with the
 * convenience `DeviceDetectionPipelineBuilder::on_premise(data_file)`. The builder
 * places an optional UA-CH high-entropy decoder before the native Hash engine,
 * with usage sharing turned off. The pipeline is built once and reused as a factory for
 * many `FlowData` instances, which is the recommended lifecycle: one long-lived
 * pipeline (often a process-wide singleton) and a short-lived flow data per
 * request.
 *
 * For each representative evidence set (a desktop User-Agent and a mobile
 * User-Agent) the example creates a `FlowData`, adds the evidence, processes it,
 * then reads common device properties back through their strongly-typed accessors:
 *
 * - `IsMobile` (a boolean),
 * - `PlatformName` and `PlatformVersion` (strings),
 * - `BrowserName` and `BrowserVersion` (strings),
 * - `DeviceId` (a string, the four hyphen-separated profile ids).
 *
 * Each accessor returns an `AspectPropertyValue<T>`, the no-value-aware wrapper
 * the typed getters use. Rather than assuming a value is present, the example
 * prints the value when one is available and the engine's no-value explanation
 * when it is not.
 *
 * The example also points out that on-premise detection exposes match metrics
 * (MatchedNodes, Difference, Drift, Iterations, Method) describing how a result
 * was reached. Those are demonstrated in the dd-onprem-match-metrics example.
 *
 * Finally the example prints the data file's tier and publish date and the
 * standard warnings: the free Lite file has a small number of properties and
 * limited accuracy, and a file more than 30 days old may miss the latest devices.
 * The Enterprise file (with automatic daily updates) is described on the
 * 51Degrees pricing page.
 *
 * Usage sharing is turned off with `.share_usage(false)`, because this is a
 * console example. In normal operation leave it on, which is the builder's
 * default, so 51Degrees receives evidence that improves detection.
 *
 * The data file is read from the first command-line argument, the
 * `51DEGREES_DD_PATH` environment variable, or the Lite Hash file in the data
 * repository inside the cxx submodule, which the example downloads when it is
 * missing. The Lite file is free; the Enterprise file with
 * the full property set is described at https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-bin-dd-onprem-getting-started.rs&utm_term=dd-onprem-getting-started.
 */
