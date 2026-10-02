//! Persistent compiled shader bytes, independent of API-specific compilation.
//!
//! A per-key OS file lock covers read/compile/publication. A crashed writer can
//! leave a partial file, but readers verify framing and the complete digest and
//! rebuild it. This prevents both duplicate concurrent compiles and reuse of a
//! partially written shader; it does not cache failures or driver pipelines.

use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::PathBuf,
};

const MAGIC: &[u8; 16] = b"TILEINKSHADER-01";
const HEADER_SIZE: usize = 16 + 32 + 32 + 8;
const MAX_ARTIFACT_SIZE: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CacheKey([u8; 32]);

impl CacheKey {
    /// Inputs must include language, target, entry/variant, ABI, complete source
    /// graph, compiler/dependencies/SDK identity and every compiler option.
    pub fn new(parts: &[&[u8]]) -> Self {
        let mut hash = Sha256::new();
        hash.update(MAGIC);
        for part in parts {
            hash.update((part.len() as u64).to_le_bytes());
            hash.update(part);
        }
        Self(hash.finalize().into())
    }

    pub fn hex(&self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

pub struct ShaderCache {
    root: PathBuf,
}

pub struct CachedShader {
    pub bytes: Vec<u8>,
    pub hit: bool,
}

impl ShaderCache {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn get_or_compile(
        &self,
        key: &CacheKey,
        compile: impl FnOnce() -> io::Result<Vec<u8>>,
    ) -> io::Result<CachedShader> {
        self.get_or_compile_validated(key, |_| Ok(true), compile)
    }

    pub fn get_or_compile_validated(
        &self,
        key: &CacheKey,
        accept: impl FnOnce(&[u8]) -> io::Result<bool>,
        compile: impl FnOnce() -> io::Result<Vec<u8>>,
    ) -> io::Result<CachedShader> {
        fs::create_dir_all(&self.root)?;
        let stem = key.hex();
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join(format!("{stem}.lock")))?;
        lock.lock()?;
        let path = self.root.join(format!("{stem}.shader"));
        if let Some(bytes) = read_verified(&path, key)?
            && accept(&bytes)?
        {
            return Ok(CachedShader { bytes, hit: true });
        }
        let bytes = compile()?;
        if bytes.is_empty() || bytes.len() as u64 > MAX_ARTIFACT_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid compiled shader size",
            ));
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        file.write_all(MAGIC)?;
        file.write_all(&key.0)?;
        file.write_all(&Sha256::digest(&bytes))?;
        file.write_all(&(bytes.len() as u64).to_le_bytes())?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        Ok(CachedShader { bytes, hit: false })
    }
}

fn read_verified(path: &std::path::Path, key: &CacheKey) -> io::Result<Option<Vec<u8>>> {
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let size = file.metadata()?.len();
    if size <= HEADER_SIZE as u64 || size > HEADER_SIZE as u64 + MAX_ARTIFACT_SIZE {
        return Ok(None);
    }
    let mut header = [0; HEADER_SIZE];
    if let Err(error) = file.read_exact(&mut header) {
        return if error.kind() == io::ErrorKind::UnexpectedEof {
            Ok(None)
        } else {
            Err(error)
        };
    }
    if &header[..16] != MAGIC || header[16..48] != key.0 {
        return Ok(None);
    }
    let length = u64::from_le_bytes(header[80..88].try_into().unwrap());
    if length != size - HEADER_SIZE as u64 {
        return Ok(None);
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(length + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != length || Sha256::digest(&bytes)[..] != header[48..80] {
        return Ok(None);
    }
    Ok(Some(bytes))
}
