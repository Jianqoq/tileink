param(
    [ValidateSet('all', 'filters', 'masking', 'paint-servers', 'painting', 'shapes', 'structure', 'text')]
    [string]$Type = 'all',
    [switch]$ContinueOnError
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'quiet_runner.ps1')
$repo = Resolve-Path (Join-Path $PSScriptRoot '../..')
$testsRoot = Join-Path $repo 'src/svg/tests'
$logPath = New-QuietRunLog -Name "svg-$Type"
$metadataPath = "$logPath.metadata.json"
Push-Location $repo
try {
    Invoke-QuietCommand -Label 'Cargo metadata' -FilePath 'cargo' -ArgumentList @('metadata', '--format-version', '1', '--no-deps') -LogPath $logPath -StdoutPath $metadataPath
    $metadata = Get-Content -LiteralPath $metadataPath -Raw | ConvertFrom-Json
    Remove-Item -LiteralPath $metadataPath -Force
    $example = Join-Path $metadata.target_directory 'release/examples/svg_fixture_render.exe'
    Invoke-QuietCommand -Label 'Build SVG fixture renderer' -FilePath 'cargo' -ArgumentList @('build', '--release', '--example', 'svg_fixture_render') -LogPath $logPath
    if (-not (Test-Path -LiteralPath $example)) { throw "Missing SVG renderer: $example" }
    $dirs = if ($Type -eq 'all') { Get-ChildItem -LiteralPath $testsRoot -Directory | Sort-Object Name } else { @(Get-Item -LiteralPath (Join-Path $testsRoot $Type)) }
    $failures = [System.Collections.Generic.List[string]]::new()
    $step = 0
    foreach ($dir in $dirs) {
        $step++
        $label = "SVG [$($dir.Name)]"
        Write-QuietProgress -Label $label -Step $step -Total $dirs.Count
        $result = Invoke-QuietCommand -Label $label -FilePath $example -ArgumentList @($dir.FullName) -LogPath $logPath -AllowFailure
        if ($result -ne 0) {
            $failures.Add($dir.FullName)
            if (-not $ContinueOnError) { throw "SVG render failed: $($dir.FullName)" }
        }
    }
    if ($failures.Count -gt 0) { throw "$($failures.Count) SVG folders failed: $($failures -join ', ')" }
} finally { Pop-Location }
Complete-QuietRun -Label "SVG tests [$Type]" -LogPath $logPath
