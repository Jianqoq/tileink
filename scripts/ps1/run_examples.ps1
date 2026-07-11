param(
    [ValidateSet("native", "portable", "both")]
    [string]$WgpuMode = "both"
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "quiet_runner.ps1")

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "../..")
$examples = @("wgpu_examples")
$logPath = New-QuietRunLog -Name "examples"
$metadataPath = "$logPath.metadata.json"
$totalSteps = $examples.Count + 2

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
    for ($index = 0; $index -lt $examples.Count; $index++) {
        $name = $examples[$index]
        $label = "Build example [$name]"
        Write-QuietProgress -Label $label -Step ($index + 1) -Total $totalSteps
        $exitCode = Invoke-QuietCommand -Label $label -FilePath "cargo" `
            -ArgumentList @("build", "--release", "--example", $name) -LogPath $logPath
    }

    Write-QuietProgress -Label "Read Cargo metadata" -Step ($examples.Count + 1) -Total $totalSteps
    $exitCode = Invoke-QuietCommand -Label "Cargo metadata" -FilePath "cargo" `
        -ArgumentList @("metadata", "--format-version", "1", "--no-deps") `
        -LogPath $logPath -StdoutPath $metadataPath
    $metadata = Get-Content -LiteralPath $metadataPath -Raw | ConvertFrom-Json
    Remove-Item -LiteralPath $metadataPath -Force
    $examplesOutDir = Join-Path $metadata.target_directory "release\examples"

    $wgpuExe = Get-ExampleExecutable -ExamplesOutDir $examplesOutDir -Name "wgpu_examples"
    $oldMode = $env:TILEINK_WGPU_MODE
    $oldComparePortable = $env:TILEINK_WGPU_COMPARE_PORTABLE
    try {
        if ($WgpuMode -eq "native") {
            $env:TILEINK_WGPU_MODE = "native"
            Remove-Item Env:\TILEINK_WGPU_COMPARE_PORTABLE -ErrorAction SilentlyContinue
            $label = "Run examples [native]"
        } else {
            $env:TILEINK_WGPU_MODE = "native"
            $env:TILEINK_WGPU_COMPARE_PORTABLE = "1"
            $label = "Run examples [native + portable pixel compare]"
        }
        Write-QuietProgress -Label $label -Step $totalSteps -Total $totalSteps
        $exitCode = Invoke-QuietCommand -Label $label -FilePath $wgpuExe -LogPath $logPath
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

Complete-QuietRun -Label "examples; outputs are in examples/wgpu/out" -LogPath $logPath
