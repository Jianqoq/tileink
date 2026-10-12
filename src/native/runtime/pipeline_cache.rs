//! Opaque driver pipeline data is distinct from portable DXIL/SPIR-V artifacts.
//! Driver/API/device identity and the shader+layout contract are part of its key.
use super::shader_cache::{CacheKey, ShaderCache};
use std::{io, path::PathBuf};

pub fn load_or_create_for_layout(
    identity: &[u8],
    shader_key: &str,
    layout: &[u8],
    accept: impl FnOnce(&[u8]) -> io::Result<bool>,
    create: impl FnOnce() -> io::Result<Vec<u8>>,
) -> io::Result<bool> {
    let root = std::env::var_os("TILEINK_NATIVE_PIPELINE_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/native-pipelines")
        });
    let key = CacheKey::new(&[layout, identity, shader_key.as_bytes()]);
    let result = ShaderCache::new(root).get_or_compile(&key, accept, create)?;
    Ok(result.hit)
}
