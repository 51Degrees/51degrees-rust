# The on-premise data files the tests read are linked from the data repository
# inside the cxx submodule, so setup-environment.ps1 downloads them with that
# repository's script once the submodules are checked out, rather than here.
Write-Output "The data files are downloaded by setup-environment.ps1"
