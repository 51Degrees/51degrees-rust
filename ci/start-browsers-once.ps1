<#
.SYNOPSIS
Starts one session in each browser the browser contract tests use, through
the driver they use, opens a page in it and ends it, printing how long each
part took.

.DESCRIPTION
The first session of a browser on a fresh runner takes many times longer
than the sessions after it, and can pass the 60 seconds the shared suite
allows one WebDriver command, so ci/run-browser-contract.ps1 runs this before
the tests. Each browser and driver is found as the suite finds them: the
driver from CHROMEWEBDRIVER or GECKOWEBDRIVER, as a file or a folder holding
it, then from the path, and the browser from CHROME_BIN or FIREFOX_BIN, then
from the path. Each session is headless, as in the tests. A browser whose
driver is not found is passed over with a warning. Any other failure stops
the script, and the caller runs the tests either way.

.PARAMETER Url
The page each browser opens.

.PARAMETER LimitSeconds
The longest each step may take, being the driver starting, the session
starting, the page loading and the session ending.
#>
param(
    [Parameter(Mandatory = $true)]
    [string]$Url,
    [int]$LimitSeconds = 300
)
$ErrorActionPreference = "Stop"

# The first of the names in a folder on the path, as the suite searches.
function Find-OnPath([string[]]$Names) {
    foreach ($folder in ($env:PATH -split [IO.Path]::PathSeparator)) {
        if (-not $folder) { continue }
        foreach ($name in $Names) {
            $candidate = Join-Path $folder $name
            if (Test-Path $candidate -PathType Leaf) { return $candidate }
        }
    }
    return $null
}

# The driver the variable names, as a file or in the folder it names, then
# the first on the path.
function Find-Driver([string]$Variable, [string[]]$Names) {
    $configured = [Environment]::GetEnvironmentVariable($Variable)
    if ($configured) {
        if (Test-Path $configured -PathType Leaf) { return $configured }
        foreach ($name in $Names) {
            $candidate = Join-Path $configured $name
            if (Test-Path $candidate -PathType Leaf) { return $candidate }
        }
    }
    return Find-OnPath $Names
}

# The browser the variable names when the file is there, then the first on
# the path. Null leaves the driver to find the browser itself.
function Find-Browser([string]$Variable, [string[]]$Names) {
    $configured = [Environment]::GetEnvironmentVariable($Variable)
    if ($configured -and (Test-Path $configured -PathType Leaf)) {
        return $configured
    }
    return Find-OnPath $Names
}

function Get-FreePort {
    $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
    $listener.Start()
    try { return $listener.LocalEndpoint.Port } finally { $listener.Stop() }
}

# Sends one WebDriver command and gives its value.
function Send-Command([string]$Method, [string]$Uri, $Body) {
    $request = @{
        Method = $Method; Uri = $Uri; TimeoutSec = $LimitSeconds
        ContentType = "application/json"
    }
    if ($null -ne $Body) {
        $request.Body = ConvertTo-Json -InputObject $Body -Depth 8 -Compress
    }
    return (Invoke-RestMethod @request).value
}

function Get-Seconds([Diagnostics.Stopwatch]$Watch) {
    $seconds = [Math]::Round($Watch.Elapsed.TotalSeconds, 1)
    $Watch.Restart()
    return $seconds
}

$browsers = @(
    @{
        Name = "Chrome"; Variable = "CHROMEWEBDRIVER"
        Drivers = @("chromedriver", "chromedriver.exe")
        BinaryVariable = "CHROME_BIN"
        Binaries = @("google-chrome", "google-chrome-stable",
            "chromium-browser", "chromium", "chrome.exe")
        PortArguments = { param($port) @("--port=$port") }
        Capabilities = { param($binary)
            $options = @{ args = @("--headless") }
            if ($binary) { $options.binary = $binary }
            @{ browserName = "chrome"; "goog:chromeOptions" = $options }
        }
    },
    @{
        Name = "Firefox"; Variable = "GECKOWEBDRIVER"
        Drivers = @("geckodriver", "geckodriver.exe")
        BinaryVariable = "FIREFOX_BIN"
        Binaries = @("firefox", "firefox.exe")
        PortArguments = { param($port) @("--port", "$port") }
        Capabilities = { param($binary)
            $options = @{ args = @("--headless") }
            if ($binary) { $options.binary = $binary }
            @{ browserName = "firefox"; "moz:firefoxOptions" = $options }
        }
    }
)

foreach ($browser in $browsers) {
    $driver = Find-Driver $browser.Variable $browser.Drivers
    if (-not $driver) {
        Write-Host ("::warning::No $($browser.Name) driver was found, so " +
            "$($browser.Name) is not started before the tests.")
        continue
    }
    $binary = Find-Browser $browser.BinaryVariable $browser.Binaries
    $port = Get-FreePort
    $base = "http://127.0.0.1:$port"
    $logs = Join-Path ([IO.Path]::GetTempPath()) "start-$($browser.Name.ToLower())"
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $process = Start-Process -FilePath $driver -PassThru -NoNewWindow `
        -ArgumentList (& $browser.PortArguments $port) `
        -RedirectStandardOutput "$logs.out.txt" -RedirectStandardError "$logs.err.txt"
    $session = $null
    try {
        $deadline = [DateTime]::UtcNow.AddSeconds($LimitSeconds)
        $ready = $false
        while (-not $ready) {
            if ($process.HasExited) {
                throw "The $($browser.Name) driver exited with code " +
                    "$($process.ExitCode) before it was ready."
            }
            if ([DateTime]::UtcNow -gt $deadline) {
                throw "The $($browser.Name) driver was not ready within " +
                    "$LimitSeconds seconds."
            }
            try {
                $ready = (Send-Command GET "$base/status" $null).ready -eq $true
            } catch {
                Start-Sleep -Milliseconds 250
            }
        }
        $driverSeconds = Get-Seconds $watch
        $capabilities = & $browser.Capabilities $binary
        $session = (Send-Command POST "$base/session" @{
            capabilities = @{ alwaysMatch = $capabilities } }).sessionId
        $sessionSeconds = Get-Seconds $watch
        $null = Send-Command POST "$base/session/$session/url" @{ url = $Url }
        $pageSeconds = Get-Seconds $watch
        $null = Send-Command DELETE "$base/session/$session" $null
        $session = $null
        $endSeconds = Get-Seconds $watch
        Write-Host ("$($browser.Name) started once: driver $driverSeconds s, " +
            "session $sessionSeconds s, page $pageSeconds s, " +
            "end $endSeconds s ($driver)")
    } finally {
        if ($session) {
            try { $null = Send-Command DELETE "$base/session/$session" $null } catch { }
        }
        if (-not $process.HasExited) {
            $process.Kill($true)
            $process.WaitForExit()
        }
    }
}
