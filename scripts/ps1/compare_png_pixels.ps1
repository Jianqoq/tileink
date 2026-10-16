param([string]$BaseRef = "HEAD")

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "quiet_runner.ps1")

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "../..")
$logPath = New-QuietRunLog -Name "png-pixels"

Push-Location $repoRoot
try {
    Write-QuietProgress -Label "Compare PNG pixels against $BaseRef"
    $exitCode = Invoke-QuietCommand -Label "PNG pixel comparison" -FilePath "cargo" `
        -ArgumentList @("run", "--release", "--example", "compare_png_pixels", "--", $BaseRef) `
        -LogPath $logPath -AllowFailure
} finally {
    Pop-Location
}

Get-Content -LiteralPath $logPath | Where-Object {
    $_.StartsWith("Baseline:") -or $_.StartsWith("Summary:") -or $_.StartsWith("Result:")
} | ForEach-Object { Write-Host $_ }
if ($exitCode -eq 0) {
    Complete-QuietRun -Label "PNG pixels match; full per-file report is in the log" -LogPath $logPath
}
exit $exitCode
