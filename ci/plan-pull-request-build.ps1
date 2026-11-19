<#
.SYNOPSIS
Decides what the Pull Request workflow builds, being the systems it runs on
and the workspace members it builds.
.DESCRIPTION
A pull request into rc or a release branch builds on Windows and the two
Ubuntu LTS releases and leaves macOS out, because the organisation shares
five macOS runners and every language builds its drafts without them. A pull
request into main, which releases rc, a push to main and a run started by
hand build on macOS as well, and a run started by hand can leave it out.

A pull request builds each workspace member whose folder holds a changed
file, and every member that depends on one of those, dev-dependencies
included, as `cargo metadata` gives them. A changed file outside every
member's folder builds the whole workspace, unless no step of the workflow
reads it, which is true of Markdown and LICENSE files, the examples, parity
and docs folders, the other workflows, and the scripts in ci/ the workflow
does not run. Every other event builds the whole workspace.

Writes these to $OutputFile, the step's GITHUB_OUTPUT:
- os, the systems as a JSON array for the build matrix;
- packages, being --workspace, a -p option for each member to build, or
  empty when nothing needs building;
- native, true when fiftyone-native is built, for the steps that need both
  C libraries;
- fodid, true when fodid is built, for its steps with every feature on.
.PARAMETER Event
The event that started the run, github.event_name.
.PARAMETER BaseRef
The branch a pull request merges into, github.base_ref.
.PARAMETER MacOS
For a run started by hand, whether to build on macOS too.
.PARAMETER ChangedFiles
The files a pull request changes, relative to the repository root.
.PARAMETER MetadataFile
The output of `cargo metadata --no-deps --format-version 1`.
.PARAMETER OutputFile
Where the outputs are written, GITHUB_OUTPUT by default.
#>
param(
    [Parameter(Mandatory)] [string] $Event,
    [string] $BaseRef = '',
    [bool] $MacOS = $true,
    [string[]] $ChangedFiles = @(),
    [Parameter(Mandatory)] [string] $MetadataFile,
    [string] $OutputFile = $env:GITHUB_OUTPUT
)

$ErrorActionPreference = 'Stop'

# The systems every build runs on, and the one a release build adds.
$Systems = @('windows-latest', 'ubuntu-22.04', 'ubuntu-24.04')
$MacSystem = 'macos-latest'

# The files in ci/ and .github/ that the Pull Request workflow runs or reads.
# A change to one of them builds the whole workspace. The tests check that
# every ci/ path the workflow names is here.
$BuildScripts = @(
    '.github/workflows/pull-request.yml',
    '.github/actions/',
    'ci/plan-pull-request-build.ps1',
    'ci/copy-owid-source.ps1',
    'ci/check-native-symbols.ps1',
    'ci/measure-wasm-size.sh',
    'ci/wasm-size/',
    'ci/copy-translations.ps1',
    'ci/record-derived-properties-commit.ps1',
    'ci/check-packages.sh'
)

# True when no step of the Pull Request workflow reads the file at $Path,
# which is outside every member's folder.
function Test-NotBuilt([string] $Path) {
    foreach ($script in $BuildScripts) {
        if ($Path -eq $script -or ($script.EndsWith('/') -and $Path.StartsWith($script))) {
            return $false
        }
    }
    $name = Split-Path -Leaf $Path
    return $Path -match '\.md$' -or
        $name -like 'LICENSE*' -or
        $Path -match '^(examples|parity|docs|\.vscode)/' -or
        $Path -match '^\.github/workflows/' -or
        $Path -match '^ci/' -or
        $Path -in @('.gitignore', '.gitattributes', '.utm-lint.json')
}

# The members of the workspace, each with its folder relative to the root and
# the members it depends on in any way.
function Get-Members([string] $File) {
    $metadata = Get-Content -Raw $File | ConvertFrom-Json
    $root = ([IO.Path]::GetFullPath($metadata.workspace_root)).TrimEnd('\', '/')
    $names = @($metadata.packages.name)
    foreach ($package in $metadata.packages) {
        $folder = [IO.Path]::GetFullPath((Split-Path -Parent $package.manifest_path))
        $relative = $folder.Substring($root.Length).TrimStart('\', '/') -replace '\\', '/'
        [pscustomobject]@{
            Name      = $package.name
            Folder    = $relative
            DependsOn = @($package.dependencies |
                Where-Object { $_.path -and $_.name -in $names -and $_.name -ne $package.name } |
                ForEach-Object name | Sort-Object -Unique)
        }
    }
}

# The member whose folder holds $Path, the deepest when folders nest, or
# $null when none does.
function Find-Member([object[]] $Members, [string] $Path) {
    $Members |
        Where-Object { $_.Folder -and $Path.StartsWith($_.Folder + '/') } |
        Sort-Object { $_.Folder.Length } -Descending |
        Select-Object -First 1
}

# $Changed and every member that depends on one of them, directly or not.
function Add-Dependants([object[]] $Members, [string[]] $Changed) {
    $selected = [Collections.Generic.HashSet[string]]::new([string[]] $Changed)
    do {
        $added = $false
        foreach ($member in $Members) {
            if (-not $selected.Contains($member.Name) -and
                    @($member.DependsOn | Where-Object { $selected.Contains($_) }).Count) {
                $null = $selected.Add($member.Name)
                $added = $true
            }
        }
    } while ($added)
    return @($Members.Name | Where-Object { $selected.Contains($_) })
}

$members = @(Get-Members $MetadataFile)
$pullRequest = $Event -eq 'pull_request'
$withMac = if ($Event -eq 'workflow_dispatch') { $MacOS } else { -not $pullRequest -or $BaseRef -eq 'main' }
$os = if ($withMac) { $Systems + $MacSystem } else { $Systems }

$whole = -not $pullRequest -or $BaseRef -eq 'main'
$reason = if ($whole) { "a $Event into or on $(if ($BaseRef) { $BaseRef } else { 'main' }) builds everything" } else { '' }
$changed = [Collections.Generic.List[string]]::new()
if (-not $whole) {
    foreach ($path in $ChangedFiles) {
        $path = $path.Trim() -replace '\\', '/'
        if (-not $path) { continue }
        $member = Find-Member $members $path
        if ($member) {
            if (-not $changed.Contains($member.Name)) { $changed.Add($member.Name) }
        }
        elseif (-not (Test-NotBuilt $path)) {
            $whole = $true
            $reason = "$path is outside every member's folder and the build reads it"
            break
        }
    }
}

if ($whole) {
    $selected = @($members.Name)
    $packages = '--workspace'
}
else {
    $selected = @(Add-Dependants $members $changed)
    $packages = ($selected | ForEach-Object { "-p $_" }) -join ' '
}

Write-Host "Systems: $($os -join ', ').$(if (-not $withMac) { ' macOS is left out.' })"
if ($whole) {
    Write-Host "Builds the whole workspace, because $reason."
}
elseif ($selected.Count) {
    Write-Host "Changed: $($changed -join ', ')"
    Write-Host "Builds $($selected.Count) of $($members.Count) members: $($selected -join ', ')"
}
else {
    Write-Host 'Builds nothing, because no change is one the build reads.'
}

if ($OutputFile) {
    @(
        "os=$(ConvertTo-Json -Compress @($os))",
        "packages=$packages",
        "native=$(($selected -contains 'fiftyone-native').ToString().ToLower())",
        "fodid=$(($selected -contains 'fodid').ToString().ToLower())"
    ) | Add-Content -Path $OutputFile -Encoding utf8
}
