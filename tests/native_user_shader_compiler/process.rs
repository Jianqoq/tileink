use super::{NativeSource, runtime_dxc};
use std::{fs, path::Path, process::Command, rc::Rc};
use tileink::{ShaderBinding, ShaderBindingKind};

const SOURCE: &str = "RWByteAddressBuffer destination : register(u3); [numthreads(256,1,1)] void paint() { destination.Store(0, 1); }";

fn in_worker(name: &str, run: impl FnOnce(&Path)) {
    if let Some(root) = std::env::var_os("TILEINK_DXC_PROCESS_TEST_ROOT") {
        run(Path::new(&root));
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let compiler = root
        .path()
        .join(if cfg!(windows) { "dxc.exe" } else { "dxc" });
    let output = Command::new("rustc")
        .args(["--edition=2024", "-O"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/dxc_process.rs"))
        .arg("-o")
        .arg(&compiler)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for name in if cfg!(windows) {
        &["dxcompiler.dll", "dxil.dll"][..]
    } else {
        &["libdxcompiler.so"][..]
    } {
        fs::write(root.path().join(name), b"compiler identity fixture").unwrap();
    }
    fs::write(root.path().join("mode"), "valid").unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &format!("process::{name}"), "--test-threads=1"])
        .env("TILEINK_DXC_PROCESS_TEST_ROOT", root.path())
        .env("TILEINK_NATIVE_DXC_PATH", compiler)
        .env("TILEINK_NATIVE_SHADER_CACHE_DIR", root.path().join("cache"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn source(text: &str) -> NativeSource {
    NativeSource {
        entry: "paint".into(),
        workgroup: [256, 1, 1],
        hlsl: Some(text.into()),
        interface_bindings: Rc::from([ShaderBinding {
            name: "destination".into(),
            slot: 3,
            kind: ShaderBindingKind::ReadWriteBuffer,
        }]),
    }
}
fn compiles(root: &Path) -> usize {
    fs::read_to_string(root.join("calls"))
        .unwrap()
        .lines()
        .filter(|&line| line == "compile")
        .count()
}
fn cached_error(root: &Path, mode: &str, expected: &str) {
    let source = source(SOURCE);
    let first = runtime_dxc::compile(&source, "dxil").unwrap();
    fs::write(root.join("mode"), mode).unwrap();
    let error = runtime_dxc::compile(&source, "dxil")
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains(expected), "{error}");
    assert_eq!(
        compiles(root),
        1,
        "cached validation failure must not invoke compilation"
    );
    fs::write(root.join("mode"), "valid").unwrap();
    let reused = runtime_dxc::compile(&source, "dxil").unwrap();
    assert_eq!(first.key, reused.key);
    assert_eq!(first.bytes, reused.bytes);
    assert_eq!(compiles(root), 1);
}

#[test]
fn cached_reflection_process_errors_are_returned_without_recompiling() {
    in_worker(
        "cached_reflection_process_errors_are_returned_without_recompiling",
        |root| {
            cached_error(
                root,
                "reflection-failure",
                "deliberate reflection process failure",
            )
        },
    );
}
#[test]
fn cached_reflection_encoding_errors_are_returned_without_recompiling() {
    in_worker(
        "cached_reflection_encoding_errors_are_returned_without_recompiling",
        |root| cached_error(root, "invalid-encoding", "DXIL reflection encoding"),
    );
}
#[test]
fn cached_abi_errors_are_returned_without_recompiling() {
    in_worker(
        "cached_abi_errors_are_returned_without_recompiling",
        |root| cached_error(root, "wrong-abi", "DXIL workgroup"),
    );
}
#[test]
fn runtime_cache_reuses_source_identity_recovers_truncation_and_never_caches_failures() {
    in_worker(
        "runtime_cache_reuses_source_identity_recovers_truncation_and_never_caches_failures",
        |root| {
            let initial = source(SOURCE);
            fs::write(root.join("mode"), "compile-failure").unwrap();
            assert!(
                runtime_dxc::compile(&initial, "dxil")
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("deliberate compile process failure")
            );
            assert!(!root.join("cache").read_dir().unwrap().any(|entry| {
                entry
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|ext| ext == "shader")
            }));
            fs::write(root.join("mode"), "valid").unwrap();
            let first = runtime_dxc::compile(&initial, "dxil").unwrap();
            let cached = runtime_dxc::compile(&initial, "dxil").unwrap();
            assert_eq!(first.key, cached.key);
            assert_eq!(first.bytes, cached.bytes);
            assert_eq!(compiles(root), 2);
            fs::write(
                root.join("cache").join(format!("{}.shader", first.key)),
                b"truncated",
            )
            .unwrap();
            runtime_dxc::compile(&initial, "dxil").unwrap();
            assert_eq!(compiles(root), 3);
            let changed =
                runtime_dxc::compile(&source(&format!("{SOURCE}\n// changed")), "dxil").unwrap();
            assert_ne!(first.key, changed.key);
            assert_eq!(compiles(root), 4);
        },
    );
}
