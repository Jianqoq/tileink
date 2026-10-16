use super::{NativeSource, runtime_dxc};
use std::{fs, path::Path, process::Command, rc::Rc};
use tileink::{ShaderBinding, ShaderBindingKind};

const SOURCE: &str = "RWByteAddressBuffer destination : register(u3); [numthreads(256,1,1)] void paint() { destination.Store(0, 1); }";

const fn compiler_library() -> &'static str {
    if cfg!(windows) {
        "dxcompiler.dll"
    } else if cfg!(target_os = "macos") {
        "libdxcompiler.dylib"
    } else {
        "libdxcompiler.so"
    }
}

const fn validator_library() -> &'static str {
    if cfg!(windows) {
        "dxil.dll"
    } else if cfg!(target_os = "macos") {
        "libdxil.dylib"
    } else {
        "libdxil.so"
    }
}

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
    fs::write(
        root.path().join(compiler_library()),
        b"compiler identity fixture",
    )
    .unwrap();
    if cfg!(windows) {
        fs::write(root.path().join("dxil.dll"), b"validator identity fixture").unwrap();
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
            );
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
fn compiler_library_changes_invalidate_runtime_cache_identity() {
    in_worker(
        "compiler_library_changes_invalidate_runtime_cache_identity",
        |root| {
            let source = source(SOURCE);
            let initial = runtime_dxc::compile(&source, "dxil").unwrap();
            fs::write(root.join(compiler_library()), b"updated compiler library").unwrap();
            let updated = runtime_dxc::compile(&source, "dxil").unwrap();
            assert_ne!(initial.key, updated.key);
            assert_eq!(compiles(root), 2);
            fs::remove_file(root.join(compiler_library())).unwrap();
            assert!(runtime_dxc::compile(&source, "dxil").is_err());
            assert_eq!(compiles(root), 2, "missing libraries must fail discovery");
        },
    );
}

#[test]
fn validator_library_changes_and_presence_invalidate_runtime_cache_identity() {
    in_worker(
        "validator_library_changes_and_presence_invalidate_runtime_cache_identity",
        |root| {
            let source = source(SOURCE);
            let library = root.join(validator_library());
            fs::write(&library, b"validator identity fixture").unwrap();
            let initial = runtime_dxc::compile(&source, "dxil").unwrap();
            fs::write(&library, b"updated validator library").unwrap();
            let updated = runtime_dxc::compile(&source, "dxil").unwrap();
            assert_ne!(initial.key, updated.key);
            fs::remove_file(&library).unwrap();
            let absent = runtime_dxc::compile(&source, "dxil").unwrap();
            assert_ne!(initial.key, absent.key);
            assert_ne!(updated.key, absent.key);
            assert_eq!(compiles(root), 3);
            fs::write(&library, b"validator identity fixture").unwrap();
            assert_eq!(
                initial.key,
                runtime_dxc::compile(&source, "dxil").unwrap().key
            );
            assert_eq!(
                compiles(root),
                3,
                "restoring the library must reuse its cache"
            );
        },
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
