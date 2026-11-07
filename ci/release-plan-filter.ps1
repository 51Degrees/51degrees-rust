<#
.SYNOPSIS
Called by common-ci's steps/release-plan.ps1 for each crate with changed
files. Returns the files whose change releases the crate.

.DESCRIPTION
A crate's Cargo.toml counts only when something other than the [package]
keys that describe the crate on crates.io changed, being its description,
the pages it points to (homepage, documentation, repository and readme), its
keywords, categories and authors. Those keys change no code a user builds,
so a change to them alone waits for the crate's next real release. Every
other file counts.

.PARAMETER RepoPath
The repository.

.PARAMETER Package
The crate's entry from ci/packages.json.

.PARAMETER From
The crate's last tag, which the files changed since.

.PARAMETER Files
The changed files left after the plan's ignore lists, from the repository
root.
#>
param (
    [Parameter(Mandatory = $true)]
    [string]$RepoPath,
    [Parameter(Mandatory = $true)]
    $Package,
    [Parameter(Mandatory = $true)]
    [string]$From,
    [string[]]$Files = @()
)
$ErrorActionPreference = "Stop"

# The [package] keys that only describe the crate. A key may also be given as
# '<key>.workspace = true'.
$DescriptiveKeys = @(
    'authors', 'categories', 'description', 'documentation', 'homepage',
    'keywords', 'readme', 'repository'
)

# The manifest's text without the descriptive [package] keys, one entry per
# line with its spaces trimmed, so a change to only those keys leaves it equal.
function Get-CountedManifest([string[]]$Lines) {
    $table = ''
    $skipping = $false
    foreach ($line in $Lines) {
        $text = $line.Trim()
        if ($skipping) {
            # A value written over several lines ends at the line closing it.
            if ($text -match '^[^#]*\]') {
                $skipping = $false
            }
            continue
        }
        if ($text -match '^\[\[?([^\]]+)\]\]?$') {
            $table = $Matches[1].Trim()
        } elseif ($table -eq 'package' -and
            $text -match '^([A-Za-z0-9_-]+)(\.workspace)?\s*=\s*(.*)$' -and
            $DescriptiveKeys -contains $Matches[1]) {
            $value = $Matches[3]
            if ($value.StartsWith('[') -and $value -notmatch '\]') {
                $skipping = $true
            }
            continue
        }
        if ($text -ne '') {
            $text
        }
    }
}

function Get-ManifestLines([string]$Ref, [string]$Path) {
    $PSNativeCommandUseErrorActionPreference = $false
    $lines = @(git -C $RepoPath show "${Ref}:$Path" 2>$null)
    $found = $LASTEXITCODE -eq 0
    $global:LASTEXITCODE = 0
    if (-not $found) {
        return $null
    }
    return $lines
}

$manifest = "$($Package.Path)/Cargo.toml"
foreach ($file in $Files) {
    if ($file -ne $manifest) {
        $file
        continue
    }
    $before = Get-ManifestLines $From $manifest
    $after = Get-ManifestLines 'HEAD' $manifest
    if ($null -eq $before -or $null -eq $after) {
        $file
        continue
    }
    $changed = Compare-Object @(Get-CountedManifest $before) `
        @(Get-CountedManifest $after) -SyncWindow 0
    if ($null -ne $changed) {
        $file
    }
}
