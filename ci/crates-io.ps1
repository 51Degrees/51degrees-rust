# Dot-sourced by the release scripts that read crates.io. crates.io asks each
# client for a User-Agent saying who it is and for no more than one request a
# second, so every request goes through Get-CratesIoCrate.
#
# A script that reads crates.io takes a -Registry hashtable of each crate's
# record, by name, as crates.io's API gives it. The tests pass one, so they
# make no network call, and a crate the table does not name is one crates.io
# does not have.

$CratesIoApi = 'https://crates.io/api/v1/crates'
$CratesIoUserAgent = '51degrees-rust release scripts (support@51degrees.com)'
$CratesIoRequestIntervalMilliseconds = 1000
$CratesIoLastRequest = [datetime]::MinValue

# The crate's record from crates.io, holding 'crate' and 'versions', or $null
# when crates.io has no crate of that name. Any other answer stops the caller.
function Get-CratesIoCrate([string]$Name, [hashtable]$Registry) {
    if ($null -ne $Registry) {
        return $Registry[$Name]
    }
    $elapsed = ([datetime]::UtcNow - $script:CratesIoLastRequest).TotalMilliseconds
    if ($elapsed -lt $CratesIoRequestIntervalMilliseconds) {
        Start-Sleep -Milliseconds ([int]($CratesIoRequestIntervalMilliseconds - $elapsed))
    }
    $script:CratesIoLastRequest = [datetime]::UtcNow
    $record = Invoke-RestMethod -Uri "$CratesIoApi/$Name" `
        -Headers @{ 'User-Agent' = $CratesIoUserAgent } `
        -SkipHttpErrorCheck -StatusCodeVariable status
    if ($status -eq 404) {
        return $null
    }
    if ($status -ne 200) {
        throw "crates.io answered $status when asked about $Name."
    }
    return $record
}

# The highest version of the crate on crates.io that is not yanked, as a
# [version], or $null when crates.io has none. crates.io's max_version leaves
# yanked versions out, and is 0.0.0 when every version is yanked.
function Get-CratesIoHighest($Record) {
    if ($null -eq $Record) {
        return $null
    }
    $match = [regex]::Match("$($Record.crate.max_version)", '^(\d+)\.(\d+)\.(\d+)')
    if ($match.Success -eq $false) {
        return $null
    }
    $highest = [version]::new([int]$match.Groups[1].Value,
        [int]$match.Groups[2].Value, [int]$match.Groups[3].Value)
    if ($highest -eq [version]::new(0, 0, 0)) {
        return $null
    }
    return $highest
}
