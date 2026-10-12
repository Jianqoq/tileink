//! The same validated DXC toolchain and content cache used at build time.
//! Application source is compiled verbatim; compiler errors are returned.
#[allow(dead_code)]
#[path = "../../../../build/native/abi.rs"]
mod abi;
#[allow(dead_code)]
#[path = "../../../../build/native/dxc.rs"]
mod compiler;
#[allow(dead_code)]
#[path = "../../../../build/native/dxil_reflection.rs"]
mod dxil_reflection;
#[allow(dead_code)]
#[path = "../../../../build/native/spirv.rs"]
mod spirv;
#[path = "../../../../build/native/toolchain.rs"]
mod toolchain;
use super::{NativeSource, Result, cache};
use std::{
    env,
    path::{Path, PathBuf},
};

pub(crate) struct Compiled {
    pub(crate) bytes: Vec<u8>,
    pub(crate) key: String,
}
pub(crate) fn compile(source: &NativeSource, target: &str) -> Result<Compiled> {
    let text = source.text(false)?;
    let entry = source.entry.as_ref();
    let user_cache = env::var_os("LOCALAPPDATA")
        .or_else(|| env::var_os("XDG_CACHE_HOME"))
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")));
    let roots = toolchain::cache_roots(
        Path::new(env!("CARGO_MANIFEST_DIR")),
        Path::new(env!("OUT_DIR")),
        env!("TILEINK_BUILD_TARGET"),
        user_cache.clone(),
    );
    let explicit = env::var_os("TILEINK_NATIVE_DXC_PATH").map(PathBuf::from);
    let compiler = compiler::Dxc::discover(toolchain::resolve(explicit, &roots)?)?;
    let mut flags = compiler::Dxc::flags(target, entry)?;
    if target == "dxil" {
        flags.push("-Gis".into());
    }
    let recipe = serde_json::to_vec(
        &serde_json::json!({"compiler":compiler.identity,"flags":flags,"target":target,"entry":entry}),
    )?;
    let interface = interface(source);
    let key = cache::CacheKey::new(&[&recipe, text.as_bytes(), &interface.cache_bytes()]);
    let key_text = key.hex();
    let work = Work::new(&key_text);
    let root = env::var_os("TILEINK_NATIVE_SHADER_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            user_cache
                .unwrap_or_else(env::temp_dir)
                .join("tileink/runtime-shaders")
        });
    let validate = |bytes: &[u8]| -> std::io::Result<()> {
        compiler::validate_container(target, bytes)?;
        if target == "spirv" {
            spirv::validate_user(bytes, entry, &interface)
        } else {
            dxil_reflection::validate_user(
                &compiler.reflect_dxil(bytes, &work.0)?,
                entry,
                &interface,
            )
        }
    };
    let artifact = cache::ShaderCache::new(root).get_or_compile(
        &key,
        |bytes| {
            // A verified portable artifact has the exact source/toolchain/ABI
            // key. Reflection failures are errors, not a reason to retry DXC.
            validate(bytes)?;
            Ok(true)
        },
        || {
            let bytes = compiler.compile(text, &flags, &work.0, target)?;
            validate(&bytes)?;
            Ok(bytes)
        },
    )?;
    Ok(Compiled {
        bytes: artifact.bytes,
        key: key_text,
    })
}
pub(crate) fn interface(source: &NativeSource) -> abi::Interface {
    use abi::{Field, Kind, Resource, Scalar};
    let fields: Vec<Field> = [
        "width",
        "height",
        "region_x0",
        "region_y0",
        "region_width",
        "region_height",
        "dispatch_width",
        "pixel_count",
        "compact_tiles",
        "active_tile_count",
        "tiles_width",
        "tiles_height",
        "inverse_xx",
        "inverse_yx",
        "inverse_xy",
        "inverse_yy",
        "inverse_tx",
        "inverse_ty",
        "sample_xx",
        "sample_yx",
        "sample_xy",
        "sample_yy",
        "sample_radius",
        "uniform_words",
        "output_x0",
        "output_y0",
        "output_width",
        "output_height",
    ]
    .into_iter()
    .enumerate()
    .map(|(i, name)| Field {
        name: name.into(),
        offset: i as u32 * 4,
        lanes: 1,
        scalar: if (12..23).contains(&i) {
            Scalar::F32
        } else {
            Scalar::U32
        },
    })
    .collect();
    let resources = source
        .interface_bindings
        .iter()
        .map(|b| {
            use crate::ShaderBindingKind as K;
            let (kind, size) = match b.kind {
                K::Config => (Kind::Uniform, 112),
                K::Uniform(n) => (Kind::Uniform, n),
                K::TextureArray => (Kind::TextureArray, 4),
                K::TextureTable(_) => (Kind::TextureTable, 4),
                K::ReadBuffer => (Kind::Read, 4),
                K::ReadWriteBuffer => (Kind::Write, 4),
                K::Texture => (Kind::Texture, 4),
                K::WriteTexture(f) | K::ReadWriteTexture(f) => {
                    (Kind::TextureWrite, f.bytes_per_pixel() as u32)
                }
                K::Sampler => (Kind::Sampler, 0),
            };
            (
                b.name.clone(),
                Resource {
                    binding: b.slot,
                    kind,
                    size,
                    count: if let K::TextureTable(n) = b.kind {
                        n
                    } else {
                        1
                    },
                    fields: if b.kind == K::Config {
                        fields.clone()
                    } else {
                        vec![]
                    },
                    internal: false,
                },
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let entries = std::collections::BTreeMap::from([(
        source.entry.to_string(),
        resources.keys().cloned().collect(),
    )]);
    abi::Interface {
        workgroup: source.workgroup,
        descriptor_set: 0,
        resources,
        entries,
    }
}
struct Work(PathBuf);
impl Work {
    fn new(key: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        Self(env::temp_dir().join(format!(
            "tileink-program-{}-{}-{key}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )))
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
