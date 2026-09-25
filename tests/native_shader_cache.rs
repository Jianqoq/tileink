#[path = "../build/native/cache.rs"]
mod cache;

use cache::{CacheKey, ShaderCache};
use std::{cell::Cell, fs, io, path::PathBuf};

fn directory(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "tileink-native-cache-{}-{name}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn a_second_cache_instance_reuses_compiled_bytes_without_invoking_compiler() {
    let root = directory("reuse");
    let key = CacheKey::new(&[b"hlsl", b"dxil", b"source", b"toolchain"]);
    let calls = Cell::new(0);
    let compile = || {
        calls.set(calls.get() + 1);
        Ok(b"compiled shader".to_vec())
    };
    let first = ShaderCache::new(root.clone())
        .get_or_compile(&key, compile)
        .unwrap();
    let second = ShaderCache::new(root.clone())
        .get_or_compile(&key, compile)
        .unwrap();
    assert_eq!(calls.get(), 1);
    assert!(!first.hit);
    assert!(second.hit);
    assert_eq!(first.bytes, second.bytes);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn key_frames_parts_and_invalidates_every_compilation_input() {
    let inputs: [&[u8]; 8] = [
        b"hlsl",
        b"spirv",
        b"entry",
        b"variant",
        b"abi",
        b"include",
        b"compiler-sdk",
        b"flags",
    ];
    let initial = CacheKey::new(&inputs);
    for index in 0..inputs.len() {
        let mut changed = inputs;
        changed[index] = b"changed";
        assert_ne!(initial, CacheKey::new(&changed));
    }
    assert_ne!(CacheKey::new(&[b"ab", b"c"]), CacheKey::new(&[b"a", b"bc"]));
    assert_ne!(CacheKey::new(&[b"a"]), CacheKey::new(&[b"a", b""]));
}

#[test]
fn corrupt_or_truncated_entries_recompile_instead_of_returning_bad_shader() {
    let root = directory("corrupt");
    let cache = ShaderCache::new(root.clone());
    let key = CacheKey::new(&[b"shader"]);
    cache.get_or_compile(&key, || Ok(vec![1, 2, 3])).unwrap();
    let path = root.join(format!("{}.shader", key.hex()));
    let original = fs::read(&path).unwrap();
    for data in [vec![], original[..original.len() - 1].to_vec(), {
        let mut wrong = original.clone();
        *wrong.last_mut().unwrap() ^= 1;
        wrong
    }] {
        fs::write(&path, data).unwrap();
        let result = cache.get_or_compile(&key, || Ok(vec![4, 5, 6])).unwrap();
        assert!(!result.hit);
        assert_eq!(result.bytes, [4, 5, 6]);
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_or_empty_compilation_is_never_cached() {
    let root = directory("failure");
    let cache = ShaderCache::new(root.clone());
    let key = CacheKey::new(&[b"broken"]);
    assert!(
        cache
            .get_or_compile(&key, || Err(io::Error::other("compiler failed")))
            .is_err()
    );
    assert!(cache.get_or_compile(&key, || Ok(vec![])).is_err());
    let result = cache.get_or_compile(&key, || Ok(vec![7])).unwrap();
    assert!(!result.hit);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn concurrent_builds_compile_one_artifact_and_observe_complete_bytes() {
    use std::sync::{
        Arc, Barrier,
        atomic::{AtomicUsize, Ordering},
    };
    let root = directory("concurrent");
    let calls = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(4));
    let jobs: Vec<_> = (0..4)
        .map(|_| {
            let root = root.clone();
            let calls = calls.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                ShaderCache::new(root)
                    .get_or_compile(&CacheKey::new(&[b"shared"]), || {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok(vec![42; 4096])
                    })
                    .unwrap()
                    .bytes
            })
        })
        .collect();
    for job in jobs {
        assert_eq!(job.join().unwrap(), vec![42; 4096]);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn target_rejection_rebuilds_the_entry_under_the_same_lock() {
    let root = directory("target-validation");
    let cache = ShaderCache::new(root.clone());
    let key = CacheKey::new(&[b"target-compatible"]);
    cache.get_or_compile(&key, || Ok(vec![1])).unwrap();
    let rebuilt = cache
        .get_or_compile_validated(&key, |bytes| Ok(bytes == [2]), || Ok(vec![2]))
        .unwrap();
    assert!(!rebuilt.hit);
    assert!(
        cache
            .get_or_compile_validated(
                &key,
                |bytes| Ok(bytes == [2]),
                || panic!("compiler called on valid cache")
            )
            .unwrap()
            .hit
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cross_process_cache_worker() {
    let Some(root) = std::env::var_os("TILEINK_CACHE_TEST_WORKER") else {
        return;
    };
    let root = PathBuf::from(root);
    ShaderCache::new(root.clone())
        .get_or_compile(&CacheKey::new(&[b"processes"]), || {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(root.join("compiled-once"))?;
            Ok(vec![9; 8192])
        })
        .unwrap();
}

#[test]
fn separate_processes_compile_the_same_key_only_once() {
    use std::process::{Command, Stdio};
    let root = directory("process-lock");
    let mut children = Vec::new();
    for _ in 0..4 {
        children.push(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "cross_process_cache_worker", "--test-threads=1"])
                .env("TILEINK_CACHE_TEST_WORKER", &root)
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        );
    }
    for mut child in children {
        assert!(child.wait().unwrap().success());
    }
    assert!(root.join("compiled-once").is_file());
    fs::remove_dir_all(root).unwrap();
}
