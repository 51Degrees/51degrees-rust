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

//! @page dd-onprem-metadata-example Metadata (Device Detection, On-premise)
//!
//! On-premise metadata console example. See the descriptive block at the bottom
//! of this file for the full write-up.
//!
//! @snippet dd-onprem-metadata.rs example

use std::fmt::Write as _;
use std::path::PathBuf;

use anyhow::{Context, Result};

use fiftyone_device_detection::{
    DeviceDetectionOnPremiseEngine, DeviceDetectionOnPremiseEngineBuilder, PerformanceProfile,
};
use fiftyone_pipeline_core::PropertyValueType;
use fiftyone_pipeline_engines::{AspectEngine, OnPremiseAspectEngine};

/// Options the example runs with, so `main` and the test share one entry point.
pub struct ExampleOptions {
    /// The path to the on-premise Hash data file the engine loads.
    pub data_file: PathBuf,
    /// The performance profile to load the data file with.
    pub profile: PerformanceProfile,
}

// [example]
/// Run the on-premise metadata example.
///
/// Builds a Hash engine directly (no full pipeline is needed to read metadata),
/// then enumerates the components and their properties, each property's
/// category, value type, description and possible values, the accepted
/// evidence keys, and the data file's publish date and tier.
pub fn run(options: ExampleOptions) -> Result<()> {
    // A full pipeline is not required to read metadata. Building the engine
    // directly is enough, and is also how you would obtain the metadata if you
    // already had an engine. Updates are disabled (the builder defaults to no
    // auto-update), which suits a one-shot metadata dump.
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(&options.data_file)
        .performance_profile(options.profile)
        .build()
        .context("failed to build the on-premise Hash engine")?;

    output_data_file_details(engine.as_ref());
    output_components(engine.as_ref());
    output_evidence_keys(&engine.evidence_keys());

    // Print the standard data-file warnings last, as the other examples do.
    // The engine is already in hand, so the engine-taking helper prints the same
    // lines without building a second engine.
    println!();
    device_detection_examples::print_data_file_warnings_for(engine.as_ref());

    Ok(())
}
// [example]

/// Print the data file's publish date and data tier.
///
/// The tier and publish date are how a caller tells a free Lite file apart from a
/// paid Premium or Enterprise file, and how it learns whether the data is recent.
fn output_data_file_details(engine: &impl OnPremiseAspectEngine) {
    let mut message = String::new();
    let _ = writeln!(message, "--- Data file ---");
    let _ = writeln!(message, "Data tier   : {}", engine.data_source_tier());
    match engine.data_file_published() {
        Some(published) => {
            let _ = writeln!(message, "Published   : {published}");
        }
        None => {
            let _ = writeln!(message, "Published   : unknown");
        }
    }
    print!("{message}");
}

/// The number of a property's values the example prints.
const SHOWN_VALUES: usize = 20;

/// Print each component with its properties, and each property's category,
/// value type, description, data tiers and first few possible values.
///
/// The device-detection data model groups properties into components, being
/// the hardware, the software platform (the operating system), the browser and
/// the crawler, with the engine's match metrics in a `Metrics` component of
/// their own. The engine reads a property's values from the data file when
/// they are asked for, because a data file can hold hundreds of thousands. The
/// number of properties depends on the data tier: the free Lite tier contains
/// fewer than 20 of the 200-plus properties in the full data file.
fn output_components(engine: &DeviceDetectionOnPremiseEngine) {
    println!();
    println!(
        "--- Components and their properties ({} properties in total) ---",
        engine.aspect_properties().len()
    );
    for component in engine.components() {
        let mut message = String::new();
        let _ = writeln!(
            message,
            "Component - {} ({} properties)",
            component.name,
            component.properties().len()
        );
        for property in component.properties() {
            let _ = writeln!(
                message,
                "\tProperty - {} [Category: {}] ({}) - {}",
                property.name(),
                property.core().category,
                value_type_name(property.core().value_type),
                property.aspect().description()
            );
            // The data tiers a property is present in help explain a missing
            // value: a property listed only under higher tiers needs a data-file
            // upgrade rather than a configuration change.
            let _ = writeln!(
                message,
                "\t\tData tiers: {}",
                property.aspect().data_tiers_where_present().join(", ")
            );
            // Most match metrics have no defined values, so they are left out.
            if property.core().category != "Device Metrics" {
                let values = match engine.property_values(property.name()) {
                    Ok(values) => values.unwrap_or_default(),
                    Err(error) => {
                        println!("\t\tValues could not be read: {error}");
                        Vec::new()
                    }
                };
                let shown: Vec<String> = values
                    .iter()
                    .take(SHOWN_VALUES)
                    .map(|value| {
                        // A script's value runs to many lines, so only its first
                        // is shown.
                        let name = value.name.lines().next().unwrap_or_default();
                        if value.description.is_empty() {
                            name.to_owned()
                        } else {
                            format!("{name} ({})", value.description)
                        }
                    })
                    .collect();
                let mut line = format!("\t\tPossible values: {}", shown.join(", "));
                if values.len() > SHOWN_VALUES {
                    let _ = write!(line, " + {} more ...", values.len() - SHOWN_VALUES);
                }
                let _ = writeln!(message, "{line}");
            }
        }
        print!("{message}");
    }
}

/// Print the evidence keys device detection accepts.
///
/// These are the keys that, when added to the evidence of a flow data, could
/// affect the result. Each header the data file reads is accepted as a header
/// and as a query value, the query form being for off-line processing.
fn output_evidence_keys(keys: &[String]) {
    println!();
    println!("--- Accepted evidence keys ---");
    for key in keys {
        println!("\t{key}");
    }
    println!(
        "\tand any query or cookie value named 51d_..., which the client-side \
         JavaScript sends back"
    );
}

/// A short display name for a property value type, used in the metadata listing.
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
        // PropertyValueType is non-exhaustive, so a future variant renders
        // generically rather than failing to compile.
        _ => "Other",
    }
}

/// Resolve the data file then run the example, with the same fallback chain as
/// the other on-premise examples (argument, env var, shipped Lite file).
fn main() -> Result<()> {
    let data_file = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .or_else(examples_shared::dd_data_path);

    let Some(data_file) = data_file else {
        eprintln!(
            "No device-detection data file found. Set 51DEGREES_DD_PATH (or pass \
             the path as the first argument), or run `git submodule update \
             --recursive` so the Lite Hash file in the cxx submodule is present."
        );
        return Ok(());
    };

    run(ExampleOptions {
        data_file,
        profile: PerformanceProfile::LowMemory,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run the example against the Lite Hash file, skipping when none is present.
    #[test]
    fn runs_against_the_lite_data_file() {
        let Some(data_file) = examples_shared::dd_data_path() else {
            eprintln!("skipping: no on-premise data file found (set 51DEGREES_DD_PATH)");
            return;
        };
        run(ExampleOptions {
            data_file,
            profile: PerformanceProfile::LowMemory,
        })
        .expect("the on-premise metadata example should complete");
    }
}

/*
 * @example dd-onprem-metadata.rs
 *
 * The device-detection on-premise metadata console example. It shows how to read
 * the metadata embedded in an on-premise Hash data file.
 *
 * The device-detection data file contains metadata describing the records in the
 * data model. It is useful to know the kinds of record present:
 *
 * - Component - a major aspect of the entity making a request. There are four
 *   components: Hardware, Software Platform (OS), Browser and Crawler.
 * - Profile - the details for one instance of a component, for example the iPhone
 *   13 hardware profile or the Android 12 platform profile.
 * - Property - each property has a value (or values) for each profile, for
 *   example the hardware property `IsMobile` or the browser property
 *   `BrowserName`.
 *
 * No full pipeline is needed to read metadata: the example builds a Hash engine
 * directly with `DeviceDetectionOnPremiseEngineBuilder`. If you already had a
 * pipeline you would obtain the engine's metadata the same way once you have a
 * reference to the engine.
 *
 * The example then prints:
 *
 * - The data file's data tier and publish date, which is how you tell a free Lite
 *   file apart from a paid Premium or Enterprise file and learn whether the data
 *   is recent.
 * - Every component with its properties, and each property's category, value
 *   type, description, the data tiers in which it is present and up to 20 of
 *   the values it can have. The match metrics are in a `Metrics` component of
 *   their own. The number of properties depends on the tier: the Lite tier
 *   contains fewer than 20 of the 200-plus properties available in the full
 *   data file.
 * - The evidence keys device detection accepts. These are the keys that, when
 *   added to a flow data's evidence, could affect the result. Each header the
 *   data file reads is accepted as a header and as a query value, alongside the
 *   high-entropy values keys and the `51d_` values the client-side JavaScript
 *   sends back.
 *
 * Finally the example prints the standard data-file warnings (Lite tier has
 * limited accuracy; a file more than 30 days old may miss the latest devices).
 *
 * The data file is read from the first command-line argument, the
 * `51DEGREES_DD_PATH` environment variable, or the Lite Hash file shipped in the
 * cxx submodule.
 */
