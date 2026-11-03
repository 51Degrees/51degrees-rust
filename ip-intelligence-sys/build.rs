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

//! Build script for `fiftyone-ip-intelligence-sys`.
//!
//! This compiles the on-premise IP Intelligence C library directly with the
//! `cc` crate and links it statically as `fiftyone-ip-intelligence-c`. The
//! library is built self contained, meaning the shared common C sources in
//! `src/common` and the graph sources in `src/ipi/graph` are compiled in
//! alongside the `ipi.c` and `ipi_weighted_results.c` engine sources in
//! `src/ipi`. Every C source is compiled into one static library.
//!
//! The library is deliberately self contained rather than reusing the
//! `fiftyone-common-c` native library owned by `fiftyone-common-sys`. The IP
//! Intelligence data format uses 64 bit file offsets, so the whole library
//! (including the shared common sources it calls) must be compiled with
//! `FIFTYONE_DEGREES_LARGE_DATA_FILE_SUPPORT`, which widens
//! `fiftyoneDegreesFileOffset` and the on disk offset types to 64 bit. The
//! common library compiled by `fiftyone-common-sys` does not set that define,
//! so its objects use 32 bit offsets and are ABI incompatible with the IP
//! Intelligence engine. Compiling a private copy of the common sources here
//! with the correct define is the only sound option.
//!
//! The `cc` crate locates the MSVC toolchain through `vswhere` on Windows, so
//! `cl.exe` does not need to be on `PATH`. On other platforms it uses the
//! system C compiler in the usual way.
//!
//! The sources are found as `resolve_cxx_dir` describes.

use std::path::{Path, PathBuf};

fn main() {
    let cxx_src = resolve_cxx_dir().join("src");
    let src = cxx_src.join("ipi");
    let common_cxx = cxx_src.join("common");
    let ip_graph = src.join("graph");

    // Rebuild whenever the override variable changes so a relocated checkout is
    // picked up without a manual clean.
    println!("cargo:rerun-if-env-changed=FIFTYONE_CXX_DIR");

    // A changed header must rebuild the C library and rerun the checks in
    // src/declarations.c, as a changed source file does. Cargo scans a
    // directory for any change.
    println!("cargo:rerun-if-changed={}", cxx_src.display());

    // Gather the C sources for the three layers that make up the C library.
    // Globbing keeps the lists in step with the 51degrees-cxx sources.
    let mut sources: Vec<PathBuf> = Vec::new();
    collect_c_sources(&common_cxx, &mut sources);
    collect_c_sources(&ip_graph, &mut sources);
    collect_c_sources(&src, &mut sources);

    assert!(
        !sources.is_empty(),
        "no .c sources found under {}",
        src.display()
    );

    // The property enumeration shim ships with this crate. It exposes a few flat
    // helpers that read the `available` properties buried deep inside the data
    // set structures, so the Rust side does not need to mirror those private C
    // layouts. It includes `ipi.h`, which is resolved through the `src` include
    // directory added below.
    let shim = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is always set by cargo"),
    )
    .join("src/shim.c");
    println!("cargo:rerun-if-changed={}", shim.display());
    sources.push(shim);

    // The forced-include header that renames this crate's private common C
    // library symbols to an ipi_ prefixed namespace. It is applied to every C
    // and C++ translation unit below, so both the common C library definitions
    // compiled here and every call site that references them are renamed
    // together at preprocess time. Device Detection keeps the unprefixed names,
    // so the two copies of the common C library no longer collide when both
    // engines are linked into one binary. See the header for the full rationale
    // and how to regenerate the symbol list.
    let symbol_prefix = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is always set by cargo"),
    )
    .join("src/symbol_prefix.h");
    assert!(
        symbol_prefix.is_file(),
        "symbol prefix header missing at {}",
        symbol_prefix.display()
    );
    println!("cargo:rerun-if-changed={}", symbol_prefix.display());
    // `CARGO_MANIFEST_DIR` is already absolute, so the joined path is absolute
    // too. It is used as is rather than canonicalised, because canonicalising on
    // Windows yields an extended-length `\\?\` prefix that the MSVC `/FI` flag
    // does not accept.
    let symbol_prefix_abs = symbol_prefix.to_string_lossy().into_owned();

    let mut build = cc::Build::new();
    build.files(&sources).warnings(false);

    // Force include the symbol prefix header into every translation unit. MSVC
    // spells this `/FI<file>` and the GNU and Clang drivers spell it
    // `-include <file>`. Both inject the header before the first line of each
    // source, so the renames are in effect for the whole unit.
    if build.get_compiler().is_like_msvc() {
        build.flag(format!("/FI{symbol_prefix_abs}"));
    } else {
        build.flag("-include").flag(&symbol_prefix_abs);
    }

    // Add only `src/ipi` to the angle bracket search path so the shim can
    // resolve `#include "ipi.h"`. `src/common` must NOT be placed on the search
    // path because it ships its own `string.h` which would shadow the C runtime
    // `<string.h>` and corrupt the CRT declarations. `src/ipi` has no such
    // shadowing header, so adding it is safe, exactly as the device detection
    // build does with `src/dd`.
    build.include(&src);

    // The IP Intelligence data format uses 64 bit file offsets, so the whole
    // library must be built with large data file support. Reduced file support
    // matches the upstream CMake option. Both are applied on every platform so
    // the on disk offset types stay 64 bit and the dataset header layout is
    // consistent across the C sources.
    build
        .define("FIFTYONE_DEGREES_LARGE_DATA_FILE_SUPPORT", None)
        .define("FIFTYONE_DEGREES_REDUCED_FILE", None);

    // The engine C sources resolve their own cross includes quote relative (for
    // example `"../common/config.h"` from `src/ipi`, or `"../../common/data.h"`
    // from `src/ipi/graph`), because the compiler always searches the including
    // file's own directory first for quote includes. The `src/ipi` include
    // directory added above is what lets the crate's own shim, which sits
    // outside the C source tree, resolve `#include "ipi.h"`.

    if build.get_compiler().is_like_msvc() {
        // Upstream builds warning-as-error under its own known toolchain. A
        // consumer toolchain version may differ, so warnings are not promoted
        // to errors here. `/w` silences them. Correctness is unaffected because
        // the sources are released warning-clean upstream.
        build.flag("/w");
        // Mirror the defines the upstream build applies on MSVC. UNICODE
        // selects the wide Windows APIs and the secure-warnings suppression
        // matches the upstream build.
        build
            .define("_CRT_SECURE_NO_WARNINGS", None)
            .define("UNICODE", None)
            .define("_UNICODE", None);
    } else {
        build.flag_if_supported("-w");
        // POSIX needs 64 bit `off_t` so that `fseeko`/`ftello` match the widened
        // file offset type used by the large data file support above.
        build.define("_FILE_OFFSET_BITS", "64");
    }

    build.compile("fiftyone-ip-intelligence-c");

    // Check the hand-written structures in src/lib.rs against the C headers
    // being compiled (see src/declarations.c). The object is not linked.
    let declarations = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is always set by cargo"),
    )
    .join("src/declarations.c");
    println!("cargo:rerun-if-changed={}", declarations.display());
    cc::Build::new()
        .file(&declarations)
        .include(&src)
        .define("FIFTYONE_DEGREES_LARGE_DATA_FILE_SUPPORT", None)
        .define("FIFTYONE_DEGREES_REDUCED_FILE", None)
        .cargo_metadata(false)
        .compile("fiftyone-ip-intelligence-declarations");

    // The math library is required by the graph and weighted value code. The
    // threading layer's double-width (128-bit) compare-exchange lowers to a
    // `__atomic_compare_exchange_16` call on Linux, which libatomic provides.
    // libatomic is linked statically so the deployed OS needs no libatomic
    // package, while libatomic still selects cmpxchg16b or a lock-based fallback
    // at run time, so no specific CPU instruction is baked in.
    if !build.get_compiler().is_like_msvc() {
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

    // `links = "fiftyone-ip-intelligence-c"` in Cargo.toml records this crate as
    // the sole owner of that native library for the rest of the workspace.
}

/// Push every `.c` file directly inside `dir` onto `sources`, emitting a
/// rerun-if-changed line for each so a touched source triggers a rebuild.
fn collect_c_sources(dir: &Path, sources: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read source dir {}: {e}", dir.display()))
    {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) == Some("c") {
            println!("cargo:rerun-if-changed={}", path.display());
            sources.push(path);
        }
    }
}

/// A file every directory holding the sources this crate compiles contains.
const MARKER: &str = "src/ipi/ipi.h";

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
