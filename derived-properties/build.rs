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

//! Build script for `fiftyone-derived-properties`.
//!
//! Sets `_51DEGREES_DERIVED_PROPERTIES_COMMIT` to the commit of
//! 51Degrees/derived-properties that the scripts under `vendor` come from.
//!
//! In the repository `vendor` is a submodule, and git gives the commit it is
//! checked out at. A crate from crates.io has no git, so
//! `ci/record-derived-properties-commit.ps1` writes the commit to
//! `SOURCE.txt` before publishing and this reads it from there. Where neither
//! gives one the commit is `unknown`, as in .NET.

use std::path::Path;
use std::process::Command;

/// The variable the crate reads the commit from.
const COMMIT_VARIABLE: &str = "_51DEGREES_DERIVED_PROPERTIES_COMMIT";

/// What the commit is where nothing records it.
const UNKNOWN: &str = "unknown";

/// The start of the line of `SOURCE.txt` that holds the commit.
const COMMIT_LINE: &str = "commit=";

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let manifest = Path::new(&manifest);
    let commit = submodule_commit(&manifest.join("vendor"))
        .or_else(|| recorded_commit(&manifest.join("SOURCE.txt")))
        .unwrap_or_else(|| {
            println!(
                "cargo:warning=The derived-properties commit is unknown, because \
                 vendor is not a git checkout and there is no SOURCE.txt."
            );
            UNKNOWN.to_owned()
        });
    println!("cargo:rustc-env={COMMIT_VARIABLE}={commit}");
}

/// The commit the submodule is checked out at, or `None` where `vendor` is
/// not a git checkout or git cannot be run.
fn submodule_commit(vendor: &Path) -> Option<String> {
    // Without this check git would search the folders above and answer with
    // the commit of whatever repository holds an unpacked crate.
    let marker = vendor.join(".git");
    if !marker.exists() {
        return None;
    }
    println!("cargo:rerun-if-changed={}", marker.display());
    // HEAD changes whenever the submodule moves, even to a commit whose
    // scripts are the same, so the commit is read again after every move.
    match git(
        vendor,
        &["rev-parse", "--path-format=absolute", "--git-path", "HEAD"],
    ) {
        Some(head) => println!("cargo:rerun-if-changed={head}"),
        None => println!("cargo:rerun-if-changed={}", vendor.display()),
    }
    git(vendor, &["rev-parse", "HEAD"])
}

/// The commit `SOURCE.txt` records, or `None` where there is no such file.
fn recorded_commit(source: &Path) -> Option<String> {
    let text = std::fs::read_to_string(source).ok()?;
    println!("cargo:rerun-if-changed={}", source.display());
    text.lines()
        .find_map(|line| line.trim().strip_prefix(COMMIT_LINE))
        .map(|commit| commit.trim().to_owned())
        .filter(|commit| !commit.is_empty())
}

/// What git prints for `args` in `folder`, or `None` if it fails.
fn git(folder: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(folder)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (!text.is_empty()).then_some(text)
}
