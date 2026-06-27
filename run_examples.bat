@echo off
set "POWERSHELL_EXE=%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe"
if exist "%POWERSHELL_EXE%" (
    "%POWERSHELL_EXE%" -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\run_examples.ps1"
) else (
    pwsh -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\run_examples.ps1"
)
