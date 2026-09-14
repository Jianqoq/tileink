//! Build-time evidence embedded into the parity executable, including cache hits.
//! This identifies the actual compiler and bytecode; runtime DXC metadata cannot
//! certify shaders compiled earlier by a different executable or toolchain.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
};

pub(crate) const DXC_FLAGS: &[&str] =
    &["-T", "cs_6_0", "-HV", "2018", "-no-warnings", "-Ges", "-O3"];

fn digest_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let length = file.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        hash.update(&buffer[..length]);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub(crate) fn manifest(
    fingerprint: &str,
    version: &str,
    toolchain: &[PathBuf],
    outputs: &[(String, PathBuf)],
) -> io::Result<Value> {
    let tools = toolchain
        .iter()
        .map(|path| {
            Ok(json!({
                "path": path, "sha256": digest_file(path)?,
            }))
        })
        .collect::<io::Result<Vec<_>>>()?;
    let artifacts = outputs
        .iter()
        .map(|(entry, path)| {
            let mut arguments = vec!["-E", entry];
            arguments.extend_from_slice(DXC_FLAGS);
            arguments.extend_from_slice(&["-Fo", "<artifact>", "<generated_hlsl>"]);
            Ok(json!({"entry_point": entry, "file": path.file_name().map(|name| name.to_string_lossy()),
            "sha256": digest_file(path)?, "arguments": arguments}))
        })
        .collect::<io::Result<Vec<_>>>()?;
    Ok(
        json!({"schema": 1, "input_fingerprint": fingerprint, "compiler_version": version,
        "toolchain": tools, "artifacts": artifacts,
        "source_language": "WGSL translated to HLSL by Naga (existing WGPU fine path)"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provenance_tracks_actual_tools_and_artifacts_and_rejects_missing_files() {
        let root = std::env::temp_dir().join(format!(
            "tileink-dxil-provenance-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let compiler = root.join("dxc.exe");
        let blob = root.join("fine.dxil");
        std::fs::write(&compiler, b"compiler-a").unwrap();
        std::fs::write(&blob, b"shader-a").unwrap();
        let tools = [compiler.clone()];
        let outputs = [("fine_tile_main".to_owned(), blob.clone())];
        let before = manifest("key", "version", &tools, &outputs).unwrap();
        std::fs::write(&compiler, b"compiler-b").unwrap();
        std::fs::write(&blob, b"shader-b").unwrap();
        let after = manifest("key", "version", &tools, &outputs).unwrap();
        assert_ne!(
            before["toolchain"][0]["sha256"],
            after["toolchain"][0]["sha256"]
        );
        assert_ne!(
            before["artifacts"][0]["sha256"],
            after["artifacts"][0]["sha256"]
        );
        assert_eq!(after["artifacts"][0]["arguments"][1], "fine_tile_main");
        assert_eq!(after["artifacts"][0]["arguments"][3], "cs_6_0");
        std::fs::remove_file(&blob).unwrap();
        assert!(manifest("key", "version", &tools, &outputs).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
