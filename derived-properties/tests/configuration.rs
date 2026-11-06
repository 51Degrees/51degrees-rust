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

//! The derived property element made from a configuration, compared with
//! what .NET's FiftyOne.Pipeline.DerivedProperty 4.5.129 made from the same
//! configuration. Each expected answer and message is the one .NET gave,
//! apart from the differences the tests name.
//!
//! .NET ships HumanConfidence and IsGdpr where the crate ships
//! HumanConfidence alone, and .NET refuses a pipeline in which no element
//! supplies a property a script reads, which the crate does not check. Both
//! are #233's, so the tests that name a shipped script read the element
//! back where .NET's pipeline build failed after making it.

use std::sync::Arc;

use fiftyone_derived_properties::{configuration, DerivedPropertyElement};
use fiftyone_pipeline_core::configuration::{
    BuilderRegistry, ConfigValue, ElementOptions, PipelineOptions,
};
use fiftyone_pipeline_core::{Error, Pipeline, PipelineBuilder, Result};

const BUILDER: &str = "DerivedPropertyElementBuilder";
const BUILDER_TYPE: &str = "fiftyone_derived_properties::element::DerivedPropertyElementBuilder";
const NO_SUB_ELEMENTS: &str = "No SubElements";
const NO_INNER: &str = "No Inner Exception.";
const NO_SCRIPTS: &str = "At least one script must be configured. Add one with AddScript, \
                          AddScriptFile, or the Scripts and ScriptFiles configuration keys. \
                          (Parameter 'scripts')";

fn registry() -> BuilderRegistry {
    let mut registry = BuilderRegistry::new();
    configuration::add_builders(&mut registry);
    registry
}

fn build(options: &PipelineOptions) -> Result<Arc<Pipeline>> {
    PipelineBuilder::new().build_from_configuration(&registry(), options)
}

/// The element `builder` with `parameters`, alone in a pipeline.
fn with(builder: &str, parameters: Vec<(&str, ConfigValue)>) -> Result<Arc<Pipeline>> {
    let element = parameters
        .into_iter()
        .fold(ElementOptions::new(builder), |element, (n, v)| {
            element.parameter(n, v)
        });
    build(&PipelineOptions::new().element(element))
}

fn text(name: &str, value: &str) -> (&'static str, ConfigValue) {
    (leak(name), ConfigValue::from(value))
}

fn list(name: &str, values: &[&str]) -> (&'static str, ConfigValue) {
    (leak(name), ConfigValue::from(values.to_vec()))
}

fn leak(name: &str) -> &'static str {
    Box::leak(name.to_owned().into_boxed_str())
}

fn message(result: Result<Arc<Pipeline>>) -> String {
    match result {
        Err(Error::PipelineConfiguration { message }) => message,
        Err(other) => panic!("not a configuration error: {other}"),
        Ok(_) => panic!("the pipeline was built"),
    }
}

/// The message .NET gives for an element that cannot be made.
fn failed(message: &str) -> String {
    format!(
        "Configuration failed to build.\nBuilderName: {BUILDER}\nSubElement Builder Names: \
         {NO_SUB_ELEMENTS}\n{message}\n{NO_INNER}\n"
    )
}

/// .NET's message for the faults found reading the scripts.
fn faults(lines: &[&str]) -> String {
    let counted = if lines.len() == 1 {
        "fault was"
    } else {
        "faults were"
    };
    failed(&format!(
        "{} {counted} found while reading derived property scripts.\n{}",
        lines.len(),
        lines.join("\n")
    ))
}

fn element(pipeline: &Pipeline) -> Arc<DerivedPropertyElement> {
    pipeline
        .resource::<DerivedPropertyElement>()
        .expect("the derived property element")
}

/// Each script's name and where it came from, in the order the element
/// runs them.
fn scripts(result: Result<Arc<Pipeline>>) -> Vec<(String, String)> {
    let pipeline = result.expect("the pipeline");
    element(&pipeline)
        .scripts()
        .iter()
        .map(|s| (s.name().to_owned(), s.source().to_owned()))
        .collect()
}

fn built_in(source: &str) -> Vec<(String, String)> {
    vec![("HumanConfidence".to_owned(), format!("built in {source}"))]
}

/// The source of each script is the name as given, which .NET's log line
/// for the script shows.
#[test]
fn scripts_names_shipped_scripts_ignoring_case_and_space() {
    for (parameter, source) in [
        (text("Scripts", "HumanConfidence"), "HumanConfidence"),
        (text("Scripts", "humanconfidence"), "humanconfidence"),
        (text("Scripts", " HumanConfidence "), "HumanConfidence"),
        (text("Scripts", "\"HumanConfidence\""), "HumanConfidence"),
        (text("Scripts", "HumanConfidence,"), "HumanConfidence"),
        (list("Scripts", &[" HumanConfidence "]), "HumanConfidence"),
        (text("SetScripts", "HumanConfidence"), "HumanConfidence"),
        (text("scripts", "HumanConfidence"), "HumanConfidence"),
    ] {
        let shown = format!("{parameter:?}");
        assert_eq!(
            scripts(with(BUILDER, vec![parameter])),
            built_in(source),
            "{shown}"
        );
    }
}

/// .NET's list of the shipped scripts also names IsGdpr.
#[test]
fn a_script_name_that_is_unknown_or_empty_is_a_fault() {
    let unknown = |name: &str| {
        format!(
            "{name} (built in) at (document): there is no script called '{name}' in this \
             package. The scripts in this package are HumanConfidence. A script from your own \
             environment is added with ScriptFiles rather than Scripts."
        )
    };
    assert_eq!(
        message(with(BUILDER, vec![text("Scripts", "Unknown")])),
        faults(&[&unknown("Unknown")])
    );
    assert_eq!(
        message(with(BUILDER, vec![text("Scripts", "Unknown,,Other")])),
        faults(&[
            &unknown("Unknown"),
            "script (built in) at (document): a built in script name is empty",
            &unknown("Other"),
        ])
    );
}

#[test]
fn a_configuration_giving_no_script_fails() {
    for parameters in [
        vec![],
        vec![text("Scripts", "")],
        vec![text("ScriptFiles", "")],
    ] {
        let shown = format!("{parameters:?}");
        assert_eq!(
            message(with(BUILDER, parameters)),
            failed(NO_SCRIPTS),
            "{shown}"
        );
    }
}

/// .NET's fault says "two scripts in the same element write the property
/// 'HumanConfidence', being 'HumanConfidence' and 'HumanConfidence'. One
/// element writes each property once".
#[test]
fn a_script_named_twice_is_a_fault() {
    let twice = "HumanConfidence (built in HumanConfidence) at Output.Name: two scripts both \
                 produce the property 'HumanConfidence', and only one of the two could be \
                 published under the 'derived' key";
    for parameters in [
        vec![text("Scripts", "HumanConfidence,HumanConfidence")],
        vec![
            text("Scripts", "HumanConfidence"),
            text("AddScript", "HumanConfidence"),
        ],
    ] {
        let shown = format!("{parameters:?}");
        assert_eq!(
            message(with(BUILDER, parameters)),
            faults(&[twice]),
            "{shown}"
        );
    }
}

/// .NET reads the value as a `BuiltInScript`, whose names it matches
/// ignoring case and whose numbers start at 0, and adds the script under
/// its own name.
#[test]
fn add_script_takes_a_shipped_script_by_name_or_number() {
    for value in [
        "HumanConfidence",
        "humanconfidence",
        " HumanConfidence ",
        "0",
    ] {
        assert_eq!(
            scripts(with(BUILDER, vec![text("AddScript", value)])),
            built_in("HumanConfidence"),
            "'{value}'"
        );
    }
}

/// A value that is not a `BuiltInScript` reaches .NET's `AddScript` taking
/// a name and a text, and a number no script has reaches the method, which
/// refuses it. .NET's 1 is IsGdpr, and its list also names IsGdpr.
#[test]
fn add_script_refuses_other_values_as_dotnet_does() {
    let two = failed(&format!(
        "Method 'AddScript' on builder '{BUILDER_TYPE}' for element 0 takes more than one \
         parameter. This is not supported."
    ));
    for parameter in [
        text("AddScript", "x"),
        text("AddScript", ""),
        text("AddScript", "5x"),
        list("AddScript", &["HumanConfidence"]),
    ] {
        let shown = format!("{parameter:?}");
        assert_eq!(message(with(BUILDER, vec![parameter])), two, "{shown}");
    }
    for value in ["99999999999", "2147483648"] {
        assert_eq!(
            message(with(BUILDER, vec![text("AddScript", value)])),
            failed("Value was either too large or too small for an Int32."),
            "'{value}'"
        );
    }
    for (value, number) in [
        ("5", "5"),
        ("-1", "-1"),
        ("+05", "5"),
        ("1", "1"),
        ("-2147483648", "-2147483648"),
    ] {
        assert_eq!(
            message(with(BUILDER, vec![text("AddScript", value)])),
            failed(&format!(
                "'{number}' is not a script in this package. The scripts in this package are \
                 HumanConfidence. (Parameter 'script')"
            )),
            "'{value}'"
        );
    }
}

/// .NET lists the same four settings in the same order, giving
/// `RuntimeParameterInfo` as each type, and leaves out the `AddScript`
/// taking two values.
#[test]
fn an_unknown_setting_lists_dotnets_settings() {
    for name in ["Script", "ScriptFile"] {
        assert_eq!(
            message(with(BUILDER, vec![text(name, "x")])),
            failed(&format!(
                "Builder '{BUILDER_TYPE}' for element 0 has no 'Set' methods or 'Build' methods \
                 that match (case-insensitively) the following parameters: {}. The available \
                 configuration methods on this builder are: \nAddScript (BuiltInScript)\n\
                 SetScripts (List`1)\nAddScriptFile (String)\nSetScriptFiles (List`1)\nThe \
                 available 'Build' methods have the following signatures: \n\n",
                name.to_uppercase()
            )),
            "{name}"
        );
    }
}

/// The tests that read script files, compiled only off WebAssembly, where a
/// test has no file system.
#[cfg(not(target_family = "wasm"))]
mod from_files {
    use std::path::{Path, PathBuf};

    use fiftyone_pipeline_core::{FlowElement, PropertyValue};

    use super::*;

    /// A folder of script files for one test, removed when the test ends.
    struct Scripts(PathBuf);

    impl Scripts {
        /// A.yaml and B.yaml, each giving its output Low and reading nothing,
        /// and Bad.yaml, whose own name is not its file name, as .NET's run had
        /// them.
        fn new(test: &str) -> Self {
            let folder = std::env::temp_dir().join(format!(
                "fiftyone-derived-configuration-{}-{test}",
                std::process::id()
            ));
            std::fs::create_dir_all(&folder).unwrap();
            for (file, name, output) in [
                ("A.yaml", "A", "AOut"),
                ("B.yaml", "B", "BOut"),
                ("Bad.yaml", "Wrong", "WrongOut"),
            ] {
                std::fs::write(folder.join(file), script(name, output)).unwrap();
            }
            Scripts(folder)
        }

        fn path(&self, file: &str) -> String {
            self.0.join(file).display().to_string()
        }
    }

    impl Drop for Scripts {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A script naming no source property, so it builds on its own.
    fn script(name: &str, output: &str) -> String {
        format!(
            "Format: 1\nName: {name}\nVersion: 1.0.0\nOutput:\n  Name: {output}\n  Description: A \
         property that reads no source property.\n  ValueType: string\n  IsList: \
         false\nRules:\n  - Else: Low\n"
        )
    }

    fn files(scripts: &Scripts, names: &[&str]) -> Vec<(String, String)> {
        names
            .iter()
            .map(|name| (name.to_string(), scripts.path(&format!("{name}.yaml"))))
            .collect()
    }

    #[test]
    fn the_builder_is_found_by_dotnets_names() {
        let scripts = Scripts::new("names");
        for name in [
            BUILDER,
            "DerivedPropertyElement",
            "derivedpropertyelementbuilder",
        ] {
            let pipeline = with(name, vec![text("ScriptFiles", &scripts.path("A.yaml"))]).unwrap();
            let keys: Vec<&str> = pipeline
                .flow_elements()
                .iter()
                .map(|e| e.data_key())
                .collect();
            assert_eq!(keys, ["derived"], "{name}");
            let made = element(&pipeline);
            let properties: Vec<&str> = made.properties().iter().map(|p| p.name.as_str()).collect();
            assert_eq!(properties, ["AOut"], "{name}");

            let mut data = pipeline.create_flow_data();
            data.process().unwrap();
            let derived = data.get_str("derived").expect("the derived element data");
            assert_eq!(
                derived.get("AOut").unwrap(),
                PropertyValue::String("Low".to_owned()),
                "{name}"
            );
        }
    }

    #[test]
    fn script_files_reads_each_path_in_order() {
        let s = Scripts::new("files");
        let (a, b) = (s.path("A.yaml"), s.path("B.yaml"));
        for parameter in [
            text("ScriptFiles", &format!("{b},{a}")),
            text("ScriptFiles", &format!("  {b}  ,  {a}  ")),
            list("ScriptFiles", &[&b, &a]),
            list("ScriptFiles", &[&format!(" {b}"), &format!("{a} ")]),
        ] {
            let shown = format!("{parameter:?}");
            assert_eq!(
                scripts(with(BUILDER, vec![parameter])),
                files(&s, &["B", "A"]),
                "{shown}"
            );
        }
        assert_eq!(
            scripts(with(BUILDER, vec![text("AddScriptFile", &a)])),
            files(&s, &["A"])
        );
    }

    /// .NET's fault for the missing file is "script (<path>) at (document): the
    /// file does not exist", and it gives the line of the bad file's name, being
    /// "Bad (<path>) line 2 at Name". The crate reads a path as
    /// `add_script_file` does, which #233 changes.
    #[test]
    fn faults_in_script_files_are_reported_together() {
        let s = Scripts::new("faults");
        let missing = s.path("missing.yaml");
        let bad = s.path("Bad.yaml");
        let found = message(with(
            BUILDER,
            vec![text("ScriptFiles", &format!("{missing},{bad}"))],
        ));
        let opening = failed("2 faults were found while reading derived property scripts.\n");
        let opening = opening
            .split_once(NO_INNER)
            .unwrap()
            .0
            .trim_end_matches('\n');
        assert!(found.starts_with(opening), "{found}");
        assert!(
            found.contains(&format!(
                "\nmissing ({missing}) at (document): the script file could not be read: "
            )),
            "{found}"
        );
        assert!(
            found.ends_with(&format!(
                "\nBad ({bad}) at Name: script name 'Wrong' must equal the file name 'Bad'\n\
             {NO_INNER}\n"
            )),
            "{found}"
        );
    }

    /// .NET adds a script for each file a wildcard matches, in name order,
    /// which is #233's. The crate reads the wildcard as a path.
    #[test]
    fn a_wildcard_is_read_as_a_path() {
        let s = Scripts::new("wildcard");
        let pattern = s.path("*.yaml");
        let found = message(with(BUILDER, vec![text("ScriptFiles", &pattern)]));
        assert!(
            found.contains(&format!(
                "\n* ({pattern}) at (document): the script file could not be read: "
            )),
            "{found}"
        );
    }

    /// `AddScriptFile` takes the path as it is, so space before it is part of
    /// the path, as in .NET, whose fault is "script ( <path>) at (document):
    /// the file does not exist". A list is refused with .NET's message.
    #[test]
    fn add_script_file_takes_one_path_as_it_is() {
        let s = Scripts::new("one");
        let spaced = format!(" {}", s.path("A.yaml"));
        let found = message(with(BUILDER, vec![text("AddScriptFile", &spaced)]));
        assert!(
            found.contains(&format!(
                "\nA ({spaced}) at (document): the script file could not be read: "
            )),
            "{found}"
        );
        assert_eq!(
            message(with(
                BUILDER,
                vec![list("AddScriptFile", &[&s.path("A.yaml")])]
            )),
            failed("Object of type 'System.String[]' cannot be converted to type 'System.String'.")
        );
    }

    /// Settings given in code apply in the order given, and those a file gives
    /// in name order, as .NET's configuration gives them.
    #[test]
    fn settings_apply_in_the_order_given_and_a_file_in_name_order() {
        let s = Scripts::new("order");
        let (a, b) = (s.path("A.yaml"), s.path("B.yaml"));
        assert_eq!(
            scripts(with(
                BUILDER,
                vec![text("ScriptFiles", &b), text("AddScriptFile", &a)]
            )),
            files(&s, &["B", "A"])
        );
        let mut both = files(&s, &["A"]);
        both.extend(built_in("HumanConfidence"));
        assert_eq!(
            scripts(with(
                BUILDER,
                vec![text("ScriptFiles", &a), text("Scripts", "HumanConfidence")]
            )),
            both
        );

        let json = |path: &str| path.replace('\\', "\\\\");
        let options = PipelineOptions::from_json(&format!(
            r#"{{
            "PipelineOptions": {{
                "Elements": [
                    {{
                        "BuilderName": "DerivedPropertyElement",
                        "BuildParameters": {{
                            "ScriptFiles": "{}",
                            "AddScriptFile": "{}"
                        }}
                    }}
                ]
            }}
        }}"#,
            json(&b),
            json(&a)
        ))
        .unwrap();
        assert_eq!(scripts(build(&options)), files(&s, &["A", "B"]));

        let options = PipelineOptions::from_json(&format!(
            r#"{{
            "PipelineOptions": {{
                "Elements": [
                    {{
                        "BuilderName": "DerivedPropertyElement",
                        "BuildParameters": {{
                            "Scripts": "HumanConfidence",
                            "ScriptFiles": "{}"
                        }}
                    }}
                ]
            }}
        }}"#,
            json(&a)
        ))
        .unwrap();
        assert_eq!(scripts(build(&options)), both);
    }

    /// The paths here are absolute, so each source is the full path .NET gives.
    #[test]
    fn a_script_file_source_is_its_path() {
        let s = Scripts::new("source");
        let a = s.path("A.yaml");
        assert!(Path::new(&a).is_absolute());
        assert_eq!(
            scripts(with(BUILDER, vec![text("ScriptFiles", &a)])),
            vec![("A".to_owned(), a)]
        );
    }
}
