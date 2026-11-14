<#
.SYNOPSIS
Moves the examples' lock to the latest versions on crates.io. common-ci's
nightly package update runs this and opens a pull request when the lock
changed.

.DESCRIPTION
A fresh clone builds the examples with the committed examples/Cargo.lock, so
the lock decides which release of each crate someone who has just cloned the
repository runs. Each crate is released at its own version, so after a
release the lock names the crates of the release before until this moves it.
The examples take the 51Degrees crates and every other dependency from
crates.io, and all of them move together, as the other languages' package
updates move theirs. The library workspace's own lock is not changed.

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
Push-Location $examples
try {
    cargo update
} finally {
    Pop-Location
}
if ((Get-Content -Raw $lock) -ceq $before) {
    Write-Output "examples/Cargo.lock already names the latest versions."
} else {
    Write-Output "examples/Cargo.lock moved to the latest versions."
}
