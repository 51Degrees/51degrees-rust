# Tests that the published Device Detection crates package no tests or
# examples, as the Device Detection specification asks: "Examples and tests
# SHOULD NOT be included in any published packages." Each crate is listed as
# `cargo package` would package it, which compiles nothing.

BeforeDiscovery {
    # The Device Detection crates ci/crates.txt publishes.
    $script:Crates = @(
        'fiftyone-device-detection-sys'
        'fiftyone-device-detection-shared'
        'fiftyone-device-detection-cloud'
        'fiftyone-device-detection-onpremise'
        'fiftyone-device-detection'
    )
}

BeforeAll {
    $script:Root = Split-Path $PSScriptRoot -Parent
}

Describe 'The published Device Detection crates' {
    It '<_> packages no test or example' -ForEach $script:Crates {
        Push-Location $script:Root
        try {
            $files = @(cargo package --list --allow-dirty --quiet -p $_)
            $exit = $LASTEXITCODE
        }
        finally {
            Pop-Location
        }
        $exit | Should -Be 0
        $files | Should -Not -BeNullOrEmpty
        $files | Where-Object { $_ -match '^(tests|examples|benches)[\\/]' } |
            Should -BeNullOrEmpty
    }
}
