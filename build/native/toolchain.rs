//! Native compiler selection; explicit overrides never silently fall back.
use std::{
    io,
    path::{Path, PathBuf},
};

pub fn resolve(explicit: Option<PathBuf>, cache_roots: &[PathBuf]) -> io::Result<PathBuf> {
    if let Some(path) = explicit {
        // A typo in an explicit override must still fail in Dxc::discover.
        return Ok(path);
    }
    let bin = if cfg!(target_os = "windows") {
        if cfg!(target_arch = "aarch64") {
            "bin/arm64/dxc.exe"
        } else {
            "bin/x64/dxc.exe"
        }
    } else {
        "bin/dxc"
    };
    let candidates: Vec<_> = cache_roots
        .iter()
        .map(|root| root.join("dxc-v1.8.2502").join(bin))
        .collect();
    candidates.iter().find(|path| path.is_file()).cloned().ok_or_else(|| {
        io::Error::other(format!(
            "native DXC not found; set TILEINK_NATIVE_DXC_PATH (or TILEINK_DXC_PATH) to an absolute compiler path, or install DXC v1.8.2502 in a default cache. Searched: {}",
            candidates.iter().map(|path| path.display().to_string()).collect::<Vec<_>>().join(", ")
        ))
    })
}

pub fn cache_roots(
    package: &Path,
    out: &Path,
    target_triple: &str,
    user_cache: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    // Cargo resolves relative target-dir settings before producing OUT_DIR:
    // <target>[/<triple>]/<profile>/build/<package-hash>/out. A dependency's
    // current directory is not the consumer's invocation directory.
    if let Some(target) = out.ancestors().nth(4) {
        roots.push(target.join("toolchains"));
        if target.file_name().is_some_and(|name| name == target_triple)
            && let Some(parent) = target.parent()
        {
            roots.push(parent.join("toolchains"));
        }
    }
    roots.push(package.join("target/toolchains"));
    if let Some(cache) = user_cache {
        roots.push(cache.join("tileink/toolchains"));
    }
    roots.dedup();
    roots
}
