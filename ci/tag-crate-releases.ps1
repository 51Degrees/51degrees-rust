<#
.SYNOPSIS
Tags each crate a publish released, '<crate>/<version>', and gives each tag a
GitHub release of its own.

.DESCRIPTION
The tag is what the crate's next release counts from. The tags are pushed
together, in one atomic push, so either every crate of the release has its
tag or none has, and a rerun plans the same release again rather than
leaving a crate untagged while a crate depending on it is tagged.

Every crate tag on HEAD then gets a GitHub release if it has none, so a rerun
finishes the releases a failed run left undone, even though the plan of a
rerun is empty once the tags are pushed. The notes list the commits that
changed the crate's folders since its previous tag, or, for a crate released
only because a crate it depends on was, those crates and their versions.

.PARAMETER RepoName
The directory the repository is checked out into.

.PARAMETER Repository
The repository on GitHub, such as 51Degrees/51degrees-rust.

.PARAMETER PlanFile
The plan ci/get-release-plan.ps1 wrote before anything was published.

.PARAMETER DryRun
Tag locally and print each release, pushing and creating nothing.
#>
param (
    [Parameter(Mandatory)][string]$RepoName,
    [Parameter(Mandatory)][string]$Repository,
    [Parameter(Mandatory)][string]$PlanFile,
    [switch]$DryRun
)
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
. (Join-Path $PSScriptRoot 'packages.ps1')

$packages = @(Get-Packages $RepoName | Where-Object Publish -eq $true)
$plan = Read-ReleasePlan $PlanFile
$head = git -C $RepoName rev-parse HEAD

# 1. A tag for each crate of the plan, made here unless a rerun made it.
$tags = @()
foreach ($release in $plan.Releases) {
    $tag = "$($release.Name)/$($release.Version)"
    $at = git -C $RepoName tag --list $tag --format='%(objectname)'
    if ($at -and $at -ne $head) {
        throw "Tag $tag already points at $at, not at $head, so it is left alone."
    }
    if (-not $at) {
        git -C $RepoName tag $tag $head
    }
    $tags += $tag
}
if ($tags.Count -gt 0) {
    if ($DryRun) {
        Write-Output "Dry run, tagged locally and not pushed: $($tags -join ', ')"
    } else {
        git -C $RepoName push --atomic origin @($tags | ForEach-Object { "refs/tags/$_" })
        Write-Output "Pushed $($tags.Count) tag(s): $($tags -join ', ')"
    }
}

# 2. A GitHub release for every crate tag on HEAD that has none.
$onHead = @{}
foreach ($package in $packages) {
    $versions = @(Get-ReleasedVersions $RepoName $package.Name -AtHead)
    if ($versions.Count -gt 0) {
        $onHead[$package.Name] = "$($versions[-1])"
    }
}
if ($onHead.Count -eq 0) {
    Write-Output "No crate is tagged at $head, so there is nothing to release."
}
foreach ($package in $packages) {
    if (-not $onHead.ContainsKey($package.Name)) {
        continue
    }
    $version = $onHead[$package.Name]
    $tag = "$($package.Name)/$version"

    if (-not $DryRun) {
        # A missing release makes gh release view exit non-zero, which is an
        # answer here, not a failure.
        $PSNativeCommandUseErrorActionPreference = $false
        gh release view $tag -R $Repository *> $null
        $exists = $LASTEXITCODE -eq 0
        $global:LASTEXITCODE = 0
        $PSNativeCommandUseErrorActionPreference = $true
        if ($exists) {
            Write-Output "Release $tag is already here, leaving it alone."
            continue
        }
    }

    $earlier = @(Get-ReleasedVersions $RepoName $package.Name |
        Where-Object { $_ -lt [version]$version })
    $notes = @()
    $paths = Get-PackagePaths $package
    if ($earlier.Count -gt 0) {
        $previous = "$($package.Name)/$($earlier[-1])"
        $commits = @(git -C $RepoName log --format='- %s (%h)' "$previous..$head" -- $paths)
        if ($commits.Count -gt 0) {
            $notes += "Changes since $($earlier[-1]):"
            $notes += ""
            $notes += $commits
        }
    }
    $moved = @($package.DependsOn | Where-Object { $onHead.ContainsKey($_) } |
        ForEach-Object { "- $_ $($onHead[$_])" })
    if ($moved.Count -gt 0) {
        if ($notes.Count -gt 0) { $notes += "" }
        $notes += "Requires these crates at the versions released with it:"
        $notes += ""
        $notes += $moved
    }
    if ($notes.Count -eq 0) {
        $notes += "$($package.Name) $version."
    }
    $text = $notes -join "`n"

    if ($DryRun) {
        Write-Output "Dry run, would create release $tag with notes:"
        Write-Output $text
        Write-Output ""
        continue
    }
    gh release create $tag -R $Repository --verify-tag `
        --title "$($package.Name) $version" --notes $text
    Write-Output "Released $tag"
}
