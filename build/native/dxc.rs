//! Explicit DXC toolchain and target-specific recipes. No downloads or fallback.
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

pub struct Dxc {
    executable: PathBuf,
    pub identity: serde_json::Value,
}

impl Dxc {
    pub fn discover(path: PathBuf) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(io::Error::other("TILEINK_DXC_PATH must be absolute"));
        }
        let executable = path.canonicalize()?;
        let version = run(Command::new(&executable).arg("--version"), "DXC version")?;
        let mut files = BTreeMap::new();
        let parent = executable.parent().unwrap();
        let mut inputs = vec![executable.clone()];
        if cfg!(windows) {
            inputs.push(parent.join("dxcompiler.dll"));
            inputs.push(parent.join("dxil.dll"));
        } else {
            // Linux packages may use a sibling lib directory. Require a known
            // library location rather than silently omitting compiler identity.
            let adjacent = parent.join("libdxcompiler.so");
            inputs.push(if adjacent.is_file() {
                adjacent
            } else {
                parent.join("../lib/libdxcompiler.so").canonicalize()?
            });
        }
        for input in inputs {
            println!("cargo:rerun-if-changed={}", input.display());
            let hash = match fs::read(&input) {
                Ok(bytes) => digest(&bytes),
                Err(error)
                    if error.kind() == io::ErrorKind::NotFound
                        && input.file_name().is_some_and(|name| name == "dxil.dll") =>
                {
                    "absent".to_owned()
                }
                Err(error) => {
                    return Err(io::Error::other(format!(
                        "toolchain input {}: {error}",
                        input.display()
                    )));
                }
            };
            files.insert(input.to_string_lossy().into_owned(), hash);
        }
        let identity =
            serde_json::json!({"version":String::from_utf8_lossy(&version),"files":files});
        Ok(Self {
            executable,
            identity,
        })
    }

    pub fn reflect_dxil(&self, bytes: &[u8], work: &Path) -> io::Result<String> {
        fs::create_dir_all(work)?;
        let path = work.join("validate.dxil");
        fs::write(&path, bytes)?;
        let text = run(
            Command::new(&self.executable).arg("-dumpbin").arg(&path),
            "DXIL reflection",
        )?;
        String::from_utf8(text)
            .map_err(|e| io::Error::other(format!("DXIL reflection encoding: {e}")))
    }

    pub fn flags(target: &str, entry: &str) -> io::Result<Vec<String>> {
        let mut args = vec![
            "-T", "cs_6_0", "-E", entry, "-HV", "2021", "-Ges", "-WX", "-O3", "-Gis",
        ];
        match target {
            "dxil" => (),
            "spirv" => args.extend(["-spirv", "-fspv-target-env=vulkan1.1"]),
            _ => return Err(io::Error::other("unknown DXC shader target")),
        }
        Ok(args.into_iter().map(str::to_owned).collect())
    }

    pub fn compile(
        &self,
        source: &str,
        flags: &[String],
        work: &Path,
        target: &str,
    ) -> io::Result<Vec<u8>> {
        fs::create_dir_all(work)?;
        let input = work.join("expanded.hlsl");
        let output = work.join(format!("shader.{target}"));
        fs::write(&input, source)?;
        run(
            Command::new(&self.executable)
                .args(flags)
                .arg(&input)
                .arg("-Fo")
                .arg(&output),
            &format!("native shader {target}, flags {flags:?}"),
        )?;
        let bytes = fs::read(&output)?;
        validate_container(target, &bytes)?;
        Ok(bytes)
    }
}

pub fn validate_container(target: &str, bytes: &[u8]) -> io::Result<()> {
    let valid = match target {
        "dxil" => {
            bytes.len() >= 32
                && &bytes[..4] == b"DXBC"
                && u32::from_le_bytes(bytes[24..28].try_into().unwrap()) as usize == bytes.len()
        }
        "spirv" => {
            bytes.len() >= 20
                && bytes.len().is_multiple_of(4)
                && bytes[..4] == [3, 2, 35, 7]
                && bytes[16..20] == [0; 4]
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "invalid {target} shader container"
        )))
    }
}

pub fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn run(command: &mut Command, context: &str) -> io::Result<Vec<u8>> {
    let result = command
        .output()
        .map_err(|e| io::Error::other(format!("{context}: {e}")))?;
    if !result.status.success() {
        return Err(io::Error::other(format!(
            "{context}: {}\n{}\n{}",
            result.status,
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        )));
    }
    let mut output = result.stdout;
    output.extend(result.stderr);
    Ok(output)
}
