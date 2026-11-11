<#
.SYNOPSIS
Runs every on-premise example as someone who has just cloned the repository
would, on the free data files with no settings, and fails when one fails.

.DESCRIPTION
Each example runs from examples/ with no arguments and with every
_51DEGREES_ variable removed, so it finds the free data files in cxx/data
itself. Before any example starts, the data repository's script is run once,
as an example runs it, to download every free file the repository links that
is not already there, so no two examples download the same file. A try that
fails, or that does not finish in time and is stopped, is made again, and a
file still missing after the last try fails the step before the examples are
built.

The console examples must exit 0, print the contact-us link for the
Enterprise data file, and end with the "Find out more" section. A data file
an example could not download fails the step too. An example that runs too
long is stopped with every process it started, so a download it began cannot
keep a file open for the next one. The two on-premise web examples are
started, and their page must carry the same link and section.

Each example's output is printed in a group, and the lines about the
Enterprise data file and any download are printed outside the groups so the
log shows them without opening one.

The examples are built against this tree's crates, through
ci/test-examples-against-source.ps1, which leaves the working tree as it
found it.

.PARAMETER Root
The workspace root. Defaults to the repository this script sits in.
#>
param (
    [string]$Root = (Join-Path $PSScriptRoot "..")
)
$ErrorActionPreference = "Stop"

# The console examples, each run once with no arguments.
$Console = @(
    @{ Package = "device-detection-examples"; Bin = "dd-onprem-getting-started" },
    @{ Package = "device-detection-examples"; Bin = "dd-onprem-match-metrics" },
    @{ Package = "device-detection-examples"; Bin = "dd-onprem-metadata" },
    @{ Package = "device-detection-examples"; Bin = "dd-onprem-offline-processing" },
    @{ Package = "device-detection-examples"; Bin = "dd-onprem-performance" },
    @{ Package = "device-detection-examples"; Bin = "dd-onprem-update-data-file" },
    @{ Package = "ip-intelligence-examples"; Bin = "ipi-onprem-getting-started" },
    @{ Package = "ip-intelligence-examples"; Bin = "ipi-onprem-compare" },
    @{ Package = "ip-intelligence-examples"; Bin = "ipi-onprem-metadata" },
    @{ Package = "ip-intelligence-examples"; Bin = "ipi-onprem-metrics" },
    @{ Package = "ip-intelligence-examples"; Bin = "ipi-onprem-offline-processing" },
    @{ Package = "ip-intelligence-examples"; Bin = "ipi-onprem-performance" },
    @{ Package = "ip-intelligence-examples"; Bin = "ipi-onprem-suspicious" },
    @{ Package = "ip-intelligence-examples"; Bin = "ipi-onprem-update-data-file" }
)

# The on-premise web examples, each started, asked for its page and stopped.
$Web = @(
    @{ Package = "device-detection-examples"; Bin = "dd-web-getting-started-onprem"; Port = 5301 },
    @{ Package = "ip-intelligence-examples"; Bin = "ipi-web-getting-started-onprem"; Port = 5302 }
)

# What every example's output must carry.
$ContactUs = "51degrees.com/contact-us?utm_source=code"
$EnterpriseTerm = "utm_term=enterprise-data-file"
$FindOutMore = "Find out more"

# How long one console example, or one web example's start, may take.
$TimeoutSeconds = 600

# How long one try at downloading the free data files may take, and how many
# tries are made.
$DownloadTimeoutSeconds = 600
$DownloadTries = 3

$Examples = Join-Path $Root "examples"
$Data = Join-Path $Root "cxx/data"
$Logs = Join-Path $Examples "target/free-data-logs"
$failures = [System.Collections.Generic.List[string]]::new()

# Out of the box means no settings, so every variable the examples or the
# engines read is removed for this process and the examples it starts.
Get-ChildItem Env: |
    Where-Object { $_.Name -like "_51DEGREES_*" } |
    ForEach-Object {
        Write-Host "Removing $($_.Name) from the environment."
        Remove-Item -Path "Env:$($_.Name)"
    }

# The path to a built example.
function Get-Exe([string]$Bin) {
    $exe = Join-Path $Examples "target/debug/$Bin"
    if ($IsWindows) { $exe += ".exe" }
    return $exe
}

# Start an example with its output going to files, and return the process.
function Start-Example([string]$Bin) {
    $process = Start-Process -FilePath (Get-Exe $Bin) -PassThru -NoNewWindow `
        -WorkingDirectory $Examples `
        -RedirectStandardOutput (Join-Path $Logs "$Bin.out.txt") `
        -RedirectStandardError (Join-Path $Logs "$Bin.err.txt")
    # Reading the handle now keeps the exit code readable once it exits.
    $null = $process.Handle
    return $process
}

# Stop a process with every process it started, such as a data file download
# that would otherwise keep its partial file open after the process is gone.
function Stop-Tree($Process) {
    if (-not $Process.HasExited) {
        $Process.Kill($true)
        $Process.WaitForExit()
    }
}

# The command an example runs to download a missing data file, given no file
# names so that it downloads every file the data repository links. The script
# keeps a file that is already there.
function Get-DownloadCommand {
    if ($IsWindows) {
        $script = Join-Path $Data "get-data-files.ps1"
        return @{
            FilePath     = "pwsh"
            ArgumentList = "-NoProfile -ExecutionPolicy Bypass -File `"$script`""
        }
    }
    return @{
        FilePath     = "sh"
        ArgumentList = "`"$(Join-Path $Data "get-data-files.sh")`""
    }
}

# Download every free data file that is missing, making up to $DownloadTries
# tries. A try that runs out of time is stopped, and the size of each partial
# file it left is printed, so the log shows how far the download got. Returns
# whether every file is there.
function Get-FreeDataFiles {
    $command = Get-DownloadCommand
    for ($try = 1; $try -le $DownloadTries; $try++) {
        Write-Host ("Downloading any free data file that is missing, " +
            "try $try of $DownloadTries.")
        $started = [DateTime]::UtcNow
        $process = Start-Process @command -PassThru -NoNewWindow
        $null = $process.Handle
        if ($process.WaitForExit($DownloadTimeoutSeconds * 1000)) {
            $seconds = [int]([DateTime]::UtcNow - $started).TotalSeconds
            if ($process.ExitCode -eq 0) {
                Write-Host "Every free data file is there, after $seconds seconds."
                return $true
            }
            Write-Host ("::warning::The download failed with exit code " +
                "$($process.ExitCode) after $seconds seconds.")
            continue
        }
        Stop-Tree $process
        Write-Host ("::warning::The download did not finish within " +
            "$DownloadTimeoutSeconds seconds, so it was stopped.")
        Get-ChildItem -Path $Data -Recurse -File -Include *.part, *.unpacked, *.headers |
            ForEach-Object {
                Write-Host ("  {0} held {1:N1} MB" -f $_.Name, ($_.Length / 1MB))
                Remove-Item -LiteralPath $_.FullName -Force
            }
    }
    $failures.Add("The free data files could not be downloaded in $DownloadTries tries")
    return $false
}

# An example's standard error, or an empty string.
function Get-Errors([string]$Bin) {
    $path = Join-Path $Logs "$Bin.err.txt"
    if (-not (Test-Path $path)) { return "" }
    $text = Get-Content -Raw $path
    if ($null -eq $text) { return "" }
    return $text
}

# Print an example's output in a group, then the lines about the Enterprise
# data file and the download messages with the command each ran. A page is
# split at each tag as well, so each line is a piece of text or a link rather
# than the whole page.
function Show-Output([string]$Bin, [string]$Text) {
    $errors = Get-Errors $Bin
    Write-Host "::group::$Bin output"
    Write-Host $Text
    if ($errors) {
        Write-Host "--- standard error ---"
        Write-Host $errors
    }
    Write-Host "::endgroup::"
    $Text -split '\r?\n|(?=<)' |
        Where-Object { $_ -match 'Enterprise data file|contact-us|Find out more' } |
        Select-Object -First 14 |
        ForEach-Object { Write-Host "  $Bin | $($_.Trim())" }
    $errors -split '\r?\n' |
        Where-Object { $_ -match 'downloaded|get-data-files' } |
        ForEach-Object { Write-Host "  $Bin | $($_.Trim())" }
}

# Check that `Text` carries the contact-us link and then the section.
function Test-Output([string]$Bin, [string]$Text) {
    $link = $Text.IndexOf($EnterpriseTerm)
    $section = $Text.LastIndexOf($FindOutMore)
    if (-not $Text.Contains($ContactUs) -or $link -lt 0) {
        $failures.Add("$Bin gave no contact-us link for the Enterprise data file")
    }
    if ($section -lt 0) {
        $failures.Add("$Bin has no '$FindOutMore' section")
    } elseif ($link -gt $section) {
        $failures.Add("$Bin does not end with its '$FindOutMore' section")
    }
}

function Invoke-Console($Example) {
    $bin = $Example.Bin
    Write-Host "Running $bin"
    $process = Start-Example $bin
    $finished = $process.WaitForExit($TimeoutSeconds * 1000)
    if ($finished) {
        # Only a wait with no time limit also waits for the output to reach
        # its file, which on Linux PowerShell copies there line by line.
        $process.WaitForExit()
    } else {
        Stop-Tree $process
    }
    # What the example printed is shown even when it ran out of time, so the
    # log says what it was doing.
    $text = Get-Content -Raw (Join-Path $Logs "$bin.out.txt")
    if ($null -eq $text) { $text = "" }
    Show-Output $bin $text
    if (-not $finished) {
        $failures.Add("$bin did not finish within $TimeoutSeconds seconds")
        return
    }
    if ($process.ExitCode -ne 0) {
        $failures.Add("$bin exited with code $($process.ExitCode)")
    }
    if ((Get-Errors $bin) -match 'could not be downloaded') {
        $failures.Add("$bin could not download a data file")
    }
    Test-Output $bin $text
}

function Invoke-Web($Example) {
    $bin = $Example.Bin
    $url = "http://localhost:$($Example.Port)/"
    Write-Host "Starting $bin on $url"
    $env:PORT = "$($Example.Port)"
    $process = Start-Example $bin
    try {
        $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
        $page = $null
        while ($null -eq $page -and [DateTime]::UtcNow -lt $deadline) {
            if ($process.HasExited) { break }
            try {
                $response = Invoke-WebRequest -Uri $url -TimeoutSec 10
                if ($response.StatusCode -eq 200) { $page = $response.Content }
            } catch {
                Start-Sleep -Seconds 2
            }
        }
        if ($null -eq $page) {
            $state = if ($process.HasExited) {
                "exited with code $($process.ExitCode)"
            } else {
                "did not answer within $TimeoutSeconds seconds"
            }
            $failures.Add("$bin $state")
            $page = ""
        }
        Show-Output $bin $page
        if ($page) { Test-Output $bin $page }
    } finally {
        Stop-Tree $process
        Remove-Item Env:PORT -ErrorAction SilentlyContinue
    }
}

$run = {
    $packages = ($Console + $Web) | ForEach-Object { $_.Package } | Select-Object -Unique
    $bins = ($Console + $Web) | ForEach-Object { "--bin"; $_.Bin }
    $selection = $packages | ForEach-Object { "-p"; $_ }
    Write-Host "::group::Build the on-premise examples"
    cargo build --config source.toml @selection @bins
    Write-Host "::endgroup::"

    $null = New-Item -ItemType Directory -Force -Path $Logs
    foreach ($example in $Console) { Invoke-Console $example }
    foreach ($example in $Web) { Invoke-Web $example }
}

# Every example reads a free data file, so without them the examples are
# not built or run.
if (Get-FreeDataFiles) {
    & (Join-Path $PSScriptRoot "test-examples-against-source.ps1") -Root $Root -Command $run
}

if ($failures.Count -gt 0) {
    foreach ($failure in $failures) { Write-Host "::error::$failure" }
    exit 1
}
Write-Host "Every on-premise example ran on the free data files."
