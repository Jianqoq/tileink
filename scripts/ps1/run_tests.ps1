param(
    [ValidateSet("native", "portable", "both")]
    [string]$WgpuMode = "both",

    [string[]]$CargoArgs = @(),

    [ValidateRange(1, 64)]
    [int]$TestThreads = 1
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "../..")
$modes = if ($WgpuMode -eq "both") { @("native", "portable") } else { @($WgpuMode) }

function Invoke-ReleaseTests {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Mode
    )

    $arguments = [System.Collections.Generic.List[string]]::new()
    $arguments.Add("test")
    $arguments.Add("--release")
    foreach ($argument in $CargoArgs) {
        $arguments.Add($argument)
    }

    if (-not ($CargoArgs | Where-Object { $_ -like "--test-threads*" })) {
        if (-not $CargoArgs.Contains("--")) {
            $arguments.Add("--")
        }
        $arguments.Add("--test-threads=$TestThreads")
    }

    Write-Host "Running explicit WGPU release tests [$Mode, $TestThreads test thread(s)]"
    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    & cargo $arguments
    $exitCode = $LASTEXITCODE
    $stopwatch.Stop()
    Write-Host ("Finished [{0}] in {1:c}" -f $Mode, $stopwatch.Elapsed)

    if ($exitCode -ne 0) {
        throw "cargo test failed in '$Mode' mode with exit code $exitCode"
    }
}

Push-Location $repoRoot
try {
    $oldRunWgpu = $env:TILEINK_RUN_WGPU_TESTS
    $oldMode = $env:TILEINK_WGPU_MODE
    try {
        $env:TILEINK_RUN_WGPU_TESTS = "1"
        foreach ($mode in $modes) {
            $env:TILEINK_WGPU_MODE = $mode
            Invoke-ReleaseTests -Mode $mode
        }
    } finally {
        $env:TILEINK_RUN_WGPU_TESTS = $oldRunWgpu
        $env:TILEINK_WGPU_MODE = $oldMode
    }
} finally {
    Pop-Location
}
