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

//! @page ipi-onprem-metrics-example Metrics (IP Intelligence, On-premise)
//!
//! On-premise IP Intelligence metrics console example.
//!
//! Reports per-lookup insight for each IP address: the wall-clock time the
//! lookup took, whether the result came from a cache, and the resolved value of
//! a summary property, in place of Device Detection's match metrics. The
//! descriptive block the documentation tooling renders lives at the bottom of
//! the file.
//!
//! @snippet ipi-onprem-metrics.rs example

use std::io::Write;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Context;
use fiftyone_ip_intelligence::{IpIntelligencePipelineBuilder, PerformanceProfile, IP_DATA_KEY};
use fiftyone_pipeline_core::{Evidence, Pipeline};
use fiftyone_pipeline_engines::AspectData;

/// The options that drive a run of this example.
pub struct ExampleOptions {
    /// The full path to the `.ipi` data file to open.
    pub data_file: std::path::PathBuf,
    /// The IP addresses to look up and time.
    pub ips: Vec<String>,
}

impl ExampleOptions {
    /// Default options, using the best-available loadable data-file tier.
    pub fn from_env() -> Option<Self> {
        Self::for_tier(examples_shared::IpiTier::BestAvailable)
    }

    /// Options pinned to a specific data-file tier. The test pins the ASN tier
    /// so it runs deterministically against the always-loadable ASN file.
    pub fn for_tier(tier: examples_shared::IpiTier) -> Option<Self> {
        Some(ExampleOptions {
            data_file: examples_shared::ipi_data_path(tier)?,
            ips: vec![
                "1.1.1.1".to_owned(),
                "8.8.8.8".to_owned(),
                "2606:4700:4700::1111".to_owned(),
            ],
        })
    }
}

/// The property the metrics print summarises. For the ASN data file this is the
/// autonomous system; an Enterprise file would also populate the location
/// properties.
const SUMMARY_PROPERTY: &str = "Asn";

// [example]
/// Run the metrics example, writing all output to `out`.
///
/// Usage sharing is not enabled, as this is a console example.
pub fn run(options: &ExampleOptions, out: &mut dyn Write) -> anyhow::Result<()> {
    // HighPerformance favours lookup speed, which suits a metrics-focused run.
    // The other profiles (LowMemory, InMemory, Balanced, Default) trade memory
    // for speed and are exercised by the performance example.
    let pipeline: Arc<Pipeline> = IpIntelligencePipelineBuilder::on_premise(&options.data_file)
        .performance_profile(PerformanceProfile::HighPerformance)
        .properties([SUMMARY_PROPERTY, "AsnName"])
        .auto_update(false)
        .file_system_watcher(false)
        .build()
        .context("failed to build the on-premise IP Intelligence pipeline")?;

    writeln!(
        out,
        "Per-lookup metrics over {} IP address(es):",
        options.ips.len()
    )?;
    writeln!(out)?;

    for ip in &options.ips {
        report_lookup(&pipeline, ip, out)?;
    }

    writeln!(out)?;
    writeln!(
        out,
        "Note: IP Intelligence does not expose Device Detection's match metrics \
         (difference, drift, method, iterations). The summary property above is \
         the single value the data file resolves for each lookup."
    )?;

    Ok(())
}
// [example]

/// Time one lookup and print its insight: elapsed time, cache-hit flag and the
/// resolved value of the summary property.
fn report_lookup(pipeline: &Arc<Pipeline>, ip: &str, out: &mut dyn Write) -> anyhow::Result<()> {
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("query.client-ip", ip).build());

    let started = Instant::now();
    data.process()
        .with_context(|| format!("processing IP {ip} failed"))?;
    let elapsed = started.elapsed();

    let ip_data = data
        .get(IP_DATA_KEY)
        .context("the pipeline produced no IP Intelligence data")?;

    let summary = ip_data.string(SUMMARY_PROPERTY);
    let value = match summary.value() {
        Ok(value) => value.clone(),
        Err(_) => "(no value)".to_owned(),
    };

    writeln!(out, "IP {ip}")?;
    writeln!(
        out,
        "\tLookup time: {:.3} ms",
        elapsed.as_secs_f64() * 1000.0
    )?;
    writeln!(out, "\tCache hit: {}", ip_data.cache_hit())?;
    writeln!(out, "\t{SUMMARY_PROPERTY}: {value}")?;

    Ok(())
}

/// Read the data file, then run the example, writing to standard output.
fn main() -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let options = match std::env::args().nth(1) {
        Some(path) => ExampleOptions {
            data_file: std::path::PathBuf::from(path),
            ips: vec![
                "1.1.1.1".to_owned(),
                "8.8.8.8".to_owned(),
                "2606:4700:4700::1111".to_owned(),
            ],
        },
        None => match ExampleOptions::from_env() {
            Some(options) => options,
            None => anyhow::bail!(
                "No IP Intelligence data file could be located. Set {} to an .ipi \
                 file, or pass its path as the first argument.",
                examples_shared::IPI_PATH_ENV_VAR
            ),
        },
    };

    run(&options, &mut out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_report_timing_and_weighting() {
        let Some(options) = ExampleOptions::for_tier(examples_shared::IpiTier::Asn) else {
            eprintln!("no usable IP Intelligence data file; skipping metrics run");
            return;
        };

        let mut buffer = Vec::new();
        run(&options, &mut buffer).expect("the metrics example should run");

        let printed = String::from_utf8(buffer).expect("output should be valid UTF-8");
        assert!(printed.contains("Lookup time:"));
        assert!(printed.contains("Cache hit:"));
        // The Cloudflare IP resolves to AS13335 in the ASN file.
        assert!(
            printed.contains("AS13335"),
            "expected a real weighted ASN value, output was:\n{printed}"
        );
    }
}

/*
 * @example ipi-onprem-metrics.rs
 *
 * The on-premise IP Intelligence metrics example, adapted to the insight the
 * on-premise engine exposes.
 *
 * Unlike Device Detection, IP Intelligence does not expose match metrics
 * (difference, drift, method, iterations). This example therefore reports, per
 * lookup:
 *
 * 1. The wall-clock time the lookup took, measured around `FlowData::process`.
 * 2. Whether the result was served from a cache.
 * 3. The resolved value of a summary property.
 *
 * The example builds the pipeline with the `HighPerformance` profile. The
 * performance example explores how the other profiles (LowMemory, InMemory,
 * Balanced, Default) trade memory against speed.
 *
 * # Data file
 *
 * Resolved by `examples_shared::ipi_data_path`. Contact 51Degrees for an
 * Enterprise file with the full property set: <https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-metrics.rs&utm_term=ipi-onprem-metrics>.
 *
 * # Usage sharing
 *
 * Not enabled, because this is a console example. Production deployments are
 * encouraged to enable usage sharing.
 */
