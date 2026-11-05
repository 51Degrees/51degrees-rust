$ErrorActionPreference = "Stop"

# The free data files the tests read, each with the folder of the data
# repository in the cxx submodule where the tests look for it. common-ci's
# steps/fetch-assets.ps1 keeps them in ./assets, which the nightly restores
# from the Actions cache, and downloads only one that is not there. Both of
# the nightly's calls to this script run it from the common-ci checkout, where
# that step and ./assets are found.
$data = "$PSScriptRoot/../cxx/data"
$assets = [ordered]@{
    '51Degrees-LiteV4.1.hash'      = 'device-detection'
    '51Degrees-IPIV4AsnIpiV41.ipi' = 'ip-intelligence'
}

./steps/fetch-assets.ps1 -Assets @($assets.Keys)
foreach ($asset in $assets.Keys) {
    Copy-Item -LiteralPath "assets/$asset" -Destination "$data/$($assets[$asset])"
}
