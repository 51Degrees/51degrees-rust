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

    # The folders of every workspace member, read from the root manifest.
    function Get-MemberFolders {
        $workspace = Get-Content -Raw (Join-Path $script:Root 'Cargo.toml')
        $list = [regex]::Match($workspace, 'members\s*=\s*\[(?<list>[^\]]*)\]',
            'Singleline').Groups['list'].Value
        return @([regex]::Matches($list, '"([^"]+)"') |
            ForEach-Object { $_.Groups[1].Value })
    }

    # The files ci/check-packages.ps1 and ci/set-crate-versions.ps1 read,
    # copied so a test can change them. Every member's manifest is copied
    # with an empty src/lib.rs, which is all `cargo metadata --no-deps` needs
    # to read the workspace for ci/check-crate-requirements.ps1.
    function Copy-Workspace {
        $to = New-TempFolder
        foreach ($file in @('Cargo.toml', '.gitmodules', 'ci/crates.txt',
                'ci/packages.json', 'ci/copy-cxx-sources.ps1',
                'ci/copy-owid-source.ps1',
                'ci/record-derived-properties-commit.ps1')) {
            Copy-RepoFile $to $file
        }
        foreach ($folder in (Get-MemberFolders)) {
            Copy-RepoFile $to "$folder/Cargo.toml"
            $null = New-Item -ItemType File -Force (Join-Path $to $folder 'src' 'lib.rs')
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

    # The record crates.io's API gives a crate whose highest version that is
    # not yanked is $Highest, with any versions in $Yanked as well.
    function New-CrateRecord([string]$Highest, [string[]]$Yanked = @()) {
        $versions = @([pscustomobject]@{ num = $Highest; yanked = $false })
        $versions += @($Yanked | ForEach-Object {
            [pscustomobject]@{ num = $_; yanked = $true } })
        return [pscustomobject]@{
            crate = [pscustomobject]@{ max_version = $Highest }
            versions = $versions
        }
    }

    # A stand-in for crates.io in which every published crate of
    # ci/packages.json has $Highest as its highest version, apart from those
    # with a FirstVersion, which crates.io does not have.
    function New-Registry([string]$Highest = '4.5.18') {
        $registry = @{}
        foreach ($package in $script:Packages) {
            if ([string]::IsNullOrEmpty($package.FirstVersion) -eq $true) {
                $registry[$package.Name] = New-CrateRecord $Highest
            }
        }
        return $registry
    }

    # A repository laid out as this one, with a file in every crate folder
    # and in the folders that hold no crate, the two submodules and the
    # scripts that copy them, the real workspace Cargo.toml, ci/packages.json,
    # ci/crates.txt and ci/release-plan-filter.ps1, which common-ci's plan
    # runs from the repository it plans, and every crate tagged at 4.5.18.
    # With -RepositoryTag it has only the repository tag 4.5.18, as before
    # ci/start-crate-tags.ps1 has run.
    function New-PlanRepo([switch]$RepositoryTag) {
        $repo = New-TempFolder
        git init --quiet $repo
        foreach ($file in @('ci/packages.json', 'ci/crates.txt', 'Cargo.toml',
                'ci/release-plan-filter.ps1')) {
            Copy-RepoFile $repo $file
        }
        $files = @($script:Packages | ForEach-Object { "$($_.Path)/src/lib.rs" }) +
            @('ci/publish-crates.sh', '.github/workflows/publish.yml',
              'docs/README.md', 'examples/Cargo.toml', 'README.md',
              'AGENTS.md', 'ci/copy-cxx-sources.ps1', 'ci/copy-owid-source.ps1',
              'ci/record-derived-properties-commit.ps1')
        foreach ($file in $files) {
            $full = Join-Path $repo $file
            $null = New-Item -ItemType Directory -Force (Split-Path $full)
            Set-Content -Path $full -Value 'start'
        }
        Add-Commit $repo 'Start'
        Move-Submodule $repo 'cxx'
        Move-Submodule $repo 'owid-rust'
        if ($RepositoryTag -eq $true) {
            git -C $repo tag '4.5.18'
            return $repo
        }
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

    # Every requirement on a crate across the copied manifests, as written,
    # with its operator.
    function Get-Requirements([string]$RepoRoot, [string]$Name) {
        foreach ($folder in (Get-MemberFolders)) {
            $text = Get-Content -Raw (Join-Path $RepoRoot $folder 'Cargo.toml')
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
        $result.Output | Should -BeLike '*matches the 26 crates*'
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

    It 'refuses a -sys crate that does not list the cxx submodule and the script that copies it' {
        $copy = Copy-Workspace
        Edit-Packages $copy {
            param($packages)
            ($packages | Where-Object Name -eq 'fiftyone-ip-intelligence-sys').ExtraPaths =
                @('Cargo.toml')
        }
        $result = Invoke-Check $copy
        $result.Failed | Should -BeTrue
        $result.Output | Should -BeLike ('*fiftyone-ip-intelligence-sys has ExtraPaths ' +
            'Cargo.toml, but it is built from Cargo.toml, ci/copy-cxx-sources.ps1, cxx,*')
    }

    It 'refuses fodid without the owid-rust submodule and the script that copies it' {
        $copy = Copy-Workspace
        Edit-Packages $copy {
            param($packages)
            ($packages | Where-Object Name -eq 'fodid').ExtraPaths =
                @('cxx', 'ci/copy-cxx-sources.ps1', 'Cargo.toml')
        }
        (Invoke-Check $copy).Output | Should -BeLike ('*fodid has ExtraPaths*but it is ' +
            'built from Cargo.toml, ci/copy-owid-source.ps1, owid-rust,*')
    }

    It 'refuses the derived property crate without its submodule and the script that records its commit' {
        $copy = Copy-Workspace
        Edit-Packages $copy {
            param($packages)
            ($packages | Where-Object Name -eq 'fiftyone-derived-properties').ExtraPaths =
                @('Cargo.toml')
        }
        $result = Invoke-Check $copy
        $result.Failed | Should -BeTrue
        $result.Output | Should -BeLike ('*fiftyone-derived-properties has ExtraPaths ' +
            'Cargo.toml, but it is built from Cargo.toml, ' +
            'ci/record-derived-properties-commit.ps1, derived-properties/vendor,*')
    }

    It 'refuses a crate that does not list the workspace Cargo.toml' {
        $copy = Copy-Workspace
        Edit-Packages $copy {
            param($packages)
            $packages | Where-Object Name -eq 'fiftyone-pipeline-core' |
                ForEach-Object { $_.PSObject.Properties.Remove('ExtraPaths') }
        }
        $result = Invoke-Check $copy
        $result.Failed | Should -BeTrue
        $result.Output | Should -BeLike ('*fiftyone-pipeline-core has ExtraPaths (none), ' +
            'but it is built from Cargo.toml,*')
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
        # Whether common-ci's release plan runs ci/release-plan-filter.ps1,
        # which a pull request into rc may be tested against before it does.
        $script:FilterHook = $null -ne (Get-Command Select-CountedFiles -ErrorAction Ignore)
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
        $plan[0].Reason | Should -BeLike 'changed since 4.5.18*'
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
        $sys = @($plan | Where-Object Name -like '*-sys')
        $sys.Count | Should -Be 3
        foreach ($reason in $sys.Reason) {
            $reason | Should -BeLike 'changed since 4.5.18*'
        }
    }

    It 'plans fodid and the crates built on it when the owid-rust submodule moves' {
        Move-Submodule $script:Repo 'owid-rust'
        $plan = Get-Plan $script:Repo
        $plan.Name | Should -Be @('fodid', 'fodid-client', 'fiftyone-fodid-core',
            'fiftyone-fodid-cloud')
    }

    It 'plans the crates a copy script feeds when the script changes' {
        Add-Change $script:Repo @('ci/copy-cxx-sources.ps1')
        (Get-Plan $script:Repo).Name | Should -Be @(
            'fiftyone-common-sys', 'fiftyone-device-detection-sys',
            'fiftyone-ip-intelligence-sys', 'fiftyone-native',
            'fiftyone-device-detection-onpremise',
            'fiftyone-ip-intelligence-onpremise', 'fiftyone-ip-intelligence',
            'fiftyone-device-detection')

        git -C $script:Repo reset --quiet --hard $script:Start
        Add-Change $script:Repo @('ci/copy-owid-source.ps1')
        (Get-Plan $script:Repo).Name | Should -Be @('fodid', 'fodid-client',
            'fiftyone-fodid-core', 'fiftyone-fodid-cloud')
    }

    It 'plans the derived property crate when its scripts move or the script that records their commit changes' {
        Move-Submodule $script:Repo 'derived-properties/vendor'
        (Get-Plan $script:Repo).Name | Should -Be @('fiftyone-derived-properties')

        git -C $script:Repo reset --quiet --hard $script:Start
        Add-Change $script:Repo @('ci/record-derived-properties-commit.ps1')
        (Get-Plan $script:Repo).Name | Should -Be @('fiftyone-derived-properties')
    }

    It 'stops with what to do when a repository tag is above the crate tags' {
        Add-Commit $script:Repo 'Released under a repository tag'
        git -C $script:Repo tag '4.5.19'
        try {
            { Get-Plan $script:Repo } | Should -Throw ('*repository tag 4.5.19 is ' +
                'above*of 26 crate(s)*ci/start-crate-tags.ps1 -Push*')
        } finally {
            git -C $script:Repo tag -d '4.5.19'
        }
    }

    It 'plans as usual when the crate tags are at the last repository tag' {
        git -C $script:Repo tag '4.5.18' $script:Start
        try {
            Add-Change $script:Repo @('derived-properties/src/lib.rs')
            (Get-Plan $script:Repo).Name | Should -Be @('fiftyone-derived-properties')
        } finally {
            git -C $script:Repo tag -d '4.5.18'
        }
    }

    It 'plans every crate when the workspace Cargo.toml changes what the crates inherit' {
        $path = Join-Path $script:Repo 'Cargo.toml'
        $text = Get-Content -Raw $path
        $text | Should -Match '(?m)^rust-version = "'
        Set-Content -Path $path -NoNewline -Value ($text -replace
            '(?m)^rust-version = "[^"]*"', 'rust-version = "1.999"')
        Add-Commit $script:Repo
        $plan = Get-Plan $script:Repo
        $plan.Count | Should -Be $script:Names.Count
        $plan[0].Reason | Should -BeLike 'changed since 4.5.18*'
    }

    It 'plans nothing for a change only to CI, documentation or examples' {
        Add-Change $script:Repo @('ci/publish-crates.sh',
            '.github/workflows/publish.yml', 'docs/README.md',
            'examples/Cargo.toml', 'README.md', 'AGENTS.md')
        (Get-Plan $script:Repo).Count | Should -Be 0
    }

    It 'plans nothing for a change only to the workspace members' {
        if ($script:FilterHook -eq $false) {
            Set-ItResult -Skipped -Because ("common-ci's release plan does not " +
                "run ci/release-plan-filter.ps1 yet")
            return
        }
        $path = Join-Path $script:Repo 'Cargo.toml'
        $text = Get-Content -Raw $path
        $text | Should -Match '(?m)^members = \['
        Set-Content -Path $path -NoNewline -Value ($text -replace
            '(?m)^members = \[', "members = [`n    `"new-crate`",")
        Add-Commit $script:Repo
        git -C $script:Repo diff --name-only HEAD~1 HEAD | Should -Be 'Cargo.toml'
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

    It 'plans a crate with no tag at its FirstVersion, with every crate depending on it' {
        $first = $script:Packages | Where-Object { $_.FirstVersion }
        $first.Name | Should -Be 'fiftyone-fodid-core'
        git -C $script:Repo tag -d 'fiftyone-fodid-core/4.5.18'
        try {
            Add-Commit $script:Repo
            $plan = Get-Plan $script:Repo
            $plan.Name | Should -Be @('fiftyone-fodid-core', 'fiftyone-fodid-cloud')
            $plan[0].Version | Should -Be $first.FirstVersion
            $plan[0].Reason | Should -Be 'first release'
            $plan[1].Version | Should -Be '4.5.19'
        } finally {
            git -C $script:Repo tag 'fiftyone-fodid-core/4.5.18' $script:Start
        }
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
        $script:Start = git -C $script:Repo rev-parse HEAD
        $script:Folder = New-TempFolder
        $script:Script = Join-Path $script:Root 'ci/check-version-bump.ps1'
        function Write-Plan([string]$Name, [string]$Version) {
            $path = Join-Path $script:Folder "$Name-$Version.json"
            @{ Releases = @(@{ Name = $Name; Version = $Version;
                    Previous = '4.5.18'; Reason = 'test' })
               Versions = @{} } | ConvertTo-Json -Depth 4 | Set-Content $path
            return $path
        }
        # Runs the check against a stand-in for crates.io, keeping what it
        # printed before it failed.
        function Invoke-BumpCheck([string]$Plan, [hashtable]$Registry = (New-Registry)) {
            $lines = [Collections.Generic.List[string]]::new()
            $failure = $null
            try {
                & $script:Script -RepoName $script:Repo -PlanFile $Plan `
                    -Registry $Registry | ForEach-Object { $lines.Add("$_") }
            } catch {
                $failure = "$_"
            }
            return [pscustomobject]@{
                Failure = $failure
                Output = $lines -join "`n"
            }
        }
    }

    BeforeEach {
        git -C $script:Repo reset --quiet --hard $script:Start
    }

    AfterEach {
        $env:ALLOW_VERSION_BUMP = $null
    }

    It 'passes a crate that moves its patch number' {
        $result = Invoke-BumpCheck (Write-Plan 'fodid' '4.5.19')
        $result.Failure | Should -BeNullOrEmpty
        $result.Output | Should -BeLike '*fodid 4.5.19 against crates.io keeps 4.5, the major and minor of 4.5.18*'
    }

    It 'refuses a crate that moves its minor number against its own tags' {
        $result = Invoke-BumpCheck (Write-Plan 'fodid' '4.6.0')
        $result.Failure | Should -BeLike '*fodid 4.6.0 against its tags*'
    }

    It 'compares each crate with its own tags, not the highest of any crate' {
        git -C $script:Repo tag 'fiftyone-caching/4.6.3'
        try {
            $result = Invoke-BumpCheck (Write-Plan 'fodid' '4.6.0')
            $result.Failure | Should -BeLike '*fodid 4.6.0 against its tags*'
        } finally {
            git -C $script:Repo tag -d 'fiftyone-caching/4.6.3'
        }
    }

    It 'refuses a FirstVersion that moves the minor number the crates on crates.io share' {
        # With no tag of its own there is nothing in the tags to compare with,
        # so only crates.io can refuse it.
        git -C $script:Repo tag -d 'fiftyone-fodid-core/4.5.18'
        try {
            $result = Invoke-BumpCheck (Write-Plan 'fiftyone-fodid-core' '4.6.0')
            $result.Output | Should -BeLike ('*fiftyone-fodid-core 4.6.0 against its tags:' +
                "`nIt has no tag of its own, so it is checked against crates.io alone.*")
            $result.Output | Should -BeLike '*fiftyone-fodid-core 4.6.0 moves from 4.5, the major and minor that the crates on crates.io share*'
            $result.Failure | Should -BeLike '*fiftyone-fodid-core 4.6.0 against crates.io*'
            $result.Failure | Should -Not -BeLike '*against its tags*'

            $result = Invoke-BumpCheck (Write-Plan 'fiftyone-fodid-core' '4.5.19')
            $result.Failure | Should -BeNullOrEmpty
        } finally {
            git -C $script:Repo tag 'fiftyone-fodid-core/4.5.18' $script:Start
        }
    }

    It 'passes a FirstVersion on a branch whose history holds none of the tags, as rc does' {
        # rc does not hold the merge commits on main that the tags point at.
        $repo = New-PlanRepo
        git -C $repo tag -d 'fiftyone-fodid-core/4.5.18'
        git -C $repo checkout --quiet -b rc HEAD~1
        Add-Change $repo @('fodid-core/src/lib.rs')
        git -C $repo tag --merged HEAD | Should -BeNullOrEmpty
        $plan = Write-Plan 'fiftyone-fodid-core' '4.5.19'
        $lines = [Collections.Generic.List[string]]::new()
        { & $script:Script -RepoName $repo -PlanFile $plan -Registry (New-Registry) |
            ForEach-Object { $lines.Add("$_") } } | Should -Not -Throw
        $lines -join "`n" | Should -BeLike '*keeps 4.5, the major and minor that the crates on crates.io share*'
    }

    It 'refuses a plan that follows a tag pushed by hand, against crates.io' {
        git -C $script:Repo tag 'fodid/4.6.0'
        try {
            Add-Change $script:Repo @('fodid/src/lib.rs')
            $out = Join-Path $script:Folder 'hand-tag.json'
            $null = & (Join-Path $script:Root 'ci/get-release-plan.ps1') `
                -RepoPath $script:Repo -OutFile $out
            $plan = Read-ReleasePlan $out
            ($plan.Releases | Where-Object Name -eq 'fodid').Version | Should -Be '4.6.1'

            $registry = New-Registry
            $registry['fodid'] = New-CrateRecord '4.5.18' -Yanked '4.6.0'
            $result = Invoke-BumpCheck $out $registry
            $result.Output | Should -BeLike '*fodid 4.6.1 against its tags:*Publishing 4.6.1, against 4.6.0 already released. The patch place*'
            $result.Output | Should -BeLike '*fodid 4.6.1 moves from 4.5, the major and minor of 4.5.18, its highest version on crates.io*'
            $result.Failure | Should -BeLike '*fodid 4.6.1 against crates.io*'
            $result.Failure | Should -Not -BeLike '*against its tags*'
        } finally {
            git -C $script:Repo tag -d 'fodid/4.6.0'
        }
    }

    It 'stops when the crates on crates.io do not share one major and minor number' {
        $registry = New-Registry
        $registry['fiftyone-caching'] = New-CrateRecord '4.6.3'
        $result = Invoke-BumpCheck (Write-Plan 'fiftyone-fodid-core' '4.5.19') $registry
        $result.Failure | Should -BeLike '*crates.io does not have fiftyone-fodid-core*do not share one (found 4.5, 4.6)*'
    }

    It 'goes on, naming the numbers found, when the crates on crates.io do not share one and a person set ALLOW_VERSION_BUMP' {
        $env:ALLOW_VERSION_BUMP = 'true'
        $registry = New-Registry
        $registry['fiftyone-caching'] = New-CrateRecord '4.6.3'
        $result = Invoke-BumpCheck (Write-Plan 'fiftyone-fodid-core' '4.5.19') $registry
        $result.Failure | Should -BeNullOrEmpty
        $result.Output | Should -BeLike ('*crates.io does not have fiftyone-fodid-core*' +
            'do not share one (found 4.5, 4.6). It is not compared, allowed by ALLOW_VERSION_BUMP.*')
        $result.Output | Should -BeLike ('*fiftyone-fodid-core 4.5.19 against crates.io has ' +
            'nothing to be compared with, allowed by ALLOW_VERSION_BUMP.*')
        $result.Output | Should -BeLike '*Every crate in the plan keeps its major and minor number, or was allowed to move them.*'
    }

    It 'allows the move when a person set ALLOW_VERSION_BUMP' {
        $env:ALLOW_VERSION_BUMP = 'true'
        $result = Invoke-BumpCheck (Write-Plan 'fodid' '4.6.0')
        $result.Failure | Should -BeNullOrEmpty
        $result = Invoke-BumpCheck (Write-Plan 'fiftyone-fodid-core' '4.6.0')
        $result.Failure | Should -BeNullOrEmpty
        $result.Output | Should -BeLike '*allowed by ALLOW_VERSION_BUMP*'
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
        $onCore | ForEach-Object { $_.TrimStart('=') } | Sort-Object -Unique |
            Should -Be '4.5.19'
        Get-Requirements $copy 'fodid' | ForEach-Object { $_.TrimStart('=') } |
            Sort-Object -Unique | Should -Be '4.5.25'
        Get-Requirements $copy 'fiftyone-caching' | ForEach-Object { $_.TrimStart('=') } |
            Sort-Object -Unique | Should -Be '4.5.18'
    }

    It 'keeps an exact requirement exact, on a planned crate and on a crate keeping its last version' {
        $copy = Copy-Workspace
        $versions = Get-AllVersions '4.5.18'
        $versions['fiftyone-common-sys'] = '4.5.19'
        $versions['fiftyone-native'] = '4.5.19'
        $null = & $script:Script -PlanFile (Write-Versions $versions) -Root $copy

        $native = Get-Content -Raw (Join-Path $copy 'native' 'Cargo.toml')
        $native | Should -Match 'fiftyone-common-sys = \{ version = "=4\.5\.19"'
        $native | Should -Match 'fiftyone-pipeline-core = \{ version = "=4\.5\.18"'
        $native | Should -Match 'fiftyone-ip-intelligence-sys = \{ version = "=4\.5\.18"'
        $onCommon = @(Get-Requirements $copy 'fiftyone-common-sys')
        $onCommon.Count | Should -BeGreaterThan 0
        $onCommon | Sort-Object -Unique | Should -Be '=4.5.19'
    }

    It 'keeps a dev-dependency plain, and any other operator as it was' {
        $copy = Copy-Workspace
        $client = Join-Path $copy 'fodid-client' 'Cargo.toml'
        $text = Get-Content -Raw $client
        $own = Get-OwnVersion $copy 'fodid'
        $dev = "fodid = { path = `"../fodid`", version = `"$own`", features = [`"creator`"] }"
        $text.Contains($dev) | Should -BeTrue
        $caret = $dev.Replace("`"$own`"", "`"^$own`"")
        Set-Content -Path $client -Value $text.Replace($dev, $caret) -NoNewline
        $versions = Get-AllVersions '4.5.18'
        $versions['fodid'] = '4.5.25'
        $null = & $script:Script -PlanFile (Write-Versions $versions) -Root $copy

        Get-Content -Raw $client | Should -Match 'fodid = \{ path = "\.\./fodid", version = "=4\.5\.25" \}'
        Get-Content -Raw $client | Should -Match 'fodid = \{ path = "\.\./fodid", version = "\^4\.5\.25", features'
        $core = Get-Content -Raw (Join-Path $copy 'fodid-core' 'Cargo.toml')
        $core | Should -Match 'fodid = \{ version = "=4\.5\.25", path'
        $core | Should -Match 'fodid = \{ version = "4\.5\.25", path = "\.\./fodid", features'
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
        $onFodid = @(Get-Requirements $copy 'fodid' | Sort-Object -Unique)
        $onFodid | Should -HaveCount 2
        $onFodid | Should -Contain '=4.5.40'
        $onFodid | Should -Contain '4.5.40'
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

Describe 'ci/start-crate-tags.ps1' {
    BeforeAll {
        $script:Script = Join-Path $script:Root 'ci/start-crate-tags.ps1'
        function Get-CrateTags([string]$Repo) {
            return @(git -C $Repo tag --list | Where-Object { $_ -like '*/*' })
        }
    }

    It 'tags every crate at the release, passing over one crates.io does not have that has a FirstVersion' {
        $repo = New-PlanRepo -RepositoryTag
        $output = (& $script:Script -RepoPath $repo -Registry (New-Registry)) -join "`n"

        $tags = Get-CrateTags $repo
        $tags.Count | Should -Be ($script:Names.Count - 1)
        $tags | Should -Not -Contain 'fiftyone-fodid-core/4.5.18'
        $tags | Should -Contain 'fodid/4.5.18'
        $output | Should -BeLike '*fiftyone-fodid-core is not on crates.io*FirstVersion 4.5.19*'
        $output | Should -BeLike '*git push --atomic origin refs/tags/*'
    }

    It 'tags nothing when crates.io does not have a crate with no FirstVersion' {
        $repo = New-PlanRepo -RepositoryTag
        $registry = New-Registry
        $registry.Remove('fiftyone-caching')
        { $null = & $script:Script -RepoPath $repo -Registry $registry } |
            Should -Throw '*Nothing is tagged*'
        Get-CrateTags $repo | Should -BeNullOrEmpty
    }

    It 'tags nothing when a crate is above the release on crates.io' {
        $repo = New-PlanRepo -RepositoryTag
        $registry = New-Registry
        $registry['fodid'] = New-CrateRecord '4.5.19'
        $registry['fodid'].versions += [pscustomobject]@{ num = '4.5.18'; yanked = $false }
        $output = [Collections.Generic.List[string]]::new()
        { & $script:Script -RepoPath $repo -Registry $registry |
            ForEach-Object { $output.Add("$_") } } | Should -Throw '*Nothing is tagged*'
        $output -join "`n" | Should -BeLike '*fodid is at 4.5.19 on crates.io, above 4.5.18*'
        Get-CrateTags $repo | Should -BeNullOrEmpty
    }

    It 'tags every crate again at a later release when its own tags are all below it' {
        $repo = New-PlanRepo
        Add-Commit $repo 'Released under a repository tag'
        git -C $repo tag '4.5.19'
        (Get-CratesBehindRelease $repo).Crates.Count | Should -Be $script:Names.Count
        $registry = New-Registry '4.5.19'
        $registry['fiftyone-fodid-core'] = New-CrateRecord '4.5.19'
        $null = & $script:Script -RepoPath $repo -Registry $registry

        $tags = Get-CrateTags $repo
        $tags.Count | Should -Be ($script:Names.Count * 2)
        $tags | Should -Contain 'fodid/4.5.18'
        $tags | Should -Contain 'fiftyone-fodid-core/4.5.19'
        git -C $repo rev-parse 'refs/tags/fodid/4.5.19^{commit}' |
            Should -Be (git -C $repo rev-parse 'refs/tags/4.5.19^{commit}')
        (Get-CratesBehindRelease $repo).Crates | Should -BeNullOrEmpty
    }

    It 'tags nothing when a crate has a tag of its own at or above the release' {
        $repo = New-PlanRepo
        Add-Commit $repo 'Released under a repository tag'
        git -C $repo tag '4.5.19'
        git -C $repo tag 'fodid/4.5.20'
        $registry = New-Registry '4.5.19'
        $registry['fiftyone-fodid-core'] = New-CrateRecord '4.5.19'
        $output = [Collections.Generic.List[string]]::new()
        { & $script:Script -RepoPath $repo -Registry $registry |
            ForEach-Object { $output.Add("$_") } } | Should -Throw '*Nothing is tagged*'
        $output -join "`n" | Should -BeLike '*fodid already has tags of its own (fodid/4.5.20)*'
        Get-CrateTags $repo | Should -Not -Contain 'fiftyone-caching/4.5.19'
    }
}

Describe 'ci/publish-planned-crates.ps1' {
    BeforeAll {
        $script:Repo = New-PlanRepo
        $script:Script = Join-Path $script:Root 'ci/publish-planned-crates.ps1'
        $script:Head = git -C $script:Repo rev-parse HEAD

        # A plan releasing the named crates at 4.5.19, with every published
        # crate at 4.5.19 after it, as the first release has.
        function Write-PublishPlan([string[]]$Names) {
            $path = Join-Path (New-TempFolder) 'plan.json'
            $versions = [ordered]@{}
            foreach ($name in $script:Order) { $versions[$name] = '4.5.19' }
            @{
                Releases = @($Names | ForEach-Object {
                    @{ Name = $_; Version = '4.5.19'; Previous = ''; Reason = 'test' } })
                Versions = $versions
            } | ConvertTo-Json -Depth 4 | Set-Content $path
            return $path
        }

        # Runs the script, keeping what it printed before it failed.
        function Invoke-Publish([string]$Plan, [hashtable]$Registry, [switch]$DryRun) {
            $lines = [Collections.Generic.List[string]]::new()
            $failure = $null
            try {
                & $script:Script -PlanFile $Plan -RepoPath $script:Repo `
                    -Registry $Registry -DryRun:$DryRun |
                    ForEach-Object { $lines.Add("$_") }
            } catch {
                $failure = "$_"
            }
            return [pscustomobject]@{ Failure = $failure; Output = $lines -join "`n" }
        }
    }

    It 'publishes the crates before one crates.io does not have, then gives the commands for it' {
        $at = $script:Order.IndexOf('fiftyone-fodid-core')
        $before = @($script:Order | Select-Object -First $at)
        $package = $script:Packages | Where-Object Name -eq 'fiftyone-fodid-core'
        foreach ($dependency in $package.DependsOn) {
            $before | Should -Contain $dependency
        }

        $result = Invoke-Publish (Write-PublishPlan $script:Order) (New-Registry) -DryRun
        $result.Failure | Should -BeNullOrEmpty
        $result.Output | Should -BeLike ("*would publish $($before.Count) crate(s) " +
            "with ci/publish-crates.sh: $($before -join ', ')`n*")
        $result.Output | Should -BeLike '*A publish of this commit stops before fiftyone-fodid-core*'
        $result.Output | Should -BeLike "*git checkout --detach $script:Head*"
        $result.Output | Should -BeLike '*"fiftyone-fodid-core":"4.5.19"*'
        $result.Output | Should -BeLike '*./ci/set-crate-versions.ps1 -PlanFile $plan*cargo publish -p fiftyone-fodid-core --allow-dirty*bash ci/setup-trusted-publishing.sh*'
        $result.Output | Should -Not -BeLike '*submodule update*'
        $result.Output | Should -Not -BeLike '*copy-*.ps1*'
    }

    It 'stops, failing the run, before a crate crates.io does not have' {
        $plan = Write-PublishPlan @('fiftyone-fodid-cloud', 'fiftyone-fodid-core')
        $result = Invoke-Publish $plan (New-Registry)
        $result.Failure | Should -BeLike '*Stopped before fiftyone-fodid-core*'
        $result.Output | Should -BeLike '::error title=First version published by hand::fiftyone-fodid-core is not on crates.io*'
        $result.Output | Should -BeLike '*No crate of the plan comes before it*'
        $result.Output | Should -BeLike '*cargo publish -p fiftyone-fodid-core --allow-dirty*'
    }

    It 'checks out and copies the submodule a crate crates.io does not have takes files from' {
        $registry = New-Registry
        $registry.Remove('fodid')
        $result = Invoke-Publish (Write-PublishPlan @('fodid')) $registry -DryRun
        $result.Output | Should -BeLike '*git submodule update --init owid-rust*./ci/set-crate-versions.ps1*./ci/copy-owid-source.ps1*cargo publish -p fodid --allow-dirty*'
    }

    It 'publishes every crate of the plan when crates.io has them all' {
        $registry = New-Registry
        $registry['fiftyone-fodid-core'] = New-CrateRecord '4.5.19'
        $result = Invoke-Publish (Write-PublishPlan $script:Order) $registry -DryRun
        $result.Failure | Should -BeNullOrEmpty
        $result.Output | Should -BeLike "Dry run, would publish $($script:Order.Count) crate(s)*"
        $result.Output | Should -Not -BeLike '*stops before*'
    }
}

Describe 'The block ci/publish-planned-crates.ps1 prints for a publish by hand' {
    BeforeAll {
        $script:Repo = New-PlanRepo
        $script:Head = git -C $script:Repo rev-parse HEAD
        $script:RealCargo = (Get-Command cargo -CommandType Application |
            Select-Object -First 1).Source

        # The block printed for a first release of every crate, with crates.io
        # lacking fiftyone-fodid-core, as one script without its indent.
        $versions = [ordered]@{}
        foreach ($name in $script:Order) { $versions[$name] = '4.5.19' }
        $plan = Join-Path (New-TempFolder) 'plan.json'
        @{
            Releases = @($script:Order | ForEach-Object {
                @{ Name = $_; Version = '4.5.19'; Previous = ''; Reason = 'test' } })
            Versions = $versions
        } | ConvertTo-Json -Depth 4 | Set-Content $plan
        $lines = @(& (Join-Path $script:Root 'ci/publish-planned-crates.ps1') `
            -PlanFile $plan -RepoPath $script:Repo -Registry (New-Registry) -DryRun)
        $start = [array]::IndexOf($lines, '    & {')
        $end = [array]::IndexOf($lines, '    }', $start)
        $script:Block = @($lines[$start..$end] | ForEach-Object { $_.Substring(4) }) -join "`n"

        # A copy of the workspace holding the scripts the block runs.
        function New-HandCopy {
            $copy = Copy-Workspace
            foreach ($file in @('ci/set-crate-versions.ps1',
                    'ci/check-crate-requirements.ps1')) {
                Copy-RepoFile $copy $file
            }
            return $copy
        }

        # Runs the block in the given folder, with git, bash and cargo publish
        # recorded rather than run, and any other cargo command run, so the
        # requirement check and the block's own version check read the real
        # manifests. Git Bash is not found, so bash is the recorded one on
        # every system.
        function Invoke-HandBlock([string]$Folder, [switch]$NoToken) {
            # The stubs find these by name from whichever script calls them.
            $handCalls = [Collections.Generic.List[string]]::new()
            $handCargo = $script:RealCargo
            # The line as written, as PowerShell takes '--' out of the
            # arguments it gives a function, though git receives it.
            function git { $handCalls.Add($MyInvocation.Line.Trim()) }
            function bash { $handCalls.Add("bash $args with CRATES_IO_TOKEN $env:CRATES_IO_TOKEN") }
            function cargo {
                if ($args[0] -eq 'publish') {
                    $handCalls.Add("cargo $args")
                    return
                }
                & $handCargo @args
            }
            $saved = @{}
            foreach ($name in @('CARGO_REGISTRY_TOKEN', 'CRATES_IO_TOKEN', 'ProgramFiles')) {
                $saved[$name] = [Environment]::GetEnvironmentVariable($name)
            }
            $token = if ($NoToken) { $null } else { 'test-token' }
            [Environment]::SetEnvironmentVariable('CARGO_REGISTRY_TOKEN', $token)
            [Environment]::SetEnvironmentVariable('CRATES_IO_TOKEN', $null)
            [Environment]::SetEnvironmentVariable('ProgramFiles', (New-TempFolder))
            $failure = $null
            Push-Location $Folder
            try {
                & ([scriptblock]::Create($script:Block))
            } catch {
                $failure = "$_"
            } finally {
                Pop-Location
                foreach ($name in $saved.Keys) {
                    [Environment]::SetEnvironmentVariable($name, $saved[$name])
                }
            }
            return [pscustomobject]@{ Failure = $failure; Calls = @($handCalls) }
        }
    }

    It 'is one block that stops at the first failure' {
        $script:Block | Should -BeLike "& {`n    `$ErrorActionPreference = 'Stop'`n    `$PSNativeCommandUseErrorActionPreference = `$true`n*"
        $script:Block | Should -BeLike '*}'
        $tokens = $null
        $errors = $null
        $ast = [Management.Automation.Language.Parser]::ParseInput(
            $script:Block, [ref]$tokens, [ref]$errors)
        $errors | Should -BeNullOrEmpty
        # One statement, so nothing runs until the closing brace is entered.
        $ast.EndBlock.Statements.Count | Should -Be 1
    }

    It 'publishes the crate once the manifests give it the planned version' {
        $copy = New-HandCopy
        $result = Invoke-HandBlock $copy
        $result.Failure | Should -BeNullOrEmpty
        $result.Calls | Should -Be @(
            'git fetch origin'
            "git checkout --detach $script:Head"
            'cargo publish -p fiftyone-fodid-core --allow-dirty'
            'bash ci/setup-trusted-publishing.sh with CRATES_IO_TOKEN test-token'
            'git checkout -- .')
        $core = Get-Content -Raw (Join-Path $copy 'fodid-core' 'Cargo.toml')
        $core | Should -Match '(?m)^version = "4\.5\.19"'
        $core | Should -Match 'fodid = \{ version = "=4\.5\.19", path'
    }

    It 'stops before cargo publish when the versions were not written' {
        $copy = New-HandCopy
        # A script that writes nothing, as when it cannot run at all.
        Set-Content -Path (Join-Path $copy 'ci' 'set-crate-versions.ps1') -Value 'param($PlanFile)'
        $own = Get-OwnVersion $copy 'fodid-core'
        $own | Should -Not -Be '4.5.19'
        $result = Invoke-HandBlock $copy
        $result.Failure | Should -Be ("The manifests give fiftyone-fodid-core $own, " +
            "not 4.5.19, so it is not published.")
        $result.Calls | Should -Not -Contain 'cargo publish -p fiftyone-fodid-core --allow-dirty'
        @($result.Calls | Where-Object { $_ -like 'bash *' }) | Should -BeNullOrEmpty
    }

    It 'stops before cargo publish when writing the versions fails' {
        $copy = New-HandCopy
        Remove-Item (Join-Path $copy 'fodid-core' 'Cargo.toml')
        $result = Invoke-HandBlock $copy
        $result.Failure | Should -BeLike '*fodid-core*missing*'
        $result.Calls | Should -Be @('git fetch origin', "git checkout --detach $script:Head")
    }

    It 'stops before changing anything when no token is set' {
        $result = Invoke-HandBlock (New-HandCopy) -NoToken
        $result.Failure | Should -BeLike 'Set $env:CARGO_REGISTRY_TOKEN*'
        $result.Calls | Should -BeNullOrEmpty
    }
}

Describe 'ci/tag-crate-releases.ps1 -DryRun' {
    BeforeAll {
        $script:Repo = New-PlanRepo
        $script:Start = git -C $script:Repo rev-parse HEAD
    }

    It 'tags each planned crate at HEAD and writes notes for each, pushing nothing' {
        Add-Change $script:Repo @('common-sys/src/lib.rs')
        git -C $script:Repo -c user.name=test -c user.email=test@example.com `
            commit --quiet --amend -m 'Fix the common C library'
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

Describe 'ci/publish-crates.sh' {
    It 'fails rather than skipping when there is no crates.io token' {
        # Git Bash on Windows, where the bash on the path can start WSL.
        $bash = 'bash'
        if ($IsWindows) {
            $gitBash = Join-Path $env:ProgramFiles 'Git' 'bin' 'bash.exe'
            if (Test-Path $gitBash) {
                $bash = $gitBash
            }
        }
        $saved = $env:CARGO_REGISTRY_TOKEN
        $env:CARGO_REGISTRY_TOKEN = $null
        try {
            $output = & $bash (Join-Path $script:Root 'ci/publish-crates.sh') `
                'fiftyone-caching' 2>&1
            $code = $LASTEXITCODE
        } finally {
            $env:CARGO_REGISTRY_TOKEN = $saved
            $global:LASTEXITCODE = 0
        }
        $code | Should -Not -Be 0
        ($output -join "`n") | Should -BeLike '*CARGO_REGISTRY_TOKEN is not set*'
    }
}

Describe 'ci/release-plan-filter.ps1' {
    BeforeAll {
        $script:Filter = Join-Path $script:Root 'ci/release-plan-filter.ps1'
        $script:Crate = [pscustomobject]@{ Name = 'fodid-client'; Path = 'fodid-client' }
        $script:Manifest = @(
            '[package]'
            'name = "fodid-client"'
            'version = "4.5.2"'
            'description = "A client"'
            'homepage = "https://example.com/a"'
            'keywords = ["a", "b"]'
            'categories = ['
            '    "web-programming",'
            ']'
            'license.workspace = true'
            ''
            '[dependencies]'
            'fodid = { path = "../fodid", version = "=4.5.5" }'
        )

        # The files the filter keeps when the crate's manifest changes from
        # $Before to $After, beside a source file that always counts.
        function Get-Kept([string[]]$Before, [string[]]$After) {
            $repo = New-TempFolder
            git init --quiet $repo
            $path = Join-Path $repo 'fodid-client/Cargo.toml'
            $null = New-Item -ItemType Directory -Force (Split-Path $path)
            if ($null -ne $Before) {
                Set-Content -Path $path -Value $Before
            }
            Add-Commit $repo 'Start'
            git -C $repo tag 'fodid-client/4.5.18'
            Set-Content -Path $path -Value $After
            Add-Commit $repo 'Change'
            return @(& $script:Filter -RepoPath $repo -Package $script:Crate `
                -From 'fodid-client/4.5.18' `
                -Files @('fodid-client/Cargo.toml', 'fodid-client/src/lib.rs'))
        }

        function Set-Line([string[]]$Lines, [string]$Match, [string]$New) {
            return @($Lines | ForEach-Object { if ($_ -like $Match) { $New } else { $_ } })
        }
    }

    It 'drops a manifest whose only change is a page it points to' {
        $after = Set-Line $script:Manifest 'homepage*' 'homepage = "https://example.com/b"'
        Get-Kept $script:Manifest $after | Should -Be @('fodid-client/src/lib.rs')
    }

    It 'drops a manifest whose descriptive keys change, over several lines too' {
        $after = Set-Line $script:Manifest 'keywords*' 'keywords = ["a", "c"]'
        $after = Set-Line $after '    "web-programming",' '    "authentication",'
        $after = Set-Line $after 'description*' 'description.workspace = true'
        Get-Kept $script:Manifest $after | Should -Be @('fodid-client/src/lib.rs')
    }

    It 'keeps a manifest whose requirement on another crate changes' {
        $after = Set-Line $script:Manifest 'fodid = *' 'fodid = { path = "../fodid", version = "=4.5.6" }'
        Get-Kept $script:Manifest $after |
            Should -Be @('fodid-client/Cargo.toml', 'fodid-client/src/lib.rs')
    }

    It 'keeps a manifest that gains a feature or a build setting' {
        $after = $script:Manifest + @('', '[features]', 'creator = []')
        Get-Kept $script:Manifest $after |
            Should -Be @('fodid-client/Cargo.toml', 'fodid-client/src/lib.rs')
        $after = Set-Line $script:Manifest 'license.workspace*' 'license = "MIT"'
        Get-Kept $script:Manifest $after |
            Should -Be @('fodid-client/Cargo.toml', 'fodid-client/src/lib.rs')
    }

    It 'keeps a manifest that did not exist at the last tag' {
        Get-Kept $null $script:Manifest |
            Should -Be @('fodid-client/Cargo.toml', 'fodid-client/src/lib.rs')
    }

    It 'keeps a manifest whose only change is in case, as Cargo reads case' {
        $before = @('[package]', 'links = "fiftyone-common-c"') +
            @($script:Manifest | Select-Object -Skip 1) + @(
            ''
            '[features]'
            'default = ["Std"]'
            ''
            '[target.''cfg(target_os = "Linux")''.dependencies]'
            'libc = "0.2"'
        )
        foreach ($change in @(
                @('links = *', 'links = "FiftyOne-Common-C"'),
                @('default = *', 'default = ["std"]'),
                @('`[target.*', '[target.''cfg(target_os = "linux")''.dependencies]'))) {
            $after = Set-Line $before $change[0] $change[1]
            ($after -join "`n") | Should -Not -BeExactly ($before -join "`n")
            Get-Kept $before $after |
                Should -Be @('fodid-client/Cargo.toml', 'fodid-client/src/lib.rs') -Because $change[1]
        }
    }

    It 'finds the end of a value over several lines outside its strings' {
        # The last line of authors holds a '#' inside a string, ahead of the
        # ']' that closes the array.
        $before = @(
            '[package]'
            'name = "fodid-client"'
            'authors = ['
            '    "A <a@example.com>",'
            '    "C# team <b@example.com>" ]'
            'build = "build.rs"'
            ''
            '[dependencies]'
            'fodid = { path = "../fodid", version = "=4.5.5" }'
        )
        $after = Set-Line $before 'build = *' 'build = "other.rs"'
        Get-Kept $before $after |
            Should -Be @('fodid-client/Cargo.toml', 'fodid-client/src/lib.rs')
        $after = Set-Line $before '`[dependencies`]' '[build-dependencies]'
        Get-Kept $before $after |
            Should -Be @('fodid-client/Cargo.toml', 'fodid-client/src/lib.rs')
        $after = Set-Line $before '    "A <*' '    "B <a@example.com>",'
        Get-Kept $before $after | Should -Be @('fodid-client/src/lib.rs')
    }
}

Describe 'ci/release-plan-filter.ps1 with the workspace Cargo.toml' {
    BeforeAll {
        $script:Filter = Join-Path $script:Root 'ci/release-plan-filter.ps1'
        $script:Crate = [pscustomobject]@{ Name = 'fodid-client'; Path = 'fodid-client' }
        $script:Workspace = @(Get-Content (Join-Path $script:Root 'Cargo.toml'))

        # The files the filter keeps when the workspace manifest changes from
        # $Before to $After, beside a source file that always counts.
        function Get-KeptWorkspace([string[]]$Before, [string[]]$After) {
            $repo = New-TempFolder
            git init --quiet $repo
            $path = Join-Path $repo 'Cargo.toml'
            Set-Content -Path $path -Value $Before
            Add-Commit $repo 'Start'
            git -C $repo tag 'fodid-client/4.5.18'
            Set-Content -Path $path -Value $After
            Add-Commit $repo 'Change'
            return @(& $script:Filter -RepoPath $repo -Package $script:Crate `
                -From 'fodid-client/4.5.18' `
                -Files @('Cargo.toml', 'fodid-client/src/lib.rs'))
        }

        # The workspace manifest with the line matching $Match replaced.
        function Edit-Workspace([string]$Match, [string]$New) {
            @($script:Workspace | Where-Object { $_ -match $Match }) | Should -HaveCount 1
            return @($script:Workspace | ForEach-Object {
                if ($_ -match $Match) { $New } else { $_ } })
        }
    }

    It 'keeps it when a key the crates inherit from [workspace.package] changes' {
        $after = Edit-Workspace '^rust-version = ' 'rust-version = "1.999"'
        Get-KeptWorkspace $script:Workspace $after |
            Should -Be @('Cargo.toml', 'fodid-client/src/lib.rs')
        $after = Edit-Workspace '^license = ' 'license = "MIT"'
        Get-KeptWorkspace $script:Workspace $after |
            Should -Be @('Cargo.toml', 'fodid-client/src/lib.rs')
    }

    It 'keeps it when [workspace.dependencies] changes' {
        $after = Edit-Workspace '^serde = ' 'serde = { version = "1", features = ["derive", "rc"] }'
        Get-KeptWorkspace $script:Workspace $after |
            Should -Be @('Cargo.toml', 'fodid-client/src/lib.rs')
    }

    It 'drops it when only a key that describes the crates changes' {
        $after = Edit-Workspace '^repository = ' 'repository = "https://example.com/rust"'
        Get-KeptWorkspace $script:Workspace $after | Should -Be @('fodid-client/src/lib.rs')
    }

    It 'drops it when only its members, profiles or comments change' {
        $after = Edit-Workspace '^\s*"fodid-cloud",$' '    "fodid-cloud", "new-crate",'
        Get-KeptWorkspace $script:Workspace $after | Should -Be @('fodid-client/src/lib.rs')
        $after = Edit-Workspace '^debug = "line-tables-only"' 'debug = true'
        Get-KeptWorkspace $script:Workspace $after | Should -Be @('fodid-client/src/lib.rs')
        $after = Edit-Workspace '^# Shared third-party dependencies' '# Changed comment.'
        Get-KeptWorkspace $script:Workspace $after | Should -Be @('fodid-client/src/lib.rs')
    }
}
