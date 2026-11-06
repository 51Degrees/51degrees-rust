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

//! The derived property element, found by the name .NET gives its builder,
//! so that a configuration file written for .NET makes the same element
//! here. See [`fiftyone_pipeline_core::configuration`].
//!
//! [`add_builders`] adds `DerivedPropertyElementBuilder`, which a
//! configuration can also name `DerivedPropertyElement`, with the settings
//! of .NET's builder.
//!
//! - `Scripts` names shipped scripts, as comma-separated text. Each name is
//!   matched ignoring case once the space around it is removed.
//! - `ScriptFiles` gives the paths of script files, as comma-separated text,
//!   each with the space around it removed. `AddScriptFile` gives one path,
//!   taken as it is. Each file is read as
//!   [`DerivedPropertyElementBuilder::add_script_file`] reads one.
//! - `AddScript` names one shipped script, by name or by number. The text of
//!   a script is given in code only, so a value naming no shipped script
//!   fails with .NET's message for the `AddScript` taking a name and a text.
//!
//! The pipeline holds the element made, so it can be read back. Every fault
//! found reading the scripts is reported in one message, after a line
//! counting them, and a configuration giving no script fails.
//!
//! ```
//! use fiftyone_derived_properties::{configuration, DerivedPropertyElement};
//! use fiftyone_pipeline_core::configuration::{BuilderRegistry, PipelineOptions};
//! use fiftyone_pipeline_core::PipelineBuilder;
//!
//! let mut registry = BuilderRegistry::new();
//! configuration::add_builders(&mut registry);
//! // A real configuration names the elements supplying the properties the
//! // script reads before this one.
//! let options = PipelineOptions::from_json(
//!     r#"{
//!         "PipelineOptions": {
//!             "Elements": [
//!                 {
//!                     "BuilderName": "DerivedPropertyElement",
//!                     "BuildParameters": { "Scripts": "HumanConfidence" }
//!                 }
//!             ]
//!         }
//!     }"#,
//! )?;
//! let pipeline = PipelineBuilder::new().build_from_configuration(&registry, &options)?;
//!
//! let element = pipeline
//!     .resource::<DerivedPropertyElement>()
//!     .expect("the derived property element");
//! let names: Vec<&str> = element.scripts().iter().map(|s| s.name()).collect();
//! assert_eq!(names, ["HumanConfidence"]);
//! # Ok::<(), fiftyone_pipeline_core::Error>(())
//! ```

use std::sync::Arc;

use fiftyone_pipeline_core::configuration::{
    parse_enum, BuildContext, BuilderDefinition, BuilderRegistry, ConfigType, ParseFailure,
};
use fiftyone_pipeline_core::{Error, FlowElement, Result};

use crate::fault::Faults;
use crate::scripts::built_in_names;
use crate::{BuiltInScript, DerivedPropertyElement, DerivedPropertyElementBuilder};

/// .NET's message for a configuration that gives no script.
const NO_SCRIPTS: &str = "At least one script must be configured. Add one with AddScript, \
                          AddScriptFile, or the Scripts and ScriptFiles configuration keys. \
                          (Parameter 'scripts')";

/// Add the derived property element's builder to `registry`, as
/// `DerivedPropertyElementBuilder`. Returns `registry` for chaining.
pub fn add_builders(registry: &mut BuilderRegistry) -> &mut BuilderRegistry {
    registry.add(derived_property_element_builder())
}

/// The settings in the order .NET's `GetMethods` gives the builder's
/// methods, which is the order its overloads of `AddScript` are tried in.
fn derived_property_element_builder() -> BuilderDefinition<DerivedPropertyElementBuilder> {
    BuilderDefinition::new(
        "DerivedPropertyElementBuilder",
        |builder: DerivedPropertyElementBuilder, context: &mut BuildContext| {
            if builder.is_empty() {
                return Err(Error::configuration(NO_SCRIPTS));
            }
            let element = builder
                .build()
                .map_err(|faults| Error::configuration(faults_message(&faults)))?;
            let element = Arc::new(element);
            context.add_resource(Arc::clone(&element) as _);
            Ok(element as Arc<dyn FlowElement>)
        },
    )
    .try_setting("AddScript", add_script)
    .setting("SetScripts", |builder, names: Vec<String>| {
        names.iter().fold(builder, |builder, name| {
            builder.add_built_in_named(name.trim())
        })
    })
    .setting(
        "AddScriptFile",
        |builder: DerivedPropertyElementBuilder, path: String| builder.add_script_file(path),
    )
    .setting("SetScriptFiles", |builder, paths: Vec<String>| {
        paths.iter().fold(builder, |builder, path| {
            builder.add_script_file(path.trim())
        })
    })
    .method_with_parameters("AddScript", 2)
    .build_method(Vec::new(), |_, _| Ok(DerivedPropertyElement::builder()))
}

/// .NET's `AddScript(BuiltInScript)`, which refuses a number no shipped
/// script has.
fn add_script(
    builder: DerivedPropertyElementBuilder,
    script: ScriptNumber,
) -> Result<DerivedPropertyElementBuilder> {
    match script {
        ScriptNumber::Known(script) => Ok(builder.add_built_in_named(script.name())),
        ScriptNumber::Unknown(number) => Err(Error::configuration(format!(
            "'{number}' is not a script in this package. The scripts in this package are {}. \
             (Parameter 'script')",
            built_in_names()
        ))),
        ScriptNumber::TooLarge => Err(Error::configuration(
            "Value was either too large or too small for an Int32.",
        )),
    }
}

/// The message .NET's `DerivedScriptValidationException` gives, being a
/// line counting the faults and then one line for each.
fn faults_message(faults: &Faults) -> String {
    let counted = if faults.len() == 1 {
        "fault was"
    } else {
        "faults were"
    };
    format!(
        "{} {counted} found while reading derived property scripts.\n{faults}",
        faults.len()
    )
}

/// A shipped script read as .NET's `Enum.Parse` reads a `BuiltInScript`.
/// That gives any whole number that fits in 32 bits, including one no
/// script has, and refuses a larger one outright rather than trying the
/// next `AddScript`.
enum ScriptNumber {
    Known(BuiltInScript),
    Unknown(i32),
    TooLarge,
}

impl ConfigType for ScriptNumber {
    const TYPE_NAME: &'static str = "BuiltInScript";

    fn parse_text(text: &str) -> std::result::Result<Self, ParseFailure> {
        let values: Vec<(&str, i64, BuiltInScript)> = BuiltInScript::ALL
            .iter()
            .zip(0..)
            .map(|(script, number)| (script.name(), number, *script))
            .collect();
        parse_enum(text, &values)
            .map(ScriptNumber::Known)
            .or_else(|refused| {
                let trimmed = text.trim_matches(|c| matches!(c, '\u{9}'..='\u{d}' | ' '));
                let digits = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
                if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(refused);
                }
                Ok(trimmed
                    .parse::<i32>()
                    .map_or(ScriptNumber::TooLarge, ScriptNumber::Unknown))
            })
    }
}
