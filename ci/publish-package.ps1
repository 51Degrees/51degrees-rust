<#
.SYNOPSIS
Publishes every crate to crates.io for common-ci's shared publish.

.DESCRIPTION
Checks out the commit the packages were built from, writes the version into
every crate and copies the OWID and C sources, then publishes each crate in
ci/crates.txt in order.

A crate whose version the crates.io index already lists is skipped, and one
crates.io has accepted but not yet indexed is waited for, so a run that stopped
part way can be run again. After each publish the next crate waits until the
index lists the new version, because cargo resolves the next crate's
dependencies from the index, which can lag the upload by many minutes.

Each publish uses a crates.io token exchanged for this run's GitHub OIDC
identity, so no crates.io token is stored. crates.io accepts it because the
workflow that started the run is registered as the crates' trusted publisher
(see ci/setup-trusted-publishing.ps1). The token is revoked straight after the
publish.

Runs from the common-ci checkout, with this repository cloned beside it and
the build step's package/ folder downloaded beside that.

.PARAMETER RepoName
The folder this repository is cloned into.

.PARAMETER Version
The version being released.

.PARAMETER DryRun
Do everything except publish.
#>
param (
    [Parameter(Mandatory)][string]$RepoName,
    [Parameter(Mandatory)][string]$Version,
    [bool]$DryRun
)
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

$Registry = "https://crates.io"
$Index = "https://index.crates.io"
$UserAgent = "51degrees-rust-publish (support@51degrees.com)"
$IndexWaitMinutes = 60
$IndexPollSeconds = 15
$RequestTimeoutSeconds = 30
$RequestAttempts = 5
$RequestRetrySeconds = 20

# Keep the runner's secrets and the GitHub token out of reach of the build
# scripts cargo runs.
Remove-Item -Path Env:SECRETS, Env:GITHUB_TOKEN -ErrorAction SilentlyContinue

# The path of a crate's file in the sparse index.
function Get-IndexPath([string]$Crate) {
    $name = $Crate.ToLowerInvariant()
    switch ($name.Length) {
        1 { return "1/$name" }
        2 { return "2/$name" }
        3 { return "3/$($name[0])/$name" }
        default { return "$($name.Substring(0, 2))/$($name.Substring(2, 2))/$name" }
    }
}

# Fetch a URL, or $null when it answers 404. Any other failure is retried and
# then stops the run, because a request that failed must never be read as a
# crate that is not there.
function Get-Response([string]$Uri) {
    for ($attempt = 1; ; $attempt++) {
        try {
            return Invoke-WebRequest -UseBasicParsing -Uri $Uri -TimeoutSec $RequestTimeoutSeconds `
                -Headers @{ "User-Agent" = $UserAgent; "Cache-Control" = "no-cache" }
        } catch {
            $status = $_.Exception.Response.StatusCode
            if ($null -ne $status -and [int]$status -eq 404) {
                return $null
            }
            if ($attempt -ge $RequestAttempts) {
                throw "Fetching $Uri failed $attempt times, last with: $($_.Exception.Message)"
            }
            Write-Host "  retrying $Uri after: $($_.Exception.Message)"
            Start-Sleep -Seconds $RequestRetrySeconds
        }
    }
}

# Whether the sparse index cargo resolves from lists the version.
function Test-Indexed([string]$Crate) {
    $response = Get-Response "$Index/$(Get-IndexPath $Crate)"
    if ($null -eq $response) {
        return $false
    }
    $pattern = '"vers":"' + [regex]::Escape($Version) + '"'
    return [bool]($response.Content -split "`n" | Where-Object { $_ -match $pattern })
}

# Whether crates.io has accepted the version, indexed or not.
function Test-Uploaded([string]$Crate) {
    return $null -ne (Get-Response "$Registry/api/v1/crates/$Crate/$Version")
}

# Wait until the index lists the version, failing the run if it never does.
function Wait-Indexed([string]$Crate) {
    $deadline = (Get-Date).AddMinutes($IndexWaitMinutes)
    while (-not (Test-Indexed $Crate)) {
        if ((Get-Date) -gt $deadline) {
            throw "The crates.io index has not listed $Crate $Version after $IndexWaitMinutes minutes."
        }
        Write-Output "  waiting for the crates.io index to list $Crate $Version"
        Start-Sleep -Seconds $IndexPollSeconds
    }
    Write-Output "  $Crate $Version is in the crates.io index."
}

# A crates.io token for this run, exchanged for its GitHub OIDC identity.
function Get-PublishToken {
    if (-not $env:ACTIONS_ID_TOKEN_REQUEST_URL) {
        throw "No GitHub OIDC token is available. The calling job needs 'id-token: write'."
    }
    $identity = Invoke-RestMethod -Uri "$($env:ACTIONS_ID_TOKEN_REQUEST_URL)&audience=crates.io" `
        -Headers @{ Authorization = "Bearer $($env:ACTIONS_ID_TOKEN_REQUEST_TOKEN)" }
    $exchange = Invoke-RestMethod -Method Post -Uri "$Registry/api/v1/trusted_publishing/tokens" `
        -ContentType "application/json" -Headers @{ "User-Agent" = $UserAgent } `
        -Body (@{ jwt = $identity.value } | ConvertTo-Json)
    Write-Host "::add-mask::$($exchange.token)"
    return $exchange.token
}

function Remove-PublishToken([string]$Token) {
    try {
        Invoke-RestMethod -Method Delete -Uri "$Registry/api/v1/trusted_publishing/tokens" `
            -Headers @{ Authorization = "Bearer $Token"; "User-Agent" = $UserAgent } | Out-Null
    } catch {
        Write-Output "::warning::Revoking the crates.io token failed: $_"
    }
}

$repo = (Resolve-Path $RepoName).Path
$commit = (Get-Content -Raw -Path "package/commit.txt").Trim()
if ((git -C $repo rev-parse HEAD) -ne $commit) {
    Write-Output "Main has moved since the build, so publishing the built commit $commit."
    git -C $repo checkout --quiet --detach $commit
    git -C $repo submodule update --init --recursive
}

rustup toolchain install stable --profile minimal
rustup default stable

& "$repo/ci/set-crate-versions.ps1" -Version $Version
& "$repo/ci/copy-owid-source.ps1"
& "$repo/ci/copy-cxx-sources.ps1"

$crates = @(Get-Content -Path "$repo/ci/crates.txt" | Where-Object { $_ -notmatch '^\s*(#|$)' })
Push-Location $repo
try {
    foreach ($crate in $crates) {
        if (Test-Indexed $crate) {
            Write-Output "$crate $Version is already on crates.io, skipping."
            continue
        }
        if (Test-Uploaded $crate) {
            Write-Output "$crate $Version is on crates.io but not yet in its index."
            Wait-Indexed $crate
            continue
        }
        if ($DryRun) {
            Write-Output "Dry run, not publishing $crate $Version."
            continue
        }
        Write-Output "Publishing $crate $Version."
        $token = Get-PublishToken
        try {
            $env:CARGO_REGISTRY_TOKEN = $token
            # --allow-dirty, because the version and the source copies are
            # written into the tree on the build machine and never committed.
            cargo publish -p $crate --allow-dirty
        } finally {
            Remove-Item -Path Env:CARGO_REGISTRY_TOKEN -ErrorAction SilentlyContinue
            Remove-PublishToken $token
        }
        Wait-Indexed $crate
    }
} finally {
    Pop-Location
}
