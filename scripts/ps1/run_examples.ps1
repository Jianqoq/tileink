param(
    [ValidateSet("native", "portable", "both")]
    [string]$WgpuMode = "both"
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "../..")
$examples = @("cpu_examples", "wgpu_examples")

function Get-ExampleExecutable {
    param(
        [Parameter(Mandatory = $true)][string]$ExamplesOutDir,
        [Parameter(Mandatory = $true)][string]$Name
    )

    $exe = Join-Path $ExamplesOutDir "$Name.exe"
    if (Test-Path $exe) {
        return $exe
    }

    $exe = Join-Path $ExamplesOutDir $Name
    if (Test-Path $exe) {
        return $exe
    }

    throw "Built executable not found for example '$Name' in $ExamplesOutDir"
}

Push-Location $repoRoot
try {
    Write-Host "Building examples..."
    foreach ($name in $examples) {
        cargo build --release --example $name
    }

    $metadata = cargo metadata --format-version 1 --no-deps | ConvertFrom-Json
    $examplesOutDir = Join-Path $metadata.target_directory "release\examples"

    $cpuExe = Get-ExampleExecutable -ExamplesOutDir $examplesOutDir -Name "cpu_examples"
    Write-Host "Running example: cpu_examples"
    & $cpuExe

    $wgpuExe = Get-ExampleExecutable -ExamplesOutDir $examplesOutDir -Name "wgpu_examples"
    $oldMode = $env:TILEINK_WGPU_MODE
    $oldComparePortable = $env:TILEINK_WGPU_COMPARE_PORTABLE
    try {
        if ($WgpuMode -eq "native") {
            $env:TILEINK_WGPU_MODE = "native"
            Remove-Item Env:\TILEINK_WGPU_COMPARE_PORTABLE -ErrorAction SilentlyContinue
            Write-Host "Running example: wgpu_examples [native]"
            & $wgpuExe
        } else {
            $env:TILEINK_WGPU_MODE = "native"
            $env:TILEINK_WGPU_COMPARE_PORTABLE = "1"
            Write-Host "Running example: wgpu_examples [native + portable pixel compare]"
            & $wgpuExe
        }
    } finally {
        if ($null -eq $oldMode) {
            Remove-Item Env:\TILEINK_WGPU_MODE -ErrorAction SilentlyContinue
        } else {
            $env:TILEINK_WGPU_MODE = $oldMode
        }
        if ($null -eq $oldComparePortable) {
            Remove-Item Env:\TILEINK_WGPU_COMPARE_PORTABLE -ErrorAction SilentlyContinue
        } else {
            $env:TILEINK_WGPU_COMPARE_PORTABLE = $oldComparePortable
        }
    }
} finally {
    Pop-Location
}

Write-Host "All examples finished. Outputs are in examples/cpu/out and examples/wgpu/out."
