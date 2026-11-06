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
//! Inspects an on-premise engine and prints a summary of the data file in
//! use, each component the data file defines with its properties, each
//! property's category, value type, description, data tiers and first few
//! values, which Enterprise properties the data file lacks, and the evidence
//! keys its data file defines. The descriptive block the documentation
//! tooling renders lives at the bottom of the file.
//!
//! @snippet ipi-onprem-metadata.rs example

use std::io::Write;

use anyhow::Context;
use fiftyone_ip_intelligence::{IpIntelligenceOnPremiseEngine, PerformanceProfile};
use fiftyone_pipeline_core::PropertyValueType;
use fiftyone_pipeline_engines::AspectEngine;
use fiftyone_pipeline_engines_fiftyone::FiftyOneAspectPropertyMetaData;
use ip_intelligence_examples::ENTERPRISE_PROPERTIES;

/// Where to ask about a copy of the Enterprise data file.
const CONTACT_US_URL: &str = "https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-metadata.rs&utm_term=enterprise-data-file";

/// The links the example's output ends with.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("IP Intelligence", "https://51degrees.com/ip-intelligence?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-metadata.rs&utm_term=find-out-more"),
    ("Pricing", "https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-metadata.rs&utm_term=find-out-more"),
    ("Documentation", "https://51degrees.com/documentation/index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-metadata.rs&utm_term=find-out-more"),
];

/// The options that drive a run of this example.
pub struct ExampleOptions {
    /// The full path to the `.ipi` data file to open.
    pub data_file: std::path::PathBuf,
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
    output_components(&engine, out)?;
    let missing = output_enterprise_properties(&engine, out)?;
    output_evidence_key_details(&engine, out)?;
    output_warnings(&engine, out)?;

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

/// The number of a property's values the example prints.
const SHOWN_VALUES: usize = 20;

/// The number of characters of a value the example prints, because an area's
/// value is the whole boundary of the area as text.
const SHOWN_VALUE_CHARACTERS: usize = 60;

/// Print each component with its properties, then any property the engine
/// lists in no component.
///
/// The data file groups its properties into components, such as the location
/// and the network. The engine adds `Ip` and `IpV6`, which belong to no
/// component it lists.
fn output_components(
    engine: &IpIntelligenceOnPremiseEngine,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let components = engine.components();
    writeln!(out)?;
    writeln!(out, "Components and their properties:")?;
    for component in components.iter() {
        writeln!(
            out,
            "Component - {} ({} properties)",
            component.name,
            component.properties().len()
        )?;
        for property in component.properties() {
            output_property(engine, property, out)?;
        }
    }

    let properties = engine.fiftyone_properties();
    let others: Vec<&FiftyOneAspectPropertyMetaData> = properties
        .iter()
        .filter(|property| {
            !components.iter().any(|component| {
                component
                    .properties()
                    .iter()
                    .any(|listed| listed.name() == property.name())
            })
        })
        .collect();
    if !others.is_empty() {
        writeln!(out, "Properties in no listed component:")?;
        for property in others {
            output_property(engine, property, out)?;
        }
    }
    Ok(())
}

/// Print a property's category, value type, description and data tiers, and
/// the first few of its values with how many there are.
fn output_property(
    engine: &IpIntelligenceOnPremiseEngine,
    property: &FiftyOneAspectPropertyMetaData,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    writeln!(
        out,
        "\tProperty - {} [Category: {}] ({}) - {}",
        property.name(),
        property.core().category,
        value_type_name(property.core().value_type),
        property.aspect().description()
    )?;
    // A property listed only under higher tiers than the data file's needs a
    // better data file rather than a change of configuration.
    let tiers = property.aspect().data_tiers_where_present();
    let tiers = if tiers.is_empty() {
        "(none)".to_owned()
    } else {
        tiers.join(", ")
    };
    writeln!(out, "\t\tData tiers: {tiers}")?;

    // A property can have millions of values, so only those shown are read,
    // and the rest are counted without reading them.
    let count = engine
        .property_value_count(property.name())
        .unwrap_or_default();
    let values = match engine.property_value_range(property.name(), 0..SHOWN_VALUES) {
        Ok(values) => values.unwrap_or_default(),
        Err(error) => {
            writeln!(out, "\t\tValues could not be read: {error}")?;
            return Ok(());
        }
    };
    let shown: Vec<String> = values
        .iter()
        .map(|value| {
            let name = shorten(&value.name);
            if value.description.is_empty() {
                name
            } else {
                format!("{name} ({})", value.description)
            }
        })
        .collect();
    write!(out, "\t\tPossible values ({count}): {}", shown.join(", "))?;
    if count > SHOWN_VALUES {
        write!(out, " + {} more ...", count - SHOWN_VALUES)?;
    }
    writeln!(out)?;
    Ok(())
}

/// The first line of `value`, cut to [`SHOWN_VALUE_CHARACTERS`] characters,
/// with " ..." added when anything was left out.
fn shorten(value: &str) -> String {
    let line = value.lines().next().unwrap_or_default();
    let mut shown: String = line.chars().take(SHOWN_VALUE_CHARACTERS).collect();
    if shown.len() < value.len() {
        shown.push_str(" ...");
    }
    shown
}

/// A short display name for a property value type.
fn value_type_name(value_type: PropertyValueType) -> &'static str {
    match value_type {
        PropertyValueType::String => "String",
        PropertyValueType::Bool => "Boolean",
        PropertyValueType::Integer => "Integer",
        PropertyValueType::Double => "Double",
        PropertyValueType::Float => "Float",
        PropertyValueType::StringList => "String[]",
        PropertyValueType::JavaScript => "JavaScript",
        PropertyValueType::KeyValueList => "KeyValue[]",
        // PropertyValueType is non-exhaustive, so a later variant is named
        // generically rather than failing to compile.
        _ => "Other",
    }
}

/// Print whether the data file carries each property only the Enterprise data
/// file has, and return the ones it does not.
fn output_enterprise_properties(
    engine: &IpIntelligenceOnPremiseEngine,
    out: &mut dyn Write,
) -> anyhow::Result<Vec<&'static str>> {
    let available = engine.available_property_names();
    writeln!(out)?;
    writeln!(out, "Enterprise properties:")?;
    for name in ENTERPRISE_PROPERTIES {
        let state = if examples_shared::is_available(name, &available) {
            "in this data file"
        } else {
            examples_shared::NEEDS_ENTERPRISE_DATA_FILE
        };
        writeln!(out, "\t{name}: {state}")?;
    }
    Ok(examples_shared::missing_properties(
        ENTERPRISE_PROPERTIES,
        &available,
    ))
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
    fn metadata_lists_properties_and_evidence_keys() {
        let Some(options) = ExampleOptions::for_tier(examples_shared::IpiTier::Asn) else {
            eprintln!("no usable IP Intelligence data file; skipping metadata run");
            return;
        };

        let mut buffer = Vec::new();
        run(&options, &mut buffer).expect("the metadata example should run");

        let printed = String::from_utf8(buffer).expect("output should be valid UTF-8");
        // The headings appear, and the two properties the engine adds are
        // listed after the components, with their one value.
        assert!(printed.contains("Components and their properties"));
        assert!(printed.contains("Accepted evidence keys"));
        let others = printed
            .find("Properties in no listed component:")
            .expect("Ip and IpV6 are listed");
        let first_component = printed.find("Component - ").expect("a component");
        assert!(first_component < others, "{printed}");
        for name in ["Ip", "IpV6"] {
            let line = format!("\tProperty - {name} [Category: Network] (String)");
            assert!(printed[others..].contains(&line), "{printed}");
        }
        assert!(printed.contains("\t\tPossible values (1): N/A\n"));
        // The data file's keys are listed, every query key before any server
        // key, and nothing else is.
        let query = printed.find("\tquery.client-ip-51d\n");
        let server = printed.find("\tserver.client-ip\n");
        assert!(query.is_some() && server.is_some(), "{printed}");
        assert!(query < server, "query keys are tried first: {printed}");
        assert!(!printed.contains("header.user-agent"));
        assert!(printed.contains(examples_shared::FIND_OUT_MORE_HEADING));
        // The ASN file carries none of the Enterprise properties, so the run
        // says so. 51DEGREES_IPI_PATH can name another file.
        let asn_file = examples_shared::IPI_ASN_RELATIVE_PATH.rsplit('/').next();
        if options.data_file.file_name().and_then(|n| n.to_str()) == asn_file {
            // The ASN file's properties are listed under their component, the
            // first values of each read from the data file and the rest counted.
            assert!(
                printed.contains("\tProperty - Asn [Category: "),
                "{printed}"
            );
            assert!(printed.contains("\t\tPossible values ("), "{printed}");
            assert!(printed.contains("): AS0, AS1, AS1000, "), "{printed}");
            assert!(printed.contains(" more ...\n"), "{printed}");
            assert!(printed.contains("Country: needs the Enterprise data file"));
            assert!(printed.contains("utm_term=enterprise-data-file"));
        }
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
 * 2. Each component the data file defines, with its properties. Each property
 *    is listed with its category, value type, description and the data tiers
 *    in which it has a value, all read from the data file, then the number of
 *    values it has and the first 20 of them. The values are read from the data
 *    file when asked for, because one property can have millions. `Ip` and
 *    `IpV6`, which the engine adds, belong to no component it lists, so they
 *    come last.
 * 3. The evidence keys the engine accepts, which its data file defines, in
 *    the order the engine tries them. Each header the data file names is
 *    accepted as a query value and as a server value, and every query value
 *    is tried before any server value.
 *
 * It then lists the properties only the Enterprise data file carries
 * (`Country`, `CountryCode`, `Town`, `Latitude`, `Longitude`, `IsHosted` and
 * `IsVPN`) and whether the data file in use has each, judged from the data
 * file's own list of properties. With a free file each says "needs the
 * Enterprise data file", and the run names them with a link to contact
 * 51Degrees about a copy of the Enterprise data file.
 *
 * It also prints the standard data-file warnings (for an out-of-date or
 * Lite-tier file) via the shared `check_data_file` helper, and ends with a
 * "Find out more" section.
 *
 * # Data file
 *
 * Resolved by `examples_shared::ipi_data_path`, which gives the free Lite file
 * when no other file is set. The Lite file carries the registration properties
 * `RegisteredName`, `RegisteredOwner` and `RegisteredCountry`, the free ASN
 * file carries `Asn` and `AsnName`, and an Enterprise file carries the full
 * network and location property set. Contact 51Degrees for an Enterprise file:
 * <https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-metadata.rs&utm_term=ipi-onprem-metadata>.
 *
 * # Usage sharing
 *
 * Not enabled, because this is a console example. Production deployments are
 * encouraged to enable usage sharing.
 */
