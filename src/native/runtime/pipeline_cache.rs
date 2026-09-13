//! Opaque driver pipeline data is distinct from portable DXIL/SPIR-V artifacts.
//! Driver/API/device identity and the shader+layout contract are part of its key.
#[allow(dead_code)] // Reuse the build cache's framing/locking; this consumer validates driver blobs.
#[path = "../../../build/native/cache.rs"]
mod storage;
use std::{io, path::PathBuf};
use storage::{CacheKey, ShaderCache};

pub fn load_or_create(
    identity: &[u8],
    shader_key: &str,
    accept: impl FnOnce(&[u8]) -> io::Result<bool>,
    create: impl FnOnce() -> io::Result<Vec<u8>>,
) -> io::Result<bool> {
    let root = std::env::var_os("TILEINK_NATIVE_PIPELINE_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/native-probe-pipelines")
        });
    let key = CacheKey::new(&[
        b"native-probe-pipeline-layout-v2-texture",
        identity,
        shader_key.as_bytes(),
    ]);
    let result = ShaderCache::new(root).get_or_compile_validated(&key, accept, create)?;
    Ok(result.hit)
}
