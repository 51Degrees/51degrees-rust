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

//! The compiled in sources are the data repository's translation files.
//!
//! The data repository is the `data` submodule of the `cxx` submodule. Under
//! GitHub Actions it must be there, so a checkout without it fails rather
//! than skipping the check. WebAssembly tests cannot read the folder.

#![cfg(not(target_family = "wasm"))]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use fiftyone_ip_intelligence_translation::{COUNTRY_CODE_SOURCES, COUNTRY_SOURCES};

/// The translation folder of the data repository, or `None` when it is not
/// checked out.
fn data_folder() -> Option<PathBuf> {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("cxx")
        .join("data")
        .join("ip-intelligence")
        .join("Translations");
    if folder.is_dir() {
        return Some(folder);
    }
    assert!(
        std::env::var_os("GITHUB_ACTIONS").is_none(),
        "The data repository's translations are not at {}. Check out the \
         submodules recursively.",
        folder.display()
    );
    eprintln!(
        "The data repository's translations are not at {}, so the compiled \
         in sources are not checked.",
        folder.display()
    );
    None
}

/// The names of the YAML files in a folder.
fn yaml_files(folder: &Path) -> BTreeSet<String> {
    fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".yml") || name.ends_with(".yaml"))
        .collect()
}

#[test]
fn each_source_is_the_data_repository_file_byte_for_byte() {
    let Some(folder) = data_folder() else {
        return;
    };
    for (sources, subfolder) in [
        (&COUNTRY_CODE_SOURCES[..], folder.clone()),
        (&COUNTRY_SOURCES[..], folder.join("OSM")),
    ] {
        let names: BTreeSet<String> = sources.iter().map(|(name, _)| (*name).to_owned()).collect();
        assert_eq!(
            names,
            yaml_files(&subfolder),
            "the sources and the files in {}",
            subfolder.display()
        );
        for (name, text) in sources {
            let file = fs::read(subfolder.join(name)).unwrap();
            assert!(
                file == text.as_bytes(),
                "{name} differs from the data repository's copy"
            );
        }
    }
}
