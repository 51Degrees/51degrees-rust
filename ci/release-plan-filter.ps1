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

The manifest is read as TOML statements, so a value written over several
lines is one statement, and a bracket, quote or '#' inside a string is part
of the string. Statements are compared as written, case included, because
Cargo reads keys and values with case.

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

# Moves the state of a TOML value across one line of it, being how deep it is
# in brackets and braces and which kind of string it is inside, if any. A '#'
# outside a string ends the line, as it starts a comment.
function Step-Value($State, [string]$Line) {
    $i = 0
    while ($i -lt $Line.Length) {
        $c = $Line[$i]
        $rest = $Line.Substring($i)
        if ($State.String -eq 'basic-multi') {
            if ($c -eq '\') {
                $i += 2
            } elseif ($rest.StartsWith('"""')) {
                $State.String = $null
                $i += 3
            } else {
                $i++
            }
        } elseif ($State.String -eq 'literal-multi') {
            if ($rest.StartsWith("'''")) {
                $State.String = $null
                $i += 3
            } else {
                $i++
            }
        } elseif ($State.String -eq 'basic') {
            if ($c -eq '\') {
                $i += 2
                continue
            }
            if ($c -eq '"') {
                $State.String = $null
            }
            $i++
        } elseif ($State.String -eq 'literal') {
            if ($c -eq "'") {
                $State.String = $null
            }
            $i++
        } elseif ($c -eq '#') {
            break
        } elseif ($rest.StartsWith('"""')) {
            $State.String = 'basic-multi'
            $i += 3
        } elseif ($rest.StartsWith("'''")) {
            $State.String = 'literal-multi'
            $i += 3
        } else {
            if ($c -eq '"') {
                $State.String = 'basic'
            } elseif ($c -eq "'") {
                $State.String = 'literal'
            } elseif ($c -eq '[' -or $c -eq '{') {
                $State.Depth++
            } elseif ($c -eq ']' -or $c -eq '}') {
                $State.Depth--
            }
            $i++
        }
    }
    # A string on one line cannot run past the end of it.
    if ($State.String -eq 'basic' -or $State.String -eq 'literal') {
        $State.String = $null
    }
}

function Test-ValueOpen($State) {
    return $State.Depth -gt 0 -or $null -ne $State.String
}

# The position of the first '=' outside a quoted key, or -1.
function Find-Equals([string]$Line) {
    $quote = $null
    for ($i = 0; $i -lt $Line.Length; $i++) {
        $c = $Line[$i]
        if ($null -ne $quote) {
            if ($c -eq $quote) {
                $quote = $null
            }
        } elseif ($c -eq '"' -or $c -eq "'") {
            $quote = $c
        } elseif ($c -eq '=') {
            return $i
        }
    }
    return -1
}

# The table a header names, with the spaces TOML allows around its dots taken
# out, so [ package ] is package.
function Get-TableName([string]$Header) {
    $inner = [regex]::Match($Header, '^\[\[?(.*?)\]\]?\s*(#.*)?$').Groups[1].Value
    return ($inner.Trim() -replace '\s*\.\s*', '.')
}

# The statements of a manifest, each with the table it sits in, its first key
# for a key and value, and its text, being its lines trimmed and joined. A
# value written over several lines is one statement.
function Get-Statements([string[]]$Lines) {
    $table = ''
    $statement = $null
    $state = $null
    foreach ($line in $Lines) {
        $text = $line.Trim()
        if ($null -ne $statement) {
            $statement.Text += "`n$text"
            Step-Value $state $line
            if ((Test-ValueOpen $state) -eq $false) {
                $statement
                $statement = $null
            }
            continue
        }
        if ($text -eq '') {
            continue
        }
        if ($text.StartsWith('#')) {
            [pscustomobject]@{ Kind = 'comment'; Table = $table; Key = $null; Text = $text }
            continue
        }
        if ($text.StartsWith('[')) {
            $table = Get-TableName $text
            [pscustomobject]@{ Kind = 'header'; Table = $table; Key = $null; Text = $text }
            continue
        }
        $equals = Find-Equals $text
        $key = $null
        if ($equals -gt 0) {
            $key = ($text.Substring(0, $equals) -split '\.')[0].Trim().Trim('"', "'")
        }
        $current = [pscustomobject]@{ Kind = 'value'; Table = $table; Key = $key; Text = $text }
        $state = [pscustomobject]@{ Depth = 0; String = $null }
        if ($equals -ge 0) {
            Step-Value $state $text.Substring($equals + 1)
        }
        if (Test-ValueOpen $state) {
            $statement = $current
        } else {
            $current
        }
    }
    if ($null -ne $statement) {
        $statement
    }
}

function Test-Descriptive($Statement) {
    return $Statement.Kind -ceq 'value' -and $DescriptiveKeys -ccontains $Statement.Key
}

# The text of a crate's manifest that counts, being every statement apart
# from the descriptive keys of [package], each after the table it sits in.
function Get-CrateText([string[]]$Lines) {
    $counted = foreach ($statement in (Get-Statements $Lines)) {
        if ($statement.Table -ceq 'package' -and (Test-Descriptive $statement)) {
            continue
        }
        "[$($statement.Table)] $($statement.Text)"
    }
    return $counted -join "`n"
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

# True when the manifest changed in what counts, or is missing on one side.
function Test-ManifestChanged([string]$Path, [scriptblock]$Read) {
    $before = Get-ManifestLines $From $Path
    $after = Get-ManifestLines 'HEAD' $Path
    if ($null -eq $before -or $null -eq $after) {
        return $true
    }
    return (& $Read $before) -cne (& $Read $after)
}

$manifest = "$($Package.Path)/Cargo.toml"
foreach ($file in $Files) {
    if ($file -ceq $manifest) {
        if (Test-ManifestChanged $file ${function:Get-CrateText}) {
            $file
        }
    } else {
        $file
    }
}
