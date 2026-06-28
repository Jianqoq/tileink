param(
    [ValidateSet("all", "filters", "masking", "paint-servers", "painting", "shapes", "structure", "text")]
    [string]$Type = "all",

    [ValidateSet("both", "cpu", "cubecl")]
    [string]$Backend = "both",

    [switch]$ContinueOnError
)

$ErrorActionPreference = "Stop"

$repo = Resolve-Path (Join-Path $PSScriptRoot "..")
$testsRoot = Join-Path $repo "src\svg\tests"

Push-Location $repo
try {
    $metadata = cargo metadata --format-version 1 --no-deps | ConvertFrom-Json
    $example = Join-Path $metadata.target_directory "release\examples\svg_fixture_render.exe"
    cargo build --release --example svg_fixture_render
    if (-not (Test-Path $example)) {
        throw "Expected renderer executable was not created: $example"
    }

    $typeDirs = if ($Type -eq "all") {
        Get-ChildItem -Path $testsRoot -Directory | Sort-Object Name
    } else {
        @(Get-Item -Path (Join-Path $testsRoot $Type))
    }

    $backends = if ($Backend -eq "both") { @("cpu", "cubecl") } else { @($Backend) }
    $failures = New-Object System.Collections.Generic.List[string]

    foreach ($dir in $typeDirs) {
        $svgs = Get-ChildItem -Path $dir.FullName -Filter "*.svg" -Recurse | Sort-Object FullName
        foreach ($svg in $svgs) {
            foreach ($targetBackend in $backends) {
                $stem = [System.IO.Path]::GetFileNameWithoutExtension($svg.Name)
                $out = Join-Path $svg.DirectoryName "$stem.$targetBackend.png"
                Write-Host "[$targetBackend] $($svg.FullName)"
                & $example $targetBackend $svg.FullName $out
                if ($LASTEXITCODE -ne 0) {
                    $message = "[$targetBackend] $($svg.FullName)"
                    $failures.Add($message)
                    if (-not $ContinueOnError) {
                        throw "SVG render failed: $message"
                    }
                }
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
