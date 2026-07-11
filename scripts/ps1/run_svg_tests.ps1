param(
    [ValidateSet("all", "filters", "masking", "paint-servers", "painting", "shapes", "structure", "text")]
    [string]$Type = "all",

    [ValidateSet("wgpu")]
    [string]$Backend = "wgpu",

    [ValidateSet("native", "portable", "both")]
    [string]$WgpuMode = "both",

    [switch]$ContinueOnError
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "quiet_runner.ps1")

$repo = Resolve-Path (Join-Path $PSScriptRoot "../..")
$testsRoot = Join-Path $repo "src\svg\tests"
$logPath = New-QuietRunLog -Name "svg-$Type"
$metadataPath = "$logPath.metadata.json"

Push-Location $repo
try {
    Write-QuietProgress -Label "Read Cargo metadata"
    $metadataExit = Invoke-QuietCommand -Label "Cargo metadata" -FilePath "cargo" `
        -ArgumentList @("metadata", "--format-version", "1", "--no-deps") `
        -LogPath $logPath -StdoutPath $metadataPath
    $metadata = Get-Content -LiteralPath $metadataPath -Raw | ConvertFrom-Json
    Remove-Item -LiteralPath $metadataPath -Force
    $example = Join-Path $metadata.target_directory "release\examples\svg_fixture_render.exe"

    Write-QuietProgress -Label "Build SVG fixture renderer"
    $buildExit = Invoke-QuietCommand -Label "Build SVG fixture renderer" -FilePath "cargo" `
        -ArgumentList @("build", "--release", "--example", "svg_fixture_render") `
        -LogPath $logPath
    if (-not (Test-Path -Path $example)) {
        throw "Expected renderer executable was not created: $example"
    }

    $typeDirs = if ($Type -eq "all") {
        Get-ChildItem -Path $testsRoot -Directory | Sort-Object Name
    } else {
        @(Get-Item -Path (Join-Path $testsRoot $Type))
    }

    $failures = New-Object System.Collections.Generic.List[string]

    $renderJobs = New-Object System.Collections.Generic.List[object]
    if ($WgpuMode -eq "native") {
        $renderJobs.Add([pscustomobject]@{ Backend = "wgpu"; WgpuMode = "native"; Label = "wgpu"; ComparePortable = $false })
    } else {
        $renderJobs.Add([pscustomobject]@{ Backend = "wgpu"; WgpuMode = "native"; Label = "wgpu-portable-compare"; ComparePortable = $true })
    }

    $renderStep = 0
    $renderTotal = $typeDirs.Count * $renderJobs.Count
    foreach ($dir in $typeDirs) {
        foreach ($job in $renderJobs) {
            $renderStep++
            $label = "SVG [$($job.Label)] $($dir.Name)"
            Write-QuietProgress -Label $label -Step $renderStep -Total $renderTotal
            if ($job.ComparePortable) {
                $arguments = @($dir.FullName, $job.Backend, "--compare-wgpu-portable")
            } else {
                $arguments = @($dir.FullName, $job.Backend, "--wgpu-mode", $job.WgpuMode)
            }
            $renderExit = Invoke-QuietCommand -Label $label -FilePath $example `
                -ArgumentList $arguments -LogPath $logPath -AllowFailure
            if ($renderExit -ne 0) {
                $message = "[$($job.Label)] $($dir.FullName)"
                $failures.Add($message)
                if (-not $ContinueOnError) {
                    throw "SVG render failed: $message"
                }
            }
        }
    }

    if ($failures.Count -gt 0) {
        Write-Host "Failures:"
        $failures | ForEach-Object { Write-Host "  $_" }
        exit 1
    }
} finally {
    Pop-Location
}

Complete-QuietRun -Label "SVG tests [$Type]" -LogPath $logPath
