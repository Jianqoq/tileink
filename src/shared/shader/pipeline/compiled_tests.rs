use super::*;
use crate::native::shaders::{Binding, BindingKind as B, NativeShaderArtifact};

fn artifact(bindings: Vec<Binding>) -> &'static NativeShaderArtifact {
    Box::leak(Box::new(NativeShaderArtifact {
        format: "test descriptor only",
        entry: "main",
        cache_key: "test descriptor only",
        workgroup: [8, 8, 1],
        bindings: Box::leak(bindings.into_boxed_slice()),
        #[cfg(feature = "metal")]
        uniforms: &[],
        bytes: &[],
    }))
}

fn binding(slot: u32, kind: B, size: u32) -> Binding {
    Binding {
        slot,
        kind,
        size,
        count: 1,
        internal: false,
    }
}

#[test]
fn precompiled_writable_texture_sizes_cannot_default_to_rgba8() {
    for kind in [B::TextureWrite, B::TextureReadWrite] {
        for size in [0, 1, 3, 12, u32::MAX] {
            let descriptor = artifact(vec![binding(3, kind, size)]);
            assert!(
                std::panic::catch_unwind(|| ShaderKernel::precompiled(descriptor)).is_err(),
                "build-validated writable texture size {size} cannot invent a format"
            );
        }
    }
}

#[test]
fn precompiled_bindings_preserve_precision_and_nontexture_sizes() {
    use ShaderBindingKind as K;
    for (size, format) in [
        (4, ShaderTextureFormat::Rgba8Unorm),
        (8, ShaderTextureFormat::Rgba16Float),
        (16, ShaderTextureFormat::Rgba32Float),
    ] {
        let descriptor = artifact(vec![
            binding(0, B::Uniform, 112),
            binding(1, B::Read, 4),
            binding(2, B::Write, 4),
            binding(3, B::TextureWrite, size),
            binding(4, B::TextureReadWrite, size),
            binding(5, B::Texture, 4),
            binding(6, B::TextureArray, 4),
            Binding {
                count: 64,
                ..binding(30, B::TextureTable, 4)
            },
            binding(7, B::Sampler, 0),
            Binding {
                internal: true,
                ..binding(31, B::Uniform, 16)
            },
        ]);
        let kernel = ShaderKernel::precompiled(descriptor).unwrap();
        assert_eq!(
            kernel.bindings.iter().map(|b| b.kind).collect::<Vec<_>>(),
            [
                K::Uniform(112),
                K::ReadBuffer,
                K::ReadWriteBuffer,
                K::WriteTexture(format),
                K::ReadWriteTexture(format),
                K::Texture,
                K::TextureArray,
                K::TextureTable(64),
                K::Sampler
            ]
        );
        assert_eq!(kernel.bindings.len(), 9);
    }
}
