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

//! @page ipi-onprem-update-data-file-example Update Data File (IP Intelligence, On-premise)
//!
//! On-premise IP Intelligence data-file update console example.
//!
//! Demonstrates the data-file update mechanisms the on-premise engine exposes:
//! a programmatic reload from disk, the file-system-watcher and automatic-update
//! switches, and how the engine reports its data-file state. Automatic updates
//! and the location properties need the Enterprise data file, and the run says
//! so. The descriptive block the documentation tooling renders lives at the
//! bottom of the file.
//!
//! @snippet ipi-onprem-update-data-file.rs example

use std::io::Write;
use std::sync::Arc;

use anyhow::Context;
use fiftyone_ip_intelligence::{IpIntelligenceOnPremiseEngine, PerformanceProfile, IP_DATA_KEY};
use fiftyone_pipeline_core::{Evidence, Pipeline};
use fiftyone_pipeline_engines::OnPremiseAspectEngine;

/// Where to ask about a copy of the Enterprise data file.
const CONTACT_US_URL: &str = "https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-update-data-file.rs&utm_term=enterprise-data-file";

/// The links the example's output ends with.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("IP Intelligence", "https://51degrees.com/ip-intelligence?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-update-data-file.rs&utm_term=find-out-more"),
    ("Pricing", "https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-update-data-file.rs&utm_term=find-out-more"),
    ("Documentation", "https://51degrees.com/documentation/index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-update-data-file.rs&utm_term=find-out-more"),
];

/// The properties the engine is asked for. Asn and AsnName are in the free ASN
/// file and RegisteredName in the free Lite file, so either free file opens.
const REQUESTED_PROPERTIES: &[&str] = &["Asn", "AsnName", "RegisteredName", "Country", "Town"];

/// The requested properties only the Enterprise data file carries.
const ENTERPRISE_REQUESTED: &[&str] = &["Country", "Town"];

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
/// Run the update-data-file example, writing all output to `out`.
///
/// Usage sharing is not enabled, as this is a console example.
pub fn run(options: &ExampleOptions, out: &mut dyn Write) -> anyhow::Result<()> {
    // Build the concrete on-premise engine. The update mechanisms live on the
    // OnPremiseAspectEngine trait, so a concrete handle (kept in an Arc) is held
    // both to drive updates and to add to the pipeline. The file-system watcher
    // and automatic updates are switched on here only to *report* their state;
    // a real deployment would set them according to its update strategy.
    let engine = Arc::new(
        IpIntelligenceOnPremiseEngine::builder(&options.data_file)
            .performance_profile(PerformanceProfile::LowMemory)
            // Request the autonomous-system properties the ASN data file
            // carries, so the before/after lookups have a value to show, and
            // location properties only the Enterprise data file carries.
            .properties(REQUESTED_PROPERTIES.iter().copied())
            // Automatic daily updates would be enabled here in a deployment
            // that has a download URL. See the note below about the Distributor.
            .auto_update(false)
            // The file-system watcher reloads the engine when the file on disk
            // changes. Left off here so the example does not hold a watch handle.
            .file_system_watcher(false)
            .build()
            .context("failed to build the on-premise IP Intelligence engine")?,
    );

    report_data_file_state(engine.as_ref(), out)?;
    writeln!(out)?;

    // Add the engine to a pipeline so it can be exercised before and after a
    // reload. The pipeline holds a trait-object clone of the same engine.
    let element: Arc<dyn fiftyone_pipeline_core::FlowElement> = Arc::clone(&engine) as _;
    let pipeline: Arc<Pipeline> = Pipeline::builder()
        .add_element(element)
        .build()
        .context("failed to build the pipeline")?;

    writeln!(out, "Result before reload: {}", lookup_asn(&pipeline)?)?;

    // 1. Programmatic update: reopen the data file from disk and atomically swap
    //    it in. A concurrent lookup keeps using the previous data set until the
    //    swap completes, so this is safe to call on a live engine. In a real
    //    deployment this would run after a newer file had been downloaded into
    //    place.
    writeln!(out)?;
    writeln!(out, "Triggering a programmatic reload from disk ...")?;
    engine
        .refresh(None)
        .context("the programmatic data-file reload failed")?;
    writeln!(out, "Reload complete.")?;

    // 2. The engine still resolves after the swap.
    writeln!(out, "Result after reload:  {}", lookup_asn(&pipeline)?)?;

    // The location properties, which the engine's data file may not carry. The
    // data file's own list of properties says which it does.
    let available = engine.available_property_names();
    report_enterprise(&pipeline, &available, out)?;

    writeln!(out)?;
    writeln!(
        out,
        "Note: automatic daily updates over the network rely on the 51Degrees \
         Distributor service, which is not yet available for IP Intelligence \
         Enterprise data files. Until it is, refresh the data file by replacing \
         it on disk (optionally with the file-system watcher enabled) or by \
         calling refresh() programmatically as shown above."
    )?;
    writeln!(
        out,
        "Automatic updates need a licence key for the Enterprise data file, and \
         this run has none, so it reloads the file already on disk. Contact us \
         about the Enterprise data file and a licence key at {CONTACT_US_URL}"
    )?;

    // The tier and publish date are in the data-file state above, so only the
    // warnings are added here.
    writeln!(out)?;
    for warning in examples_shared::check_data_file(engine.as_ref()) {
        writeln!(out, "WARNING: {warning}")?;
    }
    let missing = examples_shared::missing_properties(ENTERPRISE_REQUESTED, &available);
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

/// Report what the engine knows about its data file: path, publish date and any
/// configured remote update URL.
fn report_data_file_state(
    engine: &IpIntelligenceOnPremiseEngine,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    writeln!(out, "Data-file state:")?;
    writeln!(out, "\t{}", examples_shared::data_file_info(engine))?;

    match engine.data_files().first() {
        Some(file) => {
            let path = file
                .data_file_path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "(unknown)".to_owned());
            writeln!(out, "\tPath: {path}")?;
        }
        None => writeln!(out, "\tPath: (engine has no data file)")?,
    }

    let published = engine
        .data_file_published()
        .map(|when| when.to_string())
        .unwrap_or_else(|| "unknown".to_owned());
    writeln!(out, "\tPublished: {published}")?;

    let update_url = engine
        .data_update_url(None)
        .unwrap_or_else(|| "(none configured)".to_owned());
    writeln!(out, "\tUpdate URL: {update_url}")?;
    Ok(())
}

/// Look the Cloudflare IPv4 up and render its autonomous system, used to show
/// the engine works before and after a reload.
fn lookup_asn(pipeline: &Arc<Pipeline>) -> anyhow::Result<String> {
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("query.client-ip", "1.1.1.1")
            .build(),
    );
    data.process().context("processing failed")?;
    let ip_data = data
        .get(IP_DATA_KEY)
        .context("the pipeline produced no IP Intelligence data")?;
    let asn = ip_data.string("Asn");
    Ok(match asn.value() {
        Ok(value) => value.clone(),
        Err(_) => "(no value)".to_owned(),
    })
}

/// Look the Cloudflare IPv4 up and print each location property, or that it
/// needs the Enterprise data file when the data file does not carry it.
fn report_enterprise(
    pipeline: &Arc<Pipeline>,
    available: &[String],
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("query.client-ip", "1.1.1.1")
            .build(),
    );
    data.process().context("processing failed")?;
    let ip_data = data
        .get(IP_DATA_KEY)
        .context("the pipeline produced no IP Intelligence data")?;
    for name in ENTERPRISE_REQUESTED {
        let value = if examples_shared::is_available(name, available) {
            match ip_data.string(name).value() {
                Ok(value) => value.clone(),
                Err(_) => "(no value)".to_owned(),
            }
        } else {
            examples_shared::NEEDS_ENTERPRISE_DATA_FILE.to_owned()
        };
        writeln!(out, "{name} after reload: {value}")?;
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
    fn update_data_file_reloads_and_keeps_working() {
        let Some(options) = ExampleOptions::for_tier(examples_shared::IpiTier::Asn) else {
            eprintln!("no usable IP Intelligence data file; skipping update run");
            return;
        };

        let mut buffer = Vec::new();
        run(&options, &mut buffer).expect("the update-data-file example should run");

        let printed = String::from_utf8(buffer).expect("output should be valid UTF-8");
        assert!(printed.contains("Triggering a programmatic reload"));
        assert!(printed.contains("Reload complete."));
        // The engine resolves the Cloudflare IP both before and after the reload.
        assert!(printed.contains("Result before reload: AS13335"));
        assert!(printed.contains("Result after reload:  AS13335"));
        // The Distributor caveat must be surfaced.
        assert!(printed.contains("Distributor"));
        assert!(printed.contains("Automatic updates need a licence key"));
        assert!(printed.contains(examples_shared::FIND_OUT_MORE_HEADING));
        // The ASN file carries no location properties, so the run says so.
        // 51DEGREES_IPI_PATH can name another file.
        let asn_file = examples_shared::IPI_ASN_RELATIVE_PATH.rsplit('/').next();
        if options.data_file.file_name().and_then(|n| n.to_str()) == asn_file {
            assert!(printed.contains("Country after reload: needs the Enterprise data file"));
        }
    }
}

/*
 * @example ipi-onprem-update-data-file.rs
 *
 * The on-premise IP Intelligence data-file update example, adapted to the update
 * mechanisms the on-premise engine exposes.
 *
 * It shows how to:
 *
 * 1. Build an on-premise engine and read its data-file state through the
 *    `OnPremiseAspectEngine` trait: the file path, its publish date and any
 *    configured remote update URL.
 * 2. Configure the two automatic update switches, `auto_update` (poll a remote
 *    URL for a newer file) and `file_system_watcher` (reload when the file on
 *    disk changes). They are reported here rather than left running so the
 *    example is self-contained.
 * 3. Trigger a *programmatic* update by calling `refresh`, which reopens the
 *    data file from disk and swaps it in atomically. Concurrent lookups keep
 *    using the previous data set until the swap completes, so this is safe on a
 *    live engine. The example confirms the engine still resolves the same IP
 *    after the reload.
 *
 * After the reload the example looks up `Country` and `Town`, which only the
 * Enterprise data file carries. The data file's own list of properties says
 * which it has, so with a free file each says "needs the Enterprise data
 * file". Automatic updates need a licence key for the Enterprise data file,
 * so a run without one says that too, with a link to contact 51Degrees. The
 * run also prints the standard data-file warnings and ends with a "Find out
 * more" section.
 *
 * # Distributor caveat
 *
 * Automatic daily updates over the network rely on the 51Degrees Distributor
 * service, which is not yet available for IP Intelligence Enterprise data
 * files. Until it is, update the data file by replacing it on disk (optionally
 * with the file-system watcher enabled) or by calling `refresh` as shown.
 *
 * # Data file
 *
 * Resolved by `examples_shared::ipi_data_path`. Contact 51Degrees for an
 * Enterprise file: <https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-update-data-file.rs&utm_term=ipi-onprem-update-data-file>.
 *
 * # Usage sharing
 *
 * Not enabled, because this is a console example. Production deployments are
 * encouraged to enable usage sharing.
 */
