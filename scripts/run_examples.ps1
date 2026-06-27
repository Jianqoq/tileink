$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$skip = @("bench_cpu")

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
    cargo build --release --examples

    $metadata = cargo metadata --format-version 1 --no-deps | ConvertFrom-Json
    $examplesOutDir = Join-Path $metadata.target_directory "release\examples"
    $package = $metadata.packages | Where-Object { $_.name -eq "tileink" } | Select-Object -First 1
    if ($null -eq $package) {
        throw "Package 'tileink' not found in cargo metadata"
    }

    $exampleTargets = $package.targets |
        Where-Object { $_.kind -contains "example" } |
        Where-Object {
            $path = $_.src_path.Replace("/", "\")
            $path -like "*\examples\cpu\*" -or $path -like "*\examples\cubecl\*"
        } |
        Where-Object { $skip -notcontains [IO.Path]::GetFileNameWithoutExtension($_.src_path) } |
        Sort-Object src_path

    foreach ($target in $exampleTargets) {
        $name = $target.name
        $exe = Get-ExampleExecutable -ExamplesOutDir $examplesOutDir -Name $name

        Write-Host "Running example: $name"
        & $exe
    }
} finally {
    Pop-Location
}

Write-Host "All examples finished. Outputs are in examples/cpu/out and examples/cubecl/out."
