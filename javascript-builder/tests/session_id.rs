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

//! Tests for the session id and sequence the script is given.
//!
//! The session id is written into the script inside quotes and the sequence
//! as bare code, so a value that is not safe would break the script or change
//! what it does. These tests render the script through a pipeline and check
//! what reaches it, then parse the result with `node --check`. Node is on
//! every GitHub hosted runner, and without it the parse check is skipped with
//! a message.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use fiftyone_javascript_builder::{JavaScriptBuilderElement, JAVASCRIPT_BUILDER_DATA_KEY};
use fiftyone_json_builder::JsonBuilderElement;
use fiftyone_pipeline_core::{Evidence, Pipeline};

/// Render the script for the supplied evidence. The pipeline is the JSON
/// builder and the JavaScript builder, with a host so the callback URL and
/// the sections that use it are rendered.
///
/// Errors are suppressed because the JSON builder fails on a sequence it
/// cannot read, and these tests look only at the JavaScript builder's script.
fn render(evidence: &[(&str, &str)]) -> String {
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(JsonBuilderElement::new()))
        .add_element(Arc::new(
            JavaScriptBuilderElement::builder()
                .set_minify(false)
                .build(),
        ))
        .suppress_process_exceptions(true)
        .build()
        .expect("pipeline builds");
    let mut ev = Evidence::builder().add("header.host", "localhost");
    for (key, value) in evidence {
        ev = ev.add(*key, *value);
    }
    // The JSON builder needs a sequence number, which a Sequence element
    // would otherwise give.
    if !evidence.iter().any(|(key, _)| *key == "query.sequence") {
        ev = ev.add("query.sequence", "1");
    }
    let mut data = pipeline.create_flow_data_with(ev.build());
    data.process().expect("errors are suppressed");
    data.get(JAVASCRIPT_BUILDER_DATA_KEY)
        .expect("javascript builder data present")
        .javascript()
        .to_owned()
}

/// The `var <name> = ...;` lines the script holds, trimmed.
fn lines(script: &str, name: &str) -> Vec<String> {
    let start = format!("var {name} =");
    script
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with(&start))
        .map(str::to_owned)
        .collect()
}

fn node_available() -> bool {
    let available = Command::new("node")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !available {
        eprintln!("Node is not on the path, so the script is not parsed");
    }
    available
}

fn assert_parses(script: &str) {
    if !node_available() {
        return;
    }
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "session-id-{}-{}.js",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, script).expect("script written");
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

#[test]
fn safe_session_id_is_rendered() {
    let script = render(&[("query.session-id", "abc-123")]);

    assert_eq!(
        lines(&script, "sessionId"),
        ["var sessionId = \"abc-123\";"]
    );
    assert_parses(&script);
}

#[test]
fn longest_session_id_is_rendered() {
    let session_id = "a".repeat(64);
    let script = render(&[("query.session-id", &session_id)]);

    assert_eq!(
        lines(&script, "sessionId"),
        [format!("var sessionId = \"{session_id}\";")]
    );
    assert_parses(&script);
}

#[test]
fn unsafe_session_id_is_rendered_empty() {
    // The template's own text holds "a b" and "ab", so for those two only the
    // rendered session id is read.
    let cases: [(&str, bool); 7] = [
        ("a\"b", true),
        ("a\\b", true),
        ("</script>", true),
        ("caf\u{e9}", true),
        ("", false),
        ("a b", false),
        ("ab\n", false),
    ];
    let longest = "a".repeat(65);
    for (value, check_text) in cases.iter().copied().chain([(longest.as_str(), true)]) {
        let script = render(&[("query.session-id", value)]);

        assert_eq!(
            lines(&script, "sessionId"),
            ["var sessionId = \"\";"],
            "{value:?} reached the script"
        );
        if check_text && !value.is_empty() {
            assert!(!script.contains(value), "{value:?} reached the script");
        }
        assert_parses(&script);
    }
}

#[test]
fn no_session_id_is_rendered_empty() {
    let script = render(&[]);

    assert_eq!(lines(&script, "sessionId"), ["var sessionId = \"\";"]);
    assert_eq!(lines(&script, "sequence"), ["var sequence = 1;"]);
    assert_parses(&script);
}

#[test]
fn unusable_sequence_is_rendered_as_one() {
    for value in [
        "abc",
        "-1",
        "0",
        "99999999999",
        "",
        "1;b",
        "1.5",
        "2147483648",
    ] {
        let script = render(&[("query.session-id", "abc"), ("query.sequence", value)]);

        assert_eq!(
            lines(&script, "sequence"),
            ["var sequence = 1;"],
            "{value:?} reached the script"
        );
        assert_parses(&script);
    }
}

#[test]
fn usable_sequence_is_rendered() {
    for (value, expected) in [("7", 7), (" 7 ", 7), ("2147483647", 2147483647)] {
        let script = render(&[("query.session-id", "abc"), ("query.sequence", value)]);

        assert_eq!(
            lines(&script, "sequence"),
            [format!("var sequence = {expected};")]
        );
        assert_parses(&script);
    }
}
