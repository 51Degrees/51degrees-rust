<#
.SYNOPSIS
Nothing to install for common-ci's shared publish.

.DESCRIPTION
The publish step publishes from the commit the packages were built from, so
there is no package to install first.
#>
Write-Output "No intermediate steps needed"
