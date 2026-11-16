<#
.SYNOPSIS
Compares the timing of two commits from a release timing run's log.

.DESCRIPTION
Reads the RESULT lines the Release Timing workflow prints, one per engine per
side per round, and gives each side's median with the lowest and highest of
its rounds and how many rounds each side won.

A measure counts as slower only when the head's whole range sits on the slower
side of the base's, which is the release timing rule. Overlapping ranges count
as not slower, however the medians compare, because one runner's rounds vary by
more than the difference being measured. The exit code is 1 when any measure is
slower by that rule, when a measure has no paired rounds, or when there are
fewer paired rounds than were asked for.
#>
[CmdletBinding()]
param(
    # The log of the timing run, holding its RESULT lines.
    [Parameter(Mandatory = $true)]
    [string] $Log,

    # The number of paired rounds to insist on. 0 accepts any number above
    # zero, which is what a run of unknown length uses.
    [int] $Rounds = 0
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $Log)) {
    Write-Host "::error::The timing log '$Log' does not exist."
    exit 1
}

$text = Get-Content -LiteralPath $Log -Raw -ErrorAction Stop

# RESULT dd <round> <side> <detections per second> detections <detections>
# RESULT ipi <round> <side> <lookups per second, 4 threads> single <lookups
#   per second, one thread> loaded <addresses>
$measures = [ordered] @{
    'Device Detection, detections per second on 4 threads' = @{
        Pattern = 'RESULT dd (?<round>\d+) (?<side>base|head|main) (?<value>\d+) detections'
        Figures = @{}
    }
    'IP Intelligence, lookups per second on 4 threads'     = @{
        Pattern = 'RESULT ipi (?<round>\d+) (?<side>base|head|main) (?<value>\d+) single'
        Figures = @{}
    }
    'IP Intelligence, lookups per second on one thread'    = @{
        Pattern = 'RESULT ipi (?<round>\d+) (?<side>base|head|main) \d+ single (?<value>\d+)'
        Figures = @{}
    }
}

foreach ($name in @($measures.Keys)) {
    $figures = @{ base = @{}; head = @{} }
    foreach ($match in [regex]::Matches($text, $measures[$name].Pattern)) {
        # 'main' is the name the first runs of this comparison gave the base
        # side, so a log from one of those still reads.
        $side = if ($match.Groups['side'].Value -eq 'head') { 'head' } else { 'base' }
        $figures[$side][[int] $match.Groups['round'].Value] = [long] $match.Groups['value'].Value
    }
    $measures[$name].Figures = $figures
}

# The middle value of a sorted list, or the mean of the middle two.
function Get-Median {
    param([long[]] $Values)
    $sorted = @($Values | Sort-Object)
    $middle = [int] [Math]::Floor($sorted.Count / 2)
    if ($sorted.Count % 2 -eq 1) { return [double] $sorted[$middle] }
    ([double] $sorted[$middle - 1] + [double] $sorted[$middle]) / 2.0
}

$slower = @()
$failed = $false

foreach ($name in $measures.Keys) {
    $figures = $measures[$name].Figures
    $paired = @($figures.base.Keys | Where-Object { $figures.head.ContainsKey($_) } | Sort-Object)
    Write-Host ''
    Write-Host "$name"
    if ($paired.Count -eq 0) {
        Write-Host "::error::$name has no paired rounds in '$Log', so nothing was compared."
        $failed = $true
        continue
    }
    if ($Rounds -gt 0 -and $paired.Count -lt $Rounds) {
        Write-Host "::error::$name has $($paired.Count) paired rounds where $Rounds were asked for."
        $failed = $true
    }

    $baseValues = [long[]] @($paired | ForEach-Object { $figures.base[$_] })
    $headValues = [long[]] @($paired | ForEach-Object { $figures.head[$_] })
    $baseMedian = Get-Median -Values $baseValues
    $headMedian = Get-Median -Values $headValues
    $baseLow = ($baseValues | Measure-Object -Minimum).Minimum
    $baseHigh = ($baseValues | Measure-Object -Maximum).Maximum
    $headLow = ($headValues | Measure-Object -Minimum).Minimum
    $headHigh = ($headValues | Measure-Object -Maximum).Maximum
    $headWon = @($paired | Where-Object { $figures.head[$_] -gt $figures.base[$_] }).Count
    $baseWon = @($paired | Where-Object { $figures.base[$_] -gt $figures.head[$_] }).Count
    $percent = if ($baseMedian -eq 0) { 0 } else { ($headMedian - $baseMedian) / $baseMedian * 100.0 }

    Write-Host ("  $($paired.Count) paired rounds")
    Write-Host ("  base : median {0:N0}  ({1:N0} to {2:N0})" -f $baseMedian, $baseLow, $baseHigh)
    Write-Host ("  head : median {0:N0}  ({1:N0} to {2:N0})" -f $headMedian, $headLow, $headHigh)
    Write-Host ("  rounds won: head $headWon, base $baseWon")
    Write-Host ("  head against base in the median round: {0:+0.00;-0.00;0.00} per cent" -f $percent)

    # Higher is better for every measure here, so the head is slower only when
    # its highest round is still below the base's lowest.
    if ($headHigh -lt $baseLow) {
        Write-Host "  the head's whole range is below the base's, so this measure is SLOWER"
        $slower += $name
    }
    elseif ($headLow -gt $baseHigh) {
        Write-Host "  the head's whole range is above the base's, so this measure is faster"
    }
    else {
        Write-Host "  the ranges meet, so this measure is not slower under the rule"
    }
}

Write-Host ''
if ($slower.Count -gt 0) {
    foreach ($name in $slower) {
        Write-Host "::error::The head is slower than the base in '$name', whose whole range sits below the base's."
    }
    exit 1
}
if ($failed) {
    exit 1
}
Write-Host 'Every measure meets or beats the base, so the head is not slower under the release timing rule.'
exit 0
