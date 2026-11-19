# Checks that every NativeModel difference between the two sides comes from
# .NET finding a native model name in any case, where the crates match it
# exactly, so the entries allowing those differences hide nothing else.
#
#   pwsh check-native-model-case.ps1 -Cases <json> -Rust <jsonl> -DotNet <jsonl>
#
# .NET's index keys each value ignoring case with the invariant culture, so
# for a native model name it gives the ids of every value equal to it ignoring
# case, in ascending order, and counts them together. The crates' ids for each
# value come from its property-keyed-native-value case. The check fails on a
# case giving a native model name alone, a NativeModel case or a profile case,
# whose .NET ids differ from the crates' and are not those, on a profile case
# giving a TAC whose ids differ at all, and on a NativeModel value whose .NET
# count is not the crates' counts for the values equal to it ignoring case
# added together.

param(
    [Parameter(Mandatory = $true)] [string] $Cases,
    [Parameter(Mandatory = $true)] [string] $Rust,
    [Parameter(Mandatory = $true)] [string] $DotNet
)

$ErrorActionPreference = 'Stop'

$evidenceKey = 'query.nativemodel'
$property = 'NativeModel'
$valuePrefix = 'property-keyed-native-value-'
$metadataCase = 'property-keyed-none'
$ignoringCase = [StringComparer]::InvariantCultureIgnoreCase

# The parsed lines of the NativeModel cases, the profile cases and the
# metadata case, by case id. Other lines are skipped before parsing, as there
# are many.
function Read-Lines([string] $path) {
    $lines = [Collections.Generic.Dictionary[string, Text.Json.JsonElement]]::new(
        [StringComparer]::Ordinal)
    foreach ($text in [IO.File]::ReadLines($path)) {
        if ($text -notmatch '"id":"(property-keyed-(native|profile)[^"]*|property-keyed-none)"') {
            continue
        }
        $root = [Text.Json.JsonDocument]::Parse($text).RootElement
        $lines[$root.GetProperty('id').GetString()] = $root
    }
    return , $lines
}

# The profile ids a line holds, or $null when it holds none.
function Get-Ids([Text.Json.JsonElement] $line) {
    $ids = $line.GetProperty('profileIds')
    if ($ids.ValueKind -ne [Text.Json.JsonValueKind]::Array) {
        return $null
    }
    $list = [Collections.Generic.List[long]]::new()
    foreach ($id in $ids.EnumerateArray()) {
        $list.Add($id.GetInt64())
    }
    return , $list
}

function Format-Ids($ids) {
    if ($null -eq $ids) { return 'null' }
    return '[' + ($ids -join ',') + ']'
}

$rustLines = Read-Lines $Rust
$dotnetLines = Read-Lines $DotNet

# Each NativeModel or profile case giving a native model name alone, with
# that name, in corpus order, and the ids of the profile cases giving
# anything else.
$nativeCases = [Collections.Generic.List[object]]::new()
$otherProfileCases = [Collections.Generic.List[string]]::new()
$document = [Text.Json.JsonDocument]::Parse([IO.File]::OpenRead($Cases))
foreach ($case in $document.RootElement.EnumerateArray()) {
    $id = $case.GetProperty('id').GetString()
    $isProfile = $id.StartsWith('property-keyed-profile-')
    if (-not ($id.StartsWith('property-keyed-native') -or $isProfile)) {
        continue
    }
    $evidence = @($case.GetProperty('evidence').EnumerateObject())
    if ($evidence.Count -eq 1 -and $evidence[0].Name -ceq $evidenceKey) {
        $nativeCases.Add([pscustomobject]@{ Id = $id; Value = $evidence[0].Value.GetString() })
    }
    elseif ($isProfile) {
        $otherProfileCases.Add($id)
    }
}

# The crates' ids of every value equal to each value ignoring case.
$anyCase = [Collections.Generic.Dictionary[string, Collections.Generic.List[long]]]::new(
    $ignoringCase)
foreach ($case in $nativeCases) {
    if (-not $case.Id.StartsWith($valuePrefix)) {
        continue
    }
    $ids = Get-Ids $rustLines[$case.Id]
    if (-not $anyCase.ContainsKey($case.Value)) {
        $anyCase[$case.Value] = [Collections.Generic.List[long]]::new()
    }
    if ($null -ne $ids) {
        $anyCase[$case.Value].AddRange($ids)
    }
}

$problems = [Collections.Generic.List[string]]::new()
$differing = 0
foreach ($case in $nativeCases) {
    $rustIds = Format-Ids (Get-Ids $rustLines[$case.Id])
    $dotnetIds = Format-Ids (Get-Ids $dotnetLines[$case.Id])
    if ($rustIds -ceq $dotnetIds) {
        continue
    }
    $differing++
    $expected = 'null'
    if ($anyCase.ContainsKey($case.Value)) {
        $sorted = [long[]]$anyCase[$case.Value].ToArray()
        [Array]::Sort($sorted)
        $expected = Format-Ids $sorted
    }
    if ($dotnetIds -cne $expected) {
        $problems.Add("$($case.Id) ($($case.Value)): .NET $dotnetIds, Rust $rustIds, " +
            "the crates' ids of the values equal to it ignoring case $expected")
    }
}

foreach ($id in $otherProfileCases) {
    $rustIds = Format-Ids (Get-Ids $rustLines[$id])
    $dotnetIds = Format-Ids (Get-Ids $dotnetLines[$id])
    if ($rustIds -cne $dotnetIds) {
        $differing++
        $problems.Add("${id}: .NET $dotnetIds, Rust $rustIds, from a TAC, which must match")
    }
}

# The counts each side's index holds.
function Get-Counts([Text.Json.JsonElement] $line) {
    $counts = [Collections.Generic.Dictionary[string, long]]::new([StringComparer]::Ordinal)
    $indexed = $line.GetProperty('indexedValues')
    $values = [Text.Json.JsonElement]::new()
    if ($indexed.ValueKind -eq [Text.Json.JsonValueKind]::Object -and
        $indexed.TryGetProperty($property, [ref] $values)) {
        foreach ($value in $values.EnumerateObject()) {
            $counts[$value.Name] = $value.Value.GetInt64()
        }
    }
    return , $counts
}

$rustCounts = Get-Counts $rustLines[$metadataCase]
$dotnetCounts = Get-Counts $dotnetLines[$metadataCase]
$rustAnyCase = [Collections.Generic.Dictionary[string, long]]::new($ignoringCase)
foreach ($pair in $rustCounts.GetEnumerator()) {
    $sum = 0
    [void] $rustAnyCase.TryGetValue($pair.Key, [ref] $sum)
    $rustAnyCase[$pair.Key] = $sum + $pair.Value
}
# .NET keeps the spelling it met first, so its keys are found ignoring case.
$dotnetKeys = [Collections.Generic.Dictionary[string, string]]::new($ignoringCase)
foreach ($key in $dotnetCounts.Keys) {
    $dotnetKeys[$key] = $key
}
$countsDiffering = 0
foreach ($pair in $rustCounts.GetEnumerator()) {
    $dotnetCount = 0
    if ($dotnetCounts.TryGetValue($pair.Key, [ref] $dotnetCount) -and
        $dotnetCount -eq $pair.Value) {
        continue
    }
    $countsDiffering++
    $dotnetKey = $null
    [void] $dotnetKeys.TryGetValue($pair.Key, [ref] $dotnetKey)
    if ($null -eq $dotnetKey -or $dotnetCounts[$dotnetKey] -ne $rustAnyCase[$pair.Key]) {
        $problems.Add("$property value '$($pair.Key)': the crates count $($pair.Value), " +
            ".NET's key '$dotnetKey' counts $(if ($dotnetKey) { $dotnetCounts[$dotnetKey] }), " +
            "the values equal to it ignoring case count $($rustAnyCase[$pair.Key])")
    }
}
foreach ($key in $dotnetCounts.Keys) {
    if (-not $rustAnyCase.ContainsKey($key)) {
        $countsDiffering++
        $problems.Add("$property value '$key' is in .NET's index and no value the crates " +
            "list equals it ignoring case")
    }
}

Write-Output ("$($nativeCases.Count) cases giving a native model name and " +
    "$($otherProfileCases.Count) profile cases giving a TAC, $differing with ids that differ, " +
    "$countsDiffering NativeModel values whose counts differ, $($problems.Count) not " +
    "from case alone")
foreach ($problem in $problems) {
    Write-Output "::error::$problem"
}
if ($problems.Count -gt 0) {
    exit 1
}
