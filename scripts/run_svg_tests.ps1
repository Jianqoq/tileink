param(
    [ValidateSet("all", "filters", "masking", "paint-servers", "painting", "shapes", "structure", "text")]
    [string]$Type = "all",

    [ValidateSet("both", "cpu", "cubecl", "wgpu", "cubecl-wgpu")]
    [string]$Backend = "both",

    [switch]$ContinueOnError
)

$ErrorActionPreference = "Stop"

$repo = Resolve-Path (Join-Path $PSScriptRoot "..")
$testsRoot = Join-Path $repo "src\svg\tests"

Push-Location $repo
try {
    $oldErrorActionPreference = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    $metadataJson = cargo metadata --format-version 1 --no-deps
    $metadataExit = $LASTEXITCODE
    $ErrorActionPreference = $oldErrorActionPreference
    if ($metadataExit -ne 0) {
        throw "cargo metadata failed with exit code $metadataExit"
    }
    $metadata = $metadataJson | ConvertFrom-Json
    $example = Join-Path $metadata.target_directory "release\examples\svg_fixture_render.exe"

    $oldErrorActionPreference = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    if ($Backend -eq "wgpu" -or $Backend -eq "cubecl-wgpu") {
        cargo build --release --features wgpu --example svg_fixture_render
    } else {
        cargo build --release --example svg_fixture_render
    }
    $buildExit = $LASTEXITCODE
    $ErrorActionPreference = $oldErrorActionPreference
    if ($buildExit -ne 0) {
        throw "cargo build failed with exit code $buildExit"
    }
    if (-not (Test-Path -Path $example)) {
        throw "Expected renderer executable was not created: $example"
    }

    $typeDirs = if ($Type -eq "all") {
        Get-ChildItem -Path $testsRoot -Directory | Sort-Object Name
    } else {
        @(Get-Item -Path (Join-Path $testsRoot $Type))
    }

    $failures = New-Object System.Collections.Generic.List[string]

    foreach ($dir in $typeDirs) {
        Write-Host "[$Backend] $($dir.FullName)"
        $oldErrorActionPreference = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        & $example $dir.FullName $Backend
        $renderExit = $LASTEXITCODE
        $ErrorActionPreference = $oldErrorActionPreference
        if ($renderExit -ne 0) {
            $message = "[$Backend] $($dir.FullName)"
            $failures.Add($message)
            if (-not $ContinueOnError) {
                throw "SVG render failed: $message"
            }
        }
    }

    if ($failures.Count -gt 0) {
        Write-Host ""
        Write-Host "Failures:"
        $failures | ForEach-Object { Write-Host "  $_" }
        exit 1
    }
} finally {
    Pop-Location
}
