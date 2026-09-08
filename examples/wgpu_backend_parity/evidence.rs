use super::{Result, options::Options};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};

pub fn digest_bytes(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn digest_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let length = file.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        digest.update(&buffer[..length]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn git(args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(output.stdout)
}

/// Include dirty and newly added sources as well as checked-in fonts, images and SVGs.
/// A commit alone does not identify the executable or the working tree being tested.
pub fn source_snapshot(additional_inputs: &[PathBuf], output: &Path) -> Result<Value> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let listed = String::from_utf8(git(&[
        "ls-files",
        "-z",
        "--cached",
        "--others",
        "--exclude-standard",
    ])?)?;
    let mut paths: BTreeSet<PathBuf> = listed
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(|path| root.join(path))
        .collect();
    paths.extend(additional_inputs.iter().cloned());
    let output = std::path::absolute(output)?;
    let mut files = Vec::new();
    for path in paths {
        if std::path::absolute(&path)?.starts_with(&output) {
            continue;
        }
        // Deleted tracked files are part of a dirty source snapshot, too.
        let hash = if path.is_file() {
            Some(digest_file(&path)?)
        } else {
            None
        };
        files.push(json!({"path": path.strip_prefix(root).unwrap_or(&path), "sha256": hash}));
    }
    Ok(json!(files))
}

pub fn resource_snapshot(paths: &[PathBuf]) -> Result<Value> {
    let mut resources = Vec::new();
    for path in paths {
        resources.push(json!({"path": path, "sha256": if path.is_file() { Some(digest_file(path)?) } else { None }}));
    }
    Ok(json!(resources))
}

pub fn create_manifest(
    options: &Options,
    cases: &[String],
    inputs: &[PathBuf],
    corpus: &super::svg::Corpus,
) -> Result<Value> {
    let binary = std::env::current_exe()?;
    let compiler = match &options.dxc {
        Some(path) => {
            let dxil = path.with_file_name("dxil.dll");
            json!({"requested": "DynamicDxc", "path": path, "sha256": digest_file(path)?,
                "adjacent_dxil_sha256": if dxil.is_file() { Some(digest_file(&dxil)?) } else { None }})
        }
        None => {
            json!({"requested": "Auto", "actual_compiler": "not certified; provide --dxc for a reproducible DX12 reference"})
        }
    };
    let manifest = json!({
        "schema": 1, "source_commit": String::from_utf8(git(&["rev-parse", "HEAD"])?)?.trim(),
        "source_status": String::from_utf8(git(&["status", "--porcelain"])?)?,
        "binary": {"path": binary, "sha256": digest_file(&binary)?},
        "compiler": compiler, "instance_flags": format!("{:?}", wgpu::InstanceFlags::default()),
        "cases": cases.iter().enumerate().map(|(index, id)| json!({"id": id, "svg_source": inputs.get(index)})).collect::<Vec<_>>(), "expected_frames": cases.len(),
        "texture_modes": options.textures.iter().map(|portable| if *portable { "portable" } else { "native" }).collect::<Vec<_>>(),
        "sources_and_resources": source_snapshot(&corpus.resources, &options.output)?,
        "runtime_resources": corpus.snapshot,
        "format": "Rgba8Unorm premultiplied RGBA, all four raw channels, row padding excluded",
    });
    write_new_json(&options.output.join("manifest.json"), &manifest)?;
    Ok(manifest)
}

pub fn write_new_json(path: &Path, value: &Value) -> Result<()> {
    let mut file = File::options().write(true).create_new(true).open(path)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    Ok(())
}
