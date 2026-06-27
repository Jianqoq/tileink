Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repoRoot
try {
    cargo bench --features bench-api --bench cubecl_dense_compare -- @args
} finally {
    Pop-Location
}
