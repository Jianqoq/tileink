//! Execute the shipped scripts with real PowerShell and a native Cargo recorder.
//! Source-string assertions cannot verify argument boundaries, child environment,
//! working directories or failure propagation.
use std::{
    env, fs,
    path::PathBuf,
    process::{Command, Output},
};

struct Runner {
    temp: tempfile::TempDir,
    root: PathBuf,
}

impl Runner {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("workspace with spaces");
        let scripts = root.join("scripts/ps1");
        fs::create_dir_all(&scripts).unwrap();
        for (name, source) in [
            (
                "run_tests.ps1",
                include_str!("../scripts/ps1/run_tests.ps1"),
            ),
            (
                "quiet_runner.ps1",
                include_str!("../scripts/ps1/quiet_runner.ps1"),
            ),
        ] {
            fs::write(scripts.join(name), source).unwrap();
        }
        let bin = temp.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let build = Command::new("rustc")
            .args(["--edition=2024", "-O"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/powershell_runner/cargo.rs"
            ))
            .arg("-o")
            .arg(bin.join(format!("cargo{}", env::consts::EXE_SUFFIX)))
            .output()
            .unwrap();
        assert!(build.status.success(), "{build:?}");
        let shadow = temp.path().join("shadow-bin");
        fs::create_dir(&shadow).unwrap();
        fs::copy(
            bin.join(format!("cargo{}", env::consts::EXE_SUFFIX)),
            shadow.join(format!("cargo{}", env::consts::EXE_SUFFIX)),
        )
        .unwrap();
        Self { temp, root }
    }

    fn run(&self, arguments: &[&str], exit_code: u8) -> Output {
        #[cfg(windows)]
        let powershell = PathBuf::from("powershell.exe");
        #[cfg(not(windows))]
        let powershell = {
            let path = PathBuf::from(
                env::var_os("TILEINK_TEST_POWERSHELL")
                    .expect("set TILEINK_TEST_POWERSHELL to a real PowerShell executable"),
            );
            assert!(path.is_absolute(), "PowerShell path must be absolute");
            path
        };
        let input = self.temp.path().join("arguments.json");
        // A property preserves the array without pipeline enumeration changing
        // its shape between Windows PowerShell and PowerShell 7.
        fs::write(
            &input,
            serde_json::to_vec(&serde_json::json!({ "arguments": arguments })).unwrap(),
        )
        .unwrap();
        let path = env::join_paths(
            [
                self.temp.path().join("bin"),
                self.temp.path().join("shadow-bin"),
            ]
            .into_iter()
            .chain(env::split_paths(&env::var_os("PATH").unwrap())),
        )
        .unwrap();
        Command::new(powershell)
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                r#"
$ErrorActionPreference = 'Stop'
$runnerRequest = Get-Content -LiteralPath $env:TILEINK_RUNNER_ARGS -Raw -Encoding UTF8 | ConvertFrom-Json
& $env:TILEINK_RUNNER_SCRIPT -CargoArgs $runnerRequest.arguments
"#,
            ])
            .current_dir(self.temp.path())
            .env("PATH", path)
            .env("TEMP", self.temp.path())
            .env("TMP", self.temp.path())
            .env("TMPDIR", self.temp.path())
            .env("TILEINK_RUNNER_ARGS", input)
            .env("TILEINK_RUNNER_SCRIPT", self.root.join("scripts/ps1/run_tests.ps1"))
            .env("TILEINK_RUNNER_TRACE", self.temp.path().join("trace"))
            .env("TILEINK_RUNNER_ENV_TRACE", self.temp.path().join("environment"))
            .env("TILEINK_RUNNER_CWD_TRACE", self.temp.path().join("directory"))
            .env("TILEINK_RUNNER_PROGRAM_TRACE", self.temp.path().join("program"))
            .env("TILEINK_RUNNER_EXIT", exit_code.to_string())
            .env("RAYON_NUM_THREADS", "7")
            .output()
            .unwrap()
    }

    fn read(&self, name: &str) -> String {
        fs::read_to_string(self.temp.path().join(name)).unwrap()
    }

    fn assert_arguments(&self, expected: &[&str]) {
        assert_eq!(self.read("trace"), format!("{expected:?}"));
    }
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires real PowerShell via TILEINK_TEST_POWERSHELL"
)]
fn default_command_uses_first_cargo_on_path_and_repository_directory() {
    let runner = Runner::new();
    let output = runner.run(&[], 0);
    assert!(output.status.success(), "{output:?}");
    runner.assert_arguments(&["test", "--release", "--", "--test-threads=1"]);
    assert_eq!(
        fs::canonicalize(runner.read("directory")).unwrap(),
        fs::canonicalize(&runner.root).unwrap()
    );
    assert_eq!(
        fs::canonicalize(runner.read("program")).unwrap(),
        fs::canonicalize(
            runner
                .temp
                .path()
                .join("bin")
                .join(format!("cargo{}", env::consts::EXE_SUFFIX))
        )
        .unwrap()
    );
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires real PowerShell via TILEINK_TEST_POWERSHELL"
)]
fn cargo_and_harness_arguments_keep_their_boundaries_and_contents() {
    let runner = Runner::new();
    for (arguments, expected) in [
        (
            vec!["--lib", "--", "--include-ignored", "--nocapture"],
            vec![
                "test",
                "--release",
                "--lib",
                "--",
                "--include-ignored",
                "--nocapture",
                "--test-threads=1",
            ],
        ),
        (
            vec!["--", "--ignored"],
            vec!["test", "--release", "--", "--ignored", "--test-threads=1"],
        ),
        (
            vec!["--lib", "--"],
            vec!["test", "--release", "--lib", "--", "--test-threads=1"],
        ),
        (
            vec!["--"],
            vec!["test", "--release", "--", "--test-threads=1"],
        ),
        (
            vec!["--no-default-features", "--features", "vulkan"],
            vec![
                "test",
                "--release",
                "--no-default-features",
                "--features",
                "vulkan",
                "--",
                "--test-threads=1",
            ],
        ),
        (
            vec!["spaces and 汉字", "embedded\"quote", "trailing \\", ""],
            vec![
                "test",
                "--release",
                "spaces and 汉字",
                "embedded\"quote",
                "trailing \\",
                "",
                "--",
                "--test-threads=1",
            ],
        ),
    ] {
        let output = runner.run(&arguments, 0);
        assert!(output.status.success(), "{output:?}");
        runner.assert_arguments(&expected);
    }
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires real PowerShell via TILEINK_TEST_POWERSHELL"
)]
fn rayon_is_serial_even_when_the_parent_environment_is_parallel() {
    let runner = Runner::new();
    let output = runner.run(&[], 0);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(runner.read("environment"), "1");
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires real PowerShell via TILEINK_TEST_POWERSHELL"
)]
fn cargo_failure_remains_a_failure_and_displays_its_diagnostic() {
    let runner = Runner::new();
    let output = runner.run(&["--lib"], 37);
    assert!(!output.status.success(), "{output:?}");
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        diagnostic.contains("runner test diagnostic"),
        "{diagnostic}"
    );
    assert!(diagnostic.contains("exit code 37"), "{diagnostic}");
    assert!(diagnostic.contains("Log:"), "{diagnostic}");
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires real PowerShell via TILEINK_TEST_POWERSHELL"
)]
fn overriding_test_threads_is_rejected_before_launching_cargo() {
    for arguments in [vec!["--test-threads=2"], vec!["--", "--test-threads", "2"]] {
        let runner = Runner::new();
        let output = runner.run(&arguments, 0);
        assert!(!output.status.success(), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("Test thread count is fixed at 1")
        );
        assert!(!runner.temp.path().join("trace").exists());
    }
}
