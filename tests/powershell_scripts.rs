#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "Test setup and assertions abort on failure; production builds are checked separately"
    )
)]
#[cfg(windows)]
use std::{fs, path::Path};

#[cfg(windows)]
#[test]
fn quiet_commands_follow_powershell_location() {
    use std::{os::windows::process::CommandExt, process::Command};
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", "
$ErrorActionPreference = 'Stop'
. (Join-Path $env:TILEINK_TEST_REPO_DIR 'scripts/ps1/quiet_runner.ps1')
$log = New-QuietRunLog -Name 'working-directory-regression'
Push-Location (Join-Path $env:TILEINK_TEST_REPO_DIR 'scripts/ps1')
try {
    [void](Invoke-QuietCommand -Label 'pwd' -FilePath $env:ComSpec -ArgumentList @('/d','/c','cd') -LogPath $log)
    (Get-Content -LiteralPath $log | Where-Object { $_.Trim() } | Select-Object -Last 1).Trim()
} finally { Pop-Location; Remove-Item -LiteralPath $log }
"])
        .env("TILEINK_TEST_REPO_DIR", env!("CARGO_MANIFEST_DIR"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .creation_flags(0x0800_0000)
        .output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let actual = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        fs::canonicalize(actual.trim()).unwrap(),
        fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/ps1")).unwrap()
    );
}
