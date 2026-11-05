<#
.SYNOPSIS
Writes one version into every crate manifest and every requirement between
them, so the workspace releases as a set.

.DESCRIPTION
The other five languages take their version from a tag through GitVersion and
publish the whole family at that one number. Rust carried a version in each
manifest instead, which meant three places to change, three numbers that could
drift apart, and a release decided by whoever edited a file rather than by a
tag.

This makes Rust behave like the rest. The publish workflow computes the next
version from the tags and calls this, so nothing in the repository states a
version and no commit can release one by accident.

Both the crate versions and the requirements between them are written. A
requirement left behind is worse than one never written: `cargo publish`
accepts it and the crate then cannot resolve from the registry, which is how
`fodid-client` came to require `fodid 4.6` after 4.6 was yanked.

Each requirement is written exact, as `=` and the version, so a crate of one
release builds only with the other crates of that release and the C sources
they carry. A plain version is a caret requirement, which would let a fresh
build of an older crate take a newer release of the crates beneath it.
ci/check-crate-requirements.ps1 checks the manifests before they are written,
so the committed requirements are exact too, and again after, so nothing the
pattern below misses reaches crates.io.

.PARAMETER Version
The version to write, such as 4.5.6.

.PARAMETER Root
The workspace root. Defaults to the repository this script sits in.
#>
param (
    [Parameter(Mandatory)][string]$Version,
    [string]$Root = (Join-Path $PSScriptRoot "..")
)
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

if ($Version -notmatch '^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$') {
    throw "'$Version' is not a version. Expected something like 4.5.6."
}

$checkRequirements = Join-Path $PSScriptRoot "check-crate-requirements.ps1"
& $checkRequirements -Root $Root

# Every member of the workspace, read from the workspace manifest rather than
# listed here, so a crate added to the workspace is versioned without anyone
# remembering to add it in a second place. That is the mistake ci/crates.txt
# records having been made once already, with two lists that drifted apart.
$workspace = Get-Content -Raw -Path (Join-Path $Root "Cargo.toml")
$membersMatch = [regex]::Match($workspace, 'members\s*=\s*\[(?<list>[^\]]*)\]', 'Singleline')
if (-not $membersMatch.Success) { throw "No workspace members in Cargo.toml" }
$memberDirs = [regex]::Matches($membersMatch.Groups["list"].Value, '"([^"]+)"') |
    ForEach-Object { $_.Groups[1].Value }
if ($memberDirs.Count -eq 0) { throw "Workspace members list is empty" }

$manifests = @()
$members = @()
foreach ($dir in $memberDirs) {
    $manifest = Join-Path $dir "Cargo.toml"
    $full = Join-Path $Root $manifest
    if (-not (Test-Path $full)) { throw "Workspace names ${dir} but $manifest is missing" }
    $manifests += $manifest
    # The crate name, which is what another manifest requires it by and is not
    # always the directory name: fodid-cloud publishes as fiftyone-fodid-cloud.
    $nameMatch = [regex]::Match((Get-Content -Raw -Path $full), '(?m)^name\s*=\s*"([^"]+)"')
    if ($nameMatch.Success) { $members += $nameMatch.Groups[1].Value }
}

$changed = 0
foreach ($relative in $manifests) {
    $path = Join-Path $Root $relative
    if (-not (Test-Path $path)) { throw "Manifest not found: $path" }
    $text = Get-Content -Raw -Path $path

    # The crate's own version is the first `version = "..."` of the file, which
    # sits under [package]. A dependency's version never appears at the start
    # of a line, so anchoring to the line start cannot touch one.
    $before = $text
    $text = [regex]::Replace(
        $text,
        '(?m)^version\s*=\s*"[^"]+"',
        "version = `"$Version`"",
        1)

    # A requirement on another member of this workspace, in either order of
    # keys, so { path = "../fodid", version = "=4.5.5" } and
    # { version = "=4.5.5", path = "../fodid" } are both rewritten, exact.
    foreach ($member in $members) {
        $text = [regex]::Replace(
            $text,
            "(?m)^(\s*$([regex]::Escape($member))\s*=\s*\{[^}]*?version\s*=\s*)""[^""]+""",
            "`${1}`"=$Version`"")
    }

    if ($text -ne $before) {
        Set-Content -Path $path -Value $text -NoNewline
        $changed++
        Write-Output "  $relative -> $Version"
    } else {
        Write-Output "  $relative already at $Version"
    }
}

Write-Output "Wrote $Version into $changed of $($manifests.Count) manifests."
& $checkRequirements -Root $Root
