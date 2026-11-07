<#
.SYNOPSIS
Refuses a release in which any crate moves its major or minor number, unless
a person has allowed it for this run.

.DESCRIPTION
Each crate has its own version, so each crate the plan releases is compared
with its own '<crate>/<version>' tags rather than with the highest version of
any crate. The comparison is common-ci's steps/check-version-bump.ps1, which
allows the move only when ALLOW_VERSION_BUMP is 'true'. Every refused crate is
named before the run stops, and nothing is written, published or tagged.

.PARAMETER RepoName
The directory the repository is checked out into, whose tags are each crate's
release history.

.PARAMETER PlanFile
The plan ci/get-release-plan.ps1 wrote.
#>
param (
    [Parameter(Mandatory)][string]$RepoName,
    [Parameter(Mandatory)][string]$PlanFile
)
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot 'packages.ps1')

$check = Join-Path $CommonCiRoot 'steps' 'check-version-bump.ps1'
$plan = Read-ReleasePlan $PlanFile
if ($plan.Releases.Count -eq 0) {
    Write-Output "The plan releases no crate, so there is no version to check."
    exit 0
}
$refused = @()
foreach ($release in $plan.Releases) {
    Write-Output "$($release.Name) $($release.Version):"
    & $check -Version $release.Version -RepoName $RepoName `
        -TagPrefix $release.Name
    if ($LASTEXITCODE -ne 0) {
        $refused += "$($release.Name) $($release.Version)"
    }
    $global:LASTEXITCODE = 0
}

if ($refused.Count -gt 0) {
    throw "The version of $($refused.Count) crate(s) was refused, so " +
        "nothing is published: $($refused -join ', ')"
}
Write-Output ("Every crate in the plan moves only its patch number, or was " +
    "allowed to move more.")
