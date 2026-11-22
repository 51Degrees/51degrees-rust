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

//! Runs the demo binary with no Resource Key, as someone who has just cloned
//! the repository would.

use std::process::Command;

#[test]
fn with_no_key_it_says_how_to_get_one_and_ends_with_find_out_more() {
    let mut command = Command::new(env!("CARGO_BIN_EXE_human-confidence-web"));
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("_51DEGREES_") {
            command.env_remove(name);
        }
    }
    let output = command.output().expect("the demo binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success(), "exit {:?}", output.status.code());
    assert!(
        stderr.contains("_51DEGREES_RESOURCE_KEY"),
        "no variable named in {stderr}"
    );
    assert!(
        stderr.contains(
            "https://51degrees.com/configure?utm_source=code&utm_medium=example\
             &utm_campaign=51degrees-rust\
             &utm_content=examples-human-confidence-examples-src-bin-human-confidence-web.rs\
             &utm_term=resource-key-required"
        ),
        "no link to create a Resource Key in {stderr}"
    );
    let section = stdout
        .rfind(examples_shared::FIND_OUT_MORE_HEADING)
        .expect("standard output has a Find out more section");
    let after = &stdout[section..];
    assert!(after.contains("utm_term=find-out-more"), "got {after}");
    for repository in examples_shared::CLOUD_REPOSITORIES {
        assert!(after.contains(repository), "got {after}");
    }
    assert!(
        after.trim_end().ends_with("----"),
        "the section is not the end of {stdout}"
    );
}
