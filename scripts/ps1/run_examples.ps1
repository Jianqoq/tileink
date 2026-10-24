$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'quiet_runner.ps1')
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot '../..')
$logPath = New-QuietRunLog -Name 'examples'
Push-Location $repoRoot
try {
    Write-QuietProgress -Label 'Build native examples'
    Invoke-QuietCommand -Label 'Build native examples' -FilePath 'cargo' -ArgumentList @('build', '--release', '--examples') -LogPath $logPath
} finally { Pop-Location }
Complete-QuietRun -Label 'native examples' -LogPath $logPath
