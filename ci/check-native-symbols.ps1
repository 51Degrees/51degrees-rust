<#
.SYNOPSIS
Fails when the Device Detection and IP Intelligence C libraries could bind to
each other's copy of the common C library.

.DESCRIPTION
fiftyone-common-sys and fiftyone-ip-intelligence-sys each compile their own
copy of the common C library, with different file offset widths. The IP
Intelligence copy is renamed to ipi_ prefixed names by src/common/ipiSymbols.h
in 51degrees-cxx, which ip-intelligence-sys/build.rs turns on with
FIFTYONE_DEGREES_IPI_SYMBOLS. A common C library function missing from that
list would be defined by both static libraries, and the linker
would bind every call to just one of them, so one engine would read its data
file through the other engine's code.

This reads the symbols of both libraries a build left in the target directory
and fails on either of these:

- a fiftyoneDegrees name both libraries define;
- a fiftyoneDegrees name IP Intelligence references but does not define, which
  only the Device Detection copy then provides.

It uses nm on Linux and macOS and dumpbin on Windows. Run it after building
fiftyone-common-sys and fiftyone-ip-intelligence-sys.

.PARAMETER TargetDir
The build profile folder of the cargo target directory, target/debug by
default.
#>
param(
    [string] $TargetDir = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target/debug')
)

$ErrorActionPreference = 'Stop'

function Find-Library([string] $Name) {
    $library = Get-ChildItem -Path (Join-Path $TargetDir 'build') -Recurse -File |
        Where-Object { $_.Name -match "^(lib)?$([regex]::Escape($Name))\.(a|lib)$" } |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1
    if (-not $library) { throw "no $Name library under $TargetDir/build, so build the -sys crates first" }
    $library.FullName
}

function Find-Dumpbin {
    $onPath = Get-Command dumpbin -ErrorAction SilentlyContinue
    if ($onPath) { return $onPath.Source }
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    if (Test-Path $vswhere) {
        $found = & $vswhere -latest -products * -find 'VC/Tools/MSVC/**/bin/Hostx64/x64/dumpbin.exe' |
            Select-Object -First 1
        if ($found) { return $found }
    }
    throw 'dumpbin was not found on the PATH or through vswhere'
}

# Returns @{ Defined = names; Undefined = names } for the fiftyoneDegrees and
# ipi_fiftyoneDegrees symbols of a static library.
function Get-Symbols([string] $Library) {
    $defined = [System.Collections.Generic.HashSet[string]]::new()
    $undefined = [System.Collections.Generic.HashSet[string]]::new()
    if ($IsWindows) {
        foreach ($line in & (Find-Dumpbin) /SYMBOLS $Library) {
            if ($line -match 'External\s+\|\s+((?:ipi_)?fiftyoneDegrees\w+)') {
                if ($line -match '\bUNDEF\b') { [void]$undefined.Add($Matches[1]) }
                else { [void]$defined.Add($Matches[1]) }
            }
        }
    } else {
        foreach ($line in & nm -g $Library 2>$null) {
            $fields = -split $line
            if ($fields.Count -lt 2) { continue }
            # macOS prefixes C names with an underscore.
            $name = $fields[-1] -replace '^_(?=(ipi_)?fiftyoneDegrees)', ''
            if ($name -notmatch '^(ipi_)?fiftyoneDegrees\w+$') { continue }
            if ($fields[-2] -eq 'U') { [void]$undefined.Add($name) }
            else { [void]$defined.Add($name) }
        }
    }
    if ($defined.Count -eq 0) { throw "no fiftyoneDegrees symbols read from $Library" }
    @{ Defined = $defined; Undefined = $undefined }
}

$common = Get-Symbols (Find-Library 'fiftyone-common-c')
$ipi = Get-Symbols (Find-Library 'fiftyone-ip-intelligence-c')

$both = $ipi.Defined | Where-Object { $_ -notlike 'ipi_*' -and $common.Defined.Contains($_) } | Sort-Object
$borrowed = $ipi.Undefined |
    Where-Object { $_ -notlike 'ipi_*' -and -not $ipi.Defined.Contains($_) -and $common.Defined.Contains($_) } |
    Sort-Object

Write-Output ("Device Detection common C library defines {0} fiftyoneDegrees names, IP Intelligence defines {1}." -f $common.Defined.Count, $ipi.Defined.Count)
if ($both -or $borrowed) {
    foreach ($name in $both) {
        Write-Output "Defined by both libraries: $name. 51degrees-cxx's src/common/ipiSymbols.h lacks it."
    }
    foreach ($name in $borrowed) {
        Write-Output "IP Intelligence calls the Device Detection copy of: $name. 51degrees-cxx's src/common/ipiSymbols.h lacks it."
    }
    exit 1
}
Write-Output 'No common C library name is shared between the two libraries.'
