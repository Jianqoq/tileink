param(
    [switch]$BuildOnly
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "quiet_runner.ps1")

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "../..")
$logPath = New-QuietRunLog -Name "winit-tiger"

Push-Location $repoRoot
try {
    $args = @("--release", "--example", "winit_svg_tiger")
    if ($BuildOnly) {
        $label = "Build winit SVG tiger"
        $command = "build"
    } else {
        $label = "Run winit SVG tiger"
        $command = "run"
    }
    Write-QuietProgress -Label $label -Step 1 -Total 1
    $arguments = @($command) + $args
    $exitCode = Invoke-QuietCommand -Label $label -FilePath "cargo" `
        -ArgumentList $arguments -LogPath $logPath
} finally {
    Pop-Location
}

Complete-QuietRun -Label "winit SVG tiger" -LogPath $logPath
