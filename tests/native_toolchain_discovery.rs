#[path = "../build/native/toolchain.rs"]
mod toolchain;

use std::{
    fs,
    path::{Path, PathBuf},
};

fn executable(root: &Path) -> PathBuf {
    let bin = if cfg!(target_os = "windows") {
        if cfg!(target_arch = "aarch64") {
            "bin/arm64/dxc.exe"
        } else {
            "bin/x64/dxc.exe"
        }
    } else {
        "bin/dxc"
    };
    root.join("dxc-v1.8.2502").join(bin)
}

#[test]
fn missing_override_uses_the_first_installed_default_cache() {
    let temp = tempfile::tempdir().unwrap();
    let roots = [
        temp.path().join("target cache"),
        temp.path().join("package cache"),
        temp.path().join("user cache"),
    ];
    for root in &roots[1..] {
        let path = executable(root);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"test compiler").unwrap();
    }
    assert_eq!(
        toolchain::resolve(None, &roots).unwrap(),
        executable(&roots[1])
    );
    let path = executable(&roots[0]);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"test compiler").unwrap();
    assert_eq!(toolchain::resolve(None, &roots).unwrap(), path);
}

#[test]
fn explicit_override_is_authoritative_even_when_missing() {
    let temp = tempfile::tempdir().unwrap();
    let explicit = temp.path().join("custom dxc.exe");
    assert_eq!(
        toolchain::resolve(Some(explicit.clone()), &[]).unwrap(),
        explicit
    );
}

#[test]
fn missing_defaults_report_searched_paths_and_configuration() {
    let temp = tempfile::tempdir().unwrap();
    let roots = [temp.path().join("toolchains")];
    let error = toolchain::resolve(None, &roots).unwrap_err().to_string();
    assert!(
        error.contains(&executable(&roots[0]).display().to_string()),
        "{error}"
    );
    assert!(error.contains("TILEINK_NATIVE_DXC_PATH"), "{error}");
}

#[test]
fn cache_layout_uses_cargo_output_and_preserves_precedence() {
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("package");
    let user = temp.path().join("user");
    assert_eq!(
        toolchain::cache_roots(
            &package,
            &package.join("build/release/build/tileink-hash/out"),
            "x86_64-pc-windows-msvc",
            Some(user.clone())
        ),
        vec![
            package.join("build/toolchains"),
            package.join("target/toolchains"),
            user.join("tileink/toolchains")
        ]
    );
    assert_eq!(
        toolchain::cache_roots(
            &package,
            &package.join("target/release/build/tileink-hash/out"),
            "x86_64-pc-windows-msvc",
            None
        ),
        vec![package.join("target/toolchains")]
    );
}

#[test]
fn consumer_output_cache_does_not_resolve_from_the_dependency_root() {
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("tileink");
    let consumer = temp.path().join("gfx_ui/target");
    let compiler = executable(&consumer.join("toolchains"));
    fs::create_dir_all(compiler.parent().unwrap()).unwrap();
    fs::write(&compiler, b"test compiler").unwrap();
    let roots = toolchain::cache_roots(
        &package,
        &consumer.join("release/build/tileink-hash/out"),
        "x86_64-pc-windows-msvc",
        None,
    );
    assert_eq!(toolchain::resolve(None, &roots).unwrap(), compiler);
}

#[test]
fn explicit_target_triple_also_finds_the_shared_target_cache() {
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("tileink");
    let target = temp.path().join("consumer/custom-target");
    let triple = "x86_64-pc-windows-msvc";
    let out = target.join(triple).join("debug/build/tileink-hash/out");
    let roots = toolchain::cache_roots(&package, &out, triple, None);
    assert_eq!(
        roots,
        vec![
            target.join(triple).join("toolchains"),
            target.join("toolchains"),
            package.join("target/toolchains")
        ]
    );
}
