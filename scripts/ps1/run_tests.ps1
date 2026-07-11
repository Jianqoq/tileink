param(
    [ValidateSet("native", "portable", "both")]
    [string]$WgpuMode = "both",

    [string[]]$CargoArgs = @()
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "quiet_runner.ps1")

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "../..")
$modes = if ($WgpuMode -eq "both") { @("native", "portable") } else { @($WgpuMode) }
$logPath = New-QuietRunLog -Name "tests"
$script:QuietStep = 0
$script:QuietTotal = $modes.Count * $(if ($CargoArgs.Count -gt 0) { 1 } else { 7 })

if ($CargoArgs | Where-Object { $_ -like "--test-threads*" }) {
    throw "Test thread count is fixed at 1; do not pass --test-threads in CargoArgs"
}

function Invoke-CargoTest {
    param(
        [Parameter(Mandatory = $true)][string]$Mode,
        [Parameter(Mandatory = $true)][string]$Label,
        [string]$Filter = "",
        [string[]]$HarnessArgs = @(),
        [string[]]$ExtraCargoArgs = @()
    )

    $arguments = [System.Collections.Generic.List[string]]::new()
    $arguments.Add("test")
    $arguments.Add("--release")
    foreach ($argument in $ExtraCargoArgs) {
        $arguments.Add($argument)
    }
    if ($Filter) {
        $arguments.Add($Filter)
    }
    $arguments.Add("--")
    foreach ($argument in $HarnessArgs) {
        $arguments.Add($argument)
    }
    $arguments.Add("--test-threads=1")

    $script:QuietStep++
    $progressLabel = "WGPU release tests [$Mode, $Label, single-threaded]"
    Write-QuietProgress -Label $progressLabel -Step $script:QuietStep -Total $script:QuietTotal
    $exitCode = Invoke-QuietCommand -Label $progressLabel -FilePath "cargo" `
        -ArgumentList $arguments -LogPath $logPath
}

function Invoke-ReleaseTests {
    param([Parameter(Mandatory = $true)][string]$Mode)

    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    if ($CargoArgs.Count -gt 0) {
        $separator = [Array]::IndexOf($CargoArgs, "--")
        if ($separator -ge 0) {
            $cargo = if ($separator -gt 0) {
                @($CargoArgs[0..($separator - 1)])
            } else {
                @()
            }
            $harness = if ($separator + 1 -lt $CargoArgs.Count) {
                @($CargoArgs[($separator + 1)..($CargoArgs.Count - 1)])
            } else {
                @()
            }
            Invoke-CargoTest -Mode $Mode -Label "focused" -ExtraCargoArgs $cargo -HarnessArgs $harness
        } else {
            Invoke-CargoTest -Mode $Mode -Label "focused" -ExtraCargoArgs $CargoArgs
        }
    } else {
        # WGPU/DX12 drivers become unstable when hundreds of device-heavy tests share one process.
        # These partitions still run strictly serially, but release each process's driver state.
        Invoke-CargoTest -Mode $Mode -Label "core" -HarnessArgs @(
            "--skip", "svg::tests::",
            "--skip", "wgpu::renderer::tests::"
        )
        Invoke-CargoTest -Mode $Mode -Label "svg-unit" -Filter "svg::tests::"
        Invoke-CargoTest -Mode $Mode -Label "persistent-renderer" -Filter "wgpu::renderer::tests::persistent_"
        Invoke-CargoTest -Mode $Mode -Label "snapshot-retained-renderer" -Filter "wgpu::renderer::tests::retained_"
        Invoke-CargoTest -Mode $Mode -Label "low-level-renderer" -Filter "wgpu::renderer::tests::wgpu_" -HarnessArgs @(
            "--skip", "wgpu_renderer_samples_"
        )
        Invoke-CargoTest -Mode $Mode -Label "renderer-sampling" -Filter "wgpu::renderer::tests::wgpu_renderer_samples_"
        Invoke-CargoTest -Mode $Mode -Label "renderer-misc" -Filter "wgpu::renderer::tests::" -HarnessArgs @(
            "--skip", "persistent_",
            "--skip", "retained_",
            "--skip", "wgpu_"
        )
    }
    $stopwatch.Stop()
    Add-QuietLog -LogPath $logPath -Text ("Finished [{0}] in {1:c}`n" -f $Mode, $stopwatch.Elapsed)
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

Complete-QuietRun -Label "release tests" -LogPath $logPath
