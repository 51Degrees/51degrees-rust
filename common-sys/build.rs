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

//! Build script for `fiftyone-common-sys`.
//!
//! This compiles the common C library in `src/common` of the 51degrees-cxx
//! sources directly with the `cc` crate and links it statically as
//! `fiftyone-common-c`. The C layer is enough for the shared FFI surface
//! (Exception, Status, ResourceManager, Evidence and Properties), so the C++
//! layer is deliberately not compiled here.
//!
//! The `cc` crate locates the MSVC toolchain through `vswhere` on Windows, so
//! `cl.exe` does not need to be on `PATH`. On other platforms it uses the
//! system C compiler in the usual way.
//!
//! The sources are found as `resolve_cxx_dir` describes.

use std::path::PathBuf;

fn main() {
    let common_cxx = resolve_cxx_dir().join("src").join("common");

    // Rebuild whenever the override variable changes so a relocated checkout is
    // picked up without a manual clean.
    println!("cargo:rerun-if-env-changed=FIFTYONE_CXX_DIR");

    // A changed header must rebuild the C library, as a changed source file
    // does. Cargo scans a directory for any change.
    println!("cargo:rerun-if-changed={}", common_cxx.display());

    // The `.c` files form the C library that CMake builds as `fiftyone-common-c`.
    // Globbing keeps the source list in step with the 51degrees-cxx sources.
    let mut sources: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(&common_cxx)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", common_cxx.display()))
    {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) == Some("c") {
            println!("cargo:rerun-if-changed={}", path.display());
            sources.push(path);
        }
    }
    assert!(
        !sources.is_empty(),
        "no .c sources found in {}",
        common_cxx.display()
    );

    let mut build = cc::Build::new();
    build.files(&sources).warnings(false);

    // The `src/common` directory must NOT be added to the include search path.
    // It ships a `string.h` of its own, and placing the directory on the
    // angle-bracket search path shadows the system `<string.h>` that the C
    // runtime headers pull in, which corrupts the CRT declarations. Every
    // common source includes its siblings with quotes (for example
    // `#include "cache.h"`), and the compiler searches the including file's own
    // directory first for quote includes, so no `-I` into the directory is
    // needed. This matches how the upstream CMake build keeps the directory off
    // the angle-bracket path.

    if build.get_compiler().is_like_msvc() {
        // Upstream builds with warning-as-error under its own known toolchain.
        // A consumer toolchain version may differ, so warnings are not promoted
        // to errors here. `/w` silences them. Correctness is unaffected because
        // the sources are released as warning-clean upstream.
        build.flag("/w");
    } else {
        build.flag_if_supported("-w");
    }

    if build.get_compiler().is_like_msvc() {
        // Mirror the defines the upstream CMakeLists.txt applies on MSVC.
        // UNICODE selects the wide Windows APIs and the secure-warnings
        // suppression matches the upstream build.
        build
            .define("_CRT_SECURE_NO_WARNINGS", None)
            .define("UNICODE", None)
            .define("_UNICODE", None);
    }

    build.compile("fiftyone-common-c");

    // The lock-free resource reference counting in the common C library
    // (resource.c) does a double-width (128-bit) compare-exchange, which the C
    // compiler lowers to a `__atomic_compare_exchange_16` call on Linux. Link
    // libatomic so that symbol resolves. Link it statically so the binary stays
    // self-contained (the deployed OS needs no libatomic package) while
    // libatomic still selects cmpxchg16b or a lock-based fallback at run time,
    // so no specific CPU instruction is baked in. MSVC lowers the operation to
    // an inline intrinsic, so this is Linux only. rustc cannot find libatomic.a
    // on its own (it lives in the C compiler's private library directory), so
    // ask the compiler where it is and add that directory to the link search
    // path.
    if cfg!(target_os = "linux") {
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

    // `links = "fiftyone-common-c"` in Cargo.toml records this crate as the sole
    // owner of that native library for the rest of the workspace.
}

/// A file every directory holding the sources this crate compiles contains.
const MARKER: &str = "src/common/fiftyone.h";

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

    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is always set by cargo"),
    );
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
