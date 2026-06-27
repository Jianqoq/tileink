Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repoRoot
try {
    cargo bench --features bench-api --bench gpu_compare -- @args
} finally {
    Pop-Location
}
