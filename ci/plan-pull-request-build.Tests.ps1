# Tests for ci/plan-pull-request-build.ps1, which decides what the Pull
# Request workflow builds. The rules are tested on a small made-up workspace,
# and one test reads this workspace with `cargo metadata`, which compiles
# nothing.

BeforeAll {
    $script:SavedOutput = $env:GITHUB_OUTPUT
    $env:GITHUB_OUTPUT = $null
    $script:Root = Split-Path $PSScriptRoot -Parent
    $script:Plan = Join-Path $PSScriptRoot 'plan-pull-request-build.ps1'
    $script:WorkspaceRoot = Join-Path ([IO.Path]::GetTempPath()) 'plan-workspace'

    # A member of the made-up workspace, in $Folder, with its path
    # dependencies, each 'name' or 'name:dev'.
    function New-Member([string] $Name, [string] $Folder, [string[]] $Dependencies = @()) {
        [ordered]@{
            name          = $Name
            manifest_path = Join-Path (Join-Path $script:WorkspaceRoot $Folder) 'Cargo.toml'
            dependencies  = @($Dependencies | ForEach-Object {
                $dependency, $kind = $_ -split ':'
                [ordered]@{
                    name = $dependency
                    kind = $kind
                    path = Join-Path $script:WorkspaceRoot $dependency
                }
            })
        }
    }

    # The made-up workspace's metadata, as `cargo metadata --no-deps` writes it.
    $script:Metadata = Join-Path ([IO.Path]::GetTempPath()) "plan-metadata-$([Guid]::NewGuid()).json"
    [ordered]@{
        workspace_root = $script:WorkspaceRoot
        packages       = @(
            New-Member 'core' 'core'
            New-Member 'engines' 'engines' @('core')
            New-Member 'leaf' 'leaf' @('engines')
            New-Member 'tester' 'tester' @('leaf:dev')
            New-Member 'alone' 'alone'
            New-Member 'outer' 'outer'
            New-Member 'inner' 'outer/inner'
            New-Member 'fiftyone-native' 'native' @('core')
            New-Member 'fodid' 'fodid'
        )
    } | ConvertTo-Json -Depth 6 | Set-Content -Path $script:Metadata -Encoding utf8

    # The outputs of a plan, as a hashtable.
    function Invoke-Plan {
        param(
            [string] $Event = 'pull_request',
            [string] $BaseRef = 'rc',
            [bool] $MacOS = $true,
            [string[]] $ChangedFiles = @(),
            [string] $MetadataFile = $script:Metadata
        )
        $output = Join-Path ([IO.Path]::GetTempPath()) "plan-output-$([Guid]::NewGuid())"
        try {
            $null = & $script:Plan -Event $Event -BaseRef $BaseRef -MacOS $MacOS `
                -ChangedFiles $ChangedFiles -MetadataFile $MetadataFile -OutputFile $output 6>&1
            $result = @{}
            foreach ($line in Get-Content $output) {
                $name, $value = $line -split '=', 2
                $result[$name] = $value
            }
            return $result
        }
        finally {
            Remove-Item -Force -ErrorAction SilentlyContinue $output
        }
    }
}

AfterAll {
    $env:GITHUB_OUTPUT = $script:SavedOutput
    Remove-Item -Force -ErrorAction SilentlyContinue $script:Metadata
}

Describe 'What a pull request into rc builds' {
    It 'builds a changed member that nothing depends on, and nothing else' {
        (Invoke-Plan -ChangedFiles 'alone/src/lib.rs').packages | Should -Be '-p alone'
    }

    It 'builds every member that depends on a changed one, dev-dependencies included' {
        $plan = Invoke-Plan -ChangedFiles 'core/src/lib.rs'
        $plan.packages | Should -Be '-p core -p engines -p leaf -p tester -p fiftyone-native'
    }

    It 'gives a file to the deepest member whose folder holds it' {
        (Invoke-Plan -ChangedFiles 'outer/inner/src/lib.rs').packages | Should -Be '-p inner'
        (Invoke-Plan -ChangedFiles 'outer/src/lib.rs').packages | Should -Be '-p outer'
    }

    It 'builds a member whose Markdown changes, as its documentation tests read it' {
        (Invoke-Plan -ChangedFiles 'leaf/README.md').packages | Should -Be '-p leaf -p tester'
    }

    It 'builds the whole workspace for <_>, which the build reads' -ForEach @(
        'Cargo.lock', 'Cargo.toml', 'cxx', 'owid-rust',
        'deprecated-names/environment.rs', '.github/workflows/pull-request.yml',
        '.github/actions/restore-data-files/action.yml',
        'ci/plan-pull-request-build.ps1', 'ci/copy-owid-source.ps1',
        'ci/wasm-size/src/lib.rs'
    ) {
        $plan = Invoke-Plan -ChangedFiles @('alone/src/lib.rs', $_)
        $plan.packages | Should -Be '--workspace'
    }

    It 'builds nothing for <_>, which no build step reads' -ForEach @(
        'README.md', 'AGENTS.md', 'LICENSE', 'examples/pipeline-examples/src/main.rs',
        'parity/rust/src/main.rs', 'ci/missing-ci-scripts.md',
        'ci/update-packages.ps1', '.github/workflows/examples.yml'
    ) {
        (Invoke-Plan -ChangedFiles $_).packages | Should -BeNullOrEmpty
    }

    It 'leaves macOS out' {
        (Invoke-Plan -ChangedFiles 'alone/src/lib.rs').os |
            Should -Be '["windows-latest","ubuntu-22.04","ubuntu-24.04"]'
        (Invoke-Plan -BaseRef 'release/4.5' -ChangedFiles 'alone/src/lib.rs').os |
            Should -Be '["windows-latest","ubuntu-22.04","ubuntu-24.04"]'
    }

    It 'says whether fiftyone-native and fodid are built' {
        $plan = Invoke-Plan -ChangedFiles 'core/src/lib.rs'
        $plan.native | Should -Be 'true'
        $plan.fodid | Should -Be 'false'
        $plan = Invoke-Plan -ChangedFiles 'fodid/src/lib.rs'
        $plan.native | Should -Be 'false'
        $plan.fodid | Should -Be 'true'
    }
}

Describe 'What a release build builds' {
    It 'builds everything with macOS for a pull request into main' {
        $plan = Invoke-Plan -BaseRef 'main' -ChangedFiles 'alone/src/lib.rs'
        $plan.packages | Should -Be '--workspace'
        $plan.os | Should -Be '["windows-latest","ubuntu-22.04","ubuntu-24.04","macos-latest"]'
        $plan.native | Should -Be 'true'
        $plan.fodid | Should -Be 'true'
    }

    It 'builds everything with macOS for a push to main' {
        $plan = Invoke-Plan -Event 'push' -BaseRef ''
        $plan.packages | Should -Be '--workspace'
        $plan.os | Should -Match 'macos-latest'
    }

    It 'builds everything for a run by hand, on macOS unless it is left out' {
        $plan = Invoke-Plan -Event 'workflow_dispatch' -BaseRef ''
        $plan.packages | Should -Be '--workspace'
        $plan.os | Should -Match 'macos-latest'
        (Invoke-Plan -Event 'workflow_dispatch' -BaseRef '' -MacOS $false).os |
            Should -Not -Match 'macos'
    }
}

Describe 'The planner and the workflow' {
    It 'builds the whole workspace when <_>, which the workflow runs, changes' -ForEach @(
        [regex]::Matches(
            (Get-Content -Raw (Join-Path (Split-Path $PSScriptRoot -Parent) '.github/workflows/pull-request.yml')),
            '(?<![\w-])(?:\./)?(ci/[\w./-]*[\w-])') |
            ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique
    ) {
        (Invoke-Plan -ChangedFiles $_).packages | Should -Be '--workspace'
    }
}

Describe 'This workspace' {
    BeforeAll {
        $script:Real = Join-Path ([IO.Path]::GetTempPath()) "plan-real-$([Guid]::NewGuid()).json"
        Push-Location $script:Root
        try {
            cargo metadata --no-deps --format-version 1 | Set-Content -Path $script:Real -Encoding utf8
            if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed' }
        }
        finally {
            Pop-Location
        }
    }

    AfterAll {
        Remove-Item -Force -ErrorAction SilentlyContinue $script:Real
    }

    It 'builds only fiftyone-pipeline-web-axum for a change to it' {
        (Invoke-Plan -MetadataFile $script:Real -ChangedFiles 'pipeline-web-axum/src/lib.rs').packages |
            Should -Be '-p fiftyone-pipeline-web-axum'
    }

    It 'builds the members that depend on fiftyone-pipeline-core with it, and not fodid' {
        $plan = Invoke-Plan -MetadataFile $script:Real -ChangedFiles 'pipeline-core/src/lib.rs'
        $plan.packages | Should -Match '-p fiftyone-pipeline-core\b'
        $plan.packages | Should -Match '-p fiftyone-device-detection\b'
        $plan.packages | Should -Match '-p fiftyone-pipeline-web-axum\b'
        $plan.packages | Should -Not -Match '-p fodid\b'
        $plan.native | Should -Be 'true'
    }
}
