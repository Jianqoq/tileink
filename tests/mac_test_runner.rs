#![cfg(target_os = "macos")]
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::{Command, Output},
};

#[test]
fn mac_scripts_only_reference_existing_cargo_test_targets() {
    let metadata = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--offline",
            "--no-deps",
            "--format-version",
            "1",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(metadata.status.success(), "{:?}", metadata);
    let metadata: serde_json::Value = serde_json::from_slice(&metadata.stdout).unwrap();
    let package = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"] == "tileink")
        .unwrap();
    let targets: Vec<_> = package["targets"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|target| {
            target["kind"]
                .as_array()
                .unwrap()
                .iter()
                .any(|kind| kind == "test")
        })
        .map(|target| target["name"].as_str().unwrap())
        .collect();
    for script in fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/mac")).unwrap() {
        let path = script.unwrap().path();
        if path.extension().is_some_and(|extension| extension == "sh") {
            let source = fs::read_to_string(&path).unwrap();
            for line in source
                .lines()
                .filter(|line| !line.trim_start().starts_with('#'))
            {
                let words: Vec<_> = line.split_whitespace().collect();
                for option in words.windows(2).filter(|words| words[0] == "--test") {
                    assert!(
                        targets.contains(&option[1]),
                        "{} invokes missing test target {}",
                        path.display(),
                        option[1]
                    );
                }
            }
        }
    }
}

fn run(arguments: &[&str], exit_code: u8) -> (Output, Vec<String>, String) {
    let temp = tempfile::tempdir().unwrap();
    let cargo = temp.path().join("cargo");
    fs::write(&cargo, format!(
        "#!/bin/bash\nprintf '%s\\n' \"$@\" > \"$TILEINK_RUNNER_TRACE\"\nprintf '%s' \"$RAYON_NUM_THREADS\" > \"$TILEINK_RUNNER_ENV_TRACE\"\nprintf 'runner test diagnostic\\n' >&2\nexit {exit_code}\n"
    )).unwrap();
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
    let trace = temp.path().join("trace");
    let environment = temp.path().join("environment");
    let output = Command::new("/bin/bash")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/mac/run_tests.sh"
        ))
        .args(arguments)
        .env("PATH", format!("{}:/usr/bin:/bin", temp.path().display()))
        .env("TMPDIR", temp.path())
        .env("TILEINK_RUNNER_TRACE", &trace)
        .env("TILEINK_RUNNER_ENV_TRACE", &environment)
        .env("RAYON_NUM_THREADS", "7")
        .output()
        .unwrap();
    let arguments = fs::read_to_string(trace)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    (output, arguments, fs::read_to_string(environment).unwrap())
}

#[test]
fn stock_macos_bash_runs_release_tests_serially() {
    let (output, arguments, _) = run(&[], 0);
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(
        arguments,
        [
            "test",
            "--release",
            "--no-default-features",
            "--features",
            "metal",
            "--",
            "--test-threads=1"
        ]
    );
}

#[test]
fn release_runner_forwards_cargo_and_harness_arguments_with_one_separator() {
    for (cargo, harness) in [
        (
            vec!["--lib", "native::runtime"],
            vec!["--include-ignored", "--nocapture"],
        ),
        (vec![], vec!["--ignored"]),
        (vec!["filter with spaces"], vec![]),
        (vec![], vec![]),
    ] {
        let mut input = cargo.clone();
        input.push("--");
        input.extend(&harness);
        let (output, actual, _) = run(&input, 0);
        assert!(output.status.success(), "{:?}", output);
        let mut expected = vec![
            "test",
            "--release",
            "--no-default-features",
            "--features",
            "metal",
        ];
        expected.extend(cargo);
        expected.push("--");
        expected.extend(harness);
        expected.push("--test-threads=1");
        assert_eq!(actual, expected);
    }
}

#[test]
fn release_runner_preserves_command_failure_and_its_diagnostic() {
    let (output, _, _) = run(&["--lib"], 37);
    assert_eq!(output.status.code(), Some(37));
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("runner test diagnostic"), "{error}");
    assert!(error.contains("Log:"), "{error}");
}

#[test]
fn release_runner_keeps_rayon_serial_when_the_parent_environment_is_parallel() {
    let (output, _, rayon) = run(&[], 0);
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(rayon, "1");
}
