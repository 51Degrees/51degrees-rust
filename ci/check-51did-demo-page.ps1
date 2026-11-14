<#
.SYNOPSIS
Opens the 51Did creator context demo page in headless Chrome on each route
and checks that it creates an identifier whose signature and creator context
verify.

.DESCRIPTION
The page runs the 51Degrees client script, asks the service for a 51Did,
verifies it from the browser and redeems the result on the demo's server, all
in JavaScript, so only a browser can check it. Chrome is driven through its
driver with plain WebDriver commands, the driver found from CHROMEWEBDRIVER,
as a file or a folder holding it, then from the path. For each route the page
is opened with mode set to that route, and the four status rows are read
until none is still waiting, or until the time runs out.

A route passes when the Identifier row reads "created for this browser" and
the Signature and Creator context rows read "verified". On the direct route
the Human confidence row must show a value, and on the route through the
demo's server it must say the value is shown on the direct route.

.PARAMETER Url
The demo's address, such as http://127.0.0.1:5100.

.PARAMETER Routes
The routes to open.

.PARAMETER TimeoutSeconds
How long each route's page may take to finish.
#>
param(
    [Parameter(Mandatory = $true)]
    [string]$Url,
    [string[]]$Routes = @("direct", "proxy"),
    [int]$TimeoutSeconds = 120
)
$ErrorActionPreference = "Stop"

function Find-Driver {
    $names = @("chromedriver", "chromedriver.exe")
    $configured = $env:CHROMEWEBDRIVER
    if ($configured) {
        if (Test-Path $configured -PathType Leaf) { return $configured }
        foreach ($name in $names) {
            $candidate = Join-Path $configured $name
            if (Test-Path $candidate -PathType Leaf) { return $candidate }
        }
    }
    foreach ($folder in ($env:PATH -split [IO.Path]::PathSeparator)) {
        foreach ($name in $names) {
            if (-not $folder) { continue }
            $candidate = Join-Path $folder $name
            if (Test-Path $candidate -PathType Leaf) { return $candidate }
        }
    }
    throw "No Chrome driver was found."
}

function Send-Command([string]$Method, [string]$Uri, $Body) {
    $request = @{
        Method = $Method; Uri = $Uri; TimeoutSec = 300
        ContentType = "application/json"
    }
    if ($null -ne $Body) {
        $request.Body = ConvertTo-Json -InputObject $Body -Depth 8 -Compress
    }
    return (Invoke-RestMethod @request).value
}

# The text of each status row, and of the explanation under them.
$ReadRows = @"
return ['s-human', 's-create', 's-signature', 's-context', 's-detail']
  .map(function (id) {
    var element = document.getElementById(id);
    return element ? element.textContent.trim() : '';
  });
"@

function Test-Route([string]$Base, [string]$Session, [string]$Route) {
    $null = Send-Command POST "$Base/session/$Session/url" @{
        url = "$($Url.TrimEnd('/'))/?mode=$Route" }
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        Start-Sleep -Seconds 2
        $rows = @(Send-Command POST "$Base/session/$Session/execute/sync" @{
            script = $ReadRows; args = @() })
        $busy = @($rows[0..3] | Where-Object {
            $_ -like "waiting*" -or $_ -like "working*" }).Count -gt 0
    } while ($busy -and [DateTime]::UtcNow -lt $deadline)

    $human, $create, $signature, $context, $detail = $rows
    Write-Host "Route $Route"
    Write-Host "  Human confidence: $human"
    Write-Host "  Identifier:       $create"
    Write-Host "  Signature:        $signature"
    Write-Host "  Creator context:  $context"
    if ($detail) { Write-Host "  Why:              $detail" }

    $problems = @()
    if ($busy) { $problems += "a row was still waiting after $TimeoutSeconds seconds" }
    if ($create -ne "created for this browser") { $problems += "no identifier was created" }
    if ($signature -ne "verified") { $problems += "the signature did not verify" }
    if ($context -ne "verified") { $problems += "the creator context did not verify" }
    if ($Route -eq "direct" -and
        ($human -like "waiting*" -or $human -eq "not returned for this resource key" -or -not $human)) {
        $problems += "no HumanConfidence was shown"
    }
    if ($Route -eq "proxy" -and $human -ne "shown when the browser calls the service directly") {
        $problems += "the Human confidence row does not say where the value is shown"
    }
    foreach ($problem in $problems) {
        Write-Host "::error title=51Did demo page ($Route)::$problem"
    }
    return $problems.Count -eq 0
}

$driver = Find-Driver
$listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
$listener.Start()
$port = $listener.LocalEndpoint.Port
$listener.Stop()
$base = "http://127.0.0.1:$port"
$process = Start-Process -FilePath $driver -ArgumentList "--port=$port" `
    -PassThru -NoNewWindow
$session = $null
$passed = $true
try {
    $deadline = [DateTime]::UtcNow.AddSeconds(60)
    while ($true) {
        try {
            if ((Send-Command GET "$base/status" $null).ready) { break }
        } catch { }
        if ([DateTime]::UtcNow -gt $deadline) {
            throw "The Chrome driver was not ready within 60 seconds."
        }
        Start-Sleep -Milliseconds 250
    }
    $session = (Send-Command POST "$base/session" @{
        capabilities = @{ alwaysMatch = @{
            browserName = "chrome"
            "goog:chromeOptions" = @{ args = @("--headless") } } } }).sessionId
    foreach ($route in $Routes) {
        if (-not (Test-Route $base $session $route)) { $passed = $false }
    }
} finally {
    if ($session) {
        try { $null = Send-Command DELETE "$base/session/$session" $null } catch { }
    }
    if (-not $process.HasExited) {
        $process.Kill($true)
        $process.WaitForExit()
    }
}
if (-not $passed) {
    throw "The 51Did demo page did not pass on every route."
}
Write-Host "The 51Did demo page passed on: $($Routes -join ', ')."
