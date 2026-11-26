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
use fiftyone_pipeline_engines::{DataUpdateService, OnPremiseAspectEngine};

/// The 51Degrees Distributor's download address, which the data update
/// service polls. The service adds the license key, `Download=True` and
/// `Type=IPIV41`, and names no product, so for a key that holds several
/// products it asks again for the product of the data file in use.
const UPDATE_URL: &str = "https://distributor.51degrees.com/api/v2/download";

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
    /// The license key automatic updates use. With none, the run makes no
    /// update check and reloads the file already on disk.
    pub license_key: Option<String>,
    /// The address the data update service polls, [`UPDATE_URL`] unless a
    /// caller sets another.
    pub update_url: String,
    /// The product every update asks for, such as `IPIV4Enterprise`. With
    /// none, the service asks for the product of the data file.
    pub product: Option<String>,
}

impl ExampleOptions {
    /// Default options, reading the file `_51DEGREES_IPI_PATH` names
    /// when it is set and otherwise the free ASN file, which is far smaller to download than
    /// the Lite file and serves to show an update as well.
    pub fn from_env() -> Option<Self> {
        Self::for_tier(examples_shared::IpiTier::EnterpriseOrAsn)
    }

    /// Options pinned to a specific data-file tier. The test pins the ASN tier
    /// so it runs deterministically against the always-loadable ASN file.
    pub fn for_tier(tier: examples_shared::IpiTier) -> Option<Self> {
        Some(ExampleOptions {
            data_file: examples_shared::ipi_data_path(tier)?,
            license_key: None,
            update_url: UPDATE_URL.to_owned(),
            product: None,
        })
    }
}

// [example]
/// Run the update-data-file example, writing all output to `out`.
///
/// Usage sharing is not enabled, as this is a console example.
pub fn run(options: &ExampleOptions, out: &mut dyn Write) -> anyhow::Result<()> {
    // Build the concrete on-premise engine, registered with a data update
    // service. The update mechanisms live on the OnPremiseAspectEngine trait,
    // so a concrete handle (kept in an Arc) is held both to drive updates and
    // to add to the pipeline. Dropping the service stops its polling.
    let service = Arc::new(DataUpdateService::new());
    let engine = IpIntelligenceOnPremiseEngine::builder(&options.data_file)
        .performance_profile(PerformanceProfile::LowMemory)
        // Request the autonomous-system properties the ASN data file
        // carries, so the before/after lookups have a value to show, and
        // location properties only the Enterprise data file carries.
        .properties(REQUESTED_PROPERTIES.iter().copied())
        // Automatic updates poll the update URL, which needs a license key.
        .auto_update(options.license_key.is_some())
        .data_update_url(options.update_url.as_str())
        .data_update_license_keys(options.license_key.iter().cloned())
        // The product to ask for. Empty, which is no product, unless one was
        // given.
        .data_update_product(options.product.clone().unwrap_or_default())
        // The file-system watcher reloads the engine when the file on disk
        // changes. Left off here so the example does not hold a watch handle.
        .file_system_watcher(false)
        .data_update_service(Arc::clone(&service))
        .build_shared()
        .context("failed to build the on-premise IP Intelligence engine")?;

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

    // With a license key, check the update URL now. A 304 gives `NotNeeded`,
    // and `Success` means the engine reloaded from a newer file. A key that
    // does not hold the data file's product fails the check, so a failure is
    // reported, not fatal.
    if options.license_key.is_some() {
        writeln!(out)?;
        writeln!(
            out,
            "Checking {} for a newer data file ...",
            options.update_url
        )?;
        let as_engine: Arc<dyn OnPremiseAspectEngine> = engine.clone();
        match service.check_for_update(&as_engine, None) {
            Ok(status) => writeln!(out, "Update check status: {status:?}")?,
            Err(error) => writeln!(out, "Update check did not apply an update: {error}")?,
        }
    }

    // 1. Programmatic update: reload the data set from the data file on disk
    //    in place. A concurrent lookup keeps reading the data set it started
    //    with, so this is safe to call on a live engine. In a real
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
    // data file's own list of properties says which it does. This engine names
    // its properties, so it lists those names whether or not the file carries
    // them, and the list comes from an engine that names none.
    let available = ip_intelligence_examples::data_file_properties(&options.data_file)?;
    report_enterprise(&pipeline, &available, out)?;

    writeln!(out)?;
    writeln!(
        out,
        "Automatic updates download from the 51Degrees Distributor at \
         {UPDATE_URL}, asking for the product of the data file in use, or for \
         the product given as the third argument."
    )?;
    if options.license_key.is_none() {
        writeln!(
            out,
            "Automatic updates need a license key for the Enterprise data file, and \
             this run has none, so it reloads the file already on disk. Contact us \
             about the Enterprise data file and a license key at {CONTACT_US_URL}"
        )?;
    }

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

/// Read the data file from the first argument, the license key from the
/// second or `_51DEGREES_LICENSE_KEY` and the product to ask for, such as
/// `IPIV4Enterprise`, from the third, then run the example, writing to
/// standard output.
fn main() -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let mut options = match std::env::args().nth(1) {
        Some(path) => ExampleOptions {
            data_file: std::path::PathBuf::from(path),
            license_key: None,
            update_url: UPDATE_URL.to_owned(),
            product: None,
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
    options.license_key = std::env::args()
        .nth(2)
        .or_else(examples_shared::license_key_from_env);
    options.product = std::env::args().nth(3);

    run(&options, &mut out)
}

/// The stand-in for the Distributor that the library's update tests use. The
/// test here uses part of it.
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../../pipeline-engines/tests/support/distributor.rs"]
mod distributor;

#[cfg(test)]
mod tests {
    use super::*;
    use fiftyone_ip_intelligence::IpIntelligenceOnPremiseEngine;

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
        // The run names the Distributor and says it has no license key.
        assert!(printed.contains("Distributor"));
        assert!(printed.contains("Automatic updates need a license key"));
        assert!(printed.contains(examples_shared::FIND_OUT_MORE_HEADING));
        // The ASN file carries no location properties, so the run says so.
        // _51DEGREES_IPI_PATH can name another file.
        let asn_file = examples_shared::IPI_ASN_RELATIVE_PATH.rsplit('/').next();
        if options.data_file.file_name().and_then(|n| n.to_str()) == asn_file {
            assert!(printed.contains("Country after reload: needs the Enterprise data file"));
        }
    }

    /// Run the example on the ASN file with a placeholder license key against
    /// a local stand-in for the Distributor, for a key that holds the Device
    /// Detection product and the product of the data file. With
    /// `product_argument` the example is given the data file's product as its
    /// product argument. The stand-in answers 304 to a product it offers, and
    /// the run reports that no update was needed. Gives the data file's
    /// product and each request's product and status.
    fn run_against_stand_in(product_argument: bool) -> (String, Vec<(Option<String>, u16)>) {
        let options = ExampleOptions::for_tier(examples_shared::IpiTier::Asn)
            .expect("an IP Intelligence data file (set _51DEGREES_IPI_PATH)");
        let product = IpIntelligenceOnPremiseEngine::product_from_file(&options.data_file)
            .expect("the data file names its product");
        let stand_in = distributor::StandIn::new(&["V4TAC", &product], None);

        let mut buffer = Vec::new();
        run(
            &ExampleOptions {
                // The stand-in serves any key.
                license_key: Some("ABCDEFGHIJKLMNOP".to_owned()),
                update_url: UPDATE_URL
                    .replace(fiftyone_pipeline_engines::DISTRIBUTOR_URL, &stand_in.url()),
                product: product_argument.then(|| product.clone()),
                ..options
            },
            &mut buffer,
        )
        .expect("the update-data-file example should run");

        let printed = String::from_utf8(buffer).expect("output should be valid UTF-8");
        assert!(
            printed.contains("Update check status: NotNeeded"),
            "{printed}"
        );
        let asked = stand_in
            .requests()
            .into_iter()
            .map(|request| (request.product, request.status))
            .collect();
        (product, asked)
    }

    /// With no product argument the example's update URL names no product,
    /// so its first request names none and gets the 300, and its second asks
    /// for the product of the data file.
    #[test]
    fn asks_for_the_product_of_its_data_file() {
        let (product, asked) = run_against_stand_in(false);
        assert_eq!(asked, [(None, 300), (Some(product), 304)]);
    }

    /// With a product argument the first request asks for that product, and
    /// no request leaves it out.
    #[test]
    fn asks_for_the_product_argument() {
        let (product, asked) = run_against_stand_in(true);
        assert_eq!(asked, [(Some(product), 304)]);
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
 *    disk changes). Automatic updates are on when a license key is given, as
 *    the second argument or `_51DEGREES_LICENSE_KEY`, and the run then checks
 *    the Distributor once. The watcher is left off so the example does not
 *    hold a watch handle.
 * 3. Trigger a *programmatic* update by calling `refresh`, which reloads the
 *    data set from the data file on disk in place. Concurrent lookups keep
 *    reading the data set they started with, so this is safe on a live
 *    engine. The example confirms the engine still resolves the same IP
 *    after the reload.
 *
 * After the reload the example looks up `Country` and `Town`, which only the
 * Enterprise data file carries. The data file's own list of properties says
 * which it has, so with a free file each says "needs the Enterprise data
 * file". Automatic updates need a license key for the Enterprise data file,
 * so a run without one says that too, with a link to contact 51Degrees. The
 * run also prints the standard data-file warnings and ends with a "Find out
 * more" section.
 *
 * # Distributor
 *
 * Automatic updates download from the 51Degrees Distributor. The update URL
 * names no product, so for a key that holds several products the data update
 * service asks again for the product of the data file in use.
 * A product given as the third argument, such as `IPIV4Enterprise`, is asked
 * for instead, which a key with several products needs when the data file is
 * not one of them.
 *
 * # Data file
 *
 * Resolved by `examples_shared::ipi_data_path`, which gives the free ASN file
 * when no other file is set, because the properties an update reads do not
 * matter to what it shows. Contact 51Degrees for an Enterprise file: <https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-update-data-file.rs&utm_term=ipi-onprem-update-data-file>.
 *
 * # Usage sharing
 *
 * Not enabled, because this is a console example. Production deployments are
 * encouraged to enable usage sharing.
 */
