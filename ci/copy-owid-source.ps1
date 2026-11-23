param (
    # The root of the rust working copy. Defaults to the parent of this
    # script, which is right for a developer checkout, and can be passed
    # explicitly by a caller that runs from another directory.
    [string]$RepoRoot = (Split-Path $PSScriptRoot -Parent)
)
$ErrorActionPreference = "Stop"

# Copies the OWID source into the fodid crate as the private module
# fodid::owid, so that the crate published to crates.io carries the OWID code
# itself and no OWID crate has to exist on any registry.
#
# The dependency cannot come from a package registry. The OWID library is
# maintained in the SWAN community and is moving to Prebid, and 51Degrees
# does not publish or own an owid crate, so fodid must not depend on one.
# Compiling the source in, as the .NET package compiles the owid-dotnet
# source into FiftyOne.Did.dll and the Python package copies owid-python in
# as a private module, keeps the 51Did crates publishable on their own.
#
# Nothing is fetched over the network, because owid-rust is a submodule and
# CI clones with submodules. Nothing is written back to the repository
# either, as the copy is ignored by git and main keeps the submodule as the
# single source of the OWID code. The script can be run as often as needed
# and always leaves the same result for the same submodule commit.
#
# Three mechanical changes are made on the way in, because the files were
# written as a crate root and are compiled here as a module. lib.rs becomes
# mod.rs, every absolute path that starts with crate:: is prefixed with the
# module name so that it starts with crate::owid:: instead, and the examples
# in the documentation comments reach the library through fodid:: rather
# than owid::, so they compile and run as doc tests of this crate. Nothing
# else is altered.

$owidRepo = Join-Path $RepoRoot "owid-rust"
$owidSource = Join-Path $owidRepo "src"
$target = Join-Path $RepoRoot "fodid/src/owid"

if (-not (Test-Path (Join-Path $owidSource "lib.rs"))) {
    throw "OWID source not found at '$owidSource'. Run " +
        "'git submodule update --init --recursive' first, or clone with " +
        "--recurse-submodules."
}

# The commit the copy was taken from, so the notice can say exactly which
# version of the OWID source is inside the crate.
$commit = (git -C $owidRepo rev-parse HEAD 2>$null)
if (-not $commit) {
    $commit = (git -C $RepoRoot rev-parse "HEAD:owid-rust" 2>$null)
}
if (-not $commit) {
    throw "Could not determine the owid-rust commit to record in the notice."
}

if (Test-Path $target) {
    Remove-Item -Path $target -Recurse -Force
}
$null = New-Item -ItemType Directory -Path $target -Force

foreach ($file in Get-ChildItem -Path $owidSource -Filter "*.rs" -File) {
    $name = if ($file.Name -eq "lib.rs") { "mod.rs" } else { $file.Name }
    $lines = foreach ($line in Get-Content -Path $file.FullName) {
        if ($line -cmatch '^\s*//[/!]') {
            # A documentation line. The examples in the library documentation
            # name the crate as owid, and here the same items are reached
            # through fodid, so the examples run against the re-exported
            # surface. A path that already starts with crate:: is left for
            # the rewrite below.
            $line = $line -creplace '(?<![\w:])owid::', 'fodid::'
        }
        $line.Replace("crate::", "crate::owid::")
    }
    Set-Content -Path (Join-Path $target $name) -NoNewline -Encoding utf8 `
        -Value (($lines -join "`n") + "`n")
}
Copy-Item -Path (Join-Path $owidRepo "LICENSE") `
    -Destination (Join-Path $target "LICENSE") -Force

$notice = @"
The Rust source files in this directory are the OWID (Open Web Id) library.
They are copied into the fodid crate at build time and are not part of the
51Degrees source, so they keep their own licence, which is the Apache License
2.0 in the LICENSE file beside this notice, and not the EUPL 1.2 that covers
the rest of the crate.

Copyright 2026 51 Degrees Mobile Experts Limited (51degrees.com)

Taken from the OWID project, https://github.com/SWAN-community/owid-rust,
at commit
$commit

The files are compiled as the private module owid inside the fodid crate, so
that publishing fodid never claims the crate name "owid" on any registry and
no OWID crate has to exist for fodid to build. On the way in lib.rs was
renamed mod.rs, every path starting with crate:: was changed to start with
crate::owid::, and the examples in the documentation comments reach the
library through fodid:: rather than owid::. Nothing else was altered. Use the
OWID library from the OWID project itself rather than from here.
"@
Set-Content -Path (Join-Path $target "NOTICE") -NoNewline -Encoding utf8 `
    -Value (($notice -replace "`r?`n", "`n") + "`n")

Write-Output "Copied OWID source at $commit into '$target'"
