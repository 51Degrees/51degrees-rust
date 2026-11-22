<#
.SYNOPSIS
Writes the counts and the result of a comparison with .NET as a short summary.

.DESCRIPTION
Reads the report parity-compare wrote and keeps only its counts line, being
the number of cases, differences and differences not allowed, then adds the
result of the Compare step. No other line of the report is copied, so the
summary names no case, path or answer. The Enterprise job of the comparison
workflow publishes this summary in place of its report.
#>
[CmdletBinding()]
param(
    # The report parity-compare wrote. It may be missing when the comparison
    # did not run.
    [Parameter(Mandatory = $true)]
    [string] $Report,

    # The outcome of the Compare step, being success, failure, cancelled,
    # skipped or empty.
    [Parameter(Mandatory = $true)]
    [AllowEmptyString()]
    [string] $Outcome,

    # The data the comparison ran on, for the heading.
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[a-z]+$')]
    [string] $Data,

    # The summary to write.
    [Parameter(Mandatory = $true)]
    [string] $Out
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# The whole counts line, so nothing else on the line can be copied.
$countsPattern = '^\d+ cases, \d+ differences, \d+ not allowed\.$'

$counts = $null
if (Test-Path -LiteralPath $Report) {
    $counts = Get-Content -LiteralPath $Report |
        Where-Object { $_ -match $countsPattern } |
        Select-Object -First 1
}

$result = switch ($Outcome) {
    'success' { 'passed' }
    'failure' { 'failed' }
    default { 'did not run' }
}

$lines = @(
    "## Comparison with the .NET packages ($Data)"
    ''
    $(if ($counts) { $counts } else { 'The comparison wrote no counts.' })
    ''
    "Result: $result."
)

$folder = Split-Path -Parent $Out
if ($folder -and -not (Test-Path -LiteralPath $folder)) {
    New-Item -ItemType Directory -Path $folder | Out-Null
}
Set-Content -LiteralPath $Out -Value $lines -Encoding utf8
