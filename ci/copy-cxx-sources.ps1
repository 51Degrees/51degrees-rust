param (
    # The root of the rust working copy. Defaults to the parent of this
    # script, which is right for a developer checkout, and can be passed
    # explicitly by a caller that runs from another directory.
    [string]$RepoRoot = (Split-Path $PSScriptRoot -Parent)
)
$ErrorActionPreference = "Stop"

# Copies the C sources each -sys crate compiles from the cxx submodule into a
# cxx folder inside that crate, so the crate published to crates.io carries
# them and builds with no submodule. A build in the workspace reads the
# submodule directly, so the copy is needed only before packaging.
#
# Cargo cannot package the files from the submodule itself, because it leaves
# out any include pattern that reaches outside the crate's own folder. The
# copy keeps the src/ layout of 51degrees-cxx, because the C sources include
# each other relative to their own folder (for example "../common/data.h"
# from src/dd). Only .c and .h files are copied. git ignores the copies, and
# each crate's manifest lists its cxx folder in include so that cargo
# packages it anyway. The script can be run as often as needed and always
# leaves the same result for the same submodule commit.

$cxx = Join-Path $RepoRoot "cxx"
if (-not (Test-Path (Join-Path $cxx "src/common/fiftyone.h"))) {
    throw "C sources not found at '$cxx'. Run " +
        "'git submodule update --init --recursive' first, or clone with " +
        "--recurse-submodules."
}

# The commit the copy was taken from, recorded beside each copy.
$commit = (git -C $cxx rev-parse HEAD 2>$null)
if (-not $commit) {
    $commit = (git -C $RepoRoot rev-parse "HEAD:cxx" 2>$null)
}
if (-not $commit) {
    throw "Could not determine the cxx commit to record beside the copies."
}

# The folders each crate compiles. Device Detection links the common library
# built by fiftyone-common-sys, so it takes the common headers alone.
$sources = [ordered]@{
    "common-sys" = @(
        @{ Folder = "src/common"; HeadersOnly = $false })
    "device-detection-sys" = @(
        @{ Folder = "src/common"; HeadersOnly = $true },
        @{ Folder = "src/dd"; HeadersOnly = $false },
        @{ Folder = "src/dd/hash"; HeadersOnly = $false })
    "ip-intelligence-sys" = @(
        @{ Folder = "src/common"; HeadersOnly = $false },
        @{ Folder = "src/ipi"; HeadersOnly = $false },
        @{ Folder = "src/ipi/graph"; HeadersOnly = $false })
}

foreach ($crate in $sources.Keys) {
    $target = Join-Path $RepoRoot "$crate/cxx"
    if (Test-Path $target) {
        Remove-Item -Path $target -Recurse -Force
    }
    $count = 0
    foreach ($source in $sources[$crate]) {
        $extensions = if ($source.HeadersOnly) { @(".h") } else { @(".c", ".h") }
        $from = Join-Path $cxx $source.Folder
        $to = Join-Path $target $source.Folder
        $null = New-Item -ItemType Directory -Path $to -Force
        $files = Get-ChildItem -Path $from -File |
            Where-Object { $extensions -contains $_.Extension }
        foreach ($file in $files) {
            Copy-Item -Path $file.FullName -Destination $to
            $count++
        }
    }
    Set-Content -Path (Join-Path $target "SOURCE.txt") -NoNewline -Encoding utf8 `
        -Value "repository=51Degrees/51degrees-cxx`ncommit=$commit`n"
    Write-Output "Copied $count C files at $commit into '$target'"
}
