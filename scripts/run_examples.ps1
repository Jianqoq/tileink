$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
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

    foreach ($name in $examples) {
        $exe = Get-ExampleExecutable -ExamplesOutDir $examplesOutDir -Name $name

        Write-Host "Running example: $name"
        & $exe
    }
} finally {
    Pop-Location
}

Write-Host "All examples finished. Outputs are in examples/cpu/out and examples/wgpu/out."
