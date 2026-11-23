param (
    [Parameter(Mandatory = $true)]
    [string]$RepoName
)
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

Push-Location $RepoName
try {
    Write-Output "::group::cargo test"
    cargo test --workspace
    Write-Output "::endgroup::"

    # clippy and the documentation build inherit the warning settings
    # setup-environment.ps1 exported, so any warning fails here.
    Write-Output "::group::cargo clippy"
    cargo clippy --workspace --all-targets -- -D warnings
    Write-Output "::endgroup::"

    Write-Output "::group::cargo doc"
    cargo doc --workspace --no-deps
    Write-Output "::endgroup::"
} finally {
    Pop-Location
}
