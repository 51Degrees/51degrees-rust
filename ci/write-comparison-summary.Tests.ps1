# Tests that the comparison summary holds the comparer's counts line and the
# result, and no other line of the report.

BeforeAll {
    $script:Script = Join-Path (Split-Path $PSScriptRoot -Parent) 'ci/write-comparison-summary.ps1'

    # A folder for one test's files.
    function New-Folder {
        $path = Join-Path ([System.IO.Path]::GetTempPath()) ("summary-" + [guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path $path | Out-Null
        $path
    }

    # A report in the comparer's format, with a not allowed section when
    # Failures is set.
    function New-Report {
        param([string] $Folder, [switch] $Failures)
        $lines = @(
            '## Comparison with the .NET packages (enterprise)'
            ''
            '1200 cases, 35 differences, 2 not allowed.'
            ''
        )
        if ($Failures) {
            $lines += @(
                '### Not allowed'
                ''
                '- `properties.device.hardwarename` value, 2 case(s)'
                '  - `case-one` (First case) at `properties.device.hardwarename`: .NET `"Alpha"`, Rust `"Beta"`'
                '  - `case-two` (Second case) at `properties.device.hardwarename`: .NET `"Gamma"`, Rust `"Delta"`'
                ''
            )
        }
        $lines += @(
            '### Allowed'
            ''
            '| Path | Kind | Count | Issue | Reason |'
            '| --- | --- | --- | --- | --- |'
            '| `properties.device.nativemodel` | value | 33 | #1 | Case of the name |'
        )
        $path = Join-Path $Folder 'report.md'
        Set-Content -LiteralPath $path -Value $lines -Encoding utf8
        $path
    }

    function Invoke-Summary {
        param([string] $Report, [string] $Outcome, [string] $Out)
        & $script:Script -Report $Report -Outcome $Outcome -Data enterprise -Out $Out
        Get-Content -LiteralPath $Out
    }
}

Describe 'The comparison summary' {
    It 'holds the counts and a pass for a comparison that passed' {
        $folder = New-Folder
        $report = New-Report -Folder $folder
        $summary = Invoke-Summary -Report $report -Outcome success -Out (Join-Path $folder 'summary.md')
        $summary | Should -Be @(
            '## Comparison with the .NET packages (enterprise)'
            ''
            '1200 cases, 35 differences, 2 not allowed.'
            ''
            'Result: passed.'
        )
    }

    It 'holds the counts and a fail for a comparison that failed' {
        $folder = New-Folder
        $report = New-Report -Folder $folder -Failures
        $summary = Invoke-Summary -Report $report -Outcome failure -Out (Join-Path $folder 'summary.md')
        $summary | Should -Contain '1200 cases, 35 differences, 2 not allowed.'
        $summary | Should -Contain 'Result: failed.'
    }

    It 'holds no line of the report other than the heading and the counts' {
        $folder = New-Folder
        $report = New-Report -Folder $folder -Failures
        $summary = Invoke-Summary -Report $report -Outcome failure -Out (Join-Path $folder 'summary.md')
        $text = $summary -join "`n"
        foreach ($part in '### Not allowed', '### Allowed', '| Path |', 'case-one', 'case-two', 'First case',
            'hardwarename', 'nativemodel', 'Alpha', 'Beta', 'Gamma', 'Delta', 'Case of the name') {
            $text | Should -Not -Match ([regex]::Escape($part))
        }
        foreach ($line in $summary) {
            $line | Should -Match '^(|## Comparison with the \.NET packages \(enterprise\)|\d+ cases, \d+ differences, \d+ not allowed\.|Result: (passed|failed|did not run)\.)$'
        }
    }

    It 'says the comparison did not run when the outcome is <Name>' -ForEach @(
        @{ Name = 'skipped'; Outcome = 'skipped' }
        @{ Name = 'cancelled'; Outcome = 'cancelled' }
        @{ Name = 'empty'; Outcome = '' }
    ) {
        $folder = New-Folder
        $report = New-Report -Folder $folder
        $summary = Invoke-Summary -Report $report -Outcome $Outcome -Out (Join-Path $folder 'summary.md')
        $summary | Should -Contain 'Result: did not run.'
        $summary | Should -Not -Contain 'Result: passed.'
    }

    It 'says there are no counts when there is no report' {
        $folder = New-Folder
        $summary = Invoke-Summary -Report (Join-Path $folder 'report.md') -Outcome failure -Out (Join-Path $folder 'summary.md')
        $summary | Should -Contain 'The comparison wrote no counts.'
        $summary | Should -Contain 'Result: failed.'
    }

    It 'takes no counts from a line that holds more than the counts' {
        $folder = New-Folder
        $report = Join-Path $folder 'report.md'
        Set-Content -LiteralPath $report -Encoding utf8 -Value @(
            '## Comparison with the .NET packages (enterprise)'
            ''
            '1200 cases, 35 differences, 2 not allowed. `Alpha`'
        )
        $summary = Invoke-Summary -Report $report -Outcome success -Out (Join-Path $folder 'summary.md')
        ($summary -join "`n") | Should -Not -Match 'Alpha'
        $summary | Should -Contain 'The comparison wrote no counts.'
    }

    It 'makes the folder of the summary' {
        $folder = New-Folder
        $report = New-Report -Folder $folder
        $out = Join-Path $folder 'new/summary.md'
        $summary = Invoke-Summary -Report $report -Outcome success -Out $out
        Test-Path -LiteralPath $out | Should -BeTrue
        $summary | Should -Contain 'Result: passed.'
    }
}
