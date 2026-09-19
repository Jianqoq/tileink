//! Native shader build orchestration. Runtime loads embedded compiled artifacts;
//! source compilers run only during builds, with a persistent content cache.
#[path = "native/abi.rs"]
mod abi;
#[path = "native/cache.rs"]
mod cache;
#[path = "native/dxc.rs"]
mod dxc;
#[path = "native/dxil_reflection.rs"]
mod dxil_reflection;
#[path = "native/interfaces.rs"]
mod interfaces;
#[path = "native/program.rs"]
mod program;
#[path = "native/source.rs"]
mod source;
#[path = "native/spirv.rs"]
mod spirv;

use cache::{CacheKey, ShaderCache};
use dxc::{Dxc, digest};
use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

pub fn generate() -> io::Result<()> {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    for variable in [
        "TILEINK_NATIVE_DXC_PATH",
        "TILEINK_DXC_PATH",
        "TILEINK_NATIVE_SHADER_CACHE_DIR",
        "CARGO_TARGET_DIR",
    ] {
        println!("cargo:rerun-if-env-changed={variable}");
    }
    for file in [
        "build/native.rs",
        "build/native/abi.rs",
        "build/native/interfaces.rs",
        "build/native/interfaces/scan.rs",
        "build/native/interfaces/coarse.rs",
        "build/native/interfaces/validation.rs",
        "build/native/interfaces/fine.rs",
        "build/native/interfaces/texture.rs",
        "src/shared/fine_config.rs",
        "build/native/cache.rs",
        "build/native/source.rs",
        "build/native/program.rs",
        "build/native/dxc.rs",
        "build/native/spirv.rs",
        "build/native/dxil_reflection.rs",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(file).display());
    }
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let mut targets = Vec::new();
    if cfg!(feature = "dx12") && os == "windows" {
        targets.push("dxil");
    }
    if cfg!(feature = "vulkan") && (os == "windows" || os == "linux") {
        targets.push("spirv");
    }
    let mut declarations =
        String::from("pub static SHADER_ARTIFACTS: &[NativeShaderArtifact] = &[\n");
    let mut manifest = Vec::new();
    if !targets.is_empty() {
        let path=env::var_os("TILEINK_NATIVE_DXC_PATH").or_else(|| env::var_os("TILEINK_DXC_PATH")).ok_or_else(||io::Error::other(
            "native shaders require an explicit TILEINK_NATIVE_DXC_PATH (or TILEINK_DXC_PATH); ordinary wgpu builds do not"))?;
        let compiler = Dxc::discover(PathBuf::from(path))?;
        let target_triple = env::var("TARGET").unwrap();
        let cache_root = env::var_os("TILEINK_NATIVE_SHADER_CACHE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                env::var_os("CARGO_TARGET_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| root.join("target"))
                    .join("tileink-native-shaders")
            });
        let cache = ShaderCache::new(if cache_root.is_absolute() {
            cache_root
        } else {
            root.join(cache_root)
        });
        for &(family, source) in program::FAMILIES {
            let program::Prepared {
                graph,
                description,
                abi,
            } = program::prepare(&root, family, source)?;
            for &target in &targets {
                for entry in description.entries.keys().map(String::as_str) {
                    let flags = Dxc::flags(target, entry)?;
                    let recipe = serde_json::json!({"schema":1,"language":"hlsl","target":target,"target_triple":target_triple,
                    "entry":entry,"variant":family,"flags":flags,"abi_sha256":digest(&abi),
                    "toolchain":compiler.identity,"sources":graph.files,"cumsum_chunk_size":crate::gpu_constants::get("CUMSUM_CHUNK_SIZE")});
                    let recipe_bytes = serde_json::to_vec(&recipe)?;
                    let key = CacheKey::new(&[&recipe_bytes, graph.expanded.as_bytes()]);
                    let work = out.join("native-work").join(key.hex());
                    let artifact = cache.get_or_compile(&key, || {
                        compiler.compile(&graph.expanded, &flags, &work, target)
                    })?;
                    dxc::validate_container(target, &artifact.bytes)?;
                    if target == "spirv" {
                        spirv::validate(&artifact.bytes, entry, &description)?;
                    } else {
                        let reflection = compiler.reflect_dxil(&artifact.bytes, &work)?;
                        dxil_reflection::validate(&reflection, entry, &description)?;
                    }
                    let output = out.join(format!("native-{entry}.{target}"));
                    write_changed(&output, &artifact.bytes)?;
                    let workgroup = format!("{:?}", description.workgroup);
                    let bindings = if family != "probe" && family != "range-scatter" {
                        abi::binding_declarations(&description, entry)?
                    } else {
                        "&[]".into()
                    };
                    declarations.push_str(&format!("NativeShaderArtifact {{ format: {target:?}, entry: {entry:?}, cache_key: {:?}, workgroup: {workgroup}, bindings: {bindings}, bytes: include_bytes!({:?}) }},\n",key.hex(),output));
                    manifest.push(serde_json::json!({"key":key.hex(),"artifact_sha256":digest(&artifact.bytes),"recipe":recipe}));
                    println!(
                        "cargo:warning=native shader {target}/{entry}: {}",
                        if artifact.hit {
                            "cache hit"
                        } else {
                            "compiled"
                        }
                    );
                }
            }
        }
    }
    declarations.push_str("];\n");
    write_changed(
        &out.join("tileink_native_artifacts.rs"),
        declarations.as_bytes(),
    )?;
    write_changed(
        &out.join("tileink_native_artifacts.json"),
        &serde_json::to_vec_pretty(&manifest)?,
    )
}

fn write_changed(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if fs::read(path).ok().as_deref() != Some(bytes) {
        fs::write(path, bytes)?;
    }
    Ok(())
}
