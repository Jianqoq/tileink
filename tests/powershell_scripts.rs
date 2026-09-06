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

#[test]
fn release_test_partitions_cover_nested_modules_once() {
    let source = include_str!("../scripts/ps1/run_tests.ps1");
    let argument = |text: &str, key: &str| {
        text.split_once(&format!("{key} \""))
            .map(|(_, rest)| rest.split('"').next().unwrap().to_owned())
    };
    let partitions: Vec<_> = source
        .split("Invoke-CargoTest -Mode $Mode -Label ")
        .skip(1)
        .filter(|call| !call.starts_with("\"focused\""))
        .map(|call| {
            let filter = argument(call, "-Filter").unwrap_or_default();
            let skips = call
                .split_once("-HarnessArgs @(")
                .map(|(_, rest)| {
                    rest.split(')')
                        .next()
                        .unwrap()
                        .split('"')
                        .skip(1)
                        .step_by(2)
                        .filter(|value| *value != "--skip")
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            (filter, skips)
        })
        .collect();
    assert_eq!(partitions.len(), 6);
    // Submodules previously made renderer partitions run zero tests; broadening filters
    // must also exclude those function groups from core/SVG to avoid executing them twice.
    for module in [
        "canvas::tests::",
        "svg::tests::",
        "wgpu::renderer::tests::",
        "wgpu::renderer::tests::group::nested::",
    ] {
        for test in [
            "draws_scene",
            "persistent_target_history",
            "wgpu_renderer_draws_scene",
            "wgpu_renderer_samples_linear",
            "persistent_wgpu_renderer_samples_history",
        ] {
            let name = format!("{module}{test}");
            let matches = partitions
                .iter()
                .filter(|(filter, skips)| {
                    name.contains(filter) && !skips.iter().any(|skip| name.contains(skip))
                })
                .count();
            assert_eq!(matches, 1, "release partition count for {name}");
        }
    }
}
