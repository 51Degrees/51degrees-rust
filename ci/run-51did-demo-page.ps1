<#
.SYNOPSIS
Starts the 51Did creator context demo with the 51Did keys, checks its page in
headless Chrome on each route, and stops the demo.

.DESCRIPTION
The demo reads its resource key from _51DEGREES_RESOURCE_KEY and its license
key from _51DEGREES_LICENSE_KEY, as a customer sets them. This script is given
the keys that carry 51Did under their own names,
_51DEGREES_RESOURCE_KEY_51DID and _51DEGREES_LICENSE_KEY_51DID, and starts the
demo with them as the names the demo reads. Only the demo's process is given
those two names. The page is then checked with ci/check-51did-demo-page.ps1.

The demo must already be built, as the 51Did demo page workflow builds it.

No key or license value is ever printed. Only variable names are.

.PARAMETER Exe
The demo's program. Defaults to the debug build in the examples workspace.

.PARAMETER Port
The port the demo listens on.
#>
param(
    [string]$Exe,
    [int]$Port = 5100
)
$ErrorActionPreference = "Stop"

$ResourceKeyName = "_51DEGREES_RESOURCE_KEY_51DID"
$LicenseKeyName = "_51DEGREES_LICENSE_KEY_51DID"

# The environment the demo is started with: the two keys under the names the
# demo reads, and its port. The names this script was given them under are
# left out of it.
function Get-DemoEnvironment([string]$ResourceKey, [string]$LicenseKey, [int]$Port) {
    @{
        "_51DEGREES_RESOURCE_KEY" = $ResourceKey
        "_51DEGREES_LICENSE_KEY" = $LicenseKey
        "_51DEGREES_EXAMPLE_HTTP_PORTS" = "$Port"
        $ResourceKeyName = $null
        $LicenseKeyName = $null
    }
}

$resourceKey = [Environment]::GetEnvironmentVariable($ResourceKeyName)
$licenseKey = [Environment]::GetEnvironmentVariable($LicenseKeyName)
$missing = @()
if (-not $resourceKey) { $missing += $ResourceKeyName }
if (-not $licenseKey) { $missing += $LicenseKeyName }
if ($missing) {
    throw "$($missing -join ' and ') must be set to keys that carry 51Did."
}

if (-not $Exe) {
    $Exe = Join-Path $PSScriptRoot "../examples/target/debug/fodid-web-creator-context"
    if ($IsWindows) { $Exe += ".exe" }
}
if (-not (Test-Path -PathType Leaf $Exe)) {
    throw "The demo is not built at $Exe."
}

$logs = Join-Path ([IO.Path]::GetTempPath()) "51did-demo-page"
New-Item -ItemType Directory -Force -Path $logs | Out-Null
$stdout = Join-Path $logs "demo.out.txt"
$stderr = Join-Path $logs "demo.err.txt"
$url = "http://127.0.0.1:$Port"

$demo = Start-Process -FilePath $Exe -PassThru -NoNewWindow `
    -Environment (Get-DemoEnvironment $resourceKey $licenseKey $Port) `
    -RedirectStandardOutput $stdout -RedirectStandardError $stderr
try {
    $deadline = [DateTime]::UtcNow.AddMinutes(2)
    while ($true) {
        if ($demo.HasExited) { throw "The demo exited with code $($demo.ExitCode)." }
        try {
            if ((Invoke-WebRequest "$url/" -TimeoutSec 5).StatusCode -eq 200) { break }
        } catch { }
        if ([DateTime]::UtcNow -gt $deadline) { throw "The demo did not answer within 2 minutes." }
        Start-Sleep -Seconds 1
    }
    & (Join-Path $PSScriptRoot "check-51did-demo-page.ps1") -Url $url
} finally {
    if (-not $demo.HasExited) { $demo.Kill($true) }
    Write-Host "--- demo output ---"
    Get-Content $stdout, $stderr -ErrorAction SilentlyContinue |
        ForEach-Object { $_ -replace 'AQ[A-Za-z0-9_-]{12,}', '<redacted>' }
}
