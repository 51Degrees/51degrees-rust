<#
.SYNOPSIS
Moves the crates of this repository in the examples' lock to their latest
versions on crates.io. common-ci's nightly package update runs this and opens
a pull request when the lock changed.

.DESCRIPTION
A fresh clone builds the examples with the committed examples/Cargo.lock, so
the lock decides which release of each crate someone who has just cloned the
repository runs. Each crate is released at its own version, so after a
release the lock names the crates of the release before until this moves it.

The crates moved are the ones examples/source.toml patches, which are this
repository's crates, and only those found in the lock. Cargo moves each to
the highest version the examples' requirements allow, and moves another
dependency only when one of them needs it to, so the pull request holds what
a release changed and nothing else. The library workspace's lock is not
changed.

.PARAMETER RepoName
The folder the repository is checked out in.

.PARAMETER OrgName
The organisation, which common-ci passes and this does not need.

.PARAMETER DryRun
Passed by common-ci. The lock is moved either way, and common-ci decides
whether to push it.
#>
param (
    [Parameter(Mandatory)][string]$RepoName,
    [string]$OrgName,
    [bool]$DryRun = $false
)
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

$examples = Join-Path $RepoName "examples"
$lock = Join-Path $examples "Cargo.lock"
if (-not (Test-Path $lock -PathType Leaf)) {
    throw "No examples/Cargo.lock was found in $RepoName."
}
$before = Get-Content -Raw $lock
$patched = @(Get-Content (Join-Path $examples "source.toml") |
    Where-Object { $_ -match '^([A-Za-z0-9_-]+)\s*=\s*\{\s*path\s*=' } |
    ForEach-Object { $Matches[1] })
$crates = @($patched | Where-Object {
    $before -match "(?m)^name = `"$([regex]::Escape($_))`"\r?$" })
if ($crates.Count -eq 0) {
    throw "None of the crates examples/source.toml patches is in " +
        "examples/Cargo.lock."
}
Push-Location $examples
try {
    cargo update @($crates | ForEach-Object { "-p"; $_ })
} finally {
    Pop-Location
}
if ((Get-Content -Raw $lock) -ceq $before) {
    Write-Output ("examples/Cargo.lock already names the latest version of " +
        "the $($crates.Count) crates of this repository it holds.")
} else {
    Write-Output ("examples/Cargo.lock moved the $($crates.Count) crates of " +
        "this repository it holds to their latest versions.")
}
