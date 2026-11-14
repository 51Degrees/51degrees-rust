# Dot-sourced by the release scripts in this folder. Loads common-ci's
# steps/release-plan.ps1, which works out from ci/packages.json and the
# '<crate>/<version>' tags which crates a release publishes and at which
# version, and adds what is particular to the crates here.
#
# common-ci is found at COMMON_CI when that is set, then in the folder
# common-ci inside this repository, where the workflows check it out, then in
# a checkout of common-ci beside this repository.
$releasePlan = @(
    $env:COMMON_CI,
    (Join-Path $PSScriptRoot '..' 'common-ci'),
    (Join-Path $PSScriptRoot '..' '..' 'common-ci')) |
    Where-Object { $_ } |
    ForEach-Object { Join-Path $_ 'steps' 'release-plan.ps1' } |
    Where-Object { Test-Path $_ } |
    Select-Object -First 1
if ($null -eq $releasePlan) {
    throw "common-ci's steps/release-plan.ps1 was not found. Set COMMON_CI " +
        "to a checkout of common-ci."
}
. $releasePlan
$CommonCiRoot = Split-Path (Split-Path $releasePlan -Parent) -Parent
Remove-Variable releasePlan

# The crates ci/crates.txt publishes, in the order it lists them, which puts
# every crate after each crate its published manifest requires from
# crates.io. ci/check-packages.ps1 holds the list to that order.
function Get-PublishOrder([string]$RepoPath) {
    return @(Get-Content (Join-Path $RepoPath 'ci/crates.txt') |
        ForEach-Object { $_.Trim() } |
        Where-Object { $_ -and -not $_.StartsWith('#') })
}

# The highest repository tag, such as 4.5.19, and the published crates whose
# own tags are all below it. Repository tags were made by the publish that
# released every crate at one version, and none is made now. A crate whose
# tags are below the last one was released again under it after its tags were
# started, so its tags no longer mark the commit crates.io's highest version
# came from. Crates with no tag of their own are left out.
function Get-CratesBehindRelease([string]$RepoPath, $Packages = $null) {
    if ($null -eq $Packages) {
        $Packages = Get-Packages $RepoPath
    }
    $release = @(git -C $RepoPath tag --list |
        Where-Object { $_ -match '^\d+\.\d+\.\d+$' } |
        Sort-Object { [version]$_ }) | Select-Object -Last 1
    $behind = @()
    if ($null -ne $release) {
        foreach ($package in ($Packages | Where-Object Publish -eq $true)) {
            $released = @(Get-ReleasedVersions $RepoPath $package.Name)
            if ($released.Count -gt 0 -and $released[-1] -lt [version]$release) {
                $behind += $package.Name
            }
        }
    }
    return [pscustomobject]@{ Release = $release; Crates = $behind }
}

# The plan for HEAD, from common-ci's Get-ReleasePlan, in the order the crates
# are published. Every published crate needs a tag of its own to count from,
# so a crate with none and no FirstVersion stops the plan here with what to
# do, rather than with the shared step's general message. So does a crate
# whose tags are behind the last repository tag, because its next version
# would repeat one crates.io already has and be tagged on the wrong commit.
function Get-CrateReleasePlan([string]$RepoPath, $Packages = $null) {
    if ($null -eq $Packages) {
        $Packages = Get-Packages $RepoPath
    }
    $stale = Get-CratesBehindRelease $RepoPath $Packages
    if ($stale.Crates.Count -gt 0) {
        throw "The repository tag $($stale.Release) is above every " +
            "'<crate>/<version>' tag of $($stale.Crates.Count) crate(s): " +
            "$($stale.Crates -join ', '). They were released at " +
            "$($stale.Release) after their tags were made, so their tags do " +
            "not mark what they last published. Tag every crate at " +
            "$($stale.Release) with ci/start-crate-tags.ps1 -Push, then run " +
            "this again."
    }
    $tags = @(git -C $RepoPath tag --list)
    $untagged = @()
    foreach ($package in ($Packages | Where-Object Publish -eq $true)) {
        $prefix = "$($package.Name)/"
        if ([string]::IsNullOrEmpty($package.FirstVersion) -eq $true -and
            @($tags | Where-Object { $_.StartsWith($prefix) }).Count -eq 0) {
            $untagged += $package
        }
    }
    if ($untagged.Count -gt 0) {
        throw "No '<crate>/<version>' tag was found for " +
            "$($untagged.Count) crate(s): $($untagged.Name -join ', '). Each " +
            "crate's next version continues from its own last tag. Tag every " +
            "crate at the version it last published once, with " +
            "ci/start-crate-tags.ps1, then run this again."
    }
    $plan = @(Get-ReleasePlan $RepoPath $Packages)
    $order = Get-PublishOrder $RepoPath
    return @($plan | Sort-Object { $order.IndexOf($_.Name) })
}

# The version every published crate has after a release of the plan: the
# planned version for a crate the plan releases, and the version it last
# released for any other, which is already on crates.io.
function Get-CrateVersions([string]$RepoPath, $Plan, $Packages = $null) {
    if ($null -eq $Packages) {
        $Packages = Get-Packages $RepoPath
    }
    $versions = [ordered]@{}
    foreach ($package in ($Packages | Where-Object Publish -eq $true)) {
        $release = $Plan | Where-Object Name -eq $package.Name
        if ($null -ne $release) {
            $versions[$package.Name] = "$($release.Version)"
            continue
        }
        $released = @(Get-ReleasedVersions $RepoPath $package.Name)
        if ($released.Count -eq 0) {
            throw "$($package.Name) is not in the plan and has no tag, so " +
                "it has no version to keep."
        }
        $versions[$package.Name] = "$($released[-1])"
    }
    return $versions
}

# The plan written by ci/get-release-plan.ps1, with Versions as a hashtable.
function Read-ReleasePlan([string]$Path) {
    $plan = Get-Content -Raw $Path | ConvertFrom-Json -AsHashtable
    return [pscustomobject]@{
        Releases = @($plan.Releases | ForEach-Object { [pscustomobject]$_ })
        Versions = $plan.Versions
    }
}
