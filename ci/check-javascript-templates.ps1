<#
.SYNOPSIS
Fails when the committed copy of the JavaScript templates differs from the
javascript-templates submodule.

.DESCRIPTION
Cargo packages only the files inside a crate's own folder, so the
javascript-builder crate has to carry its own copy of the templates, which it
compiles in from javascript-builder/assets. The copy is taken from the
javascript-templates submodule at the root of the repository. Every .mustache
file at the root of the submodule must be in javascript-builder/assets with
the same bytes, and the copy must hold no other .mustache file.

.EXAMPLE
git submodule update --init javascript-templates
./ci/check-javascript-templates.ps1
#>
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$source = Join-Path $root 'javascript-templates'
$copy = Join-Path $root 'javascript-builder/assets'

$templates = @(Get-ChildItem -Path $source -Filter '*.mustache' -File)
if ($templates.Count -eq 0) {
    throw "The javascript-templates submodule has no templates, so run 'git submodule update --init javascript-templates' first"
}
$commit = git -C $source rev-parse HEAD

$problems = @()
foreach ($template in $templates) {
    $copied = Join-Path $copy $template.Name
    if (-not (Test-Path $copied)) {
        $problems += "javascript-builder/assets/$($template.Name) is missing"
        continue
    }
    $expected = (Get-FileHash $template.FullName -Algorithm SHA256).Hash
    $actual = (Get-FileHash $copied -Algorithm SHA256).Hash
    if ($expected -ne $actual) {
        $problems += "javascript-builder/assets/$($template.Name) differs from javascript-templates/$($template.Name)"
    }
}
foreach ($copied in Get-ChildItem -Path $copy -Filter '*.mustache' -File) {
    if (-not (Test-Path (Join-Path $source $copied.Name))) {
        $problems += "javascript-builder/assets/$($copied.Name) is not in the submodule"
    }
}

if ($problems) {
    foreach ($problem in $problems) {
        Write-Host "::error::$problem"
    }
    throw "The copy of the JavaScript templates is not the javascript-templates submodule at $commit. Copy the .mustache files from javascript-templates into javascript-builder/assets and commit the result."
}
Write-Host "The copy of the JavaScript templates matches javascript-templates at $commit"
