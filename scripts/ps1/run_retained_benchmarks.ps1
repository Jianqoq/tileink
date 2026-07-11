param(
    [ValidateSet("all", "scale", "dirty-ratio")]
    [string]$Benchmark = "all",

    [string]$Filter = "",

    [string]$SaveBaseline = "",

    [string]$Baseline = "",

    [switch]$Quick
)

$ErrorActionPreference = "Stop"

if ($SaveBaseline -and $Baseline) {
    throw "Use either -SaveBaseline or -Baseline, not both"
}

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "../..")
$targets = switch ($Benchmark) {
    "scale" { @("retained_scale") }
    "dirty-ratio" { @("retained_dirty_ratio") }
    default { @("retained_scale", "retained_dirty_ratio") }
}

Push-Location $repoRoot
try {
    foreach ($target in $targets) {
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

        Write-Host "Running Criterion benchmark: $target"
        & cargo $arguments
        if ($LASTEXITCODE -ne 0) {
            throw "Criterion benchmark '$target' failed with exit code $LASTEXITCODE"
        }
    }
} finally {
    Pop-Location
}
