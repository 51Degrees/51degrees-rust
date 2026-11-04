<#
.SYNOPSIS
Builds the crate packages for common-ci's shared publish.

.DESCRIPTION
Writes the version into every crate, copies the OWID source into fodid and the
C sources into the -sys crates, and tests the examples against the crates at
that version. Then packages every crate in ci/crates.txt with cargo, which
builds each package against the others as a local registry, so a crate that
would not build from its package fails here rather than on crates.io. The
packages, and the commit they were built from, go to package/ beside the
clone, which the shared publish keeps for its publish step.

Runs from the common-ci checkout, with this repository cloned beside it.

.PARAMETER RepoName
The folder this repository is cloned into.

.PARAMETER Version
The version being released.
#>
param (
    [Parameter(Mandatory)][string]$RepoName,
    [Parameter(Mandatory)][string]$Version
)
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

# Keep the runner's secrets and the GitHub token out of reach of the build
# scripts cargo runs.
Remove-Item -Path Env:SECRETS, Env:GITHUB_TOKEN -ErrorAction SilentlyContinue

$repo = (Resolve-Path $RepoName).Path
$package = (New-Item -ItemType Directory -Force -Path "package").FullName

rustup toolchain install stable --profile minimal
rustup default stable

& "$repo/ci/set-crate-versions.ps1" -Version $Version
& "$repo/ci/copy-owid-source.ps1"
& "$repo/ci/copy-cxx-sources.ps1"
# A clone can leave the data files in the cxx submodule as Git LFS pointer
# files, which the examples would read as corrupt data, so fetch their content
# before the examples run.
git -C "$repo/cxx" submodule foreach --recursive 'git lfs install --local && git lfs pull'
& "$repo/ci/test-examples-against-source.ps1" -Version $Version

$crates = @(Get-Content -Path "$repo/ci/crates.txt" | Where-Object { $_ -notmatch '^\s*(#|$)' })
$selection = $crates | ForEach-Object { "-p"; $_ }
Push-Location $repo
try {
    # --allow-dirty, because the version and the source copies are written into
    # the tree on the build machine and never committed.
    cargo package --allow-dirty @selection
} finally {
    Pop-Location
}

$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { "$repo/target" }
foreach ($crate in $crates) {
    Copy-Item -Path "$target/package/$crate-$Version.crate" -Destination $package
}
git -C $repo rev-parse HEAD | Set-Content -Path (Join-Path $package "commit.txt") -NoNewline
Write-Output "Packaged $($crates.Count) crates at $Version from $(Get-Content -Raw (Join-Path $package "commit.txt"))."
