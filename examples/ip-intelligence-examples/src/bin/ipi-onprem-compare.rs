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

//! @page ipi-onprem-compare-example Compare (IP Intelligence, On-premise)
//!
//! On-premise IP Intelligence compare console example.
//!
//! Looks the same IP addresses up under several performance profiles and shows
//! the results side by side, then compares two IP addresses against each other.
//! The descriptive block the documentation tooling renders lives at the bottom
//! of the file.
//!
//! @snippet ipi-onprem-compare.rs example

use std::io::Write;
use std::sync::Arc;

use anyhow::Context;
use fiftyone_ip_intelligence::{IpIntelligencePipelineBuilder, PerformanceProfile, IP_DATA_KEY};
use fiftyone_pipeline_core::{Evidence, Pipeline};

/// The options that drive a run of this example.
pub struct ExampleOptions {
    /// The full path to the `.ipi` data file to open.
    pub data_file: std::path::PathBuf,
    /// The performance profiles to compare results across.
    pub profiles: Vec<PerformanceProfile>,
    /// The two IP addresses to compare side by side.
    pub ips: (String, String),
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
            profiles: vec![
                PerformanceProfile::HighPerformance,
                PerformanceProfile::LowMemory,
                PerformanceProfile::Balanced,
            ],
            ips: ("1.1.1.1".to_owned(), "8.8.8.8".to_owned()),
        })
    }
}

/// The property compared across the runs. The ASN data file populates the
/// autonomous system; an Enterprise file would populate the location set too.
const COMPARE_PROPERTY: &str = "Asn";

// [example]
/// Run the compare example, writing all output to `out`.
///
/// Usage sharing is not enabled, as this is a console example.
pub fn run(options: &ExampleOptions, out: &mut dyn Write) -> anyhow::Result<()> {
    compare_across_profiles(options, out)?;
    writeln!(out)?;
    compare_two_ips(options, out)?;
    Ok(())
}
// [example]

/// Build a pipeline for one profile and look an IP up, returning the rendered
/// value of the compared property.
fn lookup(
    data_file: &std::path::Path,
    profile: PerformanceProfile,
    ip: &str,
) -> anyhow::Result<String> {
    let pipeline: Arc<Pipeline> = IpIntelligencePipelineBuilder::on_premise(data_file)
        .performance_profile(profile)
        .properties([COMPARE_PROPERTY, "AsnName"])
        .auto_update(false)
        .file_system_watcher(false)
        .build()
        .with_context(|| format!("failed to build a {profile:?} pipeline"))?;

    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("query.client-ip", ip).build());
    data.process()
        .with_context(|| format!("processing IP {ip} failed"))?;

    let ip_data = data
        .get(IP_DATA_KEY)
        .context("the pipeline produced no IP Intelligence data")?;
    let value = ip_data.string(COMPARE_PROPERTY);
    Ok(match value.value() {
        Ok(value) => value.clone(),
        Err(_) => "(no value)".to_owned(),
    })
}

/// Look the first IP up under each profile and show the results side by side,
/// confirming that the choice of profile does not change the result.
fn compare_across_profiles(options: &ExampleOptions, out: &mut dyn Write) -> anyhow::Result<()> {
    let ip = &options.ips.0;
    writeln!(
        out,
        "Comparing {COMPARE_PROPERTY} for {ip} across performance profiles:"
    )?;

    let mut rendered = Vec::new();
    for &profile in &options.profiles {
        let value = lookup(&options.data_file, profile, ip)?;
        writeln!(out, "\t{profile:?}: {value}")?;
        rendered.push(value);
    }

    let agree = rendered.windows(2).all(|pair| pair[0] == pair[1]);
    writeln!(
        out,
        "\t=> profiles {} for this IP.",
        if agree {
            "agree"
        } else {
            "disagree (this can happen near the limits of the data set)"
        }
    )?;
    Ok(())
}

/// Compare the two IP addresses against each other under one profile.
fn compare_two_ips(options: &ExampleOptions, out: &mut dyn Write) -> anyhow::Result<()> {
    let profile = PerformanceProfile::HighPerformance;
    let (a, b) = &options.ips;

    let value_a = lookup(&options.data_file, profile, a)?;
    let value_b = lookup(&options.data_file, profile, b)?;

    writeln!(out, "Comparing two IP addresses (profile {profile:?}):")?;
    writeln!(out, "\t{a}: {value_a}")?;
    writeln!(out, "\t{b}: {value_b}")?;
    writeln!(
        out,
        "\t=> the two IPs map to {} autonomous system.",
        if value_a == value_b {
            "the same"
        } else {
            "a different"
        }
    )?;
    Ok(())
}

/// Read the data file, then run the example, writing to standard output.
fn main() -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let options = match std::env::args().nth(1) {
        Some(path) => ExampleOptions {
            data_file: std::path::PathBuf::from(path),
            profiles: vec![
                PerformanceProfile::HighPerformance,
                PerformanceProfile::LowMemory,
                PerformanceProfile::Balanced,
            ],
            ips: ("1.1.1.1".to_owned(), "8.8.8.8".to_owned()),
        },
        None => match ExampleOptions::from_env() {
            Some(options) => options,
            None => {
                writeln!(
                    out,
                    "No IP Intelligence data file could be located. Set {} to an \
                     .ipi file, or check out the cxx submodule.",
                    examples_shared::IPI_PATH_ENV_VAR
                )?;
                return Ok(());
            }
        },
    };

    run(&options, &mut out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_runs_across_profiles_and_ips() {
        let Some(options) = ExampleOptions::for_tier(examples_shared::IpiTier::Asn) else {
            eprintln!("no usable IP Intelligence data file; skipping compare run");
            return;
        };

        let mut buffer = Vec::new();
        run(&options, &mut buffer).expect("the compare example should run");

        let printed = String::from_utf8(buffer).expect("output should be valid UTF-8");
        assert!(printed.contains("across performance profiles"));
        assert!(printed.contains("Comparing two IP addresses"));
        // 1.1.1.1 is Cloudflare AS13335; the profiles must agree on it.
        assert!(printed.contains("AS13335"));
        assert!(printed.contains("profiles agree"));
    }
}

/*
 * @example ipi-onprem-compare.rs
 *
 * The on-premise IP Intelligence compare example, adapted to the ASN data file
 * the workspace bundles.
 *
 * It shows two kinds of comparison:
 *
 * 1. The same IP looked up under several performance profiles
 *    (`HighPerformance`, `LowMemory`, `Balanced`). The profiles trade memory
 *    against speed but must not change the *result*, so the example asserts
 *    they agree. This is a quick way to confirm that a profile change made for
 *    performance reasons has not altered detection.
 * 2. Two IP addresses looked up under one profile and compared against each
 *    other, reporting whether they map to the same autonomous system.
 *
 * Each value is shown as the single plain value the data file resolves.
 *
 * # Data file
 *
 * Resolved by `examples_shared::ipi_data_path`. Contact 51Degrees for an
 * Enterprise file with the full property set: <https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-compare.rs&utm_term=ipi-onprem-compare>.
 *
 * # Usage sharing
 *
 * Not enabled, because this is a console example. Production deployments are
 * encouraged to enable usage sharing.
 */
