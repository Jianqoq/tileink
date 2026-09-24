//! Apple-only compiler boundary. The input is maintained MSL, never translated
//! WGSL/HLSL. Tool/SDK contents and all flags participate in the artifact recipe.
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

pub struct MetalCompiler {
    metal: PathBuf,
    metallib: PathBuf,
    pub identity: serde_json::Value,
    sdk: PathBuf,
}

// Set Metal math defaults and preserveInvariance for shader parity.
// These are semantic inputs: the SDF cross-process suite detects half-alpha
// changes when offline compilation uses a different arithmetic policy.
pub const FLAGS: &[&str] = &[
    "-std=macos-metal2.4",
    "-mmacosx-version-min=12.0",
    "-ffast-math",
    "-fpreserve-invariance",
    "-frecord-sources",
];

fn run(tool: &Path, args: &[&str]) -> io::Result<String> {
    let output = Command::new(tool).args(args).output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "Metal command {} {args:?} failed: {}{}",
            tool.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .trim()
    .to_owned())
}

pub fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn tree(root: &Path, dir: &Path, files: &mut BTreeMap<String, String>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            tree(root, &path, files)?;
        } else if path.is_file() {
            files.insert(
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                digest(&fs::read(path)?),
            );
        }
    }
    Ok(())
}

impl MetalCompiler {
    pub fn discover(xcrun: PathBuf) -> io::Result<Self> {
        if !xcrun.is_absolute() || !xcrun.is_file() {
            return Err(io::Error::other(
                "Metal compilation requires an absolute existing xcrun executable",
            ));
        }
        let sdk = PathBuf::from(run(&xcrun, &["--sdk", "macosx", "--show-sdk-path"])?);
        let resource = PathBuf::from(run(
            &xcrun,
            &["--sdk", "macosx", "metal", "-print-resource-dir"],
        )?);
        // The resource directory is toolchain/usr/metal/VERSION/lib/clang/VERSION.
        // Hash the actual compiler, linker, dependencies and standard library as
        // a unit, including the files behind Apple's version-selection symlink.
        let toolchain = resource
            .ancestors()
            .nth(3)
            .ok_or_else(|| io::Error::other("invalid Metal resource directory"))?;
        let mut files = BTreeMap::new();
        tree(toolchain, toolchain, &mut files)?;
        let mut tools = BTreeMap::new();
        for name in ["metal", "metallib"] {
            let path = PathBuf::from(run(&xcrun, &["--sdk", "macosx", "--find", name])?);
            tools.insert(
                name,
                serde_json::json!({"path":path,"sha256":digest(&fs::read(&path)?)}),
            );
        }
        let identity = serde_json::json!({
            "compiler_version":run(&xcrun, &["--sdk", "macosx", "metal", "--version"])?,
            "sdk_version":run(&xcrun, &["--sdk", "macosx", "--show-sdk-version"])?,
            "sdk_build":run(&xcrun, &["--sdk", "macosx", "--show-sdk-build-version"])?,
            "sdk_settings_sha256":digest(&fs::read(sdk.join("SDKSettings.json"))?),
            "tools":tools,"toolchain_files":files
        });
        let metal = toolchain.join("bin/metal");
        let metallib = toolchain.join("bin/metallib");
        if !metal.is_file() || !metallib.is_file() {
            return Err(io::Error::other("incomplete Apple Metal toolchain"));
        }
        Ok(Self {
            metal,
            metallib,
            identity,
            sdk,
        })
    }

    pub fn compile(&self, source: &str, work: &Path) -> io::Result<Vec<u8>> {
        fs::create_dir_all(work)?;
        let input = work.join("shader.metal");
        let air = work.join("shader.air");
        let library = work.join("shader.metallib");
        // Never accept a stale output from a failed invocation.
        for path in [&air, &library] {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        fs::write(&input, source)?;
        let mut args = Vec::new();
        args.extend_from_slice(FLAGS);
        args.extend([
            "-isysroot",
            self.sdk
                .to_str()
                .ok_or_else(|| io::Error::other("non-UTF8 SDK path"))?,
            "-c",
            input.to_str().unwrap(),
            "-o",
            air.to_str().unwrap(),
        ]);
        run(&self.metal, &args)?;
        run(
            &self.metallib,
            &[air.to_str().unwrap(), "-o", library.to_str().unwrap()],
        )?;
        let bytes = fs::read(library)?;
        validate_container(&bytes)?;
        Ok(bytes)
    }
}

pub fn validate_container(bytes: &[u8]) -> io::Result<()> {
    if bytes.len() < 16 || !bytes.starts_with(b"MTLB") {
        return Err(io::Error::other("invalid Metal library container"));
    }
    // Function signatures and ABI are additionally checked by Metal reflection
    // when creating the pipeline. A magic number is not an ABI certificate.
    Ok(())
}
