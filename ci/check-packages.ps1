<#
.SYNOPSIS
Fails when ci/packages.json disagrees with the crate manifests, so the release
plan cannot drift from what cargo actually builds.

.DESCRIPTION
common-ci's steps/release-plan.ps1 decides which crates a release publishes
from ci/packages.json alone, so every fact in that file is checked here
against the files it describes.

1. The crates are those ci/crates.txt publishes, each once, each published.
2. Each Path is the workspace folder whose manifest has that crate's name.
3. Each DependsOn is the set of published crates the manifest requires by
   path in [dependencies], [build-dependencies] or a target's equivalent.
   Dev-dependencies are left out, because they never reach a user of the
   crate, so a change to one is no reason to release the crate.
4. Each ExtraPaths is the workspace Cargo.toml, whose package keys and
   dependencies every crate inherits, and each submodule the crate takes
   files from at build time with the script that copies them, read from
   ci/copy-cxx-sources.ps1 and ci/copy-owid-source.ps1. A change to any of
   these can change what the crate is built from, so it releases the crate.
   ci/release-plan-filter.ps1 decides which changes to the workspace
   Cargo.toml count.
5. ci/crates.txt lists each crate after every crate it requires by path,
   dev-dependencies with a version included, because the published crate
   keeps those as requirements on crates.io. The publish follows that order,
   so every requirement it raises names a version already published.

.PARAMETER RepoRoot
The root of the repository. Defaults to the parent of this script.
#>
param (
    [string]$RepoRoot = (Split-Path $PSScriptRoot -Parent)
)
$ErrorActionPreference = "Stop"

# The kind of dependency a manifest table holds, or $null for any other table.
# A table such as [dependencies.foo] holds one dependency, named after it.
function Get-DependencyTable([string]$Table) {
    $match = [regex]::Match($Table,
        '^(?:target\..+\.)?(?<kind>dependencies|build-dependencies|dev-dependencies)(?:\.(?<name>[A-Za-z0-9_-]+))?$')
    if ($match.Success -eq $false) {
        return $null
    }
    return [pscustomobject]@{
        Kind = $match.Groups['kind'].Value
        Name = $match.Groups['name'].Value
    }
}

# Every dependency of a manifest that names a path, with the folder it points
# at from the repository root, its kind, and whether it carries a version.
function Get-PathDependencies([string]$Root, [string]$Folder) {
    $manifest = Join-Path $Root $Folder 'Cargo.toml'
    $table = $null
    $dependencies = @()
    foreach ($line in Get-Content $manifest) {
        $text = $line.Trim()
        if ($text.StartsWith('#') -or $text -eq '') {
            continue
        }
        $header = [regex]::Match($text, '^\[(?<table>[^\[\]]+)\]$')
        if ($header.Success -eq $true) {
            $table = Get-DependencyTable $header.Groups['table'].Value.Trim()
            continue
        }
        if ($null -eq $table) {
            continue
        }
        $path = $null
        $hasVersion = $false
        if ($table.Name) {
            # A dependency of its own table, one key on each line.
            $key = [regex]::Match($text, '^(?<key>[A-Za-z0-9_-]+)\s*=\s*"(?<value>[^"]*)"')
            if ($key.Success -eq $false) {
                continue
            }
            if ($key.Groups['key'].Value -eq 'path') {
                $path = $key.Groups['value'].Value
            } elseif ($key.Groups['key'].Value -eq 'version') {
                $hasVersion = $true
            }
        } else {
            # One dependency on each line, as an inline table.
            $entry = [regex]::Match($text, '^[A-Za-z0-9_-]+\s*=\s*\{(?<body>.*)\}')
            if ($entry.Success -eq $false) {
                continue
            }
            $body = $entry.Groups['body'].Value
            $pathMatch = [regex]::Match($body, '(?:^|[\s,])path\s*=\s*"(?<value>[^"]*)"')
            if ($pathMatch.Success -eq $true) {
                $path = $pathMatch.Groups['value'].Value
            }
            $hasVersion = $body -match '(?:^|[\s,])version\s*=\s*"'
        }
        if ($null -eq $path -and $hasVersion -eq $false) {
            continue
        }
        $dependencies += [pscustomobject]@{
            Kind = $table.Kind
            Table = $table.Name
            Path = $path
            HasVersion = $hasVersion
        }
    }

    # A table dependency spreads its path and version over several lines, so
    # its lines are joined into one dependency here.
    $joined = @()
    foreach ($group in ($dependencies | Where-Object Table |
            Group-Object Kind, Table)) {
        $joined += [pscustomobject]@{
            Kind = $group.Group[0].Kind
            Path = ($group.Group | Where-Object Path | Select-Object -First 1).Path
            HasVersion = @($group.Group | Where-Object HasVersion).Count -gt 0
        }
    }
    $joined += @($dependencies | Where-Object { -not $_.Table })

    foreach ($dependency in ($joined | Where-Object Path)) {
        $full = [IO.Path]::GetFullPath((Join-Path $Root $Folder $dependency.Path))
        $relative = [IO.Path]::GetRelativePath(
            [IO.Path]::GetFullPath($Root), $full).Replace('\', '/')
        [pscustomobject]@{
            Kind = $dependency.Kind
            Folder = $relative
            HasVersion = $dependency.HasVersion
        }
    }
}

# The workspace members, each with its crate name and the folders of the
# crates it requires by path.
function Get-WorkspaceCrates([string]$Root) {
    $workspace = Get-Content -Raw (Join-Path $Root 'Cargo.toml')
    $members = [regex]::Match($workspace, 'members\s*=\s*\[(?<list>[^\]]*)\]',
        'Singleline')
    if ($members.Success -eq $false) {
        throw "No workspace members in Cargo.toml"
    }
    foreach ($folder in [regex]::Matches($members.Groups['list'].Value,
            '"([^"]+)"') | ForEach-Object { $_.Groups[1].Value }) {
        $manifest = Join-Path $Root $folder 'Cargo.toml'
        if ((Test-Path $manifest) -eq $false) {
            throw "The workspace names $folder but $folder/Cargo.toml is missing"
        }
        $name = [regex]::Match((Get-Content -Raw $manifest),
            '(?m)^name\s*=\s*"([^"]+)"').Groups[1].Value
        [pscustomobject]@{
            Name = $name
            Folder = $folder
            Dependencies = @(Get-PathDependencies $Root $folder |
                Where-Object Folder -ne $folder)
        }
    }
}

# The script that copies each submodule's files into crates.
$CopyScripts = @{
    'cxx' = 'ci/copy-cxx-sources.ps1'
    'owid-rust' = 'ci/copy-owid-source.ps1'
}

# The workspace manifest, which every crate inherits from.
$WorkspaceManifest = 'Cargo.toml'

# The crate folders each submodule's files are copied into, read from the
# scripts that copy them.
function Get-SubmoduleCopies([string]$Root) {
    $cxxScript = Get-Content -Raw (Join-Path $Root $CopyScripts['cxx'])
    $cxx = @([regex]::Matches($cxxScript, '(?m)^\s*"([^"]+)"\s*=\s*@\(') |
        ForEach-Object { $_.Groups[1].Value })
    $owidScript = Get-Content -Raw (Join-Path $Root $CopyScripts['owid-rust'])
    $owid = @([regex]::Matches($owidScript,
        '(?m)^\$target\s*=\s*Join-Path\s+\$RepoRoot\s+"([^"/]+)/') |
        ForEach-Object { $_.Groups[1].Value })
    if ($cxx.Count -eq 0) {
        throw "Could not read the crates ci/copy-cxx-sources.ps1 copies into"
    }
    if ($owid.Count -eq 0) {
        throw "Could not read the crate ci/copy-owid-source.ps1 copies into"
    }
    return @{ 'cxx' = $cxx; 'owid-rust' = $owid }
}

function Format-List([string[]]$Names) {
    $list = @($Names | Sort-Object)
    if ($list.Count -eq 0) {
        return '(none)'
    }
    return $list -join ', '
}

$failures = @()

$published = @(Get-Content (Join-Path $RepoRoot 'ci/crates.txt') |
    ForEach-Object { $_.Trim() } |
    Where-Object { $_ -and -not $_.StartsWith('#') })
$packages = @(Get-Content -Raw (Join-Path $RepoRoot 'ci/packages.json') |
    ConvertFrom-Json)
$crates = @(Get-WorkspaceCrates $RepoRoot)
$byFolder = @{}
foreach ($crate in $crates) {
    $byFolder[$crate.Folder] = $crate
}
$copies = Get-SubmoduleCopies $RepoRoot
$submodules = @(Get-Content (Join-Path $RepoRoot '.gitmodules') |
    ForEach-Object {
        $match = [regex]::Match($_, '^\s*path\s*=\s*(\S+)')
        if ($match.Success -eq $true) { $match.Groups[1].Value }
    })

# 1. The same crates as ci/crates.txt, each once, each published.
foreach ($name in $published) {
    $entries = @($packages | Where-Object Name -eq $name)
    if ($entries.Count -eq 0) {
        $failures += "ci/crates.txt publishes $name, which ci/packages.json does not list"
    } elseif ($entries.Count -gt 1) {
        $failures += "ci/packages.json lists $name $($entries.Count) times"
    }
}
foreach ($package in $packages) {
    if ($published -notcontains $package.Name) {
        $failures += "ci/packages.json lists $($package.Name), which ci/crates.txt does not publish"
    }
    if ($package.Publish -ne $true) {
        $failures += "ci/packages.json does not publish $($package.Name), which ci/crates.txt publishes"
    }
}

foreach ($package in $packages) {
    # 2. The folder is the workspace member with this crate's name.
    $crate = $byFolder[$package.Path]
    if ($null -eq $crate) {
        $failures += "$($package.Name) has Path $($package.Path), which is not a workspace member"
        continue
    }
    if ($crate.Name -ne $package.Name) {
        $failures += "$($package.Name) has Path $($package.Path), whose manifest names $($crate.Name)"
        continue
    }

    # 3. DependsOn is what the manifest requires, dev-dependencies left out.
    $required = @($crate.Dependencies | Where-Object Kind -ne 'dev-dependencies' |
        ForEach-Object { $byFolder[$_.Folder].Name } |
        Where-Object { $published -contains $_ } | Sort-Object -Unique)
    $listed = @($package.DependsOn | Sort-Object -Unique)
    if (($required -join ',') -ne ($listed -join ',')) {
        $failures += "$($package.Name) has DependsOn $(Format-List $listed), " +
            "but $($package.Path)/Cargo.toml requires $(Format-List $required)"
    }

    # 4. ExtraPaths is the workspace manifest, and each submodule copied into
    # the crate with the script that copies it.
    $copied = @($copies.Keys | Where-Object { $copies[$_] -contains $package.Path })
    $expected = @(@($WorkspaceManifest) + $copied +
        @($copied | ForEach-Object { $CopyScripts[$_] }) | Sort-Object)
    $extra = @($package.ExtraPaths | Where-Object { $_ } | Sort-Object)
    if (($expected -join ',') -cne ($extra -join ',')) {
        $failures += "$($package.Name) has ExtraPaths $(Format-List $extra), " +
            "but it is built from $(Format-List $expected), being the " +
            "workspace Cargo.toml and each submodule copied into it with " +
            "the script in ci/ that copies it"
    }
    foreach ($path in $copied) {
        if ($submodules -notcontains $path) {
            $failures += "$($package.Name) takes files from $path, which is not a submodule in .gitmodules"
        }
    }
}

# 5. ci/crates.txt lists each crate after every crate its published manifest
# requires from crates.io.
for ($index = 0; $index -lt $published.Count; $index++) {
    $crate = $crates | Where-Object Name -eq $published[$index]
    if ($null -eq $crate) {
        $failures += "ci/crates.txt publishes $($published[$index]), which is not a workspace member"
        continue
    }
    $needed = @($crate.Dependencies |
        Where-Object { $_.Kind -ne 'dev-dependencies' -or $_.HasVersion } |
        ForEach-Object { $byFolder[$_.Folder].Name } |
        Where-Object { $published -contains $_ } | Sort-Object -Unique)
    foreach ($name in $needed) {
        if ($published.IndexOf($name) -gt $index) {
            $failures += "ci/crates.txt lists $($crate.Name) before $name, " +
                "which it requires, so it could be published requiring a " +
                "version of $name not yet on crates.io"
        }
    }
}

if ($failures.Count -gt 0) {
    foreach ($failure in $failures) {
        Write-Output "FAIL $failure"
    }
    throw "ci/packages.json disagrees with the crates in $($failures.Count) " +
        "place(s). The release plan reads only that file, so it must match."
}
Write-Output ("ci/packages.json matches the $($packages.Count) crates " +
    "ci/crates.txt publishes, their manifests, the workspace manifest and " +
    "the submodules they copy.")
