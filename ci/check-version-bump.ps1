<#
.SYNOPSIS
Refuses a release in which any crate moves its major or minor number, unless
a person has allowed it for this run.

.DESCRIPTION
Each crate the plan releases is checked twice, and every refused crate is
named before the run stops, so nothing is written, published or tagged.

1. Against its own '<crate>/<version>' tags, with common-ci's
   steps/check-version-bump.ps1, so a crate is compared with its own
   releases rather than with the highest version of any crate. A crate with
   no tag of its own, released at its FirstVersion, has nothing here to be
   compared with, and common-ci's check would refuse it on a branch that
   holds no tags, such as rc, so it is checked against crates.io alone.
2. Against crates.io, because a planned version can reach the release with
   no tag of the crate's own to compare with, from a FirstVersion in
   ci/packages.json, or from a tag pushed by hand that moved the crate's
   last release. The version must keep the major and minor of the crate's
   highest version on crates.io that is not yanked. A crate crates.io does
   not have must keep the major and minor that the crates of
   ci/packages.json on crates.io share, and the check stops when they do not
   all share one, as there is then nothing to compare it with.

A move in either check is allowed only when ALLOW_VERSION_BUMP is 'true'.

.PARAMETER RepoName
The directory the repository is checked out into, whose tags are each crate's
release history.

.PARAMETER PlanFile
The plan ci/get-release-plan.ps1 wrote.

.PARAMETER Registry
Each crate's record as crates.io's API gives it, by name, in place of asking
crates.io. A crate it does not name is one crates.io does not have. For tests.
#>
param (
    [Parameter(Mandatory)][string]$RepoName,
    [Parameter(Mandatory)][string]$PlanFile,
    [hashtable]$Registry
)
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot 'packages.ps1')
. (Join-Path $PSScriptRoot 'crates-io.ps1')

$check = Join-Path $CommonCiRoot 'steps' 'check-version-bump.ps1'
$plan = Read-ReleasePlan $PlanFile
if ($plan.Releases.Count -eq 0) {
    Write-Output "The plan releases no crate, so there is no version to check."
    exit 0
}
$allowed = $env:ALLOW_VERSION_BUMP -eq 'true'
$refused = @()

# 1. Each crate against its own tags.
foreach ($release in $plan.Releases) {
    Write-Output "$($release.Name) $($release.Version) against its tags:"
    if (@(Get-ReleasedVersions $RepoName $release.Name).Count -eq 0) {
        Write-Output ("It has no tag of its own, so it is checked against " +
            "crates.io alone.")
        continue
    }
    & $check -Version $release.Version -RepoName $RepoName `
        -TagPrefix $release.Name
    if ($LASTEXITCODE -ne 0) {
        $refused += "$($release.Name) $($release.Version) against its tags"
    }
    $global:LASTEXITCODE = 0
}

# 2. Each crate against crates.io. The highest version of every published
# crate is read only when the plan holds a crate crates.io does not have.
$highest = @{}
foreach ($release in $plan.Releases) {
    $highest[$release.Name] = Get-CratesIoHighest (Get-CratesIoCrate $release.Name $Registry)
}
$unknown = @($plan.Releases | Where-Object { $null -eq $highest[$_.Name] })
$shared = $null
if ($unknown.Count -gt 0) {
    foreach ($package in (Get-Packages $RepoName | Where-Object Publish -eq $true)) {
        if ($highest.ContainsKey($package.Name) -eq $false) {
            $highest[$package.Name] = Get-CratesIoHighest (Get-CratesIoCrate $package.Name $Registry)
        }
    }
    $lines = @($highest.Values | Where-Object { $null -ne $_ } |
        ForEach-Object { "$($_.Major).$($_.Minor)" } | Sort-Object -Unique)
    if ($lines.Count -ne 1) {
        $found = if ($lines.Count -eq 0) { 'none' } else { $lines -join ', ' }
        throw "crates.io does not have $($unknown.Name -join ', '), so its " +
            "version is compared with the major and minor that the crates of " +
            "ci/packages.json on crates.io share, and they do not share one " +
            "(found $found). Nothing is published."
    }
    $shared = $lines[0]
}

foreach ($release in $plan.Releases) {
    $name = $release.Name
    $version = "$($release.Version)"
    $parts = [regex]::Match($version, '^(\d+)\.(\d+)\.\d+')
    if ($parts.Success -eq $false) {
        $refused += "$name $version, which is not a version"
        continue
    }
    $planned = "$($parts.Groups[1].Value).$($parts.Groups[2].Value)"
    $against = $highest[$name]
    if ($null -ne $against) {
        $line = "$($against.Major).$($against.Minor)"
        $what = "$line, the major and minor of $against, its highest version " +
            "on crates.io"
    } else {
        $line = $shared
        $what = "$line, the major and minor that the crates on crates.io " +
            "share, as crates.io does not have $name"
    }
    if ($planned -ceq $line) {
        Write-Output "$name $version against crates.io keeps $what."
    } elseif ($allowed -eq $true) {
        Write-Output ("$name $version against crates.io moves from $what, " +
            "allowed by ALLOW_VERSION_BUMP.")
    } else {
        Write-Output ("::error title=Version bump refused::$name $version " +
            "moves from $what.")
        $refused += "$name $version against crates.io"
    }
}

if ($refused.Count -gt 0) {
    Write-Output ""
    Write-Output ("Nothing is published. To move a major or minor number " +
        "deliberately, dispatch the publish workflow with allow-version-bump. " +
        "A published version can never be reclaimed, and a minor move means " +
        "a new documentation version.")
    throw "The version of $($refused.Count) crate(s) was refused, so " +
        "nothing is published: $($refused -join ', ')"
}
Write-Output ("Every crate in the plan keeps its major and minor number, or " +
    "was allowed to move them.")
