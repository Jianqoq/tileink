$ErrorActionPreference = "Stop"

function New-QuietRunLog {
    param([Parameter(Mandatory = $true)][string]$Name)

    $safeName = $Name -replace '[^A-Za-z0-9_.-]', '-'
    $stamp = Get-Date -Format "yyyyMMdd-HHmmss-fff"
    $path = Join-Path ([System.IO.Path]::GetTempPath()) "tileink-$safeName-$stamp-$PID.log"
    New-Item -ItemType File -Path $path -Force | Out-Null
    return $path
}

function Add-QuietLog {
    param(
        [Parameter(Mandatory = $true)][string]$LogPath,
        [Parameter(Mandatory = $true)][AllowEmptyString()][string]$Text
    )

    [System.IO.File]::AppendAllText(
        $LogPath,
        $Text,
        [System.Text.UTF8Encoding]::new($false)
    )
}

function ConvertTo-QuietNativeArgument {
    param([Parameter(Mandatory = $true)][AllowEmptyString()][string]$Value)

    if ($Value.Length -gt 0 -and $Value -notmatch '[\s"]') {
        return $Value
    }

    # Apply the CommandLineToArgvW quoting rules so paths/filters containing spaces, quotes, or
    # trailing backslashes reach Cargo and fixture executables byte-for-byte unchanged.
    $quoted = [System.Text.StringBuilder]::new()
    [void]$quoted.Append('"')
    $backslashes = 0
    foreach ($character in $Value.ToCharArray()) {
        if ($character -eq [char]92) {
            $backslashes++
        } elseif ($character -eq [char]34) {
            [void]$quoted.Append('\', $backslashes * 2 + 1)
            [void]$quoted.Append('"')
            $backslashes = 0
        } else {
            [void]$quoted.Append('\', $backslashes)
            [void]$quoted.Append($character)
            $backslashes = 0
        }
    }
    [void]$quoted.Append('\', $backslashes * 2)
    [void]$quoted.Append('"')
    return $quoted.ToString()
}

function Write-QuietProgress {
    param(
        [Parameter(Mandatory = $true)][string]$Label,
        [int]$Step = 0,
        [int]$Total = 0
    )

    if ($Total -gt 0) {
        Write-Host ("[{0}/{1}] {2}" -f $Step, $Total, $Label)
    } else {
        Write-Host ("[..] {0}" -f $Label)
    }
}

function Write-QuietFailure {
    param(
        [Parameter(Mandatory = $true)][string]$Label,
        [Parameter(Mandatory = $true)][string]$LogPath,
        [int]$TailLines = 80
    )

    Write-Host "[failed] $Label"
    Write-Host "Log: $LogPath"
    Write-Host "---- last $TailLines log lines ----"
    Get-Content -LiteralPath $LogPath -Tail $TailLines -ErrorAction SilentlyContinue |
        ForEach-Object { Write-Host $_ }
}

function Invoke-QuietCommand {
    param(
        [Parameter(Mandatory = $true)][string]$Label,
        [Parameter(Mandatory = $true)][string]$FilePath,
        [string[]]$ArgumentList = @(),
        [Parameter(Mandatory = $true)][string]$LogPath,
        [string]$StdoutPath = "",
        [switch]$AllowFailure
    )

    Add-QuietLog -LogPath $LogPath -Text "`n===== $Label =====`n"
    try {
        $command = Get-Command -Name $FilePath -CommandType Application -ErrorAction Stop
        $start = [System.Diagnostics.ProcessStartInfo]::new()
        $start.FileName = $command.Source
        # Push-Location does not change the native process directory. Propagate it
        # explicitly so Cargo and renderers run in the selected repository.
        $start.WorkingDirectory = (Get-Location).ProviderPath
        $start.Arguments = ($ArgumentList | ForEach-Object {
            ConvertTo-QuietNativeArgument -Value $_
        }) -join ' '
        $start.UseShellExecute = $false
        $start.CreateNoWindow = $true
        $start.RedirectStandardOutput = $true
        $start.RedirectStandardError = $true
        $start.StandardOutputEncoding = [System.Text.Encoding]::UTF8
        $start.StandardErrorEncoding = [System.Text.Encoding]::UTF8

        $process = [System.Diagnostics.Process]::new()
        $process.StartInfo = $start
        if (-not $process.Start()) {
            throw "Failed to start $FilePath"
        }
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        $process.WaitForExit()
        $stdoutText = $stdout.Result
        $stderrText = $stderr.Result
        $exitCode = $process.ExitCode
        $process.Dispose()

        if ($StdoutPath) {
            [System.IO.File]::WriteAllText(
                $StdoutPath,
                $stdoutText,
                [System.Text.UTF8Encoding]::new($false)
            )
        } else {
            Add-QuietLog -LogPath $LogPath -Text $stdoutText
        }
        Add-QuietLog -LogPath $LogPath -Text $stderrText
    } catch {
        Add-QuietLog -LogPath $LogPath -Text ($_ | Out-String)
        $exitCode = 1
    }

    if ($exitCode -ne 0) {
        Write-QuietFailure -Label $Label -LogPath $LogPath
        if (-not $AllowFailure) {
            throw "$Label failed with exit code $exitCode"
        }
    }
    return $exitCode
}

function Complete-QuietRun {
    param(
        [Parameter(Mandatory = $true)][string]$Label,
        [Parameter(Mandatory = $true)][string]$LogPath
    )

    Write-Host "[done] $Label"
    Write-Host "Log: $LogPath"
}
