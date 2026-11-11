param (
    # The root of the rust working copy. Defaults to the parent of this
    # script, which is right for a developer checkout, and can be passed
    # explicitly by a caller that runs from another directory.
    [string]$RepoRoot = (Split-Path $PSScriptRoot -Parent)
)
$ErrorActionPreference = "Stop"

# Writes the commit of the derived-properties submodule to
# derived-properties/SOURCE.txt, so the crate published to crates.io, which
# has no git, still knows which commit its scripts came from. The crate's
# build.rs reads the commit from git where the submodule is checked out and
# from this file otherwise, and the element logs it. git ignores the file and
# the crate's manifest lists it in include, so cargo packages it anyway. The
# script can be run as often as needed and always leaves the same result for
# the same submodule commit.

$crate = Join-Path $RepoRoot "derived-properties"
$vendor = Join-Path $crate "vendor"
# Without .git, git would search the folders above and answer with the commit
# of this repository instead.
if (-not (Test-Path (Join-Path $vendor ".git"))) {
    throw "The derived-properties scripts are not at '$vendor'. Run " +
        "'git submodule update --init --recursive' first, or clone with " +
        "--recurse-submodules."
}

$commit = (git -C $vendor rev-parse HEAD 2>$null)
if (-not ($commit -match '^[0-9a-f]{40}$')) {
    throw "Could not read the derived-properties commit from '$vendor'."
}

$source = Join-Path $crate "SOURCE.txt"
Set-Content -Path $source -NoNewline -Encoding utf8 `
    -Value "repository=51Degrees/derived-properties`ncommit=$commit`n"
Write-Output "Recorded derived-properties commit $commit in '$source'"
