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

//! @page ipi-onprem-metadata-example Metadata (IP Intelligence, On-premise)
//!
//! On-premise IP Intelligence metadata console example.
//!
//! Inspects an on-premise engine and prints its available properties (with
//! value types and any descriptions/tiers), the evidence keys its data file
//! defines and a summary of the data file in use. The descriptive block the
//! documentation tooling renders lives at the bottom of the file.
//!
//! @snippet ipi-onprem-metadata.rs example

use std::io::Write;

use anyhow::Context;
use fiftyone_ip_intelligence::{IpIntelligenceOnPremiseEngine, PerformanceProfile};
use fiftyone_pipeline_core::FlowElement;
use fiftyone_pipeline_engines::AspectEngine;

/// The options that drive a run of this example.
pub struct ExampleOptions {
    /// The full path to the `.ipi` data file to open.
    pub data_file: std::path::PathBuf,
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
        })
    }
}

// [example]
/// Run the metadata example, writing all output to `out`.
///
/// Usage sharing is not enabled, as this is a console example. A production
/// deployment should enable it.
pub fn run(options: &ExampleOptions, out: &mut dyn Write) -> anyhow::Result<()> {
    // Build the concrete on-premise engine so the engine-level metadata traits
    // (AspectEngine, OnPremiseAspectEngine) are reachable. The facade also
    // re-exports this engine for exactly this kind of introspection.
    let engine = IpIntelligenceOnPremiseEngine::builder(&options.data_file)
        .performance_profile(PerformanceProfile::LowMemory)
        .auto_update(false)
        .file_system_watcher(false)
        .build()
        .context("failed to build the on-premise IP Intelligence engine")?;

    output_data_file_details(&engine, out)?;
    output_property_details(&engine, out)?;
    output_evidence_key_details(&engine, out)?;
    output_warnings(&engine, out)?;

    Ok(())
}
// [example]

/// Print the tier and publish date of the data file in use.
fn output_data_file_details(
    engine: &IpIntelligenceOnPremiseEngine,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    writeln!(out, "Data file:")?;
    writeln!(out, "\t{}", examples_shared::data_file_info(engine))?;
    writeln!(out, "\tData source tier: {}", engine.data_source_tier())?;
    Ok(())
}

/// Print every property the engine populates, with its value type and (where
/// known) description and the data tiers in which it carries a value.
fn output_property_details(
    engine: &IpIntelligenceOnPremiseEngine,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    // The data file's full property catalogue, read from the data set, so the
    // listing reflects the tier (an Enterprise file carries the location and
    // behavioural properties an ASN file does not).
    let available = engine.available_property_names();
    writeln!(out)?;
    writeln!(out, "Available properties ({}):", available.len())?;
    // Core metadata carries the value type; the aspect metadata carries the
    // description and the data tiers. Both are paired to a catalogue name by
    // name. Every IP Intelligence property is a weighted key-value list, so the
    // type defaults to that of the engine's typed properties for any catalogue
    // entry the curated model does not describe.
    let core = engine.properties();
    let default_type = core.first().map(|property| &property.value_type);
    for name in &available {
        let value_type = core
            .iter()
            .find(|property| property.name.eq_ignore_ascii_case(name))
            .map(|property| &property.value_type)
            .or(default_type);
        let aspect = engine
            .aspect_properties()
            .iter()
            .find(|a| a.name().eq_ignore_ascii_case(name));

        let description = aspect
            .map(|a| a.description())
            .filter(|d| !d.is_empty())
            .unwrap_or("(no description)");
        let tiers = aspect
            .map(|a| a.data_tiers_where_present())
            .filter(|t| !t.is_empty())
            .map(|t| t.join(", "))
            .unwrap_or_else(|| "(unspecified)".to_owned());

        match value_type {
            Some(value_type) => writeln!(
                out,
                "\t{name} [{value_type:?}] in '{tiers}' tier(s): {description}"
            )?,
            None => writeln!(out, "\t{name} in '{tiers}' tier(s): {description}")?,
        }
    }
    Ok(())
}

/// Print the evidence keys the engine accepts, which its data file defines, in
/// the order the engine tries them.
fn output_evidence_key_details(
    engine: &IpIntelligenceOnPremiseEngine,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    writeln!(out)?;
    writeln!(out, "Accepted evidence keys, in the order they are tried:")?;
    for key in engine.evidence_keys() {
        writeln!(out, "\t{key}")?;
    }
    Ok(())
}

/// Print any standard data-file warnings (an old or Lite-tier file).
fn output_warnings(
    engine: &IpIntelligenceOnPremiseEngine,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let warnings = examples_shared::check_data_file(engine);
    if !warnings.is_empty() {
        writeln!(out)?;
        for warning in warnings {
            writeln!(out, "WARNING: {warning}")?;
        }
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
    fn metadata_lists_properties_and_evidence_keys() {
        let Some(options) = ExampleOptions::for_tier(examples_shared::IpiTier::Asn) else {
            eprintln!("no usable IP Intelligence data file; skipping metadata run");
            return;
        };

        let mut buffer = Vec::new();
        run(&options, &mut buffer).expect("the metadata example should run");

        let printed = String::from_utf8(buffer).expect("output should be valid UTF-8");
        // The headings and at least one property the ASN tier carries must
        // appear. The ASN data file's catalogue is the autonomous-system
        // properties, so assert on one of those rather than a location property a
        // richer tier would add.
        assert!(printed.contains("Available properties"));
        assert!(printed.contains("Accepted evidence keys"));
        assert!(printed.contains("Asn"));
        // The data file's keys are listed, every query key before any server
        // key, and nothing else is.
        let query = printed.find("\tquery.client-ip-51d\n");
        let server = printed.find("\tserver.client-ip\n");
        assert!(query.is_some() && server.is_some(), "{printed}");
        assert!(query < server, "query keys are tried first: {printed}");
        assert!(!printed.contains("header.user-agent"));
    }
}

/*
 * @example ipi-onprem-metadata.rs
 *
 * The on-premise IP Intelligence metadata example, adapted to the metadata the
 * on-premise engine exposes.
 *
 * It shows how to inspect an on-premise IP Intelligence engine for:
 *
 * 1. The data file in use, reported through the `AspectEngine` and
 *    `OnPremiseAspectEngine` traits (its data-source tier and publish date).
 * 2. The properties the engine populates. Each is listed with its value type
 *    (the weighted IP Intelligence properties are surfaced as a key-value list
 *    of value/weight records) and, where the metadata carries them, a
 *    description and the data tiers in which the property has a value.
 * 3. The evidence keys the engine accepts, which its data file defines, in
 *    the order the engine tries them. Each header the data file names is
 *    accepted as a query value and as a server value, and every query value
 *    is tried before any server value.
 *
 * It also prints the standard data-file warnings (for an out-of-date or
 * Lite-tier file) via the shared `check_data_file` helper.
 *
 * # Data file
 *
 * Resolved by `examples_shared::ipi_data_path`. The bundled ASN file carries
 * the autonomous-system properties; an Enterprise file carries the full network
 * and location property set. Contact 51Degrees for an Enterprise file:
 * <https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-metadata.rs&utm_term=ipi-onprem-metadata>.
 *
 * # Usage sharing
 *
 * Not enabled, because this is a console example. Production deployments are
 * encouraged to enable usage sharing.
 */
