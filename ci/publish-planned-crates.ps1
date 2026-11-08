<#
.SYNOPSIS
Publishes the crates of the plan with ci/publish-crates.sh, and stops before
a crate crates.io does not have yet.

.DESCRIPTION
crates.io trusted publishing cannot create a crate, so the first version of
a crate is published by a person with a token of their own. The crates of
the plan are published in the order of ci/crates.txt up to the first one
crates.io does not have. The run then fails, naming that crate and printing
the commands that publish it by hand from this commit with the versions this
run wrote, then register the publish workflow as its trusted publisher.
Every crate the new one requires comes before it in ci/crates.txt, so each
is on crates.io by then. A rerun once it is published skips every version
crates.io has and goes on.

.PARAMETER PlanFile
The plan ci/get-release-plan.ps1 wrote.

.PARAMETER RepoPath
The repository. Defaults to the one this script sits in.

.PARAMETER Registry
Each crate's record as crates.io's API gives it, by name, in place of asking
crates.io. A crate it does not name is one crates.io does not have. For tests.

.PARAMETER DryRun
Print what a publish would do, publishing nothing and failing on nothing.
#>
param (
    [Parameter(Mandatory)][string]$PlanFile,
    [string]$RepoPath = (Split-Path $PSScriptRoot -Parent),
    [hashtable]$Registry,
    [switch]$DryRun
)
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
. (Join-Path $PSScriptRoot 'packages.ps1')
. (Join-Path $PSScriptRoot 'crates-io.ps1')

$plan = Read-ReleasePlan $PlanFile
$order = Get-PublishOrder $RepoPath
$names = @($plan.Releases.Name | Where-Object { $_ } |
    Sort-Object { $order.IndexOf($_) })
if ($names.Count -eq 0) {
    Write-Output "The plan releases no crate, so nothing is published."
    return
}

# The crates before the first one crates.io does not have, which can be
# published now.
$new = $null
$ready = @()
foreach ($name in $names) {
    if ($null -eq (Get-CratesIoCrate $name $Registry)) {
        $new = $name
        break
    }
    $ready += $name
}

if ($ready.Count -gt 0) {
    if ($DryRun) {
        Write-Output ("Dry run, would publish $($ready.Count) crate(s) with " +
            "ci/publish-crates.sh: $($ready -join ', ')")
    } else {
        bash (Join-Path $PSScriptRoot 'publish-crates.sh') @ready
    }
}
if ($null -eq $new) {
    return
}

# The commands that publish the new crate by hand, as one PowerShell 7 block
# that stops at its first failure, so a step that fails is never followed by
# cargo publish. The versions are those this run wrote, and the block checks
# that the manifests give the crate its planned version before publishing, so
# it is published as the run would have published it. A submodule the crate
# takes files from is checked out and copied in, as the publish workflow
# does. On Windows the bash on the path can start WSL, which does not see
# the token, so the block takes Git Bash's where it is installed.
$package = Get-Packages $RepoPath | Where-Object Name -eq $new
$planned = "$($plan.Versions[$new])"
$extra = @($package.ExtraPaths | Where-Object { $_ })
$scripts = @($extra | Where-Object { $_ -match '^ci/copy-[^/]+\.ps1$' })
$submodules = @($extra | Where-Object {
    $_ -cne 'Cargo.toml' -and $scripts -notcontains $_ })
$versions = [ordered]@{}
foreach ($name in $order) {
    if ($plan.Versions.Keys -contains $name) {
        $versions[$name] = "$($plan.Versions[$name])"
    }
}
$json = @{ Versions = $versions } | ConvertTo-Json -Compress -Depth 3
$head = git -C $RepoPath rev-parse HEAD

$commands = @(
    "& {"
    "    `$ErrorActionPreference = 'Stop'"
    "    `$PSNativeCommandUseErrorActionPreference = `$true"
    "    if (`$PSVersionTable.PSVersion.Major -lt 7 -or"
    "        (`$PSVersionTable.PSVersion.Major -eq 7 -and `$PSVersionTable.PSVersion.Minor -lt 3)) {"
    "        throw 'Run this block in PowerShell 7.3 or later (pwsh).'"
    "    }"
    "    if (-not `$env:CARGO_REGISTRY_TOKEN) {"
    "        throw 'Set `$env:CARGO_REGISTRY_TOKEN to a crates.io token of an owner of the other crates first.'"
    "    }"
    "    if (-not (Test-Path ci/set-crate-versions.ps1)) {"
    "        throw 'Run this block from the root of a checkout of this repository.'"
    "    }"
    "    git fetch origin"
    "    git checkout --detach $head"
)
if ($submodules.Count -gt 0) {
    $commands += "    git submodule update --init $($submodules -join ' ')"
}
$commands += @(
    "    `$plan = Join-Path ([IO.Path]::GetTempPath()) 'release-plan.json'"
    "    Set-Content `$plan '$json'"
    "    ./ci/set-crate-versions.ps1 -PlanFile `$plan"
)
$commands += @($scripts | ForEach-Object { "    ./$_" })
$commands += @(
    "    `$metadata = (cargo metadata --no-deps --format-version 1) -join `"``n`" | ConvertFrom-Json"
    "    `$found = (`$metadata.packages | Where-Object name -eq '$new').version"
    "    if (`$found -ne '$planned') {"
    "        throw `"The manifests give $new `$found, not $planned, so it is not published.`""
    "    }"
    "    cargo publish -p $new --allow-dirty"
    "    `$env:CRATES_IO_TOKEN = `$env:CARGO_REGISTRY_TOKEN"
    "    `$bash = 'bash'"
    "    if (`$IsWindows) {"
    "        `$gitBash = Join-Path `$env:ProgramFiles 'Git' 'bin' 'bash.exe'"
    "        if (Test-Path `$gitBash) {"
    "            `$bash = `$gitBash"
    "        }"
    "    }"
    "    & `$bash ci/setup-trusted-publishing.sh"
    "    git checkout -- ."
    "}"
)

$before = "The $($ready.Count) crate(s) of the plan before it are on crates.io"
if ($ready.Count -eq 0) {
    $before = 'No crate of the plan comes before it'
} elseif ($DryRun) {
    $before = "The $($ready.Count) crate(s) of the plan before it would be " +
        "on crates.io by then"
}
$stop = "$new is not on crates.io, and trusted publishing cannot create a " +
    "crate, so the publish stops before it."
if ($DryRun) {
    Write-Output "::warning title=First version published by hand::A publish of this commit stops before $new."
    Write-Output "Dry run. $stop"
} else {
    Write-Output "::error title=First version published by hand::$stop"
    Write-Output $stop
}
Write-Output "$before."
Write-Output ""
Write-Output ("Publish its first version by hand from the root of a clean " +
    "checkout, in PowerShell 7 (pwsh). Set `$env:CARGO_REGISTRY_TOKEN to a " +
    "crates.io token of an owner of the other crates, then paste this block, " +
    "which runs once its closing brace is entered and stops at the first " +
    "failure:")
Write-Output ""
foreach ($command in $commands) {
    Write-Output "    $command"
}
Write-Output ""
Write-Output ("If the block stops, git checkout -- . puts the manifests back. " +
    "Once it has published $new, run the failed publish job again. It skips " +
    "every version crates.io has and publishes the rest.")
if ($DryRun -eq $false) {
    throw "Stopped before $new, which is not on crates.io. Publish it by hand as above, then run the job again."
}
