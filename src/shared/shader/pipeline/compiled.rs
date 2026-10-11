//! Precompiled products use exactly the same public pass and resource contract.
use super::*;
use crate::native::shaders::BindingKind as B;
impl ShaderKernel {
    /// Load a build-validated native artifact without runtime source compilation.
    /// Its reflected binding ABI remains authoritative; source kernels use the
    /// same public descriptors and executor. No filter-only kernel mode exists.
    pub fn precompiled(
        artifact: &'static crate::NativeShaderArtifact,
    ) -> Result<Self, ShaderError> {
        use ShaderBindingKind as K;
        let bindings = artifact
            .bindings
            .iter()
            .filter(|b| !b.internal)
            .map(|b| {
                let format = match b.size {
                    4 => ShaderTextureFormat::Rgba8Unorm,
                    8 => ShaderTextureFormat::Rgba16Float,
                    16 => ShaderTextureFormat::Rgba32Float,
                    _ => ShaderTextureFormat::Rgba8Unorm,
                };
                ShaderBinding {
                    name: format!("slot_{}", b.slot),
                    slot: b.slot,
                    kind: match b.kind {
                        B::Uniform => K::Uniform(b.size),
                        B::Read => K::ReadBuffer,
                        B::Write => K::ReadWriteBuffer,
                        B::Texture => K::Texture,
                        B::TextureWrite => K::WriteTexture(format),
                        B::TextureReadWrite => K::ReadWriteTexture(format),
                        B::TextureArray => K::TextureArray,
                        B::TextureTable => K::TextureTable(b.count),
                        B::Sampler => K::Sampler,
                    },
                }
            })
            .collect::<Vec<_>>();
        validate::interface(artifact.workgroup, &bindings)?;
        Ok(Self {
            code: ShaderKernelCode::Compiled(artifact),
            entry: artifact.entry.into(),
            workgroup: artifact.workgroup,
            bindings: bindings.into(),
        })
    }
}
