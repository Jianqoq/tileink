use std::{fs, path::Path};

#[test]
fn powershell_entrypoints_route_external_output_through_quiet_runner() {
    let scripts = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/ps1");
    for entry in fs::read_dir(&scripts).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("ps1")
            || path.file_name().unwrap() == "quiet_runner.ps1"
        {
            continue;
        }
        let source = fs::read_to_string(&path).unwrap();
        let delegates_to_svg_runner = source.lines().all(|line| {
            line.trim().is_empty()
                || line.contains("run_svg_tests.ps1")
                || line.trim_start().starts_with('#')
        });
        assert!(
            source.contains("quiet_runner.ps1") || delegates_to_svg_runner,
            "{} must use the shared quiet command runner",
            path.display()
        );
        assert!(
            !source.lines().any(|line| {
                let line = line.trim_start();
                line.starts_with("cargo ") || line.starts_with("& cargo ")
            }),
            "{} must not stream cargo output directly",
            path.display()
        );
    }
    let runner = fs::read_to_string(scripts.join("quiet_runner.ps1")).unwrap();
    assert!(runner.contains("RedirectStandardOutput = $true"));
    assert!(runner.contains("UTF8Encoding"));
}

#[test]
fn release_runner_is_serial_and_uses_native_feature() {
    let windows = include_str!("../scripts/ps1/run_tests.ps1");
    let mac = include_str!("../scripts/mac/run_tests.sh");
    assert!(windows.contains("--release") && windows.contains("--test-threads=1"));
    assert!(mac.contains("--features metal") && mac.contains("--test-threads=1"));
}

#[cfg(windows)]
#[test]
fn quiet_commands_follow_powershell_location() {
    use std::{os::windows::process::CommandExt, process::Command};
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", r#"
$ErrorActionPreference = 'Stop'
. (Join-Path $env:TILEINK_TEST_REPO_DIR 'scripts/ps1/quiet_runner.ps1')
$log = New-QuietRunLog -Name 'working-directory-regression'
Push-Location (Join-Path $env:TILEINK_TEST_REPO_DIR 'scripts/ps1')
try {
    [void](Invoke-QuietCommand -Label 'pwd' -FilePath $env:ComSpec -ArgumentList @('/d','/c','cd') -LogPath $log)
    (Get-Content -LiteralPath $log | Where-Object { $_.Trim() } | Select-Object -Last 1).Trim()
} finally { Pop-Location; Remove-Item -LiteralPath $log }
"#])
        .env("TILEINK_TEST_REPO_DIR", env!("CARGO_MANIFEST_DIR"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .creation_flags(0x0800_0000)
        .output().unwrap();
    assert!(output.status.success(), "{:?}", output);
    let actual = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        fs::canonicalize(actual.trim()).unwrap(),
        fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/ps1")).unwrap()
    );
}
