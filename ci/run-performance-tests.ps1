param(
    # The directory the rust workspace is checked out to. CI checks the repo out
    # into a subdirectory named after the repository, matching the other
    # languages; a local run can pass "." for the current directory.
    [string]$RepoName = ".",
    # Where the results files are written, relative to the repo directory. This is
    # the path the nightly workflow uploads and the compare-performance step reads.
    [string]$OutputDir = "test-results/performance-summary",
    # The common-ci checkout whose steps/publish-performance-results.ps1
    # validates each results file and copies it to $OutputDir. CI checks it out
    # beside this repository.
    [string]$CommonCi = (Join-Path $RepoName "../common-ci")
)
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

# Checked first, so a missing checkout fails before the long build.
$publishStep = Join-Path $CommonCi "steps/publish-performance-results.ps1"
if (!(Test-Path -Path $publishStep -PathType Leaf)) {
    Write-Error "'$publishStep' was not found. Check out 51Degrees/common-ci beside this repository, or pass -CommonCi."
}

# The IP Intelligence performance example reads the ASN file from the data
# repository in the cxx submodule (51DEGREES_IPI_PATH). -Force downloads it
# again even when an earlier run left one, so the figures are always for the
# latest file. The script is resolved against $RepoName rather than the
# caller's current directory, so a CI run from the folder above the checkout
# and a local run from the repo root both find it. A failed download stops
# the run, rather than measuring an older file. The script sets an exit code
# only when it fails, so any code an earlier command left is cleared first.
$global:LASTEXITCODE = 0
& (Join-Path $RepoName "cxx/data/get-data-files.ps1") 51Degrees-IPIV4AsnIpiV41.ipi -Force
if ($LASTEXITCODE) {
    throw "The ASN data file was not downloaded."
}

# Runs the 51Degrees Rust on-premise performance examples in release. Each
# example writes its own results JSON, in the shape the shared
# steps/compare-performance.ps1 reads, and the shared
# steps/publish-performance-results.ps1 validates it and copies it to
# results_<Name>.json under $OutputDir. The Rust on-premise engines call the
# C libraries in the cxx submodule through FFI, so the figures track the
# native C performance.
#
# Name is the configuration the graph history is keyed on, and the metric
# names inside each file are its series, so neither may change.
$performanceExamples = @(
    @{ Name = "DeviceDetection-OnPremise"; Package = "device-detection-examples"; Bin = "dd-onprem-performance" }
    @{ Name = "IpIntelligence-OnPremise"; Package = "ip-intelligence-examples"; Bin = "ipi-onprem-performance" }
)

# Absolute, because the examples run from the examples directory.
$resultsDir = Join-Path (Resolve-Path $RepoName).Path "test-results/performance"
foreach ($example in $performanceExamples) {
    $example.Results = Join-Path $resultsDir "results_$($example.Name).json"
    # A file left by an earlier run would otherwise pass for this run's figures.
    Remove-Item -Path $example.Results -Force -ErrorAction SilentlyContinue
}

# The examples depend on the published crates. ci/test-examples-against-source.ps1
# points them at this checkout for the run and puts the manifests and the lock
# back afterwards, so the figures are for the code being tested and not for the
# last release. The closure carries the list above into that script.
$runExamples = {
    foreach ($example in $performanceExamples) {
        Write-Host "Running performance example '$($example.Bin)'..."
        cargo run --release --config source.toml -p $example.Package --bin $example.Bin -- --json-output $example.Results
    }
}.GetNewClosure()
& (Join-Path $PSScriptRoot "test-examples-against-source.ps1") -Command $runExamples

foreach ($example in $performanceExamples) {
    & $publishStep -SourceFile $example.Results -Name $example.Name -RepoName $RepoName -SummaryDir $OutputDir
}
