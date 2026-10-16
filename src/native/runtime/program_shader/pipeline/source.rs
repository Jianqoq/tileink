//! Cache application kernel source independently from per-frame bindings.
use super::{NativeSource, ShaderKernel, ShaderKernelCode};
use crate::hex;
use crate::{
    ShaderBindingKind as K,
    native::shaders::{Binding, BindingKind},
};
use sha2::{Digest, Sha256};
use std::rc::Rc;

pub(crate) fn source(kernel: &ShaderKernel) -> Rc<NativeSource> {
    let ShaderKernelCode::Source { native, .. } = &kernel.code else {
        unreachable!("precompiled kernels do not compile application source")
    };
    std::rc::Rc::clone(native.get_or_init(|| make_source(kernel)))
}
fn make_source(kernel: &ShaderKernel) -> Rc<NativeSource> {
    let ShaderKernelCode::Source { sources, .. } = &kernel.code else {
        unreachable!("precompiled kernels do not compile application source")
    };
    let mut hash = Sha256::new();
    hash.update(b"tileink-native-pipeline-v1");
    for s in [
        Some(kernel.entry.as_ref()),
        sources.metal.as_deref(),
        sources.hlsl.as_deref(),
    ] {
        let bytes = s.unwrap_or_default().as_bytes();
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    for n in kernel.workgroup {
        hash.update(n.to_le_bytes());
    }
    let bindings = kernel
        .bindings
        .iter()
        .map(|b| {
            let (kind, size) = match b.kind {
                K::Config => (BindingKind::Uniform, 112),
                K::Uniform(n) => (BindingKind::Uniform, n),
                K::TextureArray => (BindingKind::TextureArray, 4),
                K::TextureTable(_) => (BindingKind::TextureTable, 4),
                K::ReadBuffer => (BindingKind::Read, 4),
                K::ReadWriteBuffer => (BindingKind::Write, 4),
                K::Texture => (BindingKind::Texture, 4),
                K::WriteTexture(f) => (BindingKind::TextureWrite, f.bytes_per_pixel() as u32),
                K::ReadWriteTexture(f) => {
                    (BindingKind::TextureReadWrite, f.bytes_per_pixel() as u32)
                }
                K::Sampler => (BindingKind::Sampler, 0),
            };
            hash.update([u8::from(b.kind == K::Config)]);
            hash.update((b.name.len() as u64).to_le_bytes());
            hash.update(b.name.as_bytes());
            for n in [b.slot, kind as u32, size] {
                hash.update(n.to_le_bytes());
            }
            let count = if let K::TextureTable(n) = b.kind {
                n
            } else {
                1
            };
            hash.update(count.to_le_bytes());
            Binding {
                slot: b.slot,
                kind,
                size,
                count,
                internal: false,
            }
        })
        .collect();
    Rc::new(NativeSource {
        key: format!("program:{}", hex::encode(&hash.finalize())),
        entry: std::rc::Rc::clone(&kernel.entry),
        bindings,
        workgroup: kernel.workgroup,
        #[cfg(feature = "metal")]
        config: kernel.bindings.iter().any(|b| b.kind == K::Config),
        #[cfg(any(feature = "dx12", feature = "vulkan"))]
        interface_bindings: std::rc::Rc::clone(&kernel.bindings),
        metal: sources.metal.as_deref().map(Rc::from),
        hlsl: sources.hlsl.as_deref().map(Rc::from),
    })
}
