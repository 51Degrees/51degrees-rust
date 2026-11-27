<#
.SYNOPSIS
Writes each crate's version into its manifest, and the same version into every
requirement on that crate from another crate of the workspace.

.DESCRIPTION
No version is committed in the manifests. A publish works out each crate's
version from its own '<crate>/<version>' tags (see ci/get-release-plan.ps1)
and passes the plan here, so a crate the plan releases gets its next version
and a crate it does not release keeps the version it last released. Every
requirement on a crate then names a version that is on crates.io or is being
published with it.

Both the crate versions and the requirements between them are written. A
requirement left behind is worse than one never written, because
`cargo publish` accepts it and the crate then cannot resolve from the
registry.

Each requirement keeps its operator and takes the required crate's version.
The committed requirements are exact, as `=` and a version, so a crate of one
release builds only with the crates of that release beneath it and the C
sources they carry, where a plain version is a caret requirement that would
let a fresh build of an older crate take a newer release of the crates
beneath it. Dev-dependencies are the exception and give the plain version.
They never reach a project's build, and an exact one would force a new
release of a crate whenever a crate its tests use is released.
ci/check-crate-requirements.ps1 checks the manifests before they are written,
so the committed requirements are in those forms, and again after, so
nothing the pattern below misses reaches crates.io.

.PARAMETER Version
One version for every crate, such as 4.5.6. The examples check uses this to
put every crate above every version on crates.io.

.PARAMETER PlanFile
The plan ci/get-release-plan.ps1 wrote, whose Versions give every published
crate its own version.

.PARAMETER Root
The workspace root. Defaults to the repository this script sits in.
#>
param (
    [Parameter(Mandatory, ParameterSetName = 'One')][string]$Version,
    [Parameter(Mandatory, ParameterSetName = 'Plan')][string]$PlanFile,
    [string]$Root = (Join-Path $PSScriptRoot "..")
)
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

$VersionPattern = '^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$'

# Every member of the workspace, read from the workspace manifest rather than
# listed here, so a crate added to the workspace is versioned without anyone
# remembering to add it in a second place.
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

# The version each crate is given. A member with none, which only a plan can
# leave out and only for a crate that is never published, is left alone.
$versionOf = @{}
if ($PSCmdlet.ParameterSetName -eq 'One') {
    foreach ($member in $members) { $versionOf[$member] = $Version }
} else {
    $plan = Get-Content -Raw -Path $PlanFile | ConvertFrom-Json -AsHashtable
    if ($null -eq $plan.Versions -or $plan.Versions.Count -eq 0) {
        throw "$PlanFile gives no crate a version"
    }
    foreach ($name in $plan.Versions.Keys) {
        if ($members -notcontains $name) {
            throw "$PlanFile gives a version to $name, which is not a member of the workspace"
        }
        $versionOf[$name] = "$($plan.Versions[$name])"
    }
    # Every published crate needs a version, or a requirement on it would keep
    # whatever the committed manifest happens to say.
    $published = Get-Content -Path (Join-Path $Root "ci/crates.txt") |
        ForEach-Object { $_.Trim() } |
        Where-Object { $_ -and -not $_.StartsWith("#") }
    $missing = @($published | Where-Object { -not $versionOf.ContainsKey($_) })
    if ($missing.Count -gt 0) {
        throw "$PlanFile gives no version to $($missing -join ', '), which ci/crates.txt publishes"
    }
}
foreach ($name in $versionOf.Keys) {
    if ($versionOf[$name] -notmatch $VersionPattern) {
        throw "'$($versionOf[$name])' for $name is not a version. Expected something like 4.5.6."
    }
}

$checkRequirements = Join-Path $PSScriptRoot "check-crate-requirements.ps1"
& $checkRequirements -Root $Root

$changed = 0
foreach ($relative in $manifests) {
    $path = Join-Path $Root $relative
    if (-not (Test-Path $path)) { throw "Manifest not found: $path" }
    $text = Get-Content -Raw -Path $path
    $before = $text
    $name = [regex]::Match($text, '(?m)^name\s*=\s*"([^"]+)"').Groups[1].Value

    # The crate's own version is the first `version = "..."` of the file, which
    # sits under [package]. A dependency's version never appears at the start
    # of a line, so anchoring to the line start cannot touch one.
    if ($versionOf.ContainsKey($name)) {
        $text = [regex]::Replace(
            $text,
            '(?m)^version\s*=\s*"[^"]+"',
            "version = `"$($versionOf[$name])`"",
            1)
    }

    # A requirement on another member of this workspace, in either order of
    # keys, so { path = "../fodid", version = "=4.5.5" } and
    # { version = "=4.5.5", path = "../fodid" } are both rewritten. The
    # operator before the version, if any, is kept.
    foreach ($member in $versionOf.Keys) {
        $text = [regex]::Replace(
            $text,
            "(?m)^(\s*$([regex]::Escape($member))\s*=\s*\{[^}]*?version\s*=\s*"")" +
                '(=|\^|~|>=|<=|>|<)?\s*[^"]*"',
            "`${1}`${2}$($versionOf[$member])`"")
    }

    $own = if ($versionOf.ContainsKey($name)) { $versionOf[$name] } else { "its own version" }
    if ($text -ne $before) {
        Set-Content -Path $path -Value $text -NoNewline
        $changed++
        Write-Output "  $relative -> $own"
    } else {
        Write-Output "  $relative already at $own"
    }
}

Write-Output "Wrote the versions into $changed of $($manifests.Count) manifests."
& $checkRequirements -Root $Root
