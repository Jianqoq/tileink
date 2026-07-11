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
            "{} must not stream cargo output directly to the terminal",
            path.display()
        );
    }

    let runner = fs::read_to_string(scripts.join("quiet_runner.ps1")).unwrap();
    assert!(runner.contains("RedirectStandardOutput = $true"));
    assert!(runner.contains("UTF8Encoding"));
    assert!(runner.contains("Write-QuietFailure"));
    assert!(runner.contains("Get-Content -LiteralPath $LogPath -Tail"));
}
