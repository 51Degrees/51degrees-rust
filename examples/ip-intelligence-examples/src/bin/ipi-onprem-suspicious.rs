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
//! source of suspicious requests. With a free data file it shows the free
//! properties and says the judgement needs the Enterprise data file. The
//! descriptive block the documentation tooling renders lives at the bottom of
//! the file.
//!
//! @snippet ipi-onprem-suspicious.rs example

use std::io::Write;
use std::sync::Arc;

use anyhow::Context;
use fiftyone_ip_intelligence::{
    IpIntelligenceDataBase, IpIntelligencePipelineBuilder, PerformanceProfile, IP_DATA_KEY,
};
use fiftyone_pipeline_core::{Evidence, Pipeline};

/// Where to ask about a copy of the Enterprise data file.
const CONTACT_US_URL: &str = "https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-suspicious.rs&utm_term=enterprise-data-file";

/// The links the example's output ends with.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("IP Intelligence", "https://51degrees.com/ip-intelligence?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-suspicious.rs&utm_term=find-out-more"),
    ("Pricing", "https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-suspicious.rs&utm_term=find-out-more"),
    ("Documentation", "https://51degrees.com/documentation/index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-suspicious.rs&utm_term=find-out-more"),
];

/// The behavioral and diversity properties the judgement reads. Only the
/// Enterprise data file carries them, and they are requested by name so the
/// on-premise engine surfaces them.
const SUSPICIOUS_PROPERTIES: &[&str] = &[
    "IsCellular",
    "HardwareDiversity",
    "BrowserDiversity",
    "LocationConfidence",
    "IsHosted",
    "CountryCode",
    "HumanProbability",
];

/// The free properties also requested and shown. The ASN file carries Asn and
/// AsnName and the Lite file RegisteredCountry, which the judgement also reads.
/// A data file opens only when it carries at least one property asked for, so
/// these let either free file open.
const FREE_PROPERTIES: &[&str] = &["Asn", "AsnName", "RegisteredCountry"];

/// Shown for a free property the data file in use does not carry.
const NOT_IN_THIS_FILE: &str = "not in this data file";

/// The options that drive a run of this example.
pub struct ExampleOptions {
    /// The full path to the `.ipi` data file to open.
    pub data_file: std::path::PathBuf,
    /// The IP addresses to assess, as client-IP evidence values.
    pub ips: Vec<String>,
}

impl ExampleOptions {
    /// Default options, using the best-available loadable data-file tier. The
    /// diversity and behavioral properties are only present in an Enterprise
    /// file, so the judgement is made only when an Enterprise file is resolved.
    pub fn from_env() -> Option<Self> {
        Self::for_tier(examples_shared::IpiTier::BestAvailable)
    }

    /// Options pinned to a specific data-file tier. One test pins the
    /// Enterprise tier, the only tier that carries the diversity and behavioral
    /// properties, and skips when it is not reachable. Another pins the ASN
    /// tier, which runs without them.
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
/// IsSuspicious verdict. When the data file does not carry the properties the
/// verdict reads, each says it needs the Enterprise data file, and so does the
/// verdict. Usage sharing is not enabled, because this is a console example. A
/// production deployment, and every web example, should enable it.
pub fn run(options: &ExampleOptions, out: &mut dyn Write) -> anyhow::Result<()> {
    // The data file's own list of properties says which of the judgement's
    // properties it carries. The free files carry none of them.
    let available = ip_intelligence_examples::data_file_properties(&options.data_file)?;

    // LowMemory pages the (potentially multi-gigabyte) Enterprise data file from
    // disk rather than loading it entirely into memory. The behavioral
    // properties are requested by name so the engine surfaces them in addition to
    // the typed location and network set.
    let pipeline: Arc<Pipeline> = IpIntelligencePipelineBuilder::on_premise(&options.data_file)
        .performance_profile(PerformanceProfile::LowMemory)
        .properties(SUSPICIOUS_PROPERTIES.iter().chain(FREE_PROPERTIES).copied())
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
        assess_ip(&pipeline, ip, &available, out)?;
    }

    writeln!(out)?;
    ip_intelligence_examples::write_data_file_warnings(&options.data_file, out)?;
    let missing = examples_shared::missing_properties(SUSPICIOUS_PROPERTIES, &available);
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

/// Assess one IP address and print the contributing values plus the verdict.
/// `available` holds the properties the data file carries.
fn assess_ip(
    pipeline: &Arc<Pipeline>,
    ip: &str,
    available: &[String],
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("query.client-ip", ip).build());
    data.process()
        .with_context(|| format!("processing IP {ip} failed"))?;

    let ip_data = data
        .get(IP_DATA_KEY)
        .context("the pipeline produced no IP Intelligence data")?;

    // The on-premise engine surfaces every non-typed property as a weighted
    // string, so each behavioral property is read through `weighted_string` and
    // reduced to its most probable candidate. A property the data file does not
    // carry reads back as a no-value, which the helpers map to a safe default.
    // That default is never shown, because its line says why there is none.
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

    // The verdict is made only when the data file carries every property it
    // reads, so a free file never shows a verdict built on missing values.
    let verdict =
        if examples_shared::missing_properties(SUSPICIOUS_PROPERTIES, available).is_empty() {
            is_suspicious(&inputs).to_string()
        } else {
            examples_shared::NEEDS_ENTERPRISE_DATA_FILE.to_owned()
        };

    let no_value = || "(no value)".to_owned();
    let lines = [
        ("Asn", plain_string(ip_data, "Asn")),
        ("AsnName", plain_string(ip_data, "AsnName")),
        ("IsCellular", inputs.is_cellular.to_string()),
        ("HardwareDiversity", inputs.hardware_diversity.to_string()),
        ("BrowserDiversity", inputs.browser_diversity.to_string()),
        (
            "LocationConfidence",
            inputs.location_confidence.clone().unwrap_or_else(no_value),
        ),
        ("IsHosted", inputs.is_hosted.to_string()),
        (
            "CountryCode",
            inputs.country.clone().unwrap_or_else(no_value),
        ),
        (
            "RegisteredCountry",
            inputs.registered_country.clone().unwrap_or_else(no_value),
        ),
        ("HumanProbability", inputs.human_probability.to_string()),
    ];

    writeln!(out)?;
    writeln!(out, "Input values:")?;
    writeln!(out, "\tquery.client-ip: {ip}")?;
    writeln!(out, "Results:")?;
    for (name, value) in lines {
        writeln!(out, "\t{name}: {}", shown(name, value, available))?;
    }
    writeln!(out, "\tIsSuspicious: {verdict}")?;

    Ok(())
}

/// The text for a property's line, being `value` when the data file carries
/// the property, otherwise why there is none.
fn shown(name: &str, value: String, available: &[String]) -> String {
    if examples_shared::is_available(name, available) {
        value
    } else if SUSPICIOUS_PROPERTIES.contains(&name) {
        examples_shared::NEEDS_ENTERPRISE_DATA_FILE.to_owned()
    } else {
        NOT_IN_THIS_FILE.to_owned()
    }
}

/// Read a plain string property, or `(no value)` when it has none.
fn plain_string(ip_data: &IpIntelligenceDataBase, name: &str) -> String {
    ip_data
        .string(name)
        .value()
        .map(String::clone)
        .unwrap_or_else(|_| "(no value)".to_owned())
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
            None => anyhow::bail!(
                "No IP Intelligence data file could be located. Set {} to an .ipi \
                 file, or pass its path as the first argument. The judgement needs \
                 the Enterprise data file. Contact us about getting a copy at {}",
                examples_shared::IPI_PATH_ENV_VAR,
                CONTACT_US_URL
            ),
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

    /// The example runs on the free ASN file, which carries none of the
    /// judgement's properties, so it gives no verdict and says why.
    #[test]
    fn suspicious_runs_against_the_asn_file() {
        let Some(options) = ExampleOptions::for_tier(examples_shared::IpiTier::Asn) else {
            eprintln!("no usable IP Intelligence data file; skipping suspicious run");
            return;
        };

        let mut buffer = Vec::new();
        run(&options, &mut buffer).expect("the suspicious example should run");

        let printed = String::from_utf8(buffer).expect("output should be valid UTF-8");
        assert!(printed.contains("Asn:"));
        assert!(printed.contains(examples_shared::FIND_OUT_MORE_HEADING));
        // 51DEGREES_IPI_PATH can name another file.
        let asn_file = examples_shared::IPI_ASN_RELATIVE_PATH.rsplit('/').next();
        if options.data_file.file_name().and_then(|n| n.to_str()) == asn_file {
            assert!(
                printed.contains("IsSuspicious: needs the Enterprise data file"),
                "a free file gives no verdict, output was:\n{printed}"
            );
            assert!(printed.contains("utm_term=enterprise-data-file"));
        }
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
 * candidate before the heuristic runs.
 *
 * The example also asks for `Asn` and `AsnName` from the free ASN file and
 * `RegisteredCountry` from the free Lite file, so it opens and shows something
 * useful on either free file. The data file's own list of properties says
 * which of the judgement's properties it carries. With a free file each of
 * them, and the IsSuspicious verdict, says "needs the Enterprise data file",
 * and the run names them with a link to contact 51Degrees about a copy of the
 * Enterprise data file. It also prints the standard data-file warnings and
 * ends with a "Find out more" section.
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
 * The judgement needs an Enterprise IP Intelligence data file (`.ipi`), which is
 * the tier that carries the diversity and behavioral properties. The data file is
 * resolved by `examples_shared::ipi_data_path`, and runs on the free ASN file
 * without a verdict. Contact 51Degrees for an
 * Enterprise file: <https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-suspicious.rs&utm_term=ipi-onprem-suspicious>.
 *
 * # Usage sharing
 *
 * Not enabled, because this is a console example. Production deployments, and
 * every web example, should enable usage sharing.
 */
