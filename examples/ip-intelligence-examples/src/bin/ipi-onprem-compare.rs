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
//! The location properties compared need the Enterprise data file, and the run
//! says so when the file in use does not carry them. The descriptive block the
//! documentation tooling renders lives at the bottom of the file.
//!
//! @snippet ipi-onprem-compare.rs example

use std::io::Write;
use std::sync::Arc;

use anyhow::Context;
use fiftyone_ip_intelligence::{IpIntelligencePipelineBuilder, PerformanceProfile, IP_DATA_KEY};
use fiftyone_pipeline_core::{Evidence, Pipeline};

/// Where to ask about a copy of the Enterprise data file.
const CONTACT_US_URL: &str = "https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-compare.rs&utm_term=enterprise-data-file";

/// The links the example's output ends with.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("IP Intelligence", "https://51degrees.com/ip-intelligence?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-compare.rs&utm_term=find-out-more"),
    ("Pricing", "https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-compare.rs&utm_term=find-out-more"),
    ("Documentation", "https://51degrees.com/documentation/index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-compare.rs&utm_term=find-out-more"),
];

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
            profiles: vec![
                PerformanceProfile::HighPerformance,
                PerformanceProfile::LowMemory,
                PerformanceProfile::Balanced,
            ],
            ips: ("1.1.1.1".to_owned(), "8.8.8.8".to_owned()),
        })
    }
}

/// The properties compared across the runs. The free ASN file carries Asn, the
/// free Lite file RegisteredName, and only the Enterprise data file the
/// location properties.
const COMPARED_PROPERTIES: &[&str] = &["Asn", "RegisteredName", "Country", "Town"];

/// The compared properties only the Enterprise data file carries.
const ENTERPRISE_COMPARED: &[&str] = &["Country", "Town"];

/// The properties the pipeline is asked for. AsnName and RegisteredName are
/// included so the request holds a property each free file carries, because a
/// data file opens only when it carries at least one property asked for.
const REQUESTED_PROPERTIES: &[&str] = &["Asn", "AsnName", "RegisteredName", "Country", "Town"];

// [example]
/// Run the compare example, writing all output to `out`.
///
/// Usage sharing is not enabled, as this is a console example.
pub fn run(options: &ExampleOptions, out: &mut dyn Write) -> anyhow::Result<()> {
    // The data file's own list of properties says which compared properties it
    // can answer. The free files carry none of the location ones.
    let available = ip_intelligence_examples::data_file_properties(&options.data_file)?;

    compare_across_profiles(options, &available, out)?;
    writeln!(out)?;
    compare_two_ips(options, &available, out)?;

    writeln!(out)?;
    ip_intelligence_examples::write_data_file_warnings(&options.data_file, out)?;
    let missing = examples_shared::missing_properties(ENTERPRISE_COMPARED, &available);
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

/// Build a pipeline for one profile and look an IP up, returning the rendered
/// value of each compared property. `available` holds the properties the data
/// file carries.
fn lookup(
    data_file: &std::path::Path,
    profile: PerformanceProfile,
    ip: &str,
    available: &[String],
) -> anyhow::Result<Vec<String>> {
    let pipeline: Arc<Pipeline> = IpIntelligencePipelineBuilder::on_premise(data_file)
        .performance_profile(profile)
        .properties(REQUESTED_PROPERTIES.iter().copied())
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
    Ok(COMPARED_PROPERTIES
        .iter()
        .map(|name| {
            if !examples_shared::is_available(name, available) {
                let reason = if ENTERPRISE_COMPARED.contains(name) {
                    examples_shared::NEEDS_ENTERPRISE_DATA_FILE
                } else {
                    "not in this data file"
                };
                return format!("{name} {reason}");
            }
            match ip_data.string(name).value() {
                Ok(value) => format!("{name} {value}"),
                Err(_) => format!("{name} (no value)"),
            }
        })
        .collect())
}

/// Look the first IP up under each profile and show the results side by side,
/// confirming that the choice of profile does not change the result.
fn compare_across_profiles(
    options: &ExampleOptions,
    available: &[String],
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let ip = &options.ips.0;
    writeln!(
        out,
        "Comparing {} for {ip} across performance profiles:",
        COMPARED_PROPERTIES.join(", ")
    )?;

    let mut rendered = Vec::new();
    for &profile in &options.profiles {
        let values = lookup(&options.data_file, profile, ip, available)?;
        writeln!(out, "\t{profile:?}: {}", values.join(", "))?;
        rendered.push(values);
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
fn compare_two_ips(
    options: &ExampleOptions,
    available: &[String],
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let profile = PerformanceProfile::HighPerformance;
    let (a, b) = &options.ips;

    let values_a = lookup(&options.data_file, profile, a, available)?;
    let values_b = lookup(&options.data_file, profile, b, available)?;

    writeln!(out, "Comparing two IP addresses (profile {profile:?}):")?;
    writeln!(out, "\t{a}: {}", values_a.join(", "))?;
    writeln!(out, "\t{b}: {}", values_b.join(", "))?;
    // Asn and RegisteredName, the compared properties a free file can carry,
    // say which network each IP is in.
    let same_network = COMPARED_PROPERTIES
        .iter()
        .zip(values_a.iter().zip(&values_b))
        .all(|(name, (a, b))| ENTERPRISE_COMPARED.contains(name) || a == b);
    writeln!(
        out,
        "\t=> the two IPs map to {} network.",
        if same_network {
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
        assert!(printed.contains(examples_shared::FIND_OUT_MORE_HEADING));
        // The ASN file carries no location properties, so the run says so.
        // _51DEGREES_IPI_PATH can name another file.
        let asn_file = examples_shared::IPI_ASN_RELATIVE_PATH.rsplit('/').next();
        if options.data_file.file_name().and_then(|n| n.to_str()) == asn_file {
            assert!(printed.contains("Country needs the Enterprise data file"));
            assert!(printed.contains("utm_term=enterprise-data-file"));
        }
    }
}

/*
 * @example ipi-onprem-compare.rs
 *
 * The on-premise IP Intelligence compare example.
 *
 * It shows two kinds of comparison:
 *
 * 1. The same IP looked up under several performance profiles
 *    (`HighPerformance`, `LowMemory`, `Balanced`). The profiles trade memory
 *    against speed but must not change the *result*, so the example asserts
 *    they agree. This is a quick way to confirm that a profile change made for
 *    performance reasons has not altered detection.
 * 2. Two IP addresses looked up under one profile and compared against each
 *    other, reporting whether they map to the same network.
 *
 * Each value is shown as the single plain value the data file resolves. The
 * example compares `Asn`, which the free ASN file carries, `RegisteredName`,
 * which the free Lite file carries, and `Country` and `Town`, which only the
 * Enterprise data file carries. The data file's own list of properties says
 * which it can answer. A free property the file lacks says "not in this data
 * file". With a free file the location properties each say "needs the
 * Enterprise data file", and the run names them with a link to contact
 * 51Degrees about a copy of the Enterprise data file. It also prints the
 * standard data-file warnings and ends with a "Find out more" section.
 *
 * # Data file
 *
 * Resolved by `examples_shared::ipi_data_path`, which gives the free Lite file
 * when no other file is set. Contact 51Degrees for an Enterprise file with the
 * full property set: <https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-compare.rs&utm_term=ipi-onprem-compare>.
 *
 * # Usage sharing
 *
 * Not enabled, because this is a console example. Production deployments are
 * encouraged to enable usage sharing.
 */
