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

//! Derived Property Element: compute a property from properties another
//! element has already produced, following a script, with the pipeline built
//! from a configuration file and then from code.

use std::any::Any;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use fiftyone_derived_properties::{configuration, DerivedData, DerivedPropertyElement};
use fiftyone_pipeline_core::configuration::{BuilderDefinition, BuilderRegistry, PipelineOptions};
use fiftyone_pipeline_core::{
    ElementData, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    NoValueError, Pipeline, PipelineBuilder, PropertyMetaData, PropertyValue, PropertyValueType,
    TypedKey,
};

/// The links the example's output ends with.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("Pipeline API", "https://51degrees.com/documentation/_pipeline_api__index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-pipeline-examples-src-bin-derived-property.rs&utm_term=find-out-more"),
    ("Device Detection", "https://51degrees.com/device-detection?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-pipeline-examples-src-bin-derived-property.rs&utm_term=find-out-more"),
    ("IP Intelligence", "https://51degrees.com/ip-intelligence?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-pipeline-examples-src-bin-derived-property.rs&utm_term=find-out-more"),
];

/// The repositories the output ends with, being this SDK, the specifications
/// and the scripts 51Degrees ships with their format reference.
const REPOSITORIES: &[&str] = &[
    "https://github.com/51Degrees/51degrees-rust",
    "https://github.com/51Degrees/specifications",
    "https://github.com/51Degrees/derived-properties",
];

/// The configuration file beside this example.
const OPTIONS_FILE: &str = "derived-property-options.json";

/// The script file beside this example. The configuration file names it, and
/// so does the builder in code.
const SCRIPT_FILE: &str = "ReaderEngagement.yaml";

/// The property the script in [`SCRIPT_FILE`] writes.
const ENGAGEMENT_PROPERTY: &str = "ReaderEngagement";

/// The property [`EVERY_TEST_SCRIPT`] writes.
const EVERY_TEST_PROPERTY: &str = "ReaderEngagementIsHigh";

/// A second script, held as text in the example itself. The text is YAML
/// here, and JSON is read just as well.
///
/// It reads the same three source properties and says whether every test
/// passed, so one element runs two scripts over one set of source properties.
const EVERY_TEST_SCRIPT: &str = r#"
Format: 1
Name: ReaderEngagementIsHigh
Version: "1.0.0"
Output:
  Name: ReaderEngagementIsHigh
  Description: Whether every one of the ReaderEngagement tests passed on this request.
  ValueType: bool
  IsList: false
  Category: Example
Checks:
  SeveralPages: { Property: session.PagesViewed, Ge: 3 }
  LongEnough:   { Property: session.SecondsSincePageLoad, Ge: 30 }
  PointerMoved: { Property: session.PointerMoved, Eq: true }
Rules:
  - When: { Failed: Checks, Eq: 0 }
    Then: true
  - Else: false
"#;

/// The line that starts each part of the output.
const RULE: &str = "==========================================";

/// The key the session element writes its data under, which is the half of
/// `session.PagesViewed` before the dot.
const SESSION_DATA_KEY: &str = "session";

/// The names of the three source properties, which must equal the names the
/// scripts use after the dot.
const PAGES_VIEWED: &str = "PagesViewed";
const SECONDS_SINCE_PAGE_LOAD: &str = "SecondsSincePageLoad";
const POINTER_MOVED: &str = "PointerMoved";

/// The evidence each source property is read from.
const PAGES_VIEWED_EVIDENCE: &str = "query.pagesviewed";
const SECONDS_EVIDENCE: &str = "query.secondssincepageload";
const POINTER_MOVED_EVIDENCE: &str = "query.pointermoved";

/// What the session element found on one request. A property is held only
/// where the request carried evidence for it, so the derived property
/// element finds a property with no evidence absent.
#[derive(Debug, Clone, Default)]
struct SessionData {
    pages_viewed: Option<i64>,
    seconds_since_page_load: Option<i64>,
    pointer_moved: Option<bool>,
}

impl SessionData {
    /// The value held for `name`, matched ignoring case.
    fn value(&self, name: &str) -> Option<PropertyValue> {
        if name.eq_ignore_ascii_case(PAGES_VIEWED) {
            self.pages_viewed.map(PropertyValue::from)
        } else if name.eq_ignore_ascii_case(SECONDS_SINCE_PAGE_LOAD) {
            self.seconds_since_page_load.map(PropertyValue::from)
        } else if name.eq_ignore_ascii_case(POINTER_MOVED) {
            self.pointer_moved.map(PropertyValue::from)
        } else {
            None
        }
    }
}

impl ElementData for SessionData {
    fn get(&self, name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        self.value(name)
            .ok_or_else(|| NoValueError::new(format!("No value for property '{name}'.")))
    }

    fn has_property(&self, name: &str) -> bool {
        self.value(name).is_some()
    }

    fn keys(&self) -> Vec<String> {
        [PAGES_VIEWED, SECONDS_SINCE_PAGE_LOAD, POINTER_MOVED]
            .into_iter()
            .filter(|name| self.value(name).is_some())
            .map(str::to_owned)
            .collect()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The element that supplies the source properties the scripts read. A real
/// pipeline puts Device Detection or IP Intelligence here, and this small
/// element stands in for one so that the example needs no resource key, no
/// data file and no network connection.
struct SessionElement {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

impl SessionElement {
    const KEY: TypedKey<SessionData> = TypedKey::new(SESSION_DATA_KEY);

    fn new() -> Self {
        // The derived property element checks, when the pipeline is built,
        // that some element lists each source property a script reads.
        let property = |name: &str, value_type| {
            PropertyMetaData::new(name, SESSION_DATA_KEY, value_type).with_category("Example")
        };
        SessionElement {
            filter: EvidenceKeyFilterWhitelist::new([
                PAGES_VIEWED_EVIDENCE,
                SECONDS_EVIDENCE,
                POINTER_MOVED_EVIDENCE,
            ]),
            properties: vec![
                property(PAGES_VIEWED, PropertyValueType::Integer),
                property(SECONDS_SINCE_PAGE_LOAD, PropertyValueType::Integer),
                property(POINTER_MOVED, PropertyValueType::Bool),
            ],
        }
    }
}

impl FlowElement for SessionElement {
    fn process(
        &self,
        data: &mut FlowData,
    ) -> std::result::Result<(), fiftyone_pipeline_core::Error> {
        let evidence = data.evidence();
        let number = |key: &str| evidence.get(key).and_then(|text| text.trim().parse().ok());
        let found = SessionData {
            pages_viewed: number(PAGES_VIEWED_EVIDENCE),
            seconds_since_page_load: number(SECONDS_EVIDENCE),
            pointer_moved: evidence
                .get(POINTER_MOVED_EVIDENCE)
                .and_then(|text| text.trim().to_ascii_lowercase().parse().ok()),
        };
        *data.get_or_add(Self::KEY, SessionData::default)? = found;
        Ok(())
    }

    fn data_key(&self) -> &str {
        SESSION_DATA_KEY
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

/// The builders a configuration file can name. The pipeline builder makes
/// each element by finding its builder by name, so the session element's
/// builder is added beside the derived property element's.
fn registry() -> BuilderRegistry {
    let mut registry = BuilderRegistry::new().with(
        BuilderDefinition::new("SessionElementBuilder", |element: SessionElement, _| {
            Ok(Arc::new(element) as Arc<dyn FlowElement>)
        })
        .build_method(Vec::new(), |_, _| Ok(SessionElement::new())),
    );
    configuration::add_builders(&mut registry);
    registry
}

/// The folder this example's source, script and configuration file are in.
fn example_folder() -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "src", "bin"].iter().collect()
}

/// Run the example, writing what it shows to `out`.
///
/// A script file path, whether the configuration file or the builder gives
/// it, is read relative to the working directory. The example moves the
/// working directory to its own folder, so it runs the same wherever it was
/// started from.
pub fn run(out: &mut dyn Write) -> Result<()> {
    let folder = example_folder();
    std::env::set_current_dir(&folder)
        .with_context(|| format!("the example's folder {} is not there", folder.display()))?;
    run_from_configuration(out)?;
    run_from_code(out)
}

/// The pipeline described in a configuration file, which names both builders
/// and gives the script file as a build parameter.
fn run_from_configuration(out: &mut dyn Write) -> Result<()> {
    writeln!(out, "{RULE}")?;
    writeln!(out, "1. Configured from {OPTIONS_FILE}")?;
    writeln!(out)?;

    let text = std::fs::read_to_string(OPTIONS_FILE)
        .with_context(|| format!("{OPTIONS_FILE} could not be read"))?;
    let options = PipelineOptions::from_json(&text)?;
    let pipeline = PipelineBuilder::new().build_from_configuration(&registry(), &options)?;

    let properties = [ENGAGEMENT_PROPERTY];
    process(
        out,
        &pipeline,
        "Request 1, every source property present.",
        Some(true),
        &properties,
    )?;
    process(
        out,
        &pipeline,
        "Request 2, one source property absent.",
        None,
        &properties,
    )?;
    writeln!(
        out,
        "The second request has no value at all, because a property the script\n\
         names was not there. The message says which property was missing and\n\
         what the element that supplies it said, which is more use than an\n\
         answer resting on part of the evidence."
    )?;
    writeln!(out)?;

    // The pipeline holds the element the configuration made, so its scripts
    // can be read back.
    let derived = pipeline
        .resource::<DerivedPropertyElement>()
        .ok_or_else(|| anyhow!("the pipeline holds no derived property element"))?;
    print_definitions(out, &derived)
}

/// The same pipeline made with the builders in code. One element runs both
/// scripts, one read from a file and one held as text.
fn run_from_code(out: &mut dyn Write) -> Result<()> {
    writeln!(out, "{RULE}")?;
    writeln!(out, "2. Configured with the builder in code")?;
    writeln!(out)?;

    let derived = DerivedPropertyElement::builder()
        // A script of your own in a file. A path may hold a wildcard, so
        // "scripts/*.yaml" adds a whole folder.
        .add_script_file(SCRIPT_FILE)
        // A script held as text. The name given here must equal the Name
        // the script itself carries.
        .add_script(EVERY_TEST_PROPERTY, EVERY_TEST_SCRIPT)
        // add_built_in adds a script that ships inside the crate. Those come
        // from the derived-properties repository, and this example uses
        // none of them so that it does not depend on what that repository
        // holds.
        .build()
        .map_err(fiftyone_pipeline_core::Error::from)?;

    // The derived property element goes after every element that supplies a
    // source property. The build fails, naming the property, when no element
    // in the pipeline supplies one.
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(SessionElement::new()))
        .add_element(Arc::new(derived))
        .build()?;

    let properties = [ENGAGEMENT_PROPERTY, EVERY_TEST_PROPERTY];
    process(
        out,
        &pipeline,
        "Request 1, every source property present.",
        Some(true),
        &properties,
    )?;
    process(
        out,
        &pipeline,
        "Request 2, one source property absent.",
        None,
        &properties,
    )?;
    writeln!(
        out,
        "One element runs both scripts, so both properties are written under\n\
         the key derived, and both say the same thing about the second request\n\
         because they read the same source properties."
    )?;
    writeln!(out)?;
    Ok(())
}

/// Process one request and print the source properties and the derived
/// properties it gave. `pointer_moved` is the evidence for
/// `session.PointerMoved`, or `None` to leave that property absent.
fn process(
    out: &mut dyn Write,
    pipeline: &Arc<Pipeline>,
    title: &str,
    pointer_moved: Option<bool>,
    derived_properties: &[&str],
) -> Result<()> {
    writeln!(out, "{title}")?;
    let mut evidence = Evidence::builder()
        .add(PAGES_VIEWED_EVIDENCE, "5")
        .add(SECONDS_EVIDENCE, "120");
    if let Some(moved) = pointer_moved {
        evidence = evidence.add(POINTER_MOVED_EVIDENCE, moved.to_string());
    }
    let mut data = pipeline.create_flow_data_with(evidence.build());
    data.process()?;

    // The source properties, read as any element's properties are read.
    let session = data
        .get(SessionElement::KEY)
        .ok_or_else(|| anyhow!("the request carries no session data"))?;
    print(out, "session.PagesViewed", &supplied(session.pages_viewed))?;
    print(
        out,
        "session.SecondsSincePageLoad",
        &supplied(session.seconds_since_page_load),
    )?;
    print(
        out,
        "session.PointerMoved",
        &supplied(session.pointer_moved),
    )?;

    // The derived properties, read from the element data the derived
    // property element writes under the key derived.
    let derived = data
        .get(DerivedPropertyElement::KEY)
        .ok_or_else(|| anyhow!("the request carries no derived data"))?;
    for name in derived_properties {
        print(out, &format!("derived.{name}"), &read(derived, name))?;
    }
    writeln!(out)?;
    Ok(())
}

/// One derived property in words, being its value or the reason it has
/// none.
fn read(derived: &DerivedData, name: &str) -> String {
    match derived.get(name) {
        Ok(value) => examples_shared::property_value_to_string(&value),
        Err(no_value) => format!("no value, because {}", no_value.message),
    }
}

/// Print the property each script defines, read from the element, so that
/// everything a script says about its property reaches the caller.
fn print_definitions(out: &mut dyn Write, element: &DerivedPropertyElement) -> Result<()> {
    writeln!(out, "The property metadata the element gives the pipeline.")?;
    for property in element.properties() {
        writeln!(
            out,
            "  {}, type {:?}, category {}, available {}",
            property.name, property.value_type, property.category, property.available
        )?;
    }
    writeln!(out)?;

    for script in element.scripts() {
        let output = script.output();
        writeln!(
            out,
            "The full definition the script '{}' carries.",
            script.name()
        )?;
        print(out, "Name", &output.name)?;
        print(out, "Description", &output.description)?;
        print(out, "ValueType", &output.value_type.to_string())?;
        print(out, "DefaultValue", &given(output.default_value.as_ref()))?;
        print(out, "Category", &given(output.category.as_ref()))?;
        print(out, "IsList", &output.is_list.to_string())?;
        print(out, "IsMandatory", &given(output.is_mandatory))?;
        print(out, "IsObsolete", &given(output.is_obsolete))?;
        print(out, "IsPopular", &given(output.is_popular))?;
        print(out, "ExportValues", &given(output.export_values))?;
        print(out, "Dependencies", &output.dependencies.join(", "))?;
        if let Some(values) = &output.values {
            writeln!(out, "  Values")?;
            for value in values {
                writeln!(
                    out,
                    "    {:<10} {}",
                    value.name,
                    value.description.as_deref().unwrap_or_default()
                )?;
            }
        }
        writeln!(out)?;
    }
    Ok(())
}

fn print(out: &mut dyn Write, name: &str, value: &str) -> io::Result<()> {
    writeln!(out, "  {name:<33} {value}")
}

/// A source property as text, so that an absent one reads as words.
fn supplied<T: ToString>(value: Option<T>) -> String {
    value.map_or_else(
        || "not supplied on this request".to_owned(),
        |value| value.to_string(),
    )
}

/// A part of a property definition as text, where the script may leave it
/// out.
fn given<T: ToString>(value: Option<T>) -> String {
    value.map_or_else(|| "not given".to_owned(), |value| value.to_string())
}

/// Run the example and end with where to find out more.
fn main() -> Result<()> {
    let result = run(&mut io::stdout().lock());
    examples_shared::print_find_out_more(FIND_OUT_MORE, REPOSITORIES);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The heading of the part made with the builder in code.
    const FROM_CODE: &str = "2. Configured with the builder in code";

    /// The message a script gives when `session.PointerMoved` is absent, up
    /// to the sentence about the usual causes.
    fn missing(property: &str) -> String {
        format!(
            "no value, because Derived property '{property}' has no value because 1 \
             source property was not available. 'session.PointerMoved' (element \
             'session' has no value for 'PointerMoved': property not present on this \
             request)."
        )
    }

    /// What the example prints, as the part made from the configuration file
    /// and the part made in code.
    fn parts() -> (String, String) {
        let mut out = Vec::new();
        run(&mut out).expect("the example runs with no key, data file or network");
        let text = String::from_utf8(out).expect("the output is text");
        let (configured, coded) = text
            .split_once(FROM_CODE)
            .expect("the output has a part made in code");
        (configured.to_owned(), coded.to_owned())
    }

    /// What each request printed for `name`, in order.
    fn printed(part: &str, name: &str) -> Vec<String> {
        part.lines()
            .filter_map(|line| line.trim_start().split_once(' '))
            .filter(|(first, _)| *first == name)
            .map(|(_, rest)| rest.trim().to_owned())
            .collect()
    }

    #[test]
    fn the_configuration_file_makes_the_pipeline_and_the_script_file_gives_a_value() {
        let (configured, _) = parts();
        assert!(
            configured.contains("1. Configured from derived-property-options.json"),
            "{configured}"
        );
        let values = printed(&configured, "derived.ReaderEngagement");
        assert_eq!(values.len(), 2, "{configured}");
        assert_eq!(values[0], "High");
    }

    #[test]
    fn an_absent_source_property_is_shown_as_no_value_naming_what_was_missing() {
        let (configured, coded) = parts();
        for part in [&configured, &coded] {
            assert_eq!(
                printed(part, "session.PointerMoved"),
                ["true", "not supplied on this request"],
                "{part}"
            );
            let values = printed(part, "derived.ReaderEngagement");
            assert!(
                values[1].starts_with(&missing(ENGAGEMENT_PROPERTY)),
                "{}",
                values[1]
            );
        }
    }

    #[test]
    fn the_builder_runs_a_script_from_a_file_and_one_held_as_text() {
        let (_, coded) = parts();
        assert_eq!(printed(&coded, "derived.ReaderEngagement")[0], "High");
        let every_test = printed(&coded, "derived.ReaderEngagementIsHigh");
        assert_eq!(every_test.len(), 2, "{coded}");
        assert_eq!(every_test[0], "true");
        assert!(
            every_test[1].starts_with(&missing(EVERY_TEST_PROPERTY)),
            "{}",
            every_test[1]
        );
    }

    #[test]
    fn the_definition_the_script_file_carries_is_read_from_the_element() {
        let (configured, _) = parts();
        assert!(
            configured
                .contains("  ReaderEngagement, type String, category Example, available true"),
            "{configured}"
        );
        assert_eq!(printed(&configured, "DefaultValue"), ["Low"]);
        assert_eq!(
            printed(&configured, "Dependencies"),
            ["session.PagesViewed, session.SecondsSincePageLoad, session.PointerMoved"]
        );
    }

    #[test]
    fn the_configuration_file_names_no_built_in_script() {
        // The example must not depend on the scripts the crate ships, so
        // the element the configuration makes runs the one script beside
        // the example and nothing else.
        std::env::set_current_dir(example_folder()).expect("the example's folder is there");
        let text = std::fs::read_to_string(OPTIONS_FILE).expect("the file is read");
        let options = PipelineOptions::from_json(&text).expect("the file is sound");
        let pipeline = PipelineBuilder::new()
            .build_from_configuration(&registry(), &options)
            .expect("the pipeline is made");
        let derived = pipeline
            .resource::<DerivedPropertyElement>()
            .expect("the pipeline holds the element");
        let names: Vec<&str> = derived
            .scripts()
            .iter()
            .map(|script| script.name())
            .collect();
        assert_eq!(names, [ENGAGEMENT_PROPERTY]);
    }
}

/* ---------------------------------------------------------------------------
 * Example: Derived Property Element
 *
 * The derived property element computes a property from properties other
 * elements in the pipeline have already produced, following a script written
 * in YAML or in JSON. It holds no data file, sends no request over the
 * network and needs no resource key, so this example runs anywhere.
 *
 * What it shows
 * -------------
 *   1. A pipeline built from a configuration file,
 *      `derived-property-options.json` beside this file, which names the
 *      builder of each element and gives the script file through the
 *      `ScriptFiles` build parameter.
 *
 *   2. A script read from a file beside the example,
 *      `ReaderEngagement.yaml`, named in the configuration file and again on
 *      the builder in code with `add_script_file`.
 *
 *   3. A script held in the example itself and passed to the builder as text
 *      with `add_script`, so one element runs two scripts.
 *
 *   4. What the caller sees when a source property a script names is not
 *      available on a request. The second request of each part leaves out the
 *      evidence for `session.PointerMoved`, and each derived property then has
 *      no value and a message naming the property that was missing.
 *
 * Where the source properties come from
 * -------------------------------------
 * `SessionElement` in this file reads three items of evidence and gives each
 * one it finds as a property under the key `session`. A real pipeline puts
 * Device Detection or IP Intelligence in its place. The derived property
 * element is added after it, and building the pipeline fails, naming the
 * property, when no element supplies a property a script reads.
 *
 * The scripts 51Degrees ships
 * ---------------------------
 * The `fiftyone-derived-properties` crate also carries the scripts in
 * https://github.com/51Degrees/derived-properties, added with `add_built_in`
 * or the `Scripts` build parameter. This example uses none of them, so it
 * does not change when one of them does.
 *
 * Running it
 * ----------
 *   cargo run -p pipeline-examples --bin derived-property
 * ------------------------------------------------------------------------- */
