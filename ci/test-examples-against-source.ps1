<#
.SYNOPSIS
Builds and tests the examples against this tree's crates, and fails if any
example still takes one of those crates from crates.io.

.DESCRIPTION
The examples depend on the published crates, and examples/source.toml patches
each one to its path in this tree. Cargo uses a patch only when its version is
the one it would pick, so this first writes a version above every published
release into the crates, then moves each patched crate in the examples' lock to
the copy in this tree, then checks the lock and runs the command, which by
default builds and tests every example.

The manifests and the lock are put back as they were when the script ends, so a
run leaves the working tree unchanged.

.PARAMETER Version
The version to give the crates. The publish workflow passes the version it is
releasing. Without it, the next patch above the highest release tag is used,
such as 4.5.9 after 4.5.8.

.PARAMETER Root
The workspace root. Defaults to the repository this script sits in.

.PARAMETER Command
What to run in examples/ while the crates point at this tree. Each cargo
command in it needs `--config source.toml`. ci/run-performance-tests.ps1 passes
the performance examples here, so the nightly figures are for this tree.
#>
param (
    [string]$Version,
    [string]$Root = (Join-Path $PSScriptRoot ".."),
    [scriptblock]$Command = { cargo test --config source.toml }
)
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

if (-not $Version) {
    # Release tags are plain versions. Tags with a suffix are pre-releases,
    # which a requirement such as 4.5.2 never selects, so they are left out.
    $released = git -C $Root tag --list |
        Where-Object { $_ -match '^\d+\.\d+\.\d+$' } |
        ForEach-Object { [version]$_ } |
        Sort-Object -Descending |
        Select-Object -First 1
    if (-not $released) {
        throw "No release tag found. The checkout needs the tags (fetch-depth: 0)."
    }
    $Version = "$($released.Major).$($released.Minor).$($released.Build + 1)"
}
Write-Output "Running the examples against this tree's crates at $Version."

$examples = Join-Path $Root "examples"
$patched = Get-Content -Path (Join-Path $examples "source.toml") |
    Where-Object { $_ -match '^([A-Za-z0-9_-]+)\s*=\s*\{\s*path\s*=' } |
    ForEach-Object { $Matches[1] }
if ($patched.Count -eq 0) { throw "No patches found in examples/source.toml" }

# Every file the run changes, kept so it can be put back afterwards.
$changed = @(Get-ChildItem -Path $Root -Filter Cargo.toml -Recurse -File |
    Where-Object { $_.FullName -notmatch '[\\/](target|examples|owid-rust)[\\/]' }) +
    @(Get-Item -Path (Join-Path $examples "Cargo.lock"))
$originals = @{}
foreach ($file in $changed) {
    $originals[$file.FullName] = [System.IO.File]::ReadAllBytes($file.FullName)
}

Push-Location $examples
try {
    & (Join-Path $PSScriptRoot "set-crate-versions.ps1") -Version $Version -Root $Root

    # The committed lock names the published versions, and Cargo keeps a locked
    # version until that package is updated. The crates of one release require
    # each other exactly, so they move in one update. Updated one at a time,
    # each would be held back by the published crates still locked beside it,
    # which require its old version exactly. Only the crates the lock holds
    # are named, because Cargo refuses the whole update if one named crate is
    # in no example's dependencies.
    $locked = @((Get-Content -Raw -Path "Cargo.lock") -split '\[\[package\]\]' |
        ForEach-Object { [regex]::Match($_, '(?m)^name = "([^"]+)"').Groups[1].Value } |
        Where-Object { $patched -contains $_ } |
        Sort-Object -Unique)
    if ($locked.Count -eq 0) {
        throw "examples/Cargo.lock holds none of the crates source.toml patches"
    }
    foreach ($crate in $patched) {
        if ($locked -notcontains $crate) {
            Write-Output "  $crate is not used by any example."
        }
    }
    $update = @("update", "--config", "source.toml") +
        @($locked | ForEach-Object { "-p", $_ })
    cargo @update

    # A patched crate that still resolves from crates.io carries a registry
    # source in the lock, where the copy in this tree carries none.
    $fromRegistry = (Get-Content -Raw -Path "Cargo.lock") -split '\[\[package\]\]' |
        ForEach-Object {
            $name = [regex]::Match($_, '(?m)^name = "([^"]+)"').Groups[1].Value
            if ($patched -contains $name -and $_ -match '(?m)^source = "registry') {
                $version = [regex]::Match($_, '(?m)^version = "([^"]+)"').Groups[1].Value
                "$name $version"
            }
        }
    if ($fromRegistry) {
        throw "These crates still come from crates.io, not this tree:`n$($fromRegistry -join "`n")"
    }

    & $Command
} finally {
    Pop-Location
    foreach ($path in $originals.Keys) {
        [System.IO.File]::WriteAllBytes($path, $originals[$path])
    }
}
