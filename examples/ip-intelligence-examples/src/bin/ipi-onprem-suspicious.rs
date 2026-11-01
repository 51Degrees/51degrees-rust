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

//! @page ipi-onprem-suspicious-example Suspicious (IP Intelligence, On-premise)
//!
//! On-premise IP Intelligence "suspicious" console example.
//!
//! Combines the diversity and behavioral properties an Enterprise data file
//! carries to make a basic judgement about whether an IP address is a likely
//! source of suspicious requests. The descriptive block the documentation tooling
//! renders lives at the bottom of the file.
//!
//! @snippet ipi-onprem-suspicious.rs example

use std::io::Write;
use std::sync::Arc;

use anyhow::Context;
use fiftyone_ip_intelligence::{
    IpIntelligenceDataBase, IpIntelligencePipelineBuilder, PerformanceProfile, IP_DATA_KEY,
};
use fiftyone_pipeline_core::{Evidence, Pipeline};

/// The behavioral and diversity properties this example reads. They are carried
/// by an Enterprise data file and are requested by name so the on-premise engine
/// surfaces them. An ASN or Lite file does not carry them, in which case each
/// reads back as a no-value and the verdict falls through to its safe default.
const SUSPICIOUS_PROPERTIES: &[&str] = &[
    "IsCellular",
    "HardwareDiversity",
    "BrowserDiversity",
    "LocationConfidence",
    "IsHosted",
    "CountryCode",
    "RegisteredCountry",
    "HumanProbability",
];

/// The options that drive a run of this example.
pub struct ExampleOptions {
    /// The full path to the `.ipi` data file to open.
    pub data_file: std::path::PathBuf,
    /// The IP addresses to assess, as client-IP evidence values.
    pub ips: Vec<String>,
}

impl ExampleOptions {
    /// Default options, using the best-available loadable data-file tier. In
    /// practice the diversity and behavioral properties are only present in an
    /// Enterprise file, so the verdict is meaningful only when an Enterprise file
    /// is resolved.
    pub fn from_env() -> Option<Self> {
        Self::for_tier(examples_shared::IpiTier::BestAvailable)
    }

    /// Options pinned to a specific data-file tier. The test pins the Enterprise
    /// tier, the only tier that carries the diversity and behavioral properties
    /// this example relies on, and skips when it is not reachable.
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

/// The diversity and behavioral values read for one IP, already reduced to the
/// single most probable candidate of each weighted property.
struct SuspiciousInputs {
    /// Whether the IP's primary connection is cellular. Cellular ranges naturally
    /// carry many hardware profiles, so this rules out a false positive.
    is_cellular: bool,
    /// How many distinct hardware profiles were observed on the IP. A high value
    /// can mean a VPN, proxy or other hosting.
    hardware_diversity: i64,
    /// How many distinct browser profiles were observed on the IP.
    browser_diversity: i64,
    /// The confidence in the observed location, for example `Low`.
    location_confidence: Option<String>,
    /// Whether the IP belongs to a hosting provider.
    is_hosted: bool,
    /// The observed country code.
    country: Option<String>,
    /// The country the IP range is registered to.
    registered_country: Option<String>,
    /// The probability the traffic is human rather than automated.
    human_probability: i64,
}

// [example]
/// Run the suspicious example against the supplied options, writing all output to
/// `out`.
///
/// Builds an on-premise IP Intelligence pipeline, requesting the diversity and
/// behavioral properties, and for each IP prints the contributing values and an
/// IsSuspicious verdict. Usage sharing is not enabled, because this is a console
/// example. A production deployment, and every web example, should enable it.
pub fn run(options: &ExampleOptions, out: &mut dyn Write) -> anyhow::Result<()> {
    // LowMemory pages the (potentially multi-gigabyte) Enterprise data file from
    // disk rather than loading it entirely into memory. The behavioral
    // properties are requested by name so the engine surfaces them in addition to
    // the typed location and network set.
    let pipeline: Arc<Pipeline> = IpIntelligencePipelineBuilder::on_premise(&options.data_file)
        .performance_profile(PerformanceProfile::LowMemory)
        .properties(SUSPICIOUS_PROPERTIES.iter().copied())
        // Automatic updates and the file-system watcher are left off so the
        // example is self-contained and does not reach out to the network.
        .auto_update(false)
        .file_system_watcher(false)
        .build()
        .context("failed to build the on-premise IP Intelligence pipeline")?;

    writeln!(
        out,
        "Assessing {} IP address(es) against {}.",
        options.ips.len(),
        options.data_file.display()
    )?;

    for ip in &options.ips {
        assess_ip(&pipeline, ip, out)?;
    }

    Ok(())
}
// [example]

/// Assess one IP address and print the contributing values plus the verdict.
fn assess_ip(pipeline: &Arc<Pipeline>, ip: &str, out: &mut dyn Write) -> anyhow::Result<()> {
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("query.client-ip", ip).build());
    data.process()
        .with_context(|| format!("processing IP {ip} failed"))?;

    let ip_data = data
        .get(IP_DATA_KEY)
        .context("the pipeline produced no IP Intelligence data")?;

    // The on-premise engine surfaces every non-typed property as a weighted
    // string, so each behavioral property is read through `weighted_string` and
    // reduced to its most probable candidate. A property the data tier does not
    // carry reads back as a no-value, which the helpers map to a safe default.
    let inputs = SuspiciousInputs {
        is_cellular: top_bool(ip_data, "IsCellular"),
        hardware_diversity: top_integer(ip_data, "HardwareDiversity"),
        browser_diversity: top_integer(ip_data, "BrowserDiversity"),
        location_confidence: top_string(ip_data, "LocationConfidence"),
        is_hosted: top_bool(ip_data, "IsHosted"),
        country: top_string(ip_data, "CountryCode"),
        registered_country: top_string(ip_data, "RegisteredCountry"),
        human_probability: top_integer(ip_data, "HumanProbability"),
    };

    let is_suspicious = is_suspicious(&inputs);

    writeln!(out)?;
    writeln!(out, "Input values:")?;
    writeln!(out, "\tquery.client-ip: {ip}")?;
    writeln!(out, "Results:")?;
    writeln!(out, "\tIsCellular: {}", inputs.is_cellular)?;
    writeln!(out, "\tHardwareDiversity: {}", inputs.hardware_diversity)?;
    writeln!(out, "\tBrowserDiversity: {}", inputs.browser_diversity)?;
    writeln!(
        out,
        "\tLocationConfidence: {}",
        inputs
            .location_confidence
            .as_deref()
            .unwrap_or("(no value)")
    )?;
    writeln!(out, "\tIsHosted: {}", inputs.is_hosted)?;
    writeln!(
        out,
        "\tCountryCode: {}",
        inputs.country.as_deref().unwrap_or("(no value)")
    )?;
    writeln!(
        out,
        "\tRegisteredCountry: {}",
        inputs.registered_country.as_deref().unwrap_or("(no value)")
    )?;
    writeln!(out, "\tHumanProbability: {}", inputs.human_probability)?;
    writeln!(out, "\tIsSuspicious: {is_suspicious}")?;

    Ok(())
}

/// The three-branch heuristic for deciding whether an IP looks suspicious.
///
/// This is a basic illustration that should not be used in production without
/// further testing and tuning. Many other properties can contribute to a real
/// judgement.
fn is_suspicious(inputs: &SuspiciousInputs) -> bool {
    // A high diversity of hardware profiles on the IP can mean a VPN, cellular,
    // proxy or other hosting. Cellular is ruled out with IsCellular, and a low
    // location confidence is further (though not conclusive) evidence of VPN or
    // proxy use rather than other hosting.
    let high_diversity_low_confidence = inputs.hardware_diversity >= 7
        && !inputs.is_cellular
        && inputs.location_confidence.as_deref() == Some("Low");

    // The observed country and the country the range is registered to differing
    // can indicate VPN or proxy use, when the IP also belongs to a host.
    let hosted_country_mismatch = inputs.is_hosted
        && matches!(inputs.country.as_deref(), Some(country) if country != "Unknown")
        && inputs.country != inputs.registered_country;

    // Browser profiles much more diverse than hardware profiles can mean some
    // devices are running multiple browsers, which can be a sign of suspicious
    // activity.
    let browser_outpaces_hardware = inputs.browser_diversity - inputs.hardware_diversity > 2;

    high_diversity_low_confidence || hosted_country_mismatch || browser_outpaces_hardware
}

/// Read the most probable candidate of a weighted string property, or `None`
/// when the property had no value (for example because the data tier does not
/// carry it).
fn top_string(ip_data: &IpIntelligenceDataBase, name: &str) -> Option<String> {
    ip_data
        .weighted_string(name)
        .value()
        .ok()
        .and_then(|list| list.first().map(|w| w.value.clone()))
}

/// Read the most probable candidate of a weighted property as an integer.
///
/// The on-premise engine surfaces these counts as weighted strings, so the top
/// candidate is parsed to an `i64`, tolerating a trailing fractional part.
/// Returns `0` when the property had no value or did not parse.
fn top_integer(ip_data: &IpIntelligenceDataBase, name: &str) -> i64 {
    match top_string(ip_data, name) {
        Some(value) => {
            let trimmed = value.trim();
            trimmed
                .parse::<i64>()
                .ok()
                .or_else(|| trimmed.parse::<f64>().ok().map(|v| v as i64))
                .unwrap_or(0)
        }
        None => 0,
    }
}

/// Read the most probable candidate of a weighted property as a boolean.
///
/// The native side renders booleans as `True`/`False` (or `1`/`0`), so both
/// spellings are accepted. Returns `false` when the property had no value.
fn top_bool(ip_data: &IpIntelligenceDataBase, name: &str) -> bool {
    match top_string(ip_data, name) {
        Some(value) => {
            let trimmed = value.trim();
            trimmed.eq_ignore_ascii_case("true") || trimmed == "1"
        }
        None => false,
    }
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
            None => {
                writeln!(
                    out,
                    "No IP Intelligence data file could be located. Set {} to an \
                     Enterprise .ipi file (the diversity and behavioral properties this \
                     example reads are only present in an Enterprise file). Contact \
                     51Degrees for one: \
                     https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-suspicious.rs&utm_term=enterprise-data-required.",
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

    /// The example runs end to end against an Enterprise data file and prints a
    /// verdict per IP. It pins the Enterprise tier, the only tier that carries the
    /// diversity and behavioral properties, and skips cleanly when that file is
    /// not reachable, so a plain `cargo test` stays green off the 51Degrees
    /// network.
    #[test]
    fn suspicious_runs_against_the_enterprise_file() {
        let Some(options) = ExampleOptions::for_tier(examples_shared::IpiTier::Enterprise) else {
            eprintln!("no Enterprise IP Intelligence data file reachable; skipping suspicious run");
            return;
        };

        let mut buffer = Vec::new();
        run(&options, &mut buffer).expect("the suspicious example should run");

        let printed = String::from_utf8(buffer).expect("output should be valid UTF-8");
        assert!(
            printed.contains("IsSuspicious"),
            "expected an IsSuspicious verdict, output was:\n{printed}"
        );
    }

    /// The heuristic's three branches each independently flag a suspicious IP,
    /// and a benign profile is not flagged. This exercises the verdict logic
    /// without needing a data file.
    #[test]
    fn heuristic_branches_flag_independently() {
        // High hardware diversity, not cellular, low location confidence.
        let branch_one = SuspiciousInputs {
            is_cellular: false,
            hardware_diversity: 8,
            browser_diversity: 8,
            location_confidence: Some("Low".to_owned()),
            is_hosted: false,
            country: Some("GB".to_owned()),
            registered_country: Some("GB".to_owned()),
            human_probability: 50,
        };
        assert!(is_suspicious(&branch_one));

        // Hosted with an observed/registered country mismatch.
        let branch_two = SuspiciousInputs {
            is_cellular: false,
            hardware_diversity: 1,
            browser_diversity: 1,
            location_confidence: Some("High".to_owned()),
            is_hosted: true,
            country: Some("US".to_owned()),
            registered_country: Some("GB".to_owned()),
            human_probability: 50,
        };
        assert!(is_suspicious(&branch_two));

        // Browser diversity far exceeding hardware diversity.
        let branch_three = SuspiciousInputs {
            is_cellular: false,
            hardware_diversity: 2,
            browser_diversity: 6,
            location_confidence: Some("High".to_owned()),
            is_hosted: false,
            country: Some("GB".to_owned()),
            registered_country: Some("GB".to_owned()),
            human_probability: 50,
        };
        assert!(is_suspicious(&branch_three));

        // A benign profile trips none of the branches.
        let benign = SuspiciousInputs {
            is_cellular: true,
            hardware_diversity: 3,
            browser_diversity: 3,
            location_confidence: Some("High".to_owned()),
            is_hosted: false,
            country: Some("GB".to_owned()),
            registered_country: Some("GB".to_owned()),
            human_probability: 95,
        };
        assert!(!is_suspicious(&benign));
    }
}

/*
 * @example ipi-onprem-suspicious.rs
 *
 * This example shows how to combine 51Degrees on-premise IP Intelligence
 * properties to judge whether an IP address is a likely source of suspicious
 * requests.
 *
 * You will learn:
 *
 * 1. How to request the diversity and behavioral properties (IsCellular,
 *    HardwareDiversity, BrowserDiversity, LocationConfidence, IsHosted,
 *    CountryCode, RegisteredCountry, HumanProbability) from the on-premise IP
 *    Intelligence engine.
 * 2. How to combine those diversity values with the observed and registered
 *    country to assess the likelihood of an IP being a source of suspicious
 *    activity.
 *
 * The IP is supplied as `query.client-ip` evidence and the result is read back
 * through the shared `IP_DATA_KEY`. The on-premise engine surfaces these
 * properties as weighted values, so each is reduced to its most probable
 * candidate before the heuristic runs. A property the data tier does not carry
 * reads back as a no-value and the contributing value falls through to its safe
 * default.
 *
 * The verdict combines three independent branches, matching the reference
 * example:
 *
 * 1. A high hardware diversity (>= 7) that is not cellular and has a low location
 *    confidence suggests a VPN or proxy.
 * 2. A hosted IP whose observed country differs from its registered country
 *    suggests VPN or proxy use.
 * 3. A browser diversity that exceeds the hardware diversity by more than two can
 *    mean devices running multiple browsers, a sign of suspicious activity.
 *
 * This is a basic illustration and should not be used in production without
 * further testing and tuning. Many other properties can contribute to a real
 * judgement.
 *
 * # Data file
 *
 * This example requires an Enterprise IP Intelligence data file (`.ipi`), which is
 * the tier that carries the diversity and behavioral properties. The data file is
 * resolved by `examples_shared::ipi_data_path`. Contact 51Degrees for an
 * Enterprise file: <https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-suspicious.rs&utm_term=ipi-onprem-suspicious>.
 *
 * # Usage sharing
 *
 * Not enabled, because this is a console example. Production deployments, and
 * every web example, should enable usage sharing.
 */
