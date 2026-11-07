# Tests for the scripts that release each crate at its own version.
#
# The release rules themselves are common-ci's steps/release-plan.ps1, tested
# there. These tests check what is particular to this repository: that
# ci/packages.json describes the crates truthfully, that the plan it gives
# releases what the rules promise for the real crate graph, and that the
# versions and requirements are written as planned. Repositories are built in
# a temporary folder, so nothing here touches this checkout or the network.
#
# common-ci is found as ci/packages.ps1 describes, so set COMMON_CI to a
# checkout of it when running these outside CI.

BeforeAll {
    # A run in a workflow step has these set, and the scripts under test
    # would write the plans of the throwaway repositories into that step.
    $script:SavedOutput = $env:GITHUB_OUTPUT
    $script:SavedSummary = $env:GITHUB_STEP_SUMMARY
    $env:GITHUB_OUTPUT = $null
    $env:GITHUB_STEP_SUMMARY = $null

    $script:Root = Split-Path $PSScriptRoot -Parent
    . (Join-Path $PSScriptRoot 'packages.ps1')
    $script:Packages = @(Get-Packages $script:Root)
    $script:Names = @($script:Packages.Name)
    $script:Order = @(Get-PublishOrder $script:Root)
    $script:Temp = @()

    function New-TempFolder {
        $path = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString())
        $null = New-Item -ItemType Directory -Force $path
        $script:Temp += $path
        return $path
    }

    function Copy-RepoFile([string]$To, [string]$Relative) {
        $target = Join-Path $To $Relative
        $null = New-Item -ItemType Directory -Force (Split-Path $target)
        Copy-Item (Join-Path $script:Root $Relative) $target
    }

    # The files ci/check-packages.ps1 and ci/set-crate-versions.ps1 read,
    # copied so a test can change them.
    function Copy-Workspace {
        $to = New-TempFolder
        foreach ($file in @('Cargo.toml', '.gitmodules', 'ci/crates.txt',
                'ci/packages.json', 'ci/copy-cxx-sources.ps1',
                'ci/copy-owid-source.ps1')) {
            Copy-RepoFile $to $file
        }
        foreach ($package in $script:Packages) {
            Copy-RepoFile $to "$($package.Path)/Cargo.toml"
        }
        return $to
    }

    # Runs ci/check-packages.ps1, keeping what it printed before it failed.
    function Invoke-Check([string]$RepoRoot) {
        $lines = [Collections.Generic.List[string]]::new()
        $failed = $false
        try {
            & (Join-Path $script:Root 'ci/check-packages.ps1') -RepoRoot $RepoRoot |
                ForEach-Object { $lines.Add("$_") }
        } catch {
            $failed = $true
        }
        return [pscustomobject]@{ Failed = $failed; Output = $lines -join "`n" }
    }

    function Edit-Packages([string]$RepoRoot, [scriptblock]$Change) {
        $path = Join-Path $RepoRoot 'ci/packages.json'
        $packages = @(Get-Content -Raw $path | ConvertFrom-Json)
        & $Change $packages
        ConvertTo-Json -InputObject $packages -Depth 4 | Set-Content $path
    }

    function Add-Commit([string]$Repo, [string]$Message = 'Change') {
        git -C $Repo add -A
        git -C $Repo -c user.name=test -c user.email=test@example.com `
            commit --quiet --allow-empty -m $Message
    }

    function Add-Change([string]$Repo, [string[]]$Files) {
        foreach ($file in $Files) {
            $full = Join-Path $Repo $file
            $null = New-Item -ItemType Directory -Force (Split-Path $full)
            Add-Content -Path $full -Value ([Guid]::NewGuid())
        }
        Add-Commit $Repo
    }

    # Points a submodule of the repository at another commit, as the nightly
    # submodule update does.
    function Move-Submodule([string]$Repo, [string]$Path) {
        $commit = [Guid]::NewGuid().ToString('N').Substring(0, 32) + '00000000'
        git -C $Repo update-index --add --cacheinfo "160000,$commit,$Path"
        git -C $Repo -c user.name=test -c user.email=test@example.com `
            commit --quiet -m "Move $Path"
    }

    # A repository laid out as this one, with a file in every crate folder
    # and in the folders that hold no crate, the two submodules, the real
    # ci/packages.json and ci/crates.txt, and every crate tagged at 4.5.18.
    function New-PlanRepo {
        $repo = New-TempFolder
        git init --quiet $repo
        foreach ($file in @('ci/packages.json', 'ci/crates.txt')) {
            Copy-RepoFile $repo $file
        }
        $files = @($script:Packages | ForEach-Object { "$($_.Path)/src/lib.rs" }) +
            @('ci/publish-crates.sh', '.github/workflows/publish.yml',
              'docs/README.md', 'examples/Cargo.toml', 'README.md',
              'AGENTS.md', 'Cargo.toml')
        foreach ($file in $files) {
            $full = Join-Path $repo $file
            $null = New-Item -ItemType Directory -Force (Split-Path $full)
            Set-Content -Path $full -Value 'start'
        }
        Add-Commit $repo 'Start'
        Move-Submodule $repo 'cxx'
        Move-Submodule $repo 'owid-rust'
        foreach ($name in $script:Names) {
            git -C $repo tag "$name/4.5.18"
        }
        return $repo
    }

    function Get-Plan([string]$Repo) {
        return @(Get-CrateReleasePlan $Repo)
    }

    # The crates that depend on the given one, directly or through another,
    # read from ci/packages.json, for a test to state what it expects.
    function Get-Dependents([string]$Name) {
        $found = [Collections.Generic.List[string]]::new()
        $found.Add($Name)
        $grew = $true
        while ($grew) {
            $grew = $false
            foreach ($package in $script:Packages) {
                if ($found.Contains($package.Name)) { continue }
                if (@($package.DependsOn | Where-Object { $found.Contains($_) }).Count -gt 0) {
                    $found.Add($package.Name)
                    $grew = $true
                }
            }
        }
        return @($found)
    }

    # Every requirement on a crate across the copied manifests.
    function Get-Requirements([string]$RepoRoot, [string]$Name) {
        foreach ($package in $script:Packages) {
            $text = Get-Content -Raw (Join-Path $RepoRoot $package.Path 'Cargo.toml')
            foreach ($match in [regex]::Matches($text,
                    "(?m)^\s*$([regex]::Escape($Name))\s*=\s*\{[^}]*?version\s*=\s*""([^""]+)""")) {
                $match.Groups[1].Value
            }
        }
    }

    function Get-OwnVersion([string]$RepoRoot, [string]$Path) {
        $text = Get-Content -Raw (Join-Path $RepoRoot $Path 'Cargo.toml')
        return [regex]::Match($text, '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
    }
}

AfterAll {
    foreach ($path in $script:Temp) {
        if (Test-Path $path) {
            Remove-Item -Recurse -Force $path
        }
    }
    $env:GITHUB_OUTPUT = $script:SavedOutput
    $env:GITHUB_STEP_SUMMARY = $script:SavedSummary
}

Describe 'ci/check-packages.ps1' {
    It 'passes on this repository' {
        $result = Invoke-Check $script:Root
        $result.Output | Should -BeLike '*matches the 25 crates*'
        $result.Failed | Should -BeFalse
    }

    It 'refuses a DependsOn that leaves out a crate the manifest requires' {
        $copy = Copy-Workspace
        Edit-Packages $copy {
            param($packages)
            ($packages | Where-Object Name -eq 'fiftyone-device-detection-sys').DependsOn = @()
        }
        $result = Invoke-Check $copy
        $result.Failed | Should -BeTrue
        $result.Output | Should -BeLike '*fiftyone-device-detection-sys has DependsOn (none)*requires fiftyone-common-sys*'
    }

    It 'refuses a DependsOn naming a crate the manifest does not require' {
        $copy = Copy-Workspace
        Edit-Packages $copy {
            param($packages)
            ($packages | Where-Object Name -eq 'fiftyone-pipeline-core').DependsOn = @('fodid')
        }
        (Invoke-Check $copy).Failed | Should -BeTrue
    }

    It 'refuses a -sys crate that does not list the cxx submodule' {
        $copy = Copy-Workspace
        Edit-Packages $copy {
            param($packages)
            $packages | Where-Object Name -eq 'fiftyone-ip-intelligence-sys' |
                ForEach-Object { $_.PSObject.Properties.Remove('ExtraPaths') }
        }
        $result = Invoke-Check $copy
        $result.Failed | Should -BeTrue
        $result.Output | Should -BeLike '*fiftyone-ip-intelligence-sys has ExtraPaths (none)*copy cxx*'
    }

    It 'refuses fodid without the owid-rust submodule' {
        $copy = Copy-Workspace
        Edit-Packages $copy {
            param($packages)
            ($packages | Where-Object Name -eq 'fodid').ExtraPaths = @('cxx')
        }
        (Invoke-Check $copy).Output | Should -BeLike '*fodid has ExtraPaths cxx*copy owid-rust*'
    }

    It 'refuses a crate ci/crates.txt publishes that ci/packages.json leaves out' {
        $copy = Copy-Workspace
        Edit-Packages $copy {
            param($packages)
            $packages[0].Name = 'not-a-crate'
        }
        $result = Invoke-Check $copy
        $result.Failed | Should -BeTrue
        $result.Output | Should -BeLike '*publishes fiftyone-common-sys, which ci/packages.json does not list*'
    }

    It 'refuses ci/crates.txt listing a crate before one it requires as a dev-dependency' {
        $copy = Copy-Workspace
        $path = Join-Path $copy 'ci/crates.txt'
        $lines = Get-Content $path | Where-Object { $_ -ne 'fiftyone-ip-intelligence' }
        $lines += 'fiftyone-ip-intelligence'
        Set-Content $path $lines
        $result = Invoke-Check $copy
        $result.Failed | Should -BeTrue
        $result.Output | Should -BeLike '*lists fiftyone-device-detection before fiftyone-ip-intelligence*'
    }

    It 'leaves dev-dependencies out of DependsOn' {
        # fiftyone-device-detection tests against fiftyone-ip-intelligence, so
        # a release of one must not release the other.
        ($script:Packages | Where-Object Name -eq 'fiftyone-device-detection').DependsOn |
            Should -Not -Contain 'fiftyone-ip-intelligence'
    }
}

Describe 'The release plan of the crates' {
    BeforeAll {
        $script:Repo = New-PlanRepo
        $script:Start = git -C $script:Repo rev-parse HEAD
    }

    BeforeEach {
        git -C $script:Repo reset --quiet --hard $script:Start
    }

    It 'plans nothing when nothing changed since the tags' {
        Add-Commit $script:Repo
        (Get-Plan $script:Repo).Count | Should -Be 0
    }

    It 'plans a crate nothing depends on alone' {
        Get-Dependents 'fiftyone-derived-properties' | Should -Be @('fiftyone-derived-properties')
        Add-Change $script:Repo @('derived-properties/src/lib.rs')
        $plan = Get-Plan $script:Repo
        $plan.Name | Should -Be @('fiftyone-derived-properties')
        $plan[0].Reason | Should -Be 'changed since 4.5.18'
    }

    It 'plans a crate others depend on with every crate depending on it, in dependency order' {
        Add-Change $script:Repo @('pipeline-core/src/lib.rs')
        $plan = Get-Plan $script:Repo
        $expected = Get-Dependents 'fiftyone-pipeline-core'
        $expected.Count | Should -BeGreaterThan 1
        @($plan.Name | Sort-Object) | Should -Be @($expected | Sort-Object)
        for ($index = 0; $index -lt $plan.Count; $index++) {
            $package = $script:Packages | Where-Object Name -eq $plan[$index].Name
            foreach ($dependency in $package.DependsOn) {
                $at = [array]::IndexOf([string[]]$plan.Name, $dependency)
                $at | Should -BeLessThan $index -Because "$($package.Name) requires $dependency"
            }
        }
    }

    It 'plans the -sys crates and every crate built on them when the cxx submodule moves' {
        Move-Submodule $script:Repo 'cxx'
        $plan = Get-Plan $script:Repo
        $plan.Name | Should -Be @(
            'fiftyone-common-sys', 'fiftyone-device-detection-sys',
            'fiftyone-ip-intelligence-sys', 'fiftyone-native',
            'fiftyone-device-detection-onpremise',
            'fiftyone-ip-intelligence-onpremise', 'fiftyone-ip-intelligence',
            'fiftyone-device-detection')
        ($plan | Where-Object Name -like '*-sys').Reason |
            Should -Be (@('changed since 4.5.18') * 3)
    }

    It 'plans fodid and the crates built on it when the owid-rust submodule moves' {
        Move-Submodule $script:Repo 'owid-rust'
        $plan = Get-Plan $script:Repo
        $plan.Name | Should -Be @('fodid', 'fodid-client', 'fiftyone-fodid-cloud')
    }

    It 'plans nothing for a change only to CI, documentation or examples' {
        Add-Change $script:Repo @('ci/publish-crates.sh',
            '.github/workflows/publish.yml', 'docs/README.md',
            'examples/Cargo.toml', 'README.md', 'AGENTS.md', 'Cargo.toml')
        (Get-Plan $script:Repo).Count | Should -Be 0
    }

    It 'gives each crate its own last tag plus one patch, and never moves major or minor' {
        git -C $script:Repo tag 'fodid/4.5.30'
        git -C $script:Repo tag 'fiftyone-pipeline-core/4.5.21'
        Add-Change $script:Repo @('pipeline-core/src/lib.rs', 'fodid/src/lib.rs')
        $plan = Get-Plan $script:Repo
        ($plan | Where-Object Name -eq 'fodid').Version | Should -Be '4.5.31'
        ($plan | Where-Object Name -eq 'fiftyone-pipeline-core').Version | Should -Be '4.5.22'
        foreach ($release in $plan) {
            $next = [version]$release.Version
            $release.Previous.Major | Should -Be $next.Major
            $release.Previous.Minor | Should -Be $next.Minor
            $next.Build | Should -Be ($release.Previous.Build + 1)
        }
        git -C $script:Repo tag -d 'fodid/4.5.30' 'fiftyone-pipeline-core/4.5.21'
    }

    It 'orders the plan as ci/crates.txt publishes, so a dev-dependency is published first' {
        Add-Change $script:Repo @('pipeline-core/src/lib.rs')
        $names = [string[]](Get-Plan $script:Repo).Name
        [array]::IndexOf($names, 'fiftyone-ip-intelligence') |
            Should -BeLessThan ([array]::IndexOf($names, 'fiftyone-device-detection'))
        $positions = @($names | ForEach-Object { $script:Order.IndexOf($_) })
        $positions | Should -Be @($positions | Sort-Object)
    }

    It 'keeps the version each crate last released when it is not planned' {
        git -C $script:Repo tag 'fodid/4.5.30'
        Add-Change $script:Repo @('derived-properties/src/lib.rs')
        $plan = Get-Plan $script:Repo
        $versions = Get-CrateVersions $script:Repo $plan
        $versions.Count | Should -Be $script:Names.Count
        $versions['fiftyone-derived-properties'] | Should -Be '4.5.19'
        $versions['fodid'] | Should -Be '4.5.30'
        $versions['fiftyone-pipeline-core'] | Should -Be '4.5.18'
        git -C $script:Repo tag -d 'fodid/4.5.30'
    }

    It 'stops with what to do when a crate has no tag of its own' {
        git -C $script:Repo tag -d 'fodid/4.5.18'
        try {
            { Get-Plan $script:Repo } | Should -Throw '*fodid*ci/start-crate-tags.ps1*'
        } finally {
            git -C $script:Repo tag 'fodid/4.5.18' $script:Start
        }
    }

    It 'writes the plan and the count for the publish steps' {
        Add-Change $script:Repo @('derived-properties/src/lib.rs')
        $folder = New-TempFolder
        $out = Join-Path $folder 'plan.json'
        $env:GITHUB_OUTPUT = Join-Path $folder 'output'
        try {
            $null = & (Join-Path $script:Root 'ci/get-release-plan.ps1') `
                -RepoPath $script:Repo -OutFile $out
        } finally {
            $env:GITHUB_OUTPUT = $null
        }
        Get-Content (Join-Path $folder 'output') | Should -Be 'count=1'
        $plan = Read-ReleasePlan $out
        $plan.Releases.Name | Should -Be @('fiftyone-derived-properties')
        $plan.Releases[0].Previous | Should -Be '4.5.18'
        $plan.Versions.Count | Should -Be $script:Names.Count
        $plan.Versions['fiftyone-derived-properties'] | Should -Be '4.5.19'
    }
}

Describe 'ci/check-version-bump.ps1' {
    BeforeAll {
        $script:Repo = New-PlanRepo
        $script:Folder = New-TempFolder
        function Write-Plan([string]$Name, [string]$Version) {
            $path = Join-Path $script:Folder "$Name-$Version.json"
            @{ Releases = @(@{ Name = $Name; Version = $Version;
                    Previous = '4.5.18'; Reason = 'test' })
               Versions = @{} } | ConvertTo-Json -Depth 4 | Set-Content $path
            return $path
        }
    }

    AfterEach {
        $env:ALLOW_VERSION_BUMP = $null
    }

    It 'passes a crate that moves its patch number' {
        $plan = Write-Plan 'fodid' '4.5.19'
        { $null = & (Join-Path $script:Root 'ci/check-version-bump.ps1') `
            -RepoName $script:Repo -PlanFile $plan } | Should -Not -Throw
    }

    It 'refuses a crate that moves its minor number against its own tags' {
        $plan = Write-Plan 'fodid' '4.6.0'
        { $null = & (Join-Path $script:Root 'ci/check-version-bump.ps1') `
            -RepoName $script:Repo -PlanFile $plan } | Should -Throw '*fodid 4.6.0*'
    }

    It 'compares each crate with its own tags, not the highest of any crate' {
        git -C $script:Repo tag 'fiftyone-caching/4.6.3'
        try {
            $plan = Write-Plan 'fodid' '4.6.0'
            { $null = & (Join-Path $script:Root 'ci/check-version-bump.ps1') `
                -RepoName $script:Repo -PlanFile $plan } | Should -Throw '*fodid 4.6.0*'
        } finally {
            git -C $script:Repo tag -d 'fiftyone-caching/4.6.3'
        }
    }

    It 'allows the move when a person set ALLOW_VERSION_BUMP' {
        $env:ALLOW_VERSION_BUMP = 'true'
        $plan = Write-Plan 'fodid' '4.6.0'
        { $null = & (Join-Path $script:Root 'ci/check-version-bump.ps1') `
            -RepoName $script:Repo -PlanFile $plan } | Should -Not -Throw
    }
}

Describe 'ci/set-crate-versions.ps1' {
    BeforeAll {
        $script:Script = Join-Path $script:Root 'ci/set-crate-versions.ps1'
        function Write-Versions([hashtable]$Versions) {
            $path = Join-Path (New-TempFolder) 'plan.json'
            @{ Releases = @(); Versions = $Versions } | ConvertTo-Json -Depth 4 |
                Set-Content $path
            return $path
        }
        function Get-AllVersions([string]$Version) {
            $versions = @{}
            foreach ($name in $script:Names) { $versions[$name] = $Version }
            return $versions
        }
    }

    It 'writes each crate its own version and every requirement on it' {
        $copy = Copy-Workspace
        $versions = Get-AllVersions '4.5.18'
        $versions['fiftyone-pipeline-core'] = '4.5.19'
        $versions['fodid'] = '4.5.25'
        $null = & $script:Script -PlanFile (Write-Versions $versions) -Root $copy

        Get-OwnVersion $copy 'pipeline-core' | Should -Be '4.5.19'
        Get-OwnVersion $copy 'fodid' | Should -Be '4.5.25'
        Get-OwnVersion $copy 'caching' | Should -Be '4.5.18'
        $onCore = @(Get-Requirements $copy 'fiftyone-pipeline-core')
        $onCore.Count | Should -BeGreaterThan 10
        $onCore | Sort-Object -Unique | Should -Be '4.5.19'
        Get-Requirements $copy 'fodid' | Sort-Object -Unique | Should -Be '4.5.25'
        Get-Requirements $copy 'fiftyone-caching' | Sort-Object -Unique | Should -Be '4.5.18'
    }

    It 'leaves a dependency on the crate itself with no version alone' {
        $copy = Copy-Workspace
        $null = & $script:Script -PlanFile (Write-Versions (Get-AllVersions '4.5.19')) -Root $copy
        Get-Content -Raw (Join-Path $copy 'fodid/Cargo.toml') |
            Should -Match ([regex]::Escape('fodid = { path = ".", features = ["creator"] }'))
    }

    It 'still writes one version into every crate' {
        $copy = Copy-Workspace
        $null = & $script:Script -Version '4.5.40' -Root $copy
        foreach ($package in $script:Packages) {
            Get-OwnVersion $copy $package.Path | Should -Be '4.5.40'
        }
        Get-Requirements $copy 'fodid' | Sort-Object -Unique | Should -Be '4.5.40'
    }

    It 'refuses a plan that gives a published crate no version' {
        $copy = Copy-Workspace
        $versions = Get-AllVersions '4.5.19'
        $versions.Remove('fodid')
        { & $script:Script -PlanFile (Write-Versions $versions) -Root $copy } |
            Should -Throw '*no version to fodid*'
    }

    It 'refuses a version for a crate that is not in the workspace' {
        $copy = Copy-Workspace
        $versions = Get-AllVersions '4.5.19'
        $versions['not-a-crate'] = '4.5.19'
        { & $script:Script -PlanFile (Write-Versions $versions) -Root $copy } |
            Should -Throw '*not-a-crate*'
    }

    It 'refuses something that is not a version' {
        $copy = Copy-Workspace
        $versions = Get-AllVersions '4.5.19'
        $versions['fodid'] = 'latest'
        { & $script:Script -PlanFile (Write-Versions $versions) -Root $copy } |
            Should -Throw '*not a version*'
        { & $script:Script -Version '4.5' -Root $copy } | Should -Throw '*not a version*'
    }
}

Describe 'ci/tag-crate-releases.ps1 -DryRun' {
    BeforeAll {
        $script:Repo = New-PlanRepo
        $script:Start = git -C $script:Repo rev-parse HEAD
    }

    It 'tags each planned crate at HEAD and writes notes for each, pushing nothing' {
        Add-Change $script:Repo @('common-sys/src/lib.rs')
        git -C $script:Repo commit --quiet --amend -m 'Fix the common C library'
        $out = Join-Path (New-TempFolder) 'plan.json'
        $null = & (Join-Path $script:Root 'ci/get-release-plan.ps1') `
            -RepoPath $script:Repo -OutFile $out
        $output = (& (Join-Path $script:Root 'ci/tag-crate-releases.ps1') `
            -RepoName $script:Repo -Repository 'example/none' -PlanFile $out `
            -DryRun) -join "`n"

        $head = git -C $script:Repo rev-parse HEAD
        foreach ($release in (Read-ReleasePlan $out).Releases) {
            git -C $script:Repo rev-parse "refs/tags/$($release.Name)/4.5.19^{commit}" |
                Should -Be $head
        }
        $output | Should -BeLike '*release fiftyone-common-sys/4.5.19*Changes since 4.5.18*Fix the common C library*'
        $output | Should -BeLike '*release fiftyone-device-detection-sys/4.5.19*Requires these crates*fiftyone-common-sys 4.5.19*'
    }
}
