<#
.SYNOPSIS
Run once, by a person, before the first release that versions each crate on
its own. Tags every published crate '<crate>/<version>' at the release it was
last published from, so each crate's versions continue from there.

.DESCRIPTION
A release works out each crate's next version from the crate's own tags (see
ci/get-release-plan.ps1), and stops when a crate has none. Before that scheme
every crate was released together under one repository tag, such as 4.5.18,
so each crate is given a tag of its own at the commit of that repository tag,
with the same version.

The version is checked against crates.io first. Each crate must have that
version published and not yanked, and nothing higher, so a tag never claims a
version the crate does not have or hides a later one. Nothing is tagged
unless every crate passes, and a crate that already has a tag of its own
stops the run, as this is only for a start.

A crate that crates.io does not have is passed over when ci/packages.json
gives it a FirstVersion, because its first release is planned at that
version and needs no tag to count from. Any other crate crates.io does not
have stops the run.

The tags are lightweight. The weekly release check dates an annotated tag by
when it was made, so an annotated tag would count as a release on the day
this runs. A lightweight tag is dated by its commit, which is the day of the
release it marks.

Without -Push the tags are made in this checkout only, and the command that
pushes them is printed.

.PARAMETER Release
The repository tag the crates were last published from, such as 4.5.18.
Defaults to the highest plain version tag.

.PARAMETER RepoPath
The repository. Defaults to the one this script sits in.

.PARAMETER Push
Push the tags to origin, all of them or none.

.PARAMETER Registry
Each crate's record as crates.io's API gives it, by name, in place of asking
crates.io. A crate it does not name is one crates.io does not have. For tests.
#>
param (
    [string]$Release,
    [string]$RepoPath = (Split-Path $PSScriptRoot -Parent),
    [switch]$Push,
    [hashtable]$Registry
)
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
. (Join-Path $PSScriptRoot 'crates-io.ps1')

if (-not $Release) {
    $Release = git -C $RepoPath tag --list |
        Where-Object { $_ -match '^\d+\.\d+\.\d+$' } |
        Sort-Object { [version]$_ } |
        Select-Object -Last 1
    if (-not $Release) {
        throw "No plain version tag was found to start from. Fetch the tags."
    }
}
if ($Release -notmatch '^\d+\.\d+\.\d+$') {
    throw "'$Release' is not a repository release tag such as 4.5.18."
}
$commit = git -C $RepoPath rev-parse "refs/tags/$Release^{commit}"

$packages = @(Get-Content -Raw (Join-Path $RepoPath 'ci/packages.json') |
    ConvertFrom-Json | Where-Object Publish -eq $true)
Write-Output ("Checking the $($packages.Count) published crates on crates.io " +
    "before tagging them at $Release, commit $($commit.Substring(0, 12)).")

$problems = @()
$tags = @()
foreach ($package in $packages) {
    $name = $package.Name
    $own = @(git -C $RepoPath for-each-ref '--format=%(refname:strip=2)' "refs/tags/$name/")
    $tag = "$name/$Release"
    if ($own.Count -gt 0) {
        if ($own.Count -eq 1 -and $own[0] -eq $tag -and
            (git -C $RepoPath rev-parse "refs/tags/$tag^{commit}") -eq $commit) {
            Write-Output "  $tag is already here."
            $tags += $tag
            continue
        }
        $problems += "$name already has tags of its own ($($own -join ', '))"
        continue
    }

    $crate = Get-CratesIoCrate $name $Registry
    if ($null -eq $crate) {
        if ([string]::IsNullOrEmpty($package.FirstVersion) -eq $false) {
            Write-Output ("  $name is not on crates.io, so it is not tagged. " +
                "It is released by its FirstVersion $($package.FirstVersion).")
        } else {
            $problems += "$name is not on crates.io, and ci/packages.json " +
                "gives it no FirstVersion"
        }
        continue
    }
    $published = $crate.versions | Where-Object num -eq $Release
    if ($null -eq $published) {
        $problems += "$name $Release is not on crates.io"
    } elseif ($published.yanked -eq $true) {
        $problems += "$name $Release is yanked on crates.io"
    } elseif ($crate.crate.max_version -ne $Release) {
        $problems += ("$name is at $($crate.crate.max_version) on crates.io, " +
            "above $Release")
    } else {
        Write-Output "  $name $Release is the highest on crates.io."
    }
    $tags += $tag
}

if ($problems.Count -gt 0) {
    foreach ($problem in $problems) {
        Write-Output "FAIL $problem"
    }
    throw "Nothing is tagged. $($problems.Count) crate(s) cannot start at $Release."
}

foreach ($tag in $tags) {
    if (-not (git -C $RepoPath tag --list $tag)) {
        git -C $RepoPath tag $tag $commit
    }
}
$refs = @($tags | ForEach-Object { "refs/tags/$_" })
if ($Push) {
    git -C $RepoPath push --atomic origin @refs
    Write-Output "Pushed $($tags.Count) tags at $Release."
} else {
    Write-Output "Tagged $($tags.Count) crates at $Release in this checkout. To push them:"
    Write-Output ""
    Write-Output "    git push --atomic origin $($refs -join ' ')"
}
