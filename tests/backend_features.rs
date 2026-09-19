//! Compile the production guard under every feature combination. This must stay
//! independent of GPU/driver availability, including for deliberately invalid builds.
use std::{path::PathBuf, process::Command};

#[test]
fn exactly_one_backend_is_accepted_by_the_compiler() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/backend_features.rs");
    let directory =
        std::env::temp_dir().join(format!("tileink-backend-features-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    for mask in 0u32..8 {
        let mut command = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()));
        command
            .arg(&source)
            .args(["--crate-type", "lib", "--emit", "metadata", "--out-dir"])
            .arg(&directory);
        for (index, feature) in ["wgpu", "dx12", "vulkan"].iter().enumerate() {
            if mask & (1 << index) != 0 {
                command.args(["--cfg", &format!("feature=\"{feature}\"")]);
            }
        }
        let output = command.output().unwrap();
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            output.status.success(),
            mask.count_ones() == 1,
            "feature mask {mask}: {diagnostic}"
        );
        if mask == 0 {
            assert!(diagnostic.contains("requires exactly one backend feature"));
        } else if mask.count_ones() > 1 {
            assert!(diagnostic.contains("backend features are mutually exclusive"));
        }
    }
    std::fs::remove_file(directory.join("libbackend_features.rmeta")).unwrap();
    std::fs::remove_dir(directory).unwrap();
}
