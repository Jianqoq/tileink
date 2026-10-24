#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "Test setup and assertions abort on failure; production builds are checked separately"
    )
)]
use std::{path::Path, process::Command};

#[test]
fn unsupported_native_present_exits_with_an_error() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join(if cfg!(windows) {
        "native_present.exe"
    } else {
        "native_present"
    });
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // Compile the real CLI with no backend features. This path must fail at
    // runtime on every host, including when the surrounding test uses Metal.
    let compile = Command::new("rustc")
        .args(["--edition=2024", "-O"])
        .arg(root.join("examples/native_window/main.rs"))
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let output = Command::new(executable).output().unwrap();
    assert!(
        !output.status.success(),
        "unsupported backend must not report a completed run"
    );
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("This example requires")
    );
}
