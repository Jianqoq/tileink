param(
    [Parameter(Mandatory=$true)][string]$Output,
    [string]$Python = 'python',
    [string[]]$Gpu = @(),
    [ValidateSet('svg', 'examples', 'retained', 'rounding')][string[]]$Suite = @('svg', 'examples', 'retained')
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'quiet_runner.ps1')
$arguments = @('-X', 'utf8', "$PSScriptRoot/../native/acceptance.py", '--output', $Output)
foreach ($identity in $Gpu) { $arguments += @('--gpu', $identity) }
foreach ($item in $Suite) { $arguments += @('--suite', $item) }
$log = New-QuietRunLog -Name 'native-acceptance'
Write-QuietProgress -Label 'Windows native acceptance'
$exitCode = Invoke-QuietCommand -Label 'Windows native acceptance' -FilePath $Python -ArgumentList $arguments -LogPath $log
Write-Host "Evidence: $Output"
Write-Host "Log: $log"
exit $exitCode
