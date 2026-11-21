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
//! prints the network and location properties. With no file named it reads
//! both free files, and says which properties need the Enterprise data file.
//! The descriptive block the documentation tooling renders lives at the bottom
//! of the file.
//!
//! @snippet ipi-onprem-getting-started.rs example

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use fiftyone_ip_intelligence::{
    AspectPropertyValue, IpIntelligenceData, IpIntelligencePipelineBuilder, PerformanceProfile,
    WeightedValue, IP_DATA_KEY,
};
use fiftyone_pipeline_core::{Evidence, Pipeline};
use ip_intelligence_examples::ENTERPRISE_PROPERTIES;

/// Where to ask about a copy of the Enterprise data file.
const CONTACT_US_URL: &str = "https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-getting-started.rs&utm_term=enterprise-data-file";

/// The links the example's output ends with.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("IP Intelligence", "https://51degrees.com/ip-intelligence?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-getting-started.rs&utm_term=find-out-more"),
    ("Pricing", "https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-getting-started.rs&utm_term=find-out-more"),
    ("Documentation", "https://51degrees.com/documentation/index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-getting-started.rs&utm_term=find-out-more"),
];

/// The addresses the example looks up, a public Cloudflare IPv4 and IPv6.
/// Both resolve to a real autonomous system in the ASN data file, so the
/// example always has something to print.
const IPS: &[&str] = &["1.1.1.1", "2606:4700:4700::1111"];

/// The options that drive a run of this example.
///
/// Keeping the inputs in one struct lets `main` (reading the environment and
/// the command line) and the test (supplying fixed values) share the same
/// [`run`] entry point.
pub struct ExampleOptions {
    /// The full paths to the `.ipi` data files to open, each looked up in
    /// turn.
    pub data_files: Vec<PathBuf>,
    /// The IP addresses to look up, as client-IP evidence values.
    pub ips: Vec<String>,
}

impl ExampleOptions {
    /// Build options that look [`IPS`] up in each of `data_files`.
    pub fn with_files(data_files: Vec<PathBuf>) -> Self {
        ExampleOptions {
            data_files,
            ips: IPS.iter().map(|ip| (*ip).to_owned()).collect(),
        }
    }

    /// Build the default options through the shared resolver. A file named by
    /// `_51DEGREES_IPI_PATH`, or the Enterprise file when its share is
    /// reachable, is read alone. Otherwise both free files from the data
    /// repository are read, each downloaded when it is missing, being the ASN
    /// file for Asn and AsnName and the Lite file for the registered
    /// properties.
    ///
    /// Returns [`None`] when no usable data file can be located, so the caller
    /// can report it rather than panicking.
    pub fn from_env() -> Option<Self> {
        let data_files = match examples_shared::ipi_data_path(examples_shared::IpiTier::Enterprise)
        {
            Some(file) => vec![file],
            None => [
                examples_shared::IpiTier::Asn,
                examples_shared::IpiTier::Lite,
            ]
            .into_iter()
            .filter_map(examples_shared::ipi_data_path)
            .collect(),
        };
        (!data_files.is_empty()).then(|| Self::with_files(data_files))
    }

    /// Build options pinned to a specific data-file tier. `main` reads the
    /// Enterprise file or both free files, while the test pins to the ASN tier
    /// so it runs deterministically against the small, current,
    /// always-loadable ASN file rather than depending on the production
    /// Enterprise share being mounted.
    pub fn for_tier(tier: examples_shared::IpiTier) -> Option<Self> {
        let data_file = examples_shared::ipi_data_path(tier)?;
        Some(Self::with_files(vec![data_file]))
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
/// Builds an on-premise IP Intelligence pipeline for each data file, looks up
/// each IP and prints the results. The pipeline is built with usage sharing
/// left off, because this is a console example. A production deployment that
/// wants to help improve the data should enable it (see the engine builder's
/// `share_usage` equivalent on the cloud/web side). The output ends with the
/// Enterprise properties no file carried, where to ask about a copy of the
/// Enterprise data file, and the "Find out more" links.
pub fn run(options: &ExampleOptions, out: &mut dyn Write) -> anyhow::Result<()> {
    // The properties any of the files carries, to name the Enterprise ones
    // none of them did.
    let mut carried: Vec<String> = Vec::new();
    for data_file in &options.data_files {
        let available = ip_intelligence_examples::data_file_properties(data_file)?;
        look_up(data_file, &options.ips, &available, out)?;
        ip_intelligence_examples::write_data_file_warnings(data_file, out)?;
        carried.extend(available);
    }

    // The Enterprise properties this example asks for.
    let asked: Vec<&str> = ENTERPRISE_PROPERTIES
        .iter()
        .copied()
        .filter(|name| DISPLAYED_PROPERTIES.contains(name))
        .collect();
    let missing = examples_shared::missing_properties(&asked, &carried);
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

/// Build a pipeline over `data_file`, whose properties are `available`, and
/// look each of `ips` up in it.
fn look_up(
    data_file: &std::path::Path,
    ips: &[String],
    available: &[String],
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    // Build a pipeline containing a single on-premise IP Intelligence engine.
    // `LowMemory` keeps the (potentially very large) data file on disk rather
    // than loading it entirely into memory, which suits a getting-started run.
    // Every property the example prints is requested, because the engine narrows
    // itself to the requested set; an Enterprise file then populates the location
    // and network properties as well as the autonomous-system ones.
    let pipeline: Arc<Pipeline> = IpIntelligencePipelineBuilder::on_premise(data_file)
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
        ips.len(),
        data_file.display()
    )?;

    for ip in ips {
        analyse_ip(&pipeline, ip, available, out)?;
    }
    writeln!(out)?;

    Ok(())
}
// [example]

/// Look one IP address up and print its properties. `available` holds the
/// properties the data file carries.
fn analyse_ip(
    pipeline: &Arc<Pipeline>,
    ip: &str,
    available: &[String],
    out: &mut dyn Write,
) -> anyhow::Result<()> {
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
    output_string("Asn", &ip_data.asn(), available, out)?;
    output_string("AsnName", &ip_data.asn_name(), available, out)?;
    // The registered-range network properties.
    output_string("RegisteredName", &ip_data.registered_name(), available, out)?;
    output_string(
        "RegisteredOwner",
        &ip_data.registered_owner(),
        available,
        out,
    )?;
    output_string(
        "RegisteredCountry",
        &ip_data.registered_country(),
        available,
        out,
    )?;
    output_string("IpRangeStart", &ip_data.ip_range_start(), available, out)?;
    output_string("IpRangeEnd", &ip_data.ip_range_end(), available, out)?;
    // The location properties.
    output_string("Country", &ip_data.country(), available, out)?;
    output_string("CountryCode", &ip_data.country_code(), available, out)?;
    output_string("State", &ip_data.state(), available, out)?;
    output_string("Town", &ip_data.town(), available, out)?;
    output_float("Latitude", &ip_data.latitude(), available, out)?;
    output_float("Longitude", &ip_data.longitude(), available, out)?;
    output_integer(
        "TimeZoneOffset",
        &ip_data.time_zone_offset(),
        available,
        out,
    )?;
    output_integer(
        "AccuracyRadiusMin",
        &ip_data.accuracy_radius_min(),
        available,
        out,
    )?;
    // The network flags, read as typed booleans.
    output_bool("IsHosted", &ip_data.is_hosted(), available, out)?;
    output_bool("IsVPN", &ip_data.is_vpn(), available, out)?;
    // The weighted country-code distributions: a single IP can overlap several
    // countries, so each resolves to a list of candidates with a 0.0..=1.0
    // weighting, ordered most probable first.
    output_weighted(
        "CountryCodesGeographical",
        &ip_data.country_codes_geographical(),
        available,
        out,
    )?;
    output_weighted(
        "CountryCodesPopulation",
        &ip_data.country_codes_population(),
        available,
        out,
    )?;
    output_weighted("Mcc", &ip_data.mcc(), available, out)?;

    Ok(())
}

/// Whether `name` is an Enterprise property the data file in use does not
/// carry, in which case its line says so in place of a value.
fn needs_enterprise(name: &str, available: &[String]) -> bool {
    ENTERPRISE_PROPERTIES
        .iter()
        .any(|property| property.eq_ignore_ascii_case(name))
        && !examples_shared::is_available(name, available)
}

/// Print a plain string property, or the no-value message when it is absent.
fn output_string(
    name: &str,
    property: &AspectPropertyValue<String>,
    available: &[String],
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    if needs_enterprise(name, available) {
        writeln!(
            out,
            "\t{name}: {}",
            examples_shared::NEEDS_ENTERPRISE_DATA_FILE
        )?;
        return Ok(());
    }
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
    available: &[String],
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    if needs_enterprise(name, available) {
        writeln!(
            out,
            "\t{name}: {}",
            examples_shared::NEEDS_ENTERPRISE_DATA_FILE
        )?;
        return Ok(());
    }
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
    available: &[String],
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    if needs_enterprise(name, available) {
        writeln!(
            out,
            "\t{name}: {}",
            examples_shared::NEEDS_ENTERPRISE_DATA_FILE
        )?;
        return Ok(());
    }
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
    available: &[String],
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    if needs_enterprise(name, available) {
        writeln!(
            out,
            "\t{name}: {}",
            examples_shared::NEEDS_ENTERPRISE_DATA_FILE
        )?;
        return Ok(());
    }
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
    available: &[String],
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    if needs_enterprise(name, available) {
        writeln!(
            out,
            "\t{name}: {}",
            examples_shared::NEEDS_ENTERPRISE_DATA_FILE
        )?;
        return Ok(());
    }
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
        Some(path) => ExampleOptions::with_files(vec![PathBuf::from(path)]),
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
        assert!(printed.contains(examples_shared::FIND_OUT_MORE_HEADING));
        // The ASN file carries none of the Enterprise properties, so the run
        // says so. _51DEGREES_IPI_PATH can name another file.
        let asn_file = examples_shared::IPI_ASN_RELATIVE_PATH.rsplit('/').next();
        if options.data_files[0].file_name().and_then(|n| n.to_str()) == asn_file {
            assert!(printed.contains("Country: needs the Enterprise data file"));
            assert!(printed.contains("utm_term=enterprise-data-file"));
        }
    }

    /// The properties this example asks for that only the Enterprise data
    /// file carries, in the order the closing message names them.
    const PAID: &[&str] = &[
        "IpRangeStart",
        "IpRangeEnd",
        "Country",
        "CountryCode",
        "State",
        "Town",
        "Latitude",
        "Longitude",
        "AccuracyRadiusMin",
        "TimeZoneOffset",
        "IsHosted",
        "IsVPN",
        "CountryCodesGeographical",
        "CountryCodesPopulation",
        "Mcc",
    ];

    /// On a free file, each property only the Enterprise data file carries
    /// says so on its line, and the closing message names exactly those.
    #[test]
    fn each_paid_property_says_it_needs_the_enterprise_data_file() {
        let Some(options) = ExampleOptions::for_tier(examples_shared::IpiTier::Asn) else {
            eprintln!("no usable IP Intelligence data file; skipping getting-started run");
            return;
        };
        // _51DEGREES_IPI_PATH can name another file, which may carry them.
        let asn_file = examples_shared::IPI_ASN_RELATIVE_PATH.rsplit('/').next();
        if options.data_files[0].file_name().and_then(|n| n.to_str()) != asn_file {
            return;
        }

        let mut buffer = Vec::new();
        run(&options, &mut buffer).expect("the getting-started example should run");
        let printed = String::from_utf8(buffer).expect("output should be valid UTF-8");

        for name in PAID {
            let line = format!(
                "\t{name}: {}\n",
                examples_shared::NEEDS_ENTERPRISE_DATA_FILE
            );
            assert!(printed.contains(&line), "{name} in:\n{printed}");
        }
        let message = examples_shared::enterprise_data_file_message(PAID, CONTACT_US_URL)
            .expect("there are paid properties to name");
        assert!(printed.contains(&message), "{message}\nnot in:\n{printed}");
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
 * (2606:4700:4700::1111). Against the free ASN data file both resolve to the
 * Cloudflare autonomous system, AS13335.
 *
 * With no file named, the example reads both free files from the data
 * repository in turn, the ASN file for `Asn` and `AsnName` and the Lite file
 * for `RegisteredName`, `RegisteredOwner` and `RegisteredCountry`. Every other
 * property it asks for, such as `Country`, `Town`, `Latitude`, `Longitude`,
 * `IsHosted` and `IsVPN`, only the Enterprise data file carries. Each data file's
 * own list of properties says which it carries, so with a free file each of
 * those prints "needs the Enterprise data file", and the run ends by naming
 * them with a link to contact 51Degrees about a copy of the Enterprise data
 * file, then a "Find out more" section.
 *
 * # Data file
 *
 * The data files are resolved by `examples_shared::ipi_data_path`, which
 * prefers the `_51DEGREES_IPI_PATH` environment variable and otherwise locates
 * the ASN and Lite files in the data repository inside the `cxx` submodule,
 * downloading each when it is missing. An Enterprise `.ipi`
 * file (with the full property set) is available from 51Degrees: see <https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-getting-started.rs&utm_term=ipi-onprem-getting-started>.
 *
 * # Usage sharing
 *
 * This is a console example, so usage sharing is *not* enabled. A production
 * deployment is encouraged to enable usage sharing to help 51Degrees improve
 * the accuracy and coverage of the data.
 */
