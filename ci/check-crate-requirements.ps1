<#
.SYNOPSIS
Fails unless every requirement one workspace crate has on another is written
as the release writes it, and unless ci/crates.txt can publish the crates in
its order.

.DESCRIPTION
The crates of one release require each other with `=`, so a crate pinned at a
version builds only with the crates of that release beneath it, and with the C
sources they carry. A plain version such as "4.5.2" is a caret requirement,
which lets a fresh build of an older crate take a newer release of the crates
beneath it.

Dev-dependencies are the exception and take a plain version. They never reach
a project's build, and an exact one would force a new release of a crate
whenever a crate its tests use is released.

Five things are checked, reading the manifests through `cargo metadata`, which
compiles nothing.

1. Every normal or build requirement on another workspace crate is `=`
   followed by that crate's version in the workspace.
2. Every dev-dependency on another workspace crate that carries a version
   gives that crate's version as a plain version.
3. A requirement with no version is a dev-dependency, which `cargo publish`
   leaves out of the published crate. Any other kind would stop the publish.
4. A crate in ci/crates.txt requires only crates that ci/crates.txt also
   publishes, so nothing published requires a crate that is not.
5. ci/crates.txt names each crate after every crate it requires, so the
   versions each crate requires are on crates.io before it is published.
   Dev-dependencies with a version count, because they stay in the published
   manifest.

It also fails when it finds no workspace crates, no crates in ci/crates.txt
or no normal requirements to check, so it can never pass by checking nothing.

.PARAMETER Root
The workspace root. Defaults to the repository this script sits in.
#>
param (
    [string]$Root = (Join-Path $PSScriptRoot "..")
)
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

$manifest = Join-Path $Root "Cargo.toml"
$metadata = (cargo metadata --no-deps --offline --format-version 1 `
    --manifest-path $manifest) -join "`n" | ConvertFrom-Json

# With --no-deps the packages are the workspace members alone.
$versions = @{}
foreach ($package in $metadata.packages) {
    $versions[$package.name] = $package.version
}
if ($versions.Count -eq 0) {
    throw "cargo metadata found no workspace crates in $manifest, so there " +
        "is nothing to check."
}

$published = @(Get-Content -Path (Join-Path $Root "ci/crates.txt") |
    ForEach-Object { $_.Trim() } |
    Where-Object { $_ -and -not $_.StartsWith("#") })
if ($published.Count -eq 0) {
    throw "ci/crates.txt names no crates, so there is nothing to check."
}
$order = @{}
for ($i = 0; $i -lt $published.Count; $i++) {
    $order[$published[$i]] = $i
}

$failures = @()
foreach ($crate in $published) {
    if (-not $versions.ContainsKey($crate)) {
        $failures += "ci/crates.txt publishes $crate, which is not a " +
            "member of the workspace"
    }
}

$exactCount = 0
$devCount = 0
foreach ($package in $metadata.packages) {
    $name = $package.name
    foreach ($dependency in $package.dependencies) {
        $target = $dependency.name
        if (-not $versions.ContainsKey($target)) { continue }
        $kind = if ($dependency.kind) { $dependency.kind } else { "normal" }
        $what = "$name requires $target ($kind)"
        $version = $versions[$target]

        if ($dependency.req -eq "*") {
            if ($kind -ne "dev") {
                $failures += "$what by path alone, which cargo publish " +
                    "refuses. Give it version = `"=$version`"."
            }
            continue
        }

        # cargo metadata reports a plain version as the caret requirement
        # Cargo reads it as, so "4.5.2" arrives as "^4.5.2".
        if ($kind -eq "dev") {
            $devCount++
            if ($dependency.req -ne "^$version") {
                $failures += "$what as `"$($dependency.req)`", where a " +
                    "dev-dependency takes the plain version `"$version`", " +
                    "so it never forces a release of $name."
            }
        } else {
            $exactCount++
            if ($dependency.req -ne "=$version") {
                $failures += "$what as `"$($dependency.req)`", where it " +
                    "must be `"=$version`", that crate's own version, so a " +
                    "build takes the crates of one release together."
            }
        }

        if ($order.ContainsKey($name)) {
            if (-not $order.ContainsKey($target)) {
                $failures += "$what, which ci/crates.txt does not publish, " +
                    "so the published $name could not resolve it."
            } elseif ($order[$target] -gt $order[$name]) {
                $failures += "$what, which ci/crates.txt publishes after " +
                    "it, so $target $version would not be on crates.io " +
                    "when $name is published."
            }
        }
    }
}

if ($exactCount -eq 0) {
    $failures += "found no normal requirements between the workspace " +
        "crates, so there was nothing to check."
}

if ($failures.Count -gt 0) {
    foreach ($failure in $failures) {
        Write-Output "FAIL $failure"
    }
    throw "$($failures.Count) problem(s) with the requirements between " +
        "the workspace crates."
}
Write-Output ("All $exactCount normal and build requirements between the " +
    "workspace crates are exact, the $devCount dev-dependencies give a " +
    "plain version, and ci/crates.txt publishes each of its " +
    "$($published.Count) crates after every crate it requires.")
