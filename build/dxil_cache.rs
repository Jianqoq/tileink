use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use sha2::{Digest, Sha256};

pub(crate) const CACHE_DIRECTORY_ENV: &str = "TILEINK_DXIL_CACHE_DIR";

const MANIFEST_FILE: &str = "manifest.sha256";

pub(crate) fn fingerprint(parts: &[&[u8]], toolchain_inputs: &[PathBuf]) -> io::Result<String> {
    let mut hash = Sha256::new();
    hash_chunk(&mut hash, b"tileink-dxil-cache-v1");
    for part in parts {
        hash_chunk(&mut hash, part);
    }
    for path in toolchain_inputs {
        hash_chunk(
            &mut hash,
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_bytes(),
        );
        hash_file(&mut hash, path)?;
    }
    Ok(hex(hash.finalize().as_slice()))
}

pub(crate) fn restore(
    root: &Path,
    key: &str,
    entry_points: &[&str],
    out_dir: &Path,
) -> io::Result<Option<Vec<(String, PathBuf)>>> {
    let cache_dir = root.join(key);
    let manifest = match fs::read_to_string(cache_dir.join(MANIFEST_FILE)) {
        Ok(manifest) => parse_manifest(&manifest),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let Some(manifest) = manifest else {
        return Ok(None);
    };

    let mut artifacts = Vec::with_capacity(entry_points.len());
    for entry_point in entry_points {
        let name = artifact_name(entry_point);
        let source = cache_dir.join(&name);
        let Some(expected) = manifest.get(&name) else {
            return Ok(None);
        };
        if file_digest(&source).ok().as_deref() != Some(expected) {
            return Ok(None);
        }
        artifacts.push(((*entry_point).to_string(), source, out_dir.join(name)));
    }

    let mut outputs = Vec::with_capacity(artifacts.len());
    for (entry_point, source, output) in artifacts {
        fs::copy(source, &output)?;
        outputs.push((entry_point, output));
    }
    Ok(Some(outputs))
}

pub(crate) fn store(root: &Path, key: &str, outputs: &[(String, PathBuf)]) -> io::Result<()> {
    fs::create_dir_all(root)?;
    let cache_dir = root.join(key);
    let temporary = root.join(format!(
        ".{key}.tmp-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    fs::create_dir(&temporary)?;

    let result = (|| {
        let mut manifest = String::new();
        for (entry_point, source) in outputs {
            let name = artifact_name(entry_point);
            let destination = temporary.join(&name);
            fs::copy(source, &destination)?;
            writeln!(manifest, "{}  {name}", file_digest(&destination)?).unwrap();
        }
        fs::write(temporary.join(MANIFEST_FILE), manifest)?;
        if cache_dir.exists() {
            fs::remove_dir_all(&cache_dir)?;
        }
        fs::rename(&temporary, cache_dir)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(temporary);
    }
    result
}

fn artifact_name(entry_point: &str) -> String {
    format!("tileink-{}-sm60.dxil", entry_point.replace('_', "-"))
}

fn parse_manifest(source: &str) -> Option<BTreeMap<String, String>> {
    source
        .lines()
        .map(|line| {
            let (digest, name) = line.split_once("  ")?;
            (digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
                .then(|| (name.to_string(), digest.to_string()))
        })
        .collect()
}

fn file_digest(path: &Path) -> io::Result<String> {
    let mut hash = Sha256::new();
    hash_file(&mut hash, path)?;
    Ok(hex(hash.finalize().as_slice()))
}

fn hash_file(hash: &mut Sha256, path: &Path) -> io::Result<()> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    hash.update(length.to_le_bytes());
    let mut buffer = [0; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(());
        }
        hash.update(&buffer[..read]);
    }
}

fn hash_chunk(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(output, "{byte:02x}").unwrap();
    }
    output
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::{fingerprint, restore, store};

    #[test]
    fn fingerprint_tracks_shader_and_toolchain_contents() {
        let root = test_directory("fingerprint");
        fs::create_dir_all(&root).unwrap();
        let compiler = root.join("dxc.exe");
        fs::write(&compiler, b"compiler-a").unwrap();

        let initial = fingerprint(&[b"shader-a"], std::slice::from_ref(&compiler)).unwrap();
        let different_shader =
            fingerprint(&[b"shader-b"], std::slice::from_ref(&compiler)).unwrap();
        fs::write(&compiler, b"compiler-b").unwrap();
        let different_compiler =
            fingerprint(&[b"shader-a"], std::slice::from_ref(&compiler)).unwrap();

        assert_ne!(initial, different_shader);
        assert_ne!(initial, different_compiler);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cache_round_trip_restores_verified_artifacts() {
        let root = test_directory("round-trip");
        let source_dir = root.join("source");
        let output_dir = root.join("output");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&output_dir).unwrap();
        let source = source_dir.join("fine.dxil");
        fs::write(&source, b"valid-dxil").unwrap();

        store(&root.join("cache"), "key", &[("fine_main".into(), source)]).unwrap();
        let restored = restore(&root.join("cache"), "key", &["fine_main"], &output_dir)
            .unwrap()
            .unwrap();

        assert_eq!(restored[0].0, "fine_main");
        assert_eq!(fs::read(&restored[0].1).unwrap(), b"valid-dxil");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn corrupted_cache_is_a_miss() {
        let root = test_directory("corrupt");
        let source_dir = root.join("source");
        let output_dir = root.join("output");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&output_dir).unwrap();
        let source = source_dir.join("fine.dxil");
        fs::write(&source, b"valid-dxil").unwrap();
        let cache = root.join("cache");
        store(&cache, "key", &[("fine_main".into(), source)]).unwrap();
        fs::write(cache.join("key/tileink-fine-main-sm60.dxil"), b"corrupt").unwrap();

        assert!(
            restore(&cache, "key", &["fine_main"], &output_dir)
                .unwrap()
                .is_none()
        );
        fs::remove_dir_all(root).unwrap();
    }

    fn test_directory(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tileink-dxil-cache-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
}
