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

//! Build script for `fiftyone-device-detection-sys`.
//!
//! This compiles the Hash on-premise C layer from the 51degrees-cxx sources
//! directly with the `cc` crate and links it statically as
//! `fiftyone-device-detection-c`. The shared common C layer is supplied by
//! the `fiftyone-common-sys` crate (linked as `fiftyone-common-c`), so only the
//! device detection and Hash sources are compiled here.
//!
//! The two library source groups are the device detection C sources in
//! `src/dd` (the `fiftyone-device-detection-c` CMake target) and the Hash C
//! sources in `src/dd/hash` (the `fiftyone-hash-c` CMake target). Both are
//! placed in a single static archive here because Cargo links one native
//! library per `links` key.
//!
//! A small C shim (`src/shim.c` in this crate) is compiled alongside them. It
//! exposes a handful of property enumeration helpers that read fields buried
//! inside the deeply nested data set structures, which keeps the Rust side free
//! of any need to mirror those private C layouts.
//!
//! The sources are found as `resolve_cxx_dir` describes. When a Lite Hash data
//! file is found beside them, its path is exported as the `51DEGREES_DD_PATH`
//! compile time environment variable so the crate's smoke test can run a real
//! detection.

use std::path::{Path, PathBuf};

fn main() {
    let cxx = resolve_cxx_dir();
    let src = cxx.join("src").join("dd");
    let hash = src.join("hash");

    // Rebuild whenever the override variable changes so a relocated checkout is
    // picked up without a manual clean.
    println!("cargo:rerun-if-env-changed=FIFTYONE_CXX_DIR");

    // A changed header must rebuild the C library and rerun the checks in
    // src/declarations.c, as a changed source file does. Cargo scans a
    // directory for any change.
    println!("cargo:rerun-if-changed={}", cxx.join("src").display());

    // Collect the device detection C sources (the fiftyone-device-detection-c
    // target) and the Hash C sources (the fiftyone-hash-c target). Globbing
    // keeps the list in step with the 51degrees-cxx sources.
    let mut sources: Vec<PathBuf> = Vec::new();
    collect_c_sources(&src, &mut sources);
    collect_c_sources(&hash, &mut sources);
    assert!(
        !sources.is_empty(),
        "no .c sources found under {}",
        src.display()
    );

    // The property enumeration shim ships with this crate.
    let shim = PathBuf::from(env("CARGO_MANIFEST_DIR")).join("src/shim.c");
    println!("cargo:rerun-if-changed={}", shim.display());
    sources.push(shim);

    let mut build = cc::Build::new();
    build.files(&sources).warnings(false);

    // Only `src/dd` is added to the angle bracket search path, so the shim and
    // the declarations check can include `"hash/hash.h"`. The C sources include
    // the shared headers relative to themselves (for example
    // `"../common/dataset.h"` from `src/dd`), which the compiler resolves from
    // the including file's own directory. `src/common` must NOT be placed on
    // the search path because it ships its own `string.h` which would shadow
    // the C runtime `<string.h>` and corrupt the CRT declarations. `src/dd` has
    // no such shadowing header, so adding it is safe.
    build.include(&src);

    if build.get_compiler().is_like_msvc() {
        // A consumer toolchain version may differ from the upstream one, so
        // warnings are not promoted to errors here. `/w` silences them. The
        // sources are released warning clean upstream, so correctness is
        // unaffected. Mirror the defines the upstream CMakeLists.txt applies on
        // MSVC. UNICODE selects the wide Windows APIs and the secure warnings
        // suppression matches the upstream build.
        build.flag("/w");
        build
            .define("_CRT_SECURE_NO_WARNINGS", None)
            .define("UNICODE", None)
            .define("_UNICODE", None);
    } else {
        build.flag_if_supported("-w");
    }

    build.compile("fiftyone-device-detection-c");

    // Check the hand-written declarations in src/lib.rs against the C headers
    // being compiled (see src/declarations.c). The object is not linked.
    // GCC and Clang reject a conflicting redeclaration outright. MSVC only
    // warns about one, with C4028 for a different parameter type and C4030 or
    // C4031 for a longer or shorter parameter list, so those are made errors.
    let declarations = PathBuf::from(env("CARGO_MANIFEST_DIR")).join("src/declarations.c");
    println!("cargo:rerun-if-changed={}", declarations.display());
    let mut check = cc::Build::new();
    check
        .file(&declarations)
        .include(&src)
        .cargo_metadata(false);
    if check.get_compiler().is_like_msvc() {
        check.flag("/we4028").flag("/we4030").flag("/we4031");
    }
    check.compile("fiftyone-device-detection-declarations");

    // On non MSVC platforms the Hash math routines pull in libm. The resource
    // manager's double-width (128-bit) compare-exchange lowers to a
    // `__atomic_compare_exchange_16` call on Linux, which libatomic provides.
    // libatomic is linked statically so the deployed OS needs no libatomic
    // package, while libatomic still selects cmpxchg16b or a lock-based fallback
    // at run time, so no specific CPU instruction is baked in.
    if !cfg!(target_env = "msvc") {
        println!("cargo:rustc-link-lib=dylib=m");
        if cfg!(target_os = "linux") {
            // rustc cannot find libatomic.a on its own (it lives in the C
            // compiler's private library directory), so ask the compiler where it
            // is and add that directory to the link search path.
            if let Ok(out) = std::process::Command::new(build.get_compiler().path())
                .arg("-print-file-name=libatomic.a")
                .output()
            {
                let printed = String::from_utf8_lossy(&out.stdout);
                let lib = std::path::Path::new(printed.trim());
                if lib.is_absolute() {
                    if let Some(dir) = lib.parent() {
                        println!("cargo:rustc-link-search=native={}", dir.display());
                    }
                }
            }
            println!("cargo:rustc-link-lib=static=atomic");
        }
    }

    // Locate a Lite Hash data file so the smoke test can run a real detection.
    // Absence is not an error: the test then only asserts the symbols link.
    if let Some(data_file) = find_lite_data_file(&cxx) {
        println!("cargo:rerun-if-changed={}", data_file.display());
        println!("cargo:rustc-env=51DEGREES_DD_PATH={}", data_file.display());
    }
}

/// Read a required environment variable, panicking with a clear message if it is
/// missing. Used for variables Cargo always sets for a build script.
fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is always set by cargo"))
}

/// Push every `.c` file in `dir` onto `sources`, registering each for rebuild.
fn collect_c_sources(dir: &Path, sources: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read directory {}: {e}", dir.display()))
    {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) == Some("c") {
            println!("cargo:rerun-if-changed={}", path.display());
            sources.push(path);
        }
    }
}

/// A file every directory holding the sources this crate compiles contains.
const MARKER: &str = "src/dd/hash/hash.h";

/// Resolve the root of the 51degrees-cxx sources.
///
/// Resolution order:
/// 1. `FIFTYONE_CXX_DIR` when set, for a developer pointing at a checkout.
/// 2. The `cxx` submodule at the root of the workspace, which a source
///    checkout builds from.
/// 3. The `cxx` folder inside this crate. `ci/copy-cxx-sources.ps1` copies
///    the sources there before publishing, so the published crate carries
///    them and builds with no submodule or environment setup.
fn resolve_cxx_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("FIFTYONE_CXX_DIR") {
        let path = PathBuf::from(dir);
        assert!(
            path.join(MARKER).is_file(),
            "FIFTYONE_CXX_DIR={} does not contain {MARKER}",
            path.display()
        );
        return path;
    }

    let manifest_dir = PathBuf::from(env("CARGO_MANIFEST_DIR"));
    let submodule = manifest_dir
        .parent()
        .map(|workspace| workspace.join("cxx"))
        .expect("crate directory has a parent");
    let copied = manifest_dir.join("cxx");

    [submodule, copied]
        .into_iter()
        .find(|dir| dir.join(MARKER).is_file())
        .expect(
            "could not find the 51degrees-cxx sources. Run \
             'git submodule update --init --recursive' to check out the cxx \
             submodule, or set FIFTYONE_CXX_DIR to a checkout",
        )
}

/// Find a Lite Hash data file under the checkout, if one is present.
///
/// The packaged file is `device-detection-data/51Degrees-LiteV4.1.hash`. Any
/// `*.hash` in that directory is accepted as a fallback so a differently named
/// Lite file still enables the smoke test.
fn find_lite_data_file(cxx: &Path) -> Option<PathBuf> {
    let data_dir = cxx.join("device-detection-data");

    let preferred = data_dir.join("51Degrees-LiteV4.1.hash");
    if preferred.is_file() {
        return Some(preferred);
    }

    let entries = std::fs::read_dir(&data_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("hash") {
            return Some(path);
        }
    }
    None
}
