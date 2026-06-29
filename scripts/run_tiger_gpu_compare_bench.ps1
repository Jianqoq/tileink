param(
    [switch]$Cuda
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$features = "bench-api"

if ($Cuda) {
    $cudaPath = [Environment]::GetEnvironmentVariable("CUDA_PATH", "Machine")
    if ([string]::IsNullOrWhiteSpace($cudaPath)) {
        throw "CUDA_PATH is not set in the machine environment."
    }

    $cudaBin = Join-Path $cudaPath "bin"
    $cudaBinX64 = Join-Path $cudaBin "x64"
    $env:CUDA_PATH = $cudaPath
    $env:CUDARC_CUDA_VERSION = "13030"
    $env:PATH = "$cudaBinX64;$cudaBin;$env:PATH"
    $features = "$features cuda"
}

Push-Location $repoRoot
try {
    cargo bench --features $features --bench tiger_gpu_compare -- @args
} finally {
    Pop-Location
}
