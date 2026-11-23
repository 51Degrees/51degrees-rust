param (
    [Parameter(Mandatory = $true)]
    [string]$RepoName
)
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

# The OWID source is compiled into the fodid crate as a private module, and
# the copy is ignored by git, so it has to be put in place before anything
# reads the crate. Every later stage depends on this having run.
& "$PSScriptRoot/copy-owid-source.ps1"

Push-Location $RepoName
try {
    # Format is checked first because it is the cheapest failure and the most
    # common one.
    Write-Output "::group::cargo fmt"
    cargo fmt --all -- --check
    Write-Output "::endgroup::"

    Write-Output "::group::cargo build"
    cargo build --workspace --all-targets
    Write-Output "::endgroup::"
} finally {
    Pop-Location
}
