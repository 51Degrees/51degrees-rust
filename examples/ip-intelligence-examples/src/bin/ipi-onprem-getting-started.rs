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

//! @page ipi-onprem-getting-started-example Getting Started (IP Intelligence, On-premise)
//!
//! On-premise IP Intelligence "getting started" console example.
//!
//! Looks an IPv4 and an IPv6 address up against a local `.ipi` data file and
//! prints the network and location properties. The descriptive block the
//! documentation tooling renders lives at the bottom of the file.
//!
//! @snippet ipi-onprem-getting-started.rs example

use std::io::Write;
use std::sync::Arc;

use anyhow::Context;
use fiftyone_ip_intelligence::{
    AspectPropertyValue, IpIntelligenceData, IpIntelligencePipelineBuilder, PerformanceProfile,
    WeightedValue, IP_DATA_KEY,
};
use fiftyone_pipeline_core::{Evidence, Pipeline};

/// The options that drive a run of this example.
///
/// Keeping the inputs in one struct lets `main` (reading the environment and
/// the command line) and the test (supplying fixed values) share the same
/// [`run`] entry point.
pub struct ExampleOptions {
    /// The full path to the `.ipi` data file to open.
    pub data_file: std::path::PathBuf,
    /// The IP addresses to look up, as client-IP evidence values.
    pub ips: Vec<String>,
}

impl ExampleOptions {
    /// Build the default options, resolving the on-premise data file through
    /// the shared resolver (the ASN file checked into the data repository, or
    /// Enterprise when the production share is reachable).
    ///
    /// Returns [`None`] when no usable data file can be located, so the caller
    /// can print a friendly hint and skip rather than panicking.
    pub fn from_env() -> Option<Self> {
        Self::for_tier(examples_shared::IpiTier::BestAvailable)
    }

    /// Build options pinned to a specific data-file tier. `main` uses the
    /// best-available tier, while the test pins to the ASN tier so it runs
    /// deterministically against the small, current, always-loadable ASN file
    /// rather than depending on the production Enterprise share being mounted.
    pub fn for_tier(tier: examples_shared::IpiTier) -> Option<Self> {
        let data_file = examples_shared::ipi_data_path(tier)?;
        Some(ExampleOptions {
            data_file,
            // A public Cloudflare IPv4 and IPv6. Both resolve to a real
            // autonomous system in the ASN data file, so the example always has
            // something to print.
            ips: vec!["1.1.1.1".to_owned(), "2606:4700:4700::1111".to_owned()],
        })
    }
}

/// Every property this example reads back and prints.
///
/// The on-premise engine narrows itself to the properties it is asked for, so
/// the example must request the whole set it displays. Requesting only `Asn` and
/// `AsnName` (which an ASN file carries beyond the curated typed set) would drop
/// the registered-range and location properties, leaving them unset even with an
/// Enterprise data file that carries them. Listing them all keeps every column
/// populated when the data file has the value.
const DISPLAYED_PROPERTIES: &[&str] = &[
    "Asn",
    "AsnName",
    "RegisteredName",
    "RegisteredOwner",
    "RegisteredCountry",
    "IpRangeStart",
    "IpRangeEnd",
    "Country",
    "CountryCode",
    "State",
    "Town",
    "Latitude",
    "Longitude",
    "TimeZoneOffset",
    "AccuracyRadiusMin",
    "IsHosted",
    "IsVPN",
    "CountryCodesGeographical",
    "CountryCodesPopulation",
    "Mcc",
];

// [example]
/// Run the getting-started example against the supplied options, writing all
/// output to `out`.
///
/// Builds an on-premise IP Intelligence pipeline, looks up each IP and prints
/// the results. The pipeline is built with usage sharing left off,
/// because this is a console example. A production deployment that wants to
/// help improve the data should enable it (see the engine builder's
/// `share_usage` equivalent on the cloud/web side).
pub fn run(options: &ExampleOptions, out: &mut dyn Write) -> anyhow::Result<()> {
    // Build a pipeline containing a single on-premise IP Intelligence engine.
    // `LowMemory` keeps the (potentially very large) data file on disk rather
    // than loading it entirely into memory, which suits a getting-started run.
    // Every property the example prints is requested, because the engine narrows
    // itself to the requested set; an Enterprise file then populates the location
    // and network properties as well as the autonomous-system ones.
    let pipeline: Arc<Pipeline> = IpIntelligencePipelineBuilder::on_premise(&options.data_file)
        .performance_profile(PerformanceProfile::LowMemory)
        .properties(DISPLAYED_PROPERTIES.iter().copied())
        // Automatic updates and the file-system watcher are left off so the
        // example is self-contained and does not reach out to the network.
        .auto_update(false)
        .file_system_watcher(false)
        .build()
        .context("failed to build the on-premise IP Intelligence pipeline")?;

    writeln!(
        out,
        "Looking up {} IP address(es) against {}.",
        options.ips.len(),
        options.data_file.display()
    )?;

    for ip in &options.ips {
        analyse_ip(&pipeline, ip, out)?;
    }

    Ok(())
}
// [example]

/// Look one IP address up and print its properties.
fn analyse_ip(pipeline: &Arc<Pipeline>, ip: &str, out: &mut dyn Write) -> anyhow::Result<()> {
    writeln!(out)?;
    writeln!(out, "Input values:")?;
    writeln!(out, "\tquery.client-ip: {ip}")?;

    // FlowData carries the evidence in and the results out. The IP address is
    // supplied as a client-IP evidence value, which the engine's evidence-key
    // filter accepts.
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("query.client-ip", ip).build());
    data.process()
        .with_context(|| format!("processing IP {ip} failed"))?;

    // Read the IP Intelligence result back through the shared data key. Each
    // accessor returns a single plain value (or a no-value explanation).
    let ip_data = data
        .get(IP_DATA_KEY)
        .context("the pipeline produced no IP Intelligence data")?;

    writeln!(out, "Results:")?;
    // Every value below is read through a strongly-typed accessor on the shared
    // IpIntelligenceData trait: a string, float, integer or bool for the plain
    // properties, and a weighted list for the country-code distributions.
    //
    // The autonomous-system properties carried by the ASN data file.
    output_string("Asn", &ip_data.asn(), out)?;
    output_string("AsnName", &ip_data.asn_name(), out)?;
    // The registered-range network properties.
    output_string("RegisteredName", &ip_data.registered_name(), out)?;
    output_string("RegisteredOwner", &ip_data.registered_owner(), out)?;
    output_string("RegisteredCountry", &ip_data.registered_country(), out)?;
    output_string("IpRangeStart", &ip_data.ip_range_start(), out)?;
    output_string("IpRangeEnd", &ip_data.ip_range_end(), out)?;
    // The location properties.
    output_string("Country", &ip_data.country(), out)?;
    output_string("CountryCode", &ip_data.country_code(), out)?;
    output_string("State", &ip_data.state(), out)?;
    output_string("Town", &ip_data.town(), out)?;
    output_float("Latitude", &ip_data.latitude(), out)?;
    output_float("Longitude", &ip_data.longitude(), out)?;
    output_integer("TimeZoneOffset", &ip_data.time_zone_offset(), out)?;
    output_integer("AccuracyRadiusMin", &ip_data.accuracy_radius_min(), out)?;
    // The network flags, read as typed booleans.
    output_bool("IsHosted", &ip_data.is_hosted(), out)?;
    output_bool("IsVPN", &ip_data.is_vpn(), out)?;
    // The weighted country-code distributions: a single IP can overlap several
    // countries, so each resolves to a list of candidates with a 0.0..=1.0
    // weighting, ordered most probable first.
    output_weighted(
        "CountryCodesGeographical",
        &ip_data.country_codes_geographical(),
        out,
    )?;
    output_weighted(
        "CountryCodesPopulation",
        &ip_data.country_codes_population(),
        out,
    )?;
    output_weighted("Mcc", &ip_data.mcc(), out)?;

    Ok(())
}

/// Print a plain string property, or the no-value message when it is absent.
fn output_string(
    name: &str,
    property: &AspectPropertyValue<String>,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    match property.value() {
        Ok(value) => writeln!(out, "\t{name}: {value}")?,
        Err(no_value) => writeln!(out, "\t{name}: {no_value}")?,
    }
    Ok(())
}

/// Print a plain floating-point property (latitude, longitude).
fn output_float(
    name: &str,
    property: &AspectPropertyValue<f32>,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    match property.value() {
        Ok(value) => writeln!(out, "\t{name}: {value:.4}")?,
        Err(no_value) => writeln!(out, "\t{name}: {no_value}")?,
    }
    Ok(())
}

/// Print a plain integer property (time-zone offset, accuracy radius).
fn output_integer(
    name: &str,
    property: &AspectPropertyValue<i32>,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    match property.value() {
        Ok(value) => writeln!(out, "\t{name}: {value}")?,
        Err(no_value) => writeln!(out, "\t{name}: {no_value}")?,
    }
    Ok(())
}

/// Print a plain boolean property (the network flags), as `true`/`false`.
fn output_bool(
    name: &str,
    property: &AspectPropertyValue<bool>,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    match property.value() {
        Ok(value) => writeln!(out, "\t{name}: {value}")?,
        Err(no_value) => writeln!(out, "\t{name}: {no_value}")?,
    }
    Ok(())
}

/// Print a weighted property (the country-code distributions, `Mcc`) as a
/// comma-separated list of `value (weight)` candidates ordered most probable
/// first, or the no-value message when it is absent.
fn output_weighted(
    name: &str,
    property: &AspectPropertyValue<Vec<WeightedValue<String>>>,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    match property.value() {
        Ok(list) => {
            let rendered = list
                .iter()
                .map(|candidate| format!("{} ({:.2})", candidate.value, candidate.weighting()))
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(out, "\t{name}: {rendered}")?;
        }
        Err(no_value) => writeln!(out, "\t{name}: {no_value}")?,
    }
    Ok(())
}

/// Read the data file, then run the example, writing to standard output.
///
/// An optional command-line argument overrides the data-file path the shared
/// resolver would otherwise pick.
fn main() -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let options = match std::env::args().nth(1) {
        Some(path) => ExampleOptions {
            data_file: std::path::PathBuf::from(path),
            ips: vec!["1.1.1.1".to_owned(), "2606:4700:4700::1111".to_owned()],
        },
        None => match ExampleOptions::from_env() {
            Some(options) => options,
            None => {
                writeln!(
                    out,
                    "No IP Intelligence data file could be located. Set {} to an \
                     .ipi file, or check out the cxx submodule so the \
                     ASN data file is present.",
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

    /// The example runs end to end against the real ASN data file and prints
    /// at least one autonomous-system value. It skips cleanly when no
    /// data file is present, so a plain `cargo test` stays green offline.
    ///
    /// The test pins the ASN tier so it always exercises the small, current,
    /// always-loadable ASN file, independent of whether the production
    /// Enterprise share happens to be mounted on the build machine.
    #[test]
    fn getting_started_runs_against_the_asn_file() {
        let Some(options) = ExampleOptions::for_tier(examples_shared::IpiTier::Asn) else {
            eprintln!("no usable IP Intelligence data file; skipping getting-started run");
            return;
        };

        let mut buffer = Vec::new();
        run(&options, &mut buffer).expect("the getting-started example should run");

        let printed = String::from_utf8(buffer).expect("output should be valid UTF-8");
        // The Cloudflare IPs resolve to autonomous system AS13335 in the ASN
        // file, so the rendered output must mention it.
        assert!(
            printed.contains("AS13335"),
            "expected the Cloudflare IPs to resolve to AS13335, output was:\n{printed}"
        );
        assert!(printed.contains("Asn:"));
    }
}

/*
 * @example ipi-onprem-getting-started.rs
 *
 * The on-premise IP Intelligence getting-started example.
 *
 * It shows how to:
 *
 * 1. Build a Pipeline containing a single on-premise IP Intelligence engine
 *    from a local `.ipi` data file, using the
 *    `IpIntelligencePipelineBuilder::on_premise` convenience builder.
 * 2. Choose a performance profile (this example uses `LowMemory`, which keeps
 *    the data file on disk instead of loading it wholly into memory).
 * 3. Pass an IP address into the pipeline as `query.client-ip` evidence.
 * 4. Read the results back through the shared `IP_DATA_KEY`, accessing each
 *    property as a single plain value. Most IP Intelligence properties resolve
 *    to one typed value (a string, a float or an integer); the example prints
 *    that value directly, or a "no value" explanation when the data file does
 *    not carry it.
 *
 * The example looks up a public IPv4 (1.1.1.1) and a public IPv6
 * (2606:4700:4700::1111). Against the bundled ASN data file both resolve to the
 * Cloudflare autonomous system, AS13335.
 *
 * # Data file
 *
 * The data file is resolved by `examples_shared::ipi_data_path`, which prefers
 * the `51DEGREES_IPI_PATH` environment variable and otherwise locates the ASN
 * file checked into the `cxx` submodule. An Enterprise `.ipi`
 * file (with the full property set) is available from 51Degrees: see <https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-getting-started.rs&utm_term=ipi-onprem-getting-started>.
 *
 * # Usage sharing
 *
 * This is a console example, so usage sharing is *not* enabled. A production
 * deployment is encouraged to enable usage sharing to help 51Degrees improve
 * the accuracy and coverage of the data.
 */
