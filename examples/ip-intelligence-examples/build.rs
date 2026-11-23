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

//! Build script for the IP-intelligence examples.
//!
//! Its only job is a Windows-specific fix. On the MSVC toolchain the Windows
//! "installer-detection" heuristic forces an elevation (UAC) prompt for any
//! executable whose file name contains words such as "update", "setup",
//! "install" or "patch". One example bin is named
//! `ipi-onprem-update-data-file`, so without an application manifest Windows
//! refuses to launch it from a non-elevated context (and `cargo test` fails with
//! OS error 740, "requires elevation"). Embedding a manifest that declares
//! `requestedExecutionLevel level="asInvoker"` tells Windows not to apply the
//! heuristic, so every example bin runs as an ordinary user process. This
//! affects only how the executables are launched on Windows; it changes no
//! behavior and no example code.
//!
//! The manifest is embedded straight by the MSVC linker (`link.exe`), so the
//! build needs no extra crates. On every other target the script does nothing.

use std::env;
use std::fs;
use std::path::Path;

/// The minimal application manifest. The single meaningful element is the
/// `requestedExecutionLevel` of `asInvoker`, which keeps the process at the
/// caller's privilege level and suppresses installer detection.
const AS_INVOKER_MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>
"#;

fn main() {
    // The fix is only relevant to the MSVC Windows toolchain, whose linker is
    // link.exe and which honours the /MANIFEST family of flags. The GNU Windows
    // toolchain and all non-Windows targets are left untouched.
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os != "windows" || target_env != "msvc" {
        return;
    }

    // Write the manifest into the build output directory so the linker can read
    // it, then embed it into every binary this crate produces.
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR is set for build scripts");
    let manifest_path = Path::new(&out_dir).join("as-invoker.manifest");
    if fs::write(&manifest_path, AS_INVOKER_MANIFEST).is_err() {
        // If the manifest cannot be written we simply skip the fix rather than
        // failing the build; the only consequence is the elevation prompt on the
        // one oddly-named bin.
        return;
    }

    // `/MANIFEST:EMBED` and `/MANIFESTINPUT:<file>` ask link.exe to embed the
    // given manifest. These apply only to the final binaries (not the rlib), so
    // `cargo:rustc-link-arg-bins` is the correct scope: it covers every bin and
    // each bin's test executable.
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}",
        manifest_path.display()
    );
    // Rebuild the link step if the manifest content (this script) changes.
    println!("cargo:rerun-if-changed=build.rs");
}
