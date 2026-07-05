param(
    [ValidateSet("native", "portable", "both")]
    [string]$WgpuMode = "both",

    [string[]]$CargoArgs = @()
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$modes = if ($WgpuMode -eq "both") { @("native", "portable") } else { @($WgpuMode) }

Push-Location $repoRoot
try {
    $oldRunWgpu = $env:TILEINK_RUN_WGPU_TESTS
    $oldMode = $env:TILEINK_WGPU_MODE
    try {
        $env:TILEINK_RUN_WGPU_TESTS = "1"
        foreach ($mode in $modes) {
            $env:TILEINK_WGPU_MODE = $mode
            Write-Host "Running release tests [$mode]"
            cargo test --release @CargoArgs
        }
    } finally {
        $env:TILEINK_RUN_WGPU_TESTS = $oldRunWgpu
        $env:TILEINK_WGPU_MODE = $oldMode
    }
} finally {
    Pop-Location
}
