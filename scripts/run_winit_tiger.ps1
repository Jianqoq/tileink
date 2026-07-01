param(
    [switch]$BuildOnly
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")

Push-Location $repoRoot
try {
    $args = @("--release", "--features", "wgpu", "--example", "winit_svg_tiger")
    if ($BuildOnly) {
        cargo build @args
    } else {
        cargo run @args
    }
} finally {
    Pop-Location
}
