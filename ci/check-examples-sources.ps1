param (
    # The root of the rust working copy. Defaults to the parent of this
    # script, which is right for a developer checkout, and can be passed
    # explicitly by a caller that runs from another directory.
    [string]$RepoRoot = (Split-Path $PSScriptRoot -Parent)
)
$ErrorActionPreference = "Stop"

# Holds the examples workspace to its contract: the examples take the
# 51Degrees crates from crates.io, and `cargo --config source.toml` is the one
# way to build them from this tree. Three things break that contract, and each
# is checked here, because the only job that builds the examples against the
# published crates runs after a release, when a failure can no longer stop it.
#
# 1. An examples manifest naming a crate outside examples/ by path. A path
#    wins over the registry whatever the build passes, so the published crate
#    is never used.
# 2. source.toml missing a crate from ci/crates.txt, so a source build takes
#    that crate from crates.io while the rest come from the tree.
# 3. examples/Cargo.lock written during a source build, which records the
#    51Degrees crates as path packages and carries the unused patches. The
#    lock is meant to describe the registry build.

$examples = [System.IO.Path]::GetFullPath((Join-Path $RepoRoot "examples"))
$separator = [System.IO.Path]::DirectorySeparatorChar
$failures = @()

# The published crates, from the one list.
$crates = @(Get-Content (Join-Path $RepoRoot "ci/crates.txt") |
    ForEach-Object { $_.Trim() } |
    Where-Object { $_ -and -not $_.StartsWith("#") })

# 1. Every path dependency of an examples manifest stays under examples/.
$members = @()
$manifests = Get-ChildItem -Path $examples -Filter Cargo.toml -Recurse -File |
    Where-Object { $_.FullName -notmatch "[\\/]target[\\/]" }
foreach ($manifest in $manifests) {
    $relative = $manifest.FullName.Substring($RepoRoot.Length).TrimStart("\", "/")
    $lineNumber = 0
    foreach ($line in Get-Content $manifest.FullName) {
        $lineNumber++
        if ($line -match '^\s*name\s*=\s*"([^"]+)"') {
            $members += $Matches[1]
        }
        if ($line -match '^\s*#') { continue }
        if ($line -match 'path\s*=\s*"([^"]+)"') {
            $target = [System.IO.Path]::GetFullPath(
                (Join-Path $manifest.DirectoryName $Matches[1]))
            if (-not $target.StartsWith($examples + $separator)) {
                $failures += "${relative}:${lineNumber} names a crate outside " +
                    "examples/ by path, so the published crate is never used: " +
                    $line.Trim()
            }
        }
    }
}

# 2. source.toml patches every published crate, and nothing else.
$patched = @()
foreach ($line in Get-Content (Join-Path $examples "source.toml")) {
    if ($line -match '^\s*([\w-]+)\s*=\s*\{\s*path\s*=') {
        $patched += $Matches[1]
    }
}
foreach ($crate in $crates) {
    if ($patched -notcontains $crate) {
        $failures += "examples/source.toml does not patch $crate, so a " +
            "source build takes it from crates.io"
    }
}
foreach ($name in $patched) {
    if ($crates -notcontains $name) {
        $failures += "examples/source.toml patches $name, which " +
            "ci/crates.txt does not publish"
    }
}

# 3. The lock describes the registry build.
$lock = Get-Content (Join-Path $examples "Cargo.lock") -Raw
if ($lock -match '\[\[patch\.unused\]\]') {
    $failures += "examples/Cargo.lock carries [[patch.unused]], so it was " +
        "written during a source build"
}
$packages = $lock -split '\[\[package\]\]' | Select-Object -Skip 1
foreach ($package in $packages) {
    if ($package -notmatch '(?m)^name = "([^"]+)"') { continue }
    $name = $Matches[1]
    if ($package -match '(?m)^source = ') { continue }
    if ($members -notcontains $name) {
        $failures += "examples/Cargo.lock records $name as a path package, " +
            "so it was written during a source build"
    }
}

if ($failures.Count -gt 0) {
    foreach ($failure in $failures) {
        Write-Output "FAIL $failure"
    }
    throw "$($failures.Count) problem(s) with how the examples reach the " +
        "51Degrees crates. A lock written during a source build is put " +
        "right by any cargo command run in examples/ without --config " +
        "source.toml, for example cargo metadata."
}
Write-Output ("The examples take the 51Degrees crates from crates.io, " +
    "source.toml patches all $($crates.Count), and the lock is a registry lock.")
