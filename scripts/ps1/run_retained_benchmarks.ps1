param(
    [ValidateSet("all", "scale", "dirty-ratio")]
    [string]$Benchmark = "all",

    [string]$Filter = "",

    [string]$SaveBaseline = "",

    [string]$Baseline = "",

    [switch]$Quick
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "quiet_runner.ps1")

if ($SaveBaseline -and $Baseline) {
    throw "Use either -SaveBaseline or -Baseline, not both"
}

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "../..")
$targets = @(switch ($Benchmark) {
    "scale" { @("retained_scale") }
    "dirty-ratio" { @("retained_dirty_ratio") }
    default { @("retained_scale", "retained_dirty_ratio") }
})
$logPath = New-QuietRunLog -Name "retained-benchmarks"

Push-Location $repoRoot
try {
    for ($index = 0; $index -lt $targets.Count; $index++) {
        $target = $targets[$index]
        $arguments = [System.Collections.Generic.List[string]]::new()
        $arguments.Add("bench")
        $arguments.Add("--bench")
        $arguments.Add($target)
        $arguments.Add("--")
        if ($Filter) {
            $arguments.Add($Filter)
        }
        if ($SaveBaseline) {
            $arguments.Add("--save-baseline")
            $arguments.Add($SaveBaseline)
        } elseif ($Baseline) {
            $arguments.Add("--baseline")
            $arguments.Add($Baseline)
        }
        if ($Quick) {
            $arguments.Add("--quick")
        }

        $label = "Criterion benchmark [$target]"
        Write-QuietProgress -Label $label -Step ($index + 1) -Total $targets.Count
        $exitCode = Invoke-QuietCommand -Label $label -FilePath "cargo" `
            -ArgumentList $arguments -LogPath $logPath
    }
} finally {
    Pop-Location
}

Complete-QuietRun -Label "retained benchmarks" -LogPath $logPath
