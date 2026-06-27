$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$examplesDir = Join-Path $repoRoot "examples"
$skip = @("bench_cpu")

Push-Location $repoRoot
try {
    Write-Host "Building examples..."
    cargo build --release --examples

    $metadata = cargo metadata --format-version 1 --no-deps | ConvertFrom-Json
    $examplesOutDir = Join-Path $metadata.target_directory "release\examples"

    Get-ChildItem -Path $examplesDir -Filter "*.rs" |
        Where-Object { $skip -notcontains $_.BaseName } |
        Sort-Object BaseName |
        ForEach-Object {
            $name = $_.BaseName
            $exe = Join-Path $examplesOutDir "$name.exe"
            if (-not (Test-Path $exe)) {
                $exe = Join-Path $examplesOutDir $name
            }
            if (-not (Test-Path $exe)) {
                throw "Built executable not found for example '$name' in $examplesOutDir"
            }

            Write-Host "Running example: $name"
            & $exe
        }
} finally {
    Pop-Location
}

Write-Host "All examples finished. Outputs are in examples/cpu_out."
