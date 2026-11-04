<#
.SYNOPSIS
Works out the version the next release publishes, for common-ci's shared
publish, and sets it in a global variable.

.DESCRIPTION
The version comes from the release tags through common-ci's GitVersion rules,
as it does for the other languages, so main gives the next plain patch, such as
4.5.10 after 4.5.9. GitVersion answers 0.0.1 for a repository with no version
tags rather than failing, and a published version can never be taken back, so
having no tags stops the run.

Runs from the common-ci checkout, with this repository cloned beside it.

.PARAMETER RepoName
The folder this repository is cloned into.

.PARAMETER VariableName
The global variable the version is written to.
#>
param (
    [Parameter(Mandatory)][string]$RepoName,
    [string]$VariableName = "Version"
)
$ErrorActionPreference = "Stop"

$tags = git -C $RepoName tag --list | Where-Object { $_ -match '^v?[0-9]+[.][0-9]+[.][0-9]+$' }
if (-not $tags) {
    Write-Output "::error title=No version tags::There is nothing for the version to count from, so this run has stopped."
    throw "No version tags in '$RepoName'. GitVersion would answer 0.0.1, which must never be published."
}

Set-Variable -Scope Global -Name $VariableName -Value (./steps/get-next-package-version.ps1 -RepoName $RepoName)
