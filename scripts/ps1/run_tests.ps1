param([string[]]$CargoArgs = @())
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'quiet_runner.ps1')
if ($CargoArgs | Where-Object { $_ -like '--test-threads*' }) { throw 'Test thread count is fixed at 1' }
$env:RAYON_NUM_THREADS = '1'
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot '../..')
$logPath = New-QuietRunLog -Name 'tests'
# Preserve the caller's Cargo/harness boundary; another separator becomes a
# harness argument instead of an option delimiter.
$arguments = @('test', '--release') + $CargoArgs
if ($CargoArgs -notcontains '--') { $arguments += '--' }
$arguments += '--test-threads=1'
Push-Location $repoRoot
try {
    Write-QuietProgress -Label 'Release tests [single-threaded]'
    Invoke-QuietCommand -Label 'Release tests [single-threaded]' -FilePath 'cargo' -ArgumentList $arguments -LogPath $logPath
} finally { Pop-Location }
Complete-QuietRun -Label 'release tests' -LogPath $logPath
