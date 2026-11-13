param (
    # The root of the rust working copy. Defaults to the parent of this
    # script, which is right for a developer checkout, and can be passed
    # explicitly by a caller that runs from another directory.
    [string]$RepoRoot = (Split-Path $PSScriptRoot -Parent)
)
$ErrorActionPreference = "Stop"

# Copies the IP Intelligence translation files from the data repository,
# which is the data submodule of the cxx submodule, into the translations
# folder of the ip-intelligence-translation crate, so the crate published to
# crates.io carries them. A build in the workspace reads the submodule
# directly, so the copy is needed only before packaging, as for the C sources
# ci/copy-cxx-sources.ps1 copies.
#
# Cargo cannot package the files from the submodule itself, because it leaves
# out any include pattern that reaches outside the crate's own folder. git
# ignores the copy, and the crate's manifest lists the folder in include so
# that cargo packages it anyway. The script can be run as often as needed and
# always leaves the same result for the same submodule commit.

$data = Join-Path $RepoRoot "cxx/data"
$from = Join-Path $data "ip-intelligence/Translations"
if (-not (Test-Path (Join-Path $from "countrycodes.en_GB.yml"))) {
    throw "The translations were not found at '$from'. Run " +
        "'git submodule update --init --recursive' first, or clone with " +
        "--recurse-submodules."
}

# The commit the copy was taken from, recorded beside the copy.
$commit = $null
if (Test-Path (Join-Path $data ".git")) {
    $commit = (git -C $data rev-parse HEAD 2>$null)
}
if (-not ($commit -match '^[0-9a-f]{40}$')) {
    $commit = (git -C (Join-Path $RepoRoot "cxx") rev-parse "HEAD:data" 2>$null)
}
if (-not ($commit -match '^[0-9a-f]{40}$')) {
    throw "Could not determine the data repository commit to record beside the copy."
}

$target = Join-Path $RepoRoot "ip-intelligence-translation/translations"
if (Test-Path $target) {
    Remove-Item -Path $target -Recurse -Force
}
$count = 0
foreach ($folder in @("", "OSM")) {
    $to = Join-Path $target $folder
    $null = New-Item -ItemType Directory -Path $to -Force
    foreach ($file in Get-ChildItem -Path (Join-Path $from $folder) -Filter "*.yml" -File) {
        Copy-Item -Path $file.FullName -Destination $to
        $count++
    }
}
Set-Content -Path (Join-Path $target "SOURCE.txt") -NoNewline -Encoding utf8 `
    -Value "repository=51Degrees/data`ncommit=$commit`n"
Write-Output "Copied $count translation files at $commit into '$target'"
