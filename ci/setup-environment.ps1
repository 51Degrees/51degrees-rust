param (
    [Parameter(Mandatory = $true)]
    [string]$RepoName
)
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

# Readies the cxx submodule, which holds the C sources the -sys crates compile
# and, at cxx/data, the data repository that links the on-premise data files
# the tests read. common-ci clones the repository with its submodules, so the
# update below normally finds nothing to do, and it is there so the script
# also works on a clone made without them.
#
# Every variable set below is an environment variable rather than a
# PowerShell one, so it survives into build-project.ps1 and
# run-unit-tests.ps1, which common-ci runs in this same process.

Write-Output "::group::Check out the cxx submodule"
git -C $RepoName submodule update --init --recursive
Write-Output "::endgroup::"

# fetch-assets.ps1, which common-ci runs first, has copied the two data files
# the tests read into the data repository, so its script finds them there and
# keeps them. It downloads a file only when one is missing. The tests skip
# without the files, so a download that fails stops the build here. The
# script sets an exit code only when it fails, so any code an earlier command
# left is cleared first.
Write-Output "::group::Download the data files"
$global:LASTEXITCODE = 0
& (Join-Path $RepoName "cxx/data/get-data-files.ps1") `
    51Degrees-LiteV4.1.hash 51Degrees-IPIV4AsnIpiV41.ipi
if ($LASTEXITCODE) {
    throw "The data files the tests read were not downloaded."
}
Write-Output "::endgroup::"

# Any compiler or documentation warning fails the build, which is the rule
# pull-request.yml sets and which the crates are written to meet.
$env:RUSTFLAGS = "-D warnings"
$env:RUSTDOCFLAGS = "-D warnings"
$env:CARGO_TERM_COLOR = "always"

# The hosted images carry a Rust toolchain, and the format and lint stages
# need two components that are not always part of a minimal profile, so ask
# for them rather than assume them.
Write-Output "::group::Rust toolchain"
rustup component add clippy rustfmt
rustc --version
cargo --version
cmake --version
Write-Output "::endgroup::"
