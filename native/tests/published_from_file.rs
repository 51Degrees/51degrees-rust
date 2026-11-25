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

//! A data file's published date is read from its header without loading the
//! file, and is the date a manager that loads the file gives.
//!
//! The tests need the files `_51DEGREES_DD_PATH` and `_51DEGREES_IPI_PATH`
//! name, and skip with a note when one is missing.

#![cfg(any(feature = "dd", feature = "ipi"))]

use std::path::{Path, PathBuf};

/// Where the published date starts in a Hash or IP Intelligence data file
/// header.
const PUBLISHED_AT: usize = 66;

/// A date written into a copy of a data file, far from any real file's.
const WRITTEN: (i16, u8, u8) = (2001, 2, 3);

fn data_file(variable: &str) -> Option<PathBuf> {
    let path = std::env::var(variable)
        .ok()
        .map(PathBuf::from)
        .filter(|path| path.is_file());
    if path.is_none() {
        eprintln!("{variable} names no data file; skipping");
    }
    path
}

/// A folder of its own for `name`, emptied.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fiftyone-published-from-file-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Write into `dir` a copy of `source` published on [`WRITTEN`], a text file
/// and a file shorter than the header, and give their paths.
fn copies(source: &Path, dir: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let mut bytes = std::fs::read(source).unwrap();
    let (year, month, day) = WRITTEN;
    bytes[PUBLISHED_AT..PUBLISHED_AT + 2].copy_from_slice(&year.to_le_bytes());
    bytes[PUBLISHED_AT + 2] = month;
    bytes[PUBLISHED_AT + 3] = day;
    let dated = dir.join(source.file_name().unwrap());
    std::fs::write(&dated, &bytes).unwrap();
    let text = dir.join("notes.txt");
    std::fs::write(
        &text,
        "not a data file, though longer than a header is. ".repeat(20),
    )
    .unwrap();
    let short = dir.join("short.dat");
    std::fs::write(&short, &bytes[..PUBLISHED_AT]).unwrap();
    (dated, text, short)
}

fn written() -> (i32, u32, u32) {
    let (year, month, day) = WRITTEN;
    (year.into(), month.into(), day.into())
}

#[cfg(feature = "dd")]
#[test]
fn a_hash_data_file_gives_its_published_date_without_loading() {
    use fiftyone_native::dd::Manager;
    use fiftyone_native::PerformanceProfile;

    let Some(source) = data_file("_51DEGREES_DD_PATH") else {
        return;
    };
    let loaded = Manager::open(&source, PerformanceProfile::LowMemory)
        .unwrap()
        .data_set_published();
    assert_eq!(Manager::published_from_file(&source).ok(), loaded);

    let dir = folder("dd");
    let (dated, text, short) = copies(&source, &dir);
    assert_eq!(Manager::published_from_file(&dated).unwrap(), written());
    for path in [text, short, dir.join("missing.hash")] {
        assert!(
            Manager::published_from_file(&path).is_err(),
            "{}",
            path.display()
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(feature = "ipi")]
#[test]
fn an_ip_intelligence_data_file_gives_its_published_date_without_loading() {
    use fiftyone_native::ipi::Manager;
    use fiftyone_native::PerformanceProfile;

    let Some(source) = data_file("_51DEGREES_IPI_PATH") else {
        return;
    };
    let loaded = Manager::open(&source, PerformanceProfile::LowMemory)
        .unwrap()
        .data_set_published();
    assert_eq!(Manager::published_from_file(&source).ok(), loaded);

    let dir = folder("ipi");
    let (dated, text, short) = copies(&source, &dir);
    assert_eq!(Manager::published_from_file(&dated).unwrap(), written());
    for path in [text, short, dir.join("missing.ipi")] {
        assert!(
            Manager::published_from_file(&path).is_err(),
            "{}",
            path.display()
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
