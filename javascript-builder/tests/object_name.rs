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

//! Tests for the client side object's name.
//!
//! The name can be set on the builder (`set_object_name`) or per request
//! (`query.fod-js-object-name`). The name is written into the script as a
//! variable name, a session storage key and a property name, so these tests
//! check that a different name works, and that a name which is not a
//! JavaScript identifier is never written into the script.
//!
//! The script is checked with `node --check` and run in Node with a minimal
//! stand in for a browser (`object_name_harness.js`). Both need Node on the
//! path, which every GitHub hosted runner has. Without Node those checks are
//! skipped with a message.
//!
//! The warning is captured by a logger installed once for this test binary.
//! Each entry records the thread that logged it, because Rust runs tests in
//! parallel and each test only reads its own entries.

use std::any::Any;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, ThreadId};

use fiftyone_javascript_builder::{
    JavaScriptBuilderElement, JavaScriptBuilderElementBuilder, EVIDENCE_OBJECT_NAME,
    JAVASCRIPT_BUILDER_DATA_KEY,
};
use fiftyone_json_builder::JsonBuilderElement;
use fiftyone_pipeline_core::{
    ElementData, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    NoValueError, Pipeline, PropertyMetaData, PropertyValue, PropertyValueType, Result, TypedKey,
};
use fiftyone_pipeline_engines_fiftyone::SequenceElement;

// ---------------------------------------------------------------------------
// An element with one value, so the payload can be read back from the object.
// ---------------------------------------------------------------------------

struct ValueData;

impl ElementData for ValueData {
    fn get(&self, name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        if name.eq_ignore_ascii_case("answer") {
            Ok(PropertyValue::String("42".to_owned()))
        } else {
            Err(NoValueError::new(format!(
                "No value for property '{name}'."
            )))
        }
    }

    fn keys(&self) -> Vec<String> {
        vec!["answer".to_owned()]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

struct ValueElement {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

impl ValueElement {
    const KEY: TypedKey<ValueData> = TypedKey::new("value");

    fn new() -> Self {
        ValueElement {
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            properties: vec![PropertyMetaData::new(
                "answer",
                "value",
                PropertyValueType::String,
            )],
        }
    }
}

impl FlowElement for ValueElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        data.get_or_add(Self::KEY, || ValueData)?;
        Ok(())
    }

    fn data_key(&self) -> &str {
        "value"
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

// ---------------------------------------------------------------------------
// A logger that keeps warnings with the thread that logged them.
// ---------------------------------------------------------------------------

struct CapturingLogger {
    entries: Mutex<Vec<(ThreadId, String)>>,
}

impl log::Log for CapturingLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            self.entries
                .lock()
                .unwrap()
                .push((thread::current().id(), record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

fn logger() -> &'static CapturingLogger {
    static LOGGER: OnceLock<&'static CapturingLogger> = OnceLock::new();
    LOGGER.get_or_init(|| {
        let logger: &'static CapturingLogger = Box::leak(Box::new(CapturingLogger {
            entries: Mutex::new(Vec::new()),
        }));
        log::set_logger(logger).expect("no other logger is installed in this binary");
        log::set_max_level(log::LevelFilter::Warn);
        logger
    })
}

/// The warnings the current thread has logged.
fn warnings_for_this_thread() -> Vec<String> {
    let me = thread::current().id();
    logger()
        .entries
        .lock()
        .unwrap()
        .iter()
        .filter(|(id, _)| *id == me)
        .map(|(_, message)| message.clone())
        .collect()
}

// ---------------------------------------------------------------------------
// Rendering and Node.
// ---------------------------------------------------------------------------

fn render(
    configure: impl FnOnce(JavaScriptBuilderElementBuilder) -> JavaScriptBuilderElement,
    evidence: &[(&str, &str)],
) -> String {
    // The sequence element leads, because the JSON builder needs the sequence
    // number it writes.
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(SequenceElement::new()))
        .add_element(Arc::new(ValueElement::new()))
        .add_element(Arc::new(JsonBuilderElement::new()))
        .add_element(Arc::new(configure(JavaScriptBuilderElement::builder())))
        .build()
        .expect("pipeline builds");
    // A host gives the script a callback URL, which renders the update
    // section that holds the page evidence lookup.
    let mut ev = Evidence::builder().add("header.host", "localhost");
    for (key, value) in evidence {
        ev = ev.add(*key, *value);
    }
    let mut data = pipeline.create_flow_data_with(ev.build());
    data.process().expect("processing succeeds");
    data.get(JAVASCRIPT_BUILDER_DATA_KEY)
        .expect("javascript builder data present")
        .javascript()
        .to_owned()
}

fn node_available() -> bool {
    let available = Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !available {
        eprintln!("Node is not on the path, so the script is not parsed or run");
    }
    available
}

/// Writes the script to a file of its own under the target directory.
fn write_script(script: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let path = dir.join(format!(
        "object-name-{}-{}.js",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, script).expect("script written");
    path
}

fn assert_parses(script: &str) {
    if !node_available() {
        return;
    }
    let path = write_script(script);
    let output = Command::new("node")
        .arg("--check")
        .arg(&path)
        .output()
        .expect("node runs");
    let _ = std::fs::remove_file(&path);
    assert!(
        output.status.success(),
        "the rendered script does not parse: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Runs the script in the harness and returns its JSON report, or `None`
/// when Node is not available.
fn run_script(script: &str, name: &str) -> Option<serde_json::Value> {
    if !node_available() {
        return None;
    }
    let path = write_script(script);
    let harness = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("object_name_harness.js");
    let output = Command::new("node")
        .arg(&harness)
        .arg(&path)
        .arg(name)
        .arg("value.answer")
        .output()
        .expect("node runs");
    let _ = std::fs::remove_file(&path);
    assert!(
        output.status.success(),
        "the harness failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Some(serde_json::from_slice(&output.stdout).expect("the harness prints JSON"))
}

fn is_identifier_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

/// The names declared with `var <name> = new fiftyoneDegreesManager`.
fn declared_names(script: &str) -> Vec<String> {
    let mut names = Vec::new();
    for (at, _) in script.match_indices("new fiftyoneDegreesManager") {
        let before = script[..at].trim_end();
        let Some(before) = before.strip_suffix('=') else {
            continue;
        };
        let before = before.trim_end();
        let Some(var_at) = before.rfind("var") else {
            continue;
        };
        let name = before[var_at + 3..].trim();
        names.push(name.to_owned());
    }
    names
}

/// True if the script declares `var fod` anywhere.
fn declares_default_name(script: &str) -> bool {
    script.match_indices("var fod").any(|(at, text)| {
        let starts_word = script[..at]
            .chars()
            .last()
            .is_none_or(|c| !is_identifier_char(c));
        let ends_word = script[at + text.len()..]
            .chars()
            .next()
            .is_none_or(|c| !is_identifier_char(c));
        starts_word && ends_word
    })
}

/// The template the crate renders. Parts of the checks depend on what the
/// template contains, so they apply once the template is refreshed.
const TEMPLATE: &str = include_str!("../assets/JavaScriptResource.mustache");

fn assert_uses_name(script: &str, name: &str) {
    assert_eq!(declared_names(script), vec![name.to_owned()]);
    if name != "fod" {
        assert!(
            !declares_default_name(script),
            "the default name must not be declared as well"
        );
    }
    assert!(script.contains(&format!("var sessionKey = \"{name}\";")));
    assert!(script.contains(&format!("window[\"{name}Evidence\"]")));
    if TEMPLATE.contains("typeof window[\"{{_objName}}\"]") {
        // The warning printed when the script is loaded twice.
        assert!(script.contains(&format!("typeof window[\"{name}\"]")));
    }
}

fn assert_runs_as(script: &str, name: &str) {
    let Some(result) = run_script(script, name) else {
        return;
    };
    assert!(result["error"].is_null(), "the script threw: {result}");
    assert_eq!(result["exists"], true, "window.{name} was not created");
    assert_eq!(result["complete"], "function");
    assert_eq!(result["onChange"], "function");
    if TEMPLATE.contains("this.refresh") {
        assert_eq!(result["refresh"], "function");
    }
    assert_eq!(result["value"], "\"42\"");
    assert_eq!(result["globals"], serde_json::json!([name]));
}

// ---------------------------------------------------------------------------
// Tests.
// ---------------------------------------------------------------------------

#[test]
fn valid_name_from_builder_is_used() {
    let script = render(
        |b| {
            b.set_object_name("myFod")
                .unwrap()
                .set_minify(false)
                .build()
        },
        &[],
    );
    assert_uses_name(&script, "myFod");
    assert_parses(&script);
    assert_runs_as(&script, "myFod");
}

#[test]
fn valid_name_from_evidence_is_used() {
    // The capture keys warnings by thread id, and a target without threads
    // (wasm32-wasip1) runs every test in this binary on the one thread, so
    // the warnings other tests logged are still visible here. Count what is
    // already there and assert this test adds nothing, as the checks for an
    // invalid name do.
    let before = warnings_for_this_thread().len();
    let script = render(
        |b| b.set_minify(false).build(),
        &[(EVIDENCE_OBJECT_NAME, "myFod")],
    );
    assert_uses_name(&script, "myFod");
    assert_parses(&script);
    assert_runs_as(&script, "myFod");
    assert_eq!(warnings_for_this_thread().len(), before);
}

#[test]
fn valid_name_from_evidence_minified() {
    let script = render(|b| b.build(), &[(EVIDENCE_OBJECT_NAME, "myFod")]);
    assert_eq!(declared_names(&script), vec!["myFod".to_owned()]);
    assert!(!declares_default_name(&script));
    assert_parses(&script);
    assert_runs_as(&script, "myFod");
}

/// A requested name that is not a JavaScript identifier is not written into
/// the script. The configured name is used, the script parses and runs, and
/// a warning is logged.
fn check_invalid_name_is_ignored(name: &str, check_text: bool) {
    let before = warnings_for_this_thread().len();
    let script = render(
        |b| b.set_minify(false).build(),
        &[(EVIDENCE_OBJECT_NAME, name)],
    );

    assert_uses_name(&script, "fod");
    assert_parses(&script);

    if check_text {
        // The requested value is still one of the request's query values,
        // which the script carries URL encoded as data inside a JSON string,
        // on the line that holds the parameters object. Every other line
        // must be free of it.
        assert!(
            !script
                .lines()
                .filter(|line| !line.contains("\"fod-js-object-name\":"))
                .any(|line| line.contains(name)),
            "the requested name was written into the script"
        );
    }

    assert_runs_as(&script, "fod");

    let warnings = warnings_for_this_thread();
    let new: Vec<_> = warnings[before..]
        .iter()
        .filter(|w| w.contains(EVIDENCE_OBJECT_NAME))
        .collect();
    assert_eq!(new.len(), 1, "{warnings:?}");
    if check_text {
        assert!(!new[0].contains(name));
    }
}

#[test]
fn invalid_name_with_a_statement_is_ignored() {
    check_invalid_name_is_ignored("a;b", true);
}

#[test]
fn invalid_name_with_a_leading_digit_is_ignored() {
    check_invalid_name_is_ignored("9bad", true);
}

#[test]
fn invalid_name_with_a_quote_is_ignored() {
    check_invalid_name_is_ignored("x\"y", true);
}

#[test]
fn empty_name_is_ignored() {
    check_invalid_name_is_ignored("", false);
}

#[test]
fn reserved_word_name_is_ignored() {
    // A reserved word is also an ordinary word in the script's comments, so
    // only the places the name is written are checked for it.
    check_invalid_name_is_ignored("class", false);
}

// A top level var cannot replace these three global values, so the object
// would never be created. Each is also an ordinary word in the script.

#[test]
fn infinity_name_is_ignored() {
    check_invalid_name_is_ignored("Infinity", false);
}

#[test]
fn nan_name_is_ignored() {
    check_invalid_name_is_ignored("NaN", false);
}

#[test]
fn undefined_name_is_ignored() {
    check_invalid_name_is_ignored("undefined", false);
}

#[test]
fn constructor_name_is_ignored() {
    // The script defines a constructor with this name, which the object
    // would replace.
    check_invalid_name_is_ignored("fiftyoneDegreesManager", false);
}

#[test]
fn invalid_name_from_evidence_uses_configured_name() {
    let script = render(
        |b| {
            b.set_object_name("myFod")
                .unwrap()
                .set_minify(false)
                .build()
        },
        &[(EVIDENCE_OBJECT_NAME, "9bad")],
    );
    assert_uses_name(&script, "myFod");
    assert_parses(&script);
}

fn assert_configured_name_is_refused(name: &str) {
    assert!(
        JavaScriptBuilderElement::builder()
            .set_object_name(name)
            .is_err(),
        "{name:?} was accepted"
    );
}

#[test]
fn invalid_configured_name_is_refused() {
    for name in ["a;b", "9bad", "x\"y", "", "var", "fod\n", "caf\u{e9}"] {
        assert_configured_name_is_refused(name);
    }
}

#[test]
fn configured_infinity_is_refused() {
    assert_configured_name_is_refused("Infinity");
}

#[test]
fn configured_nan_is_refused() {
    assert_configured_name_is_refused("NaN");
}

#[test]
fn configured_undefined_is_refused() {
    assert_configured_name_is_refused("undefined");
}

#[test]
fn configured_constructor_name_is_refused() {
    assert_configured_name_is_refused("fiftyoneDegreesManager");
}

#[test]
fn valid_configured_name_is_accepted() {
    for name in ["myFod", "_fod", "$fod9", "classy"] {
        assert!(
            JavaScriptBuilderElement::builder()
                .set_object_name(name)
                .is_ok(),
            "{name:?} was refused"
        );
    }
}
