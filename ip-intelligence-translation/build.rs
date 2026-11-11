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

//! Build script for `fiftyone-ip-intelligence-translation`.
//!
//! Sets `FIFTYONE_TRANSLATIONS_DIR` to the folder the translation sources are
//! compiled in from, which holds the IP Intelligence translation files of the
//! 51Degrees data repository.
//!
//! In the repository that is `cxx/data/ip-intelligence/Translations`, the
//! data repository being the `data` submodule of the `cxx` submodule. Cargo
//! does not package files from outside a crate's folder, so
//! `ci/copy-translations.ps1` copies them into `translations` inside this
//! crate before publishing, and the published crate reads them from there.

use std::path::PathBuf;

/// The variable the crate reads the folder from.
const FOLDER_VARIABLE: &str = "FIFTYONE_TRANSLATIONS_DIR";

/// A file every translation folder holds.
const MARKER: &str = "countrycodes.en_GB.yml";

fn main() {
    let manifest =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"));
    let submodule = manifest
        .parent()
        .expect("the crate folder has a parent")
        .join("cxx")
        .join("data")
        .join("ip-intelligence")
        .join("Translations");
    let copied = manifest.join("translations");
    let folder = [submodule, copied]
        .into_iter()
        .find(|folder| folder.join(MARKER).is_file())
        .expect(
            "could not find the IP Intelligence translations. Run \
             'git submodule update --init --recursive' to check out the data \
             repository inside the cxx submodule",
        );
    // Cargo scans a folder for any change, so a new commit of the data
    // repository that changes a translation compiles it in.
    println!("cargo:rerun-if-changed={}", folder.display());
    println!("cargo:rustc-env={FOLDER_VARIABLE}={}", folder.display());
}
