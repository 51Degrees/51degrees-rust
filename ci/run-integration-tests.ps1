# The workspace has no separate integration stage. The tests that read the
# on-premise data files and the ones that parse a 51Did all run under
# cargo test in run-unit-tests.ps1, so splitting them here would run the same
# tests twice.
Write-Output "Covered by cargo test in run-unit-tests.ps1"
