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
//! lookup took, whether the result came from a cache, the resolved value of a
//! summary property and the location accuracy measures only the Enterprise
//! data file carries, in place of Device Detection's match metrics. The
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

/// Where to ask about a copy of the Enterprise data file.
const CONTACT_US_URL: &str = "https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-metrics.rs&utm_term=enterprise-data-file";

/// The links the example's output ends with.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("IP Intelligence", "https://51degrees.com/ip-intelligence?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-metrics.rs&utm_term=find-out-more"),
    ("Pricing", "https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-metrics.rs&utm_term=find-out-more"),
    ("Documentation", "https://51degrees.com/documentation/index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-metrics.rs&utm_term=find-out-more"),
];

/// The options that drive a run of this example.
pub struct ExampleOptions {
    /// The full path to the `.ipi` data file to open.
    pub data_file: std::path::PathBuf,
    /// The IP addresses to look up and time.
    pub ips: Vec<String>,
}

impl ExampleOptions {
    /// Default options, reading the Enterprise file when its share is
    /// reachable and otherwise the free Lite file.
    pub fn from_env() -> Option<Self> {
        Self::for_tier(examples_shared::IpiTier::EnterpriseOrLite)
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

/// The properties the metrics print summarise, saying which network each IP is
/// in. The free ASN file carries Asn and the free Lite file RegisteredName.
const SUMMARY_PROPERTIES: &[&str] = &["Asn", "RegisteredName"];

/// Shown for a summary property the data file in use does not carry.
const NOT_IN_THIS_FILE: &str = "not in this data file";

/// How accurate each lookup's location is. Only the Enterprise data file
/// carries these.
const ACCURACY_PROPERTIES: &[&str] = &[
    "AccuracyRadiusMin",
    "AccuracyRadiusMax",
    "LocationConfidence",
];

/// The properties the pipeline is asked for. The summary properties give each
/// free file one it carries, because a data file opens only when it carries at
/// least one property asked for.
const REQUESTED_PROPERTIES: &[&str] = &[
    "Asn",
    "AsnName",
    "RegisteredName",
    "AccuracyRadiusMin",
    "AccuracyRadiusMax",
    "LocationConfidence",
];

// [example]
/// Run the metrics example, writing all output to `out`.
///
/// Usage sharing is not enabled, as this is a console example.
pub fn run(options: &ExampleOptions, out: &mut dyn Write) -> anyhow::Result<()> {
    // The data file's own list of properties says which accuracy measures it
    // can answer. The free files carry none of them.
    let available = ip_intelligence_examples::data_file_properties(&options.data_file)?;

    // HighPerformance favours lookup speed, which suits a metrics-focused run.
    // The other profiles (LowMemory, InMemory, Balanced, Default) trade memory
    // for speed and are exercised by the performance example.
    let pipeline: Arc<Pipeline> = IpIntelligencePipelineBuilder::on_premise(&options.data_file)
        .performance_profile(PerformanceProfile::HighPerformance)
        .properties(REQUESTED_PROPERTIES.iter().copied())
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
        report_lookup(&pipeline, ip, &available, out)?;
    }

    writeln!(out)?;
    writeln!(
        out,
        "Note: IP Intelligence does not expose Device Detection's match metrics \
         (difference, drift, method, iterations). The summary properties above \
         are the single values the data file resolves for each lookup."
    )?;

    writeln!(out)?;
    ip_intelligence_examples::write_data_file_warnings(&options.data_file, out)?;
    let missing = examples_shared::missing_properties(ACCURACY_PROPERTIES, &available);
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

/// Time one lookup and print its insight: elapsed time, cache-hit flag, the
/// resolved values of the summary properties and the accuracy measures.
/// `available` holds the properties the data file carries.
fn report_lookup(
    pipeline: &Arc<Pipeline>,
    ip: &str,
    available: &[String],
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("query.client-ip", ip).build());

    let started = Instant::now();
    data.process()
        .with_context(|| format!("processing IP {ip} failed"))?;
    let elapsed = started.elapsed();

    let ip_data = data
        .get(IP_DATA_KEY)
        .context("the pipeline produced no IP Intelligence data")?;

    writeln!(out, "IP {ip}")?;
    writeln!(
        out,
        "\tLookup time: {:.3} ms",
        elapsed.as_secs_f64() * 1000.0
    )?;
    writeln!(out, "\tCache hit: {}", ip_data.cache_hit())?;
    for name in SUMMARY_PROPERTIES {
        let value = if examples_shared::is_available(name, available) {
            match ip_data.string(name).value() {
                Ok(value) => value.clone(),
                Err(_) => "(no value)".to_owned(),
            }
        } else {
            NOT_IN_THIS_FILE.to_owned()
        };
        writeln!(out, "\t{name}: {value}")?;
    }
    for name in ACCURACY_PROPERTIES {
        let accuracy = if examples_shared::is_available(name, available) {
            examples_shared::get_property_as_string(ip_data, name)
        } else {
            examples_shared::NEEDS_ENTERPRISE_DATA_FILE.to_owned()
        };
        writeln!(out, "\t{name}: {accuracy}")?;
    }

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
        assert!(printed.contains(examples_shared::FIND_OUT_MORE_HEADING));
        // The ASN file carries no accuracy measures, so the run says so.
        // 51DEGREES_IPI_PATH can name another file.
        let asn_file = examples_shared::IPI_ASN_RELATIVE_PATH.rsplit('/').next();
        if options.data_file.file_name().and_then(|n| n.to_str()) == asn_file {
            assert!(printed.contains("AccuracyRadiusMin: needs the Enterprise data file"));
            assert!(printed.contains("utm_term=enterprise-data-file"));
        }
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
 * 3. The resolved values of `Asn` and `RegisteredName`, which say which
 *    network the IP is in. The free ASN file carries the first and the free
 *    Lite file the second, and one the file lacks says "not in this data
 *    file".
 * 4. How accurate the location is, from `AccuracyRadiusMin`,
 *    `AccuracyRadiusMax` and `LocationConfidence`, which only the Enterprise
 *    data file carries. The data file's own list of properties says which it
 *    can answer, so with a free file each says "needs the Enterprise data
 *    file", and the run names them with a link to contact 51Degrees about a
 *    copy of the Enterprise data file.
 *
 * It also prints the standard data-file warnings and ends with a "Find out
 * more" section.
 *
 * The example builds the pipeline with the `HighPerformance` profile. The
 * performance example explores how the other profiles (LowMemory, InMemory,
 * Balanced, Default) trade memory against speed.
 *
 * # Data file
 *
 * Resolved by `examples_shared::ipi_data_path`, which gives the free Lite file
 * when no other file is set. Contact 51Degrees for an Enterprise file with the
 * full property set: <https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-metrics.rs&utm_term=ipi-onprem-metrics>.
 *
 * # Usage sharing
 *
 * Not enabled, because this is a console example. Production deployments are
 * encouraged to enable usage sharing.
 */
