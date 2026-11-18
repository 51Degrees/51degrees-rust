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

//! The thread-safety specification asks for a check, run by the automated
//! tests, that processes every Flow Element in the repository, then writes
//! to its Element Data and expects the error. Each Flow Element has that
//! check in a test beside it. This test finds every Flow Element that the
//! crates and the examples implement, and fails when one has no such test,
//! so a Flow Element cannot be added without one.
//!
//! Test doubles are left out. They are the Flow Elements in `tests` and
//! `benches` folders, in `#[cfg(test)]` modules, whose items are indented,
//! and in the `parity` folder, which only compares answers with the .NET
//! packages.

// The test reads the repository's files, which WebAssembly cannot reach.
#![cfg(not(target_family = "wasm"))]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The name that most of the test files below give their read only check.
const READ_ONLY_TEST: &str = "its_element_data_is_read_only_once_processed";

/// Each Flow Element, as the file that implements it and its type, with the
/// file and the name of the test that checks its Element Data is read only
/// once it has finished processing.
const CHECKED: &[(&str, &str, &str, &str)] = &[
    (
        "agent-signature/src/element.rs",
        "AgentSignatureElement",
        "agent-signature/tests/element.rs",
        READ_ONLY_TEST,
    ),
    (
        "cloud-request-engine/src/engine.rs",
        "CloudRequestEngine",
        "cloud-request-engine/tests/mock_client.rs",
        READ_ONLY_TEST,
    ),
    (
        "derived-properties/src/element.rs",
        "DerivedPropertyElement",
        "derived-properties/tests/element.rs",
        READ_ONLY_TEST,
    ),
    (
        "device-detection-cloud/src/engine.rs",
        "DeviceDetectionCloudEngine",
        "device-detection-cloud/tests/cloud_engine.rs",
        READ_ONLY_TEST,
    ),
    (
        "device-detection-cloud/src/hardware_engine.rs",
        "HardwareProfileCloudEngine",
        "device-detection-cloud/tests/hardware_engine.rs",
        READ_ONLY_TEST,
    ),
    (
        "device-detection-onpremise/src/engine.rs",
        "DeviceDetectionOnPremiseEngine",
        "device-detection-onpremise/tests/detection.rs",
        READ_ONLY_TEST,
    ),
    (
        "device-detection-property-keyed/src/engine.rs",
        "PropertyKeyedDeviceEngine",
        "device-detection-property-keyed/tests/data_file.rs",
        READ_ONLY_TEST,
    ),
    (
        "device-detection-robots-txt/src/engine.rs",
        "RobotsTxtEngine",
        "device-detection-robots-txt/tests/data_file.rs",
        READ_ONLY_TEST,
    ),
    (
        "device-detection-shared/src/uach.rs",
        "UachJsConversionElement",
        "device-detection-shared/src/uach.rs",
        READ_ONLY_TEST,
    ),
    (
        "fodid-cloud/src/engine.rs",
        "FodIdCloudEngine",
        "fodid-cloud/tests/cloud_fodid.rs",
        READ_ONLY_TEST,
    ),
    (
        "ip-intelligence-cloud/src/engine.rs",
        "IpIntelligenceCloudEngine",
        "ip-intelligence-cloud/tests/areas.rs",
        READ_ONLY_TEST,
    ),
    (
        "ip-intelligence-onpremise/src/engine.rs",
        "IpIntelligenceOnPremiseEngine",
        "ip-intelligence-onpremise/tests/detection.rs",
        READ_ONLY_TEST,
    ),
    (
        "ip-intelligence-translation/src/cloud.rs",
        "CloudCountriesTranslationEngine",
        "ip-intelligence-translation/tests/cloud.rs",
        READ_ONLY_TEST,
    ),
    (
        "ip-intelligence-translation/src/countries.rs",
        "CountriesTranslationEngine",
        "ip-intelligence-translation/tests/engines.rs",
        READ_ONLY_TEST,
    ),
    (
        "ip-intelligence-translation/src/country_code.rs",
        "CountryCodeTranslationEngine",
        "ip-intelligence-translation/tests/engines.rs",
        READ_ONLY_TEST,
    ),
    (
        "javascript-builder/src/element.rs",
        "JavaScriptBuilderElement",
        "javascript-builder/tests/element_data_read_only.rs",
        READ_ONLY_TEST,
    ),
    (
        "json-builder/src/element.rs",
        "JsonBuilderElement",
        "json-builder/tests/element_data_read_only.rs",
        READ_ONLY_TEST,
    ),
    (
        "pipeline-engines-fiftyone/src/sequence_element.rs",
        "SequenceElement",
        "pipeline-engines-fiftyone/tests/element_data_read_only.rs",
        "each_elements_data_is_read_only_once_processed",
    ),
    (
        "pipeline-engines-fiftyone/src/set_headers_element.rs",
        "SetHeadersElement",
        "pipeline-engines-fiftyone/tests/element_data_read_only.rs",
        "each_elements_data_is_read_only_once_processed",
    ),
    (
        "pipeline-engines-fiftyone/src/share_usage_element.rs",
        "ShareUsageElement",
        "pipeline-engines-fiftyone/tests/element_data_read_only.rs",
        "each_elements_data_is_read_only_once_processed",
    ),
    (
        "pipeline-translation/src/engine.rs",
        "TranslationEngine",
        "pipeline-translation/tests/engine.rs",
        READ_ONLY_TEST,
    ),
    (
        "examples/pipeline-examples/src/bin/caching.rs",
        "CachedStarSignEngine",
        "examples/pipeline-examples/src/bin/caching.rs",
        READ_ONLY_TEST,
    ),
    (
        "examples/pipeline-examples/src/bin/derived-property.rs",
        "SessionElement",
        "examples/pipeline-examples/src/bin/derived-property.rs",
        READ_ONLY_TEST,
    ),
    (
        "examples/pipeline-examples/src/bin/ss-clientside.rs",
        "SimpleClientSideElement",
        "examples/pipeline-examples/src/bin/ss-clientside.rs",
        READ_ONLY_TEST,
    ),
    (
        "examples/pipeline-examples/src/bin/ss-cloud.rs",
        "StarSignCloudEngine",
        "examples/pipeline-examples/src/bin/ss-cloud.rs",
        READ_ONLY_TEST,
    ),
    (
        "examples/pipeline-examples/src/bin/ss-cloud.rs",
        "StubCloudRequestEngine",
        "examples/pipeline-examples/src/bin/ss-cloud.rs",
        READ_ONLY_TEST,
    ),
    (
        "examples/pipeline-examples/src/bin/ss-onprem.rs",
        "OnPremiseStarSignEngine",
        "examples/pipeline-examples/src/bin/ss-onprem.rs",
        READ_ONLY_TEST,
    ),
    (
        "examples/pipeline-examples/src/bin/ss-simple.rs",
        "SimpleStarSignElement",
        "examples/pipeline-examples/src/bin/ss-simple.rs",
        READ_ONLY_TEST,
    ),
    (
        "examples/pipeline-examples/src/bin/usage-sharing.rs",
        "SimpleStarSignElement",
        "examples/pipeline-examples/src/bin/usage-sharing.rs",
        READ_ONLY_TEST,
    ),
];

/// Folders that hold no Flow Element of the repository's own: build output,
/// submodules, test doubles and the comparison with the .NET packages.
const LEFT_OUT: &[&str] = &[
    ".git",
    "target",
    "cxx",
    "owid-rust",
    "javascript-templates",
    "vendor",
    "node_modules",
    "tests",
    "benches",
    "parity",
];

/// The workspace root, which holds this crate.
fn root() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate is in the workspace")
        .to_path_buf();
    let manifest = fs::read_to_string(root.join("Cargo.toml")).expect("the workspace manifest");
    assert!(
        manifest.contains("[workspace]"),
        "{} is not the workspace",
        root.display()
    );
    root
}

/// The type that `line` implements `FlowElement` for, when the line starts
/// an implementation at the top level of a file.
fn element_type(line: &str) -> Option<&str> {
    if !line.starts_with("impl") {
        return None;
    }
    let at = line.find("FlowElement for ")?;
    if !matches!(line[..at].chars().last(), Some(' ' | ':' | '>')) {
        return None;
    }
    let rest = &line[at + "FlowElement for ".len()..];
    let end = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

/// Every Flow Element under `root`, as the file that implements it, relative
/// to `root` with `/` between folders, and its type.
fn implemented(root: &Path) -> BTreeSet<(String, String)> {
    let mut found = BTreeSet::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(&folder).expect("the folder lists") {
            let path = entry.expect("the entry reads").path();
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if path.is_dir() {
                if !LEFT_OUT.contains(&name.as_ref()) {
                    folders.push(path);
                }
            } else if name.ends_with(".rs") {
                let text = fs::read_to_string(&path).expect("the file reads");
                let file = relative(root, &path);
                for line in text.lines() {
                    if let Some(element) = element_type(line) {
                        found.insert((file.clone(), element.to_owned()));
                    }
                }
            }
        }
    }
    found
}

/// `path` relative to `root`, with `/` between folders.
fn relative(root: &Path, path: &Path) -> String {
    let parts: Vec<String> = path
        .strip_prefix(root)
        .expect("the file is under the root")
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    parts.join("/")
}

#[test]
fn every_flow_element_is_listed_with_its_check() {
    let found = implemented(&root());
    let listed: BTreeSet<(String, String)> = CHECKED
        .iter()
        .map(|(file, name, _, _)| ((*file).to_owned(), (*name).to_owned()))
        .collect();
    let unlisted: Vec<_> = found.difference(&listed).collect();
    let gone: Vec<_> = listed.difference(&found).collect();
    assert!(
        unlisted.is_empty() && gone.is_empty(),
        "Flow Elements with no read only check listed: {unlisted:?}. Listed \
         but not found: {gone:?}"
    );
}

#[test]
fn each_listed_check_is_a_test_in_its_file() {
    let root = root();
    let missing: Vec<String> = CHECKED
        .iter()
        .filter(|(_, _, file, test)| {
            let text = fs::read_to_string(root.join(file)).unwrap_or_default();
            !text.contains(&format!("fn {test}()"))
        })
        .map(|(_, name, file, test)| format!("{name}: {file} has no test {test}"))
        .collect();
    assert!(missing.is_empty(), "{missing:#?}");
}
