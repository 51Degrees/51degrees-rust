# Tests that the release timing comparison reads a run's RESULT lines, gives
# each side's median, range and rounds won, and fails only when a measure's
# whole range sits on the slower side of the base's, which is the release
# timing rule.

BeforeAll {
    $script:Script = Join-Path (Split-Path $PSScriptRoot -Parent) 'ci/compare-timing.ps1'

    # A log of ROUNDS paired rounds, taking each side's figures from the lists
    # given. The lines are exactly those the timing workflow prints.
    function New-Log {
        param(
            [int[]] $BaseDetections,
            [int[]] $HeadDetections,
            [int[]] $BaseLookups = $null,
            [int[]] $HeadLookups = $null,
            [int[]] $BaseSingle = $null,
            [int[]] $HeadSingle = $null
        )
        if ($null -eq $BaseLookups) { $BaseLookups = $BaseDetections }
        if ($null -eq $HeadLookups) { $HeadLookups = $HeadDetections }
        if ($null -eq $BaseSingle) { $BaseSingle = $BaseDetections }
        if ($null -eq $HeadSingle) { $HeadSingle = $HeadDetections }
        $lines = foreach ($i in 0..($BaseDetections.Count - 1)) {
            $round = $i + 1
            "RESULT dd $round base $($BaseDetections[$i]) detections 40000"
            "RESULT dd $round head $($HeadDetections[$i]) detections 40000"
            "RESULT ipi $round base $($BaseLookups[$i]) single $($BaseSingle[$i]) loaded 1"
            "RESULT ipi $round head $($HeadLookups[$i]) single $($HeadSingle[$i]) loaded 1"
        }
        $path = Join-Path ([System.IO.Path]::GetTempPath()) ("timing-" + [guid]::NewGuid().ToString('N') + ".txt")
        Set-Content -LiteralPath $path -Value $lines -Encoding utf8
        $path
    }
}

Describe 'The release timing comparison' {
    It 'passes when the two ranges meet' {
        # The head is a little slower in the median round and the ranges
        # overlap, which the rule counts as not slower.
        $log = New-Log -BaseDetections @(100, 110, 120, 130) -HeadDetections @(95, 105, 115, 125)
        $out = & pwsh -NoProfile -File $script:Script -Log $log 2>&1
        $LASTEXITCODE | Should -Be 0
        ($out -join "`n") | Should -Match 'the ranges meet'
    }

    It 'fails when the head is wholly slower than the base' {
        # Every head round is below every base round, so the head's whole
        # range sits on the slower side and the rule counts it as slower.
        $log = New-Log -BaseDetections @(100, 110, 120, 130) -HeadDetections @(10, 20, 30, 40)
        $out = & pwsh -NoProfile -File $script:Script -Log $log 2>&1
        $LASTEXITCODE | Should -Be 1
        ($out -join "`n") | Should -Match 'slower than'
    }

    It 'passes when the head is wholly faster than the base' {
        $log = New-Log -BaseDetections @(10, 20, 30, 40) -HeadDetections @(100, 110, 120, 130)
        $out = & pwsh -NoProfile -File $script:Script -Log $log 2>&1
        $LASTEXITCODE | Should -Be 0
    }

    It 'fails on one measure even when the others meet' {
        # Device Detection and the multi-threaded lookups meet, and the
        # single-threaded lookups are wholly slower, so the run fails.
        $log = New-Log -BaseDetections @(100, 110, 120, 130) `
                       -HeadDetections @(95, 105, 115, 125) `
                       -BaseSingle @(100, 110, 120, 130) `
                       -HeadSingle @(10, 20, 30, 40)
        $out = & pwsh -NoProfile -File $script:Script -Log $log 2>&1
        $LASTEXITCODE | Should -Be 1
        ($out -join "`n") | Should -Match 'one thread'
    }

    It 'refuses a log with no results, rather than passing' {
        $path = Join-Path ([System.IO.Path]::GetTempPath()) ("timing-empty-" + [guid]::NewGuid().ToString('N') + ".txt")
        Set-Content -LiteralPath $path -Value @('nothing here', 'RESULT of nothing') -Encoding utf8
        $out = & pwsh -NoProfile -File $script:Script -Log $path 2>&1
        $LASTEXITCODE | Should -Be 1
        ($out -join "`n") | Should -Match 'no paired rounds'
    }

    It 'refuses fewer paired rounds than it was told to expect' {
        $log = New-Log -BaseDetections @(100, 110) -HeadDetections @(95, 105)
        $out = & pwsh -NoProfile -File $script:Script -Log $log -Rounds 30 2>&1
        $LASTEXITCODE | Should -Be 1
        ($out -join "`n") | Should -Match '2 paired rounds'
    }

    It 'ignores a round only one side finished' {
        # A round with no head figure is not a paired round, so it is left out
        # rather than compared against nothing.
        $log = New-Log -BaseDetections @(100, 110, 120, 130) -HeadDetections @(95, 105, 115, 125)
        Add-Content -LiteralPath $log -Value 'RESULT dd 5 base 1 detections 40000' -Encoding utf8
        $out = & pwsh -NoProfile -File $script:Script -Log $log 2>&1
        $LASTEXITCODE | Should -Be 0
        ($out -join "`n") | Should -Match '4 paired rounds'
    }
}
