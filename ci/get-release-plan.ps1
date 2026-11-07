<#
.SYNOPSIS
Prints which crates a release of HEAD publishes, each at its own version, and
writes the plan for the publish steps that follow.

.DESCRIPTION
Each crate is tagged '<crate>/<version>'. A crate is released when a file
under its folder, or under a submodule it is built from, changed since its
last tag, and with it every crate that depends on it, because the requirement
on it moves. Only the patch number moves. The rules are common-ci's
steps/release-plan.ps1, read from ci/packages.json.

Nothing is changed, so this is the dry run of a publish as well as its first
step. The plan is read before anything is tagged, because the tags a publish
makes change what the plan would say.

.PARAMETER RepoPath
The repository to plan a release of. Defaults to the one this script sits in.

.PARAMETER OutFile
Where to write the plan as JSON, for the steps of a publish that follow. It
holds the releases in the order to publish them, and the version every
published crate has after the release, including those that keep the version
they last released.
#>
param (
    [string]$RepoPath = (Split-Path $PSScriptRoot -Parent),
    [string]$OutFile
)
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
. (Join-Path $PSScriptRoot 'packages.ps1')

$packages = Get-Packages $RepoPath
$plan = @(Get-CrateReleasePlan $RepoPath $packages)
$versions = Get-CrateVersions $RepoPath $plan $packages
$published = @($packages | Where-Object Publish -eq $true)

$head = git -C $RepoPath rev-parse --short HEAD
if ($plan.Count -eq 0) {
    Write-Output ("No crate changed since its last tag, so a release of " +
        "$head publishes nothing.")
} else {
    Write-Output ("A release of $head publishes $($plan.Count) of the " +
        "$($published.Count) crates, in this order:")
    $plan | Format-Table @{ Label = 'Crate'; Expression = { $_.Name } },
        @{ Label = 'Last'; Expression = { "$($_.Previous)" } },
        Version, Reason -AutoSize | Out-String -Width 120 |
        ForEach-Object { $_.TrimEnd() } | Write-Output
    $kept = $published.Count - $plan.Count
    if ($kept -gt 0) {
        Write-Output ""
        Write-Output "The other $kept keep the version they last released."
    }
}

$releases = @($plan | ForEach-Object {
    [ordered]@{
        Name = $_.Name
        Version = "$($_.Version)"
        Previous = "$($_.Previous)"
        Reason = $_.Reason
    }
})

if ($OutFile) {
    [ordered]@{ Releases = $releases; Versions = $versions } |
        ConvertTo-Json -Depth 4 | Set-Content -Path $OutFile -Encoding utf8
}

if ($env:GITHUB_OUTPUT) {
    "count=$($plan.Count)" | Out-File -Append -Encoding utf8 $env:GITHUB_OUTPUT
}

if ($env:GITHUB_STEP_SUMMARY) {
    $summary = @("## Release plan for $head", "")
    if ($plan.Count -eq 0) {
        $summary += "No crate changed since its last tag, so nothing is published."
    } else {
        $summary += "| Crate | Last | Version | Reason |"
        $summary += "| --- | --- | --- | --- |"
        foreach ($release in $plan) {
            $summary += "| $($release.Name) | $($release.Previous) | " +
                "$($release.Version) | $($release.Reason) |"
        }
        $kept = $published.Count - $plan.Count
        if ($kept -gt 0) {
            $summary += ""
            $summary += "The other $kept crates keep the version they last released."
        }
    }
    $summary | Out-File -Append -Encoding utf8 $env:GITHUB_STEP_SUMMARY
}
