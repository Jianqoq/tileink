#![cfg(target_os = "macos")]
use std::{fs, os::unix::fs::PermissionsExt, process::Command};

#[test]
fn stock_macos_bash_runs_release_tests_serially() {
    let temp = tempfile::tempdir().unwrap();
    let cargo = temp.path().join("cargo");
    fs::write(
        &cargo,
        "#!/bin/bash\nprintf '%s\\n' \"$*\" >> \"$TILEINK_RUNNER_TRACE\"\n",
    )
    .unwrap();
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
    let trace = temp.path().join("trace");
    let output = Command::new("/bin/bash")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/mac/run_tests.sh"
        ))
        .env("PATH", format!("{}:/usr/bin:/bin", temp.path().display()))
        .env("TMPDIR", temp.path())
        .env("TILEINK_RUNNER_TRACE", &trace)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let calls = fs::read_to_string(&trace).unwrap();
    assert_eq!(calls.lines().count(), 1);
    assert!(
        calls.contains("test --release --no-default-features --features metal -- --test-threads=1")
    );
}

#[test]
fn metal_m5_runner_uses_exclusive_builds_and_validated_serial_execution() {
    let temp = tempfile::tempdir().unwrap();
    let cargo = temp.path().join("cargo");
    fs::write(
        &cargo,
        "#!/bin/bash\nprintf '%s|%s\\n' \"$MTL_DEBUG_LAYER\" \"$*\" >> \"$TILEINK_RUNNER_TRACE\"\n",
    )
    .unwrap();
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
    let trace = temp.path().join("trace");
    for mode in ["--retained", "--present"] {
        fs::write(&trace, "").unwrap();
        let output = Command::new("/bin/bash")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/scripts/mac/run_native_metal_tests.sh"
            ))
            .arg(mode)
            .env("PATH", format!("{}:/usr/bin:/bin", temp.path().display()))
            .env("TILEINK_RUNNER_TRACE", &trace)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let calls = fs::read_to_string(&trace).unwrap();
        assert_eq!(calls.lines().count(), 2);
        for call in calls.lines() {
            assert!(call.starts_with("1|"));
            assert!(call.contains("--release"));
            if call.contains("|test ") {
                assert_eq!(call.matches("--test-threads=1").count(), 1);
            }
        }
        assert!(
            calls
                .lines()
                .last()
                .unwrap()
                .contains("--no-default-features --features metal")
        );
        if mode == "--retained" {
            assert!(!calls.lines().next().unwrap().contains("--features"));
        } else {
            assert!(calls.contains("-- metal --smoke"));
        }
    }
}
