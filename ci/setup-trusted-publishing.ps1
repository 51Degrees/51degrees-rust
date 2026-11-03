<#
.SYNOPSIS
Registers this repository's two publishing workflows on crates.io as the
trusted publishers of every crate in ci/crates.txt.

.DESCRIPTION
The nightly pipeline publishes each night and publish.yml publishes by hand,
both through common-ci's shared publish. crates.io checks the workflow that
started the run, so each workflow needs its own registration. A registration
that already exists is skipped, so running this again changes nothing.

Run it again whenever a crate is added to ci/crates.txt, because the publish
stops with 403 at a crate that is not registered. A brand new crate has to be
published by hand once first, because Trusted Publishing cannot create a
crate. See https://crates.io/docs/trusted-publishing.

Needs a crates.io API token in CRATES_IO_TOKEN that belongs to an owner of the
crates and has the trusted-publishing scope:

    $env:CRATES_IO_TOKEN = Read-Host -MaskInput "crates.io token"
    ./ci/setup-trusted-publishing.ps1
#>
#Requires -Version 7
$ErrorActionPreference = "Stop"

if (-not $env:CRATES_IO_TOKEN) {
    throw "Set CRATES_IO_TOKEN to a crates.io API token of an owner of the crates."
}

$Owner = "51Degrees"
$Repo = "51degrees-rust"
$Workflows = "nightly-pipeline.yml", "publish.yml"
$Api = "https://crates.io/api/v1/trusted_publishing/github_configs"
$Headers = @{
    "Authorization" = $env:CRATES_IO_TOKEN
    "User-Agent" = "51degrees-trusted-publishing-setup (support@51degrees.com)"
}

# The crates to register, from the list ci/publish-package.ps1 publishes, so a
# crate is never published without being registered.
$CratesFile = Join-Path $PSScriptRoot "crates.txt"
$Crates = @(Get-Content $CratesFile |
    Where-Object { $_ -notmatch '^\s*(#|$)' } |
    ForEach-Object { $_.Trim() })
if ($Crates.Count -eq 0) {
    throw "$CratesFile names no crates."
}

# The crate's GitHub trusted publisher registrations, with Listed false when
# crates.io cannot list them.
function Get-Registrations([string]$Crate) {
    try {
        $response = Invoke-WebRequest -UseBasicParsing -Headers $Headers `
            -Uri "${Api}?crate=$Crate"
        return @{
            Listed = $true
            Registrations = @(($response.Content | ConvertFrom-Json).github_configs)
        }
    }
    catch {
        return @{ Listed = $false }
    }
}

$failures = 0
foreach ($crate in $Crates) {
    $listing = Get-Registrations $crate
    foreach ($workflow in $Workflows) {
        if (-not $listing.Listed) {
            Write-Output "! $crate ($workflow): FAILED -> could not list its trusted publishers"
            $failures++
            continue
        }
        # crates.io matches the owner and repository without regard to case.
        $existing = $listing.Registrations | Where-Object {
            $_.repository_owner -ieq $Owner -and
            $_.repository_name -ieq $Repo -and
            $_.workflow_filename -ceq $workflow
        }
        if ($existing) {
            Write-Output "= $crate ($workflow): trusted publisher already configured, skipping"
            continue
        }
        $body = [ordered]@{
            github_config = [ordered]@{
                crate = $crate
                repository_owner = $Owner
                repository_name = $Repo
                workflow_filename = $workflow
                environment = $null
            }
        } | ConvertTo-Json -Compress
        try {
            Invoke-WebRequest -UseBasicParsing -Method Post -Headers $Headers `
                -ContentType "application/json" -Body $body -Uri $Api | Out-Null
            Write-Output "+ $crate ($workflow): trusted publisher registered"
        }
        catch {
            Write-Output "! $crate ($workflow): FAILED -> $($_.Exception.Message) $($_.ErrorDetails.Message)"
            $failures++
        }
    }
}

Write-Output ""
if ($failures -gt 0) {
    Write-Output "Trusted publishing setup finished with $failures failure(s)."
    exit 1
}
Write-Output "Trusted publishing configured for all crates."
