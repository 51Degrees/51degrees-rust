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

//! A data file opens from a folder whose name holds letters outside ASCII.

#[allow(dead_code)]
#[path = "../../deprecated-names/environment.rs"]
mod deprecated_names;

use std::path::{Path, PathBuf};

use fiftyone_native::PerformanceProfile;

fn data_file(variable: &str) -> Option<PathBuf> {
    let path = PathBuf::from(crate::deprecated_names::var(variable).ok()?);
    path.is_file().then_some(path)
}

/// Copy `source` into a new folder named with accented letters, open the copy
/// with `open`, and remove the folder again.
fn open_from_accented_folder<T, E: std::fmt::Display>(
    source: &Path,
    name: &str,
    open: impl FnOnce(&Path) -> Result<T, E>,
) {
    let folder = std::env::temp_dir().join(format!("Données {name} {}", std::process::id()));
    std::fs::create_dir_all(&folder).expect("a temporary folder");
    let copy = folder.join(source.file_name().expect("a file name"));
    std::fs::copy(source, &copy).expect("the data file copies");
    let opened = open(&copy).map(|_| ());
    std::fs::remove_dir_all(&folder).ok();
    if let Err(error) = opened {
        panic!("{} does not open: {error}", copy.display());
    }
}

#[cfg(feature = "dd")]
#[test]
fn a_hash_data_file_opens_from_a_folder_with_accented_letters() {
    let Some(source) = data_file("_51DEGREES_DD_PATH") else {
        eprintln!("_51DEGREES_DD_PATH names no Hash data file; skipping");
        return;
    };
    open_from_accented_folder(&source, "dd", |path| {
        fiftyone_native::dd::Manager::open(path, PerformanceProfile::LowMemory)
    });
}

#[cfg(feature = "ipi")]
#[test]
fn an_ip_intelligence_data_file_opens_from_a_folder_with_accented_letters() {
    let Some(source) = data_file("_51DEGREES_IPI_PATH") else {
        eprintln!("_51DEGREES_IPI_PATH names no IP Intelligence data file; skipping");
        return;
    };
    open_from_accented_folder(&source, "ipi", |path| {
        fiftyone_native::ipi::Manager::open(path, PerformanceProfile::LowMemory)
    });
}
