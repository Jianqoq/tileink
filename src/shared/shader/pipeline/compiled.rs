//! Precompiled products use exactly the same public pass and resource contract.
use super::*;
use crate::native::shaders::BindingKind as B;
impl ShaderKernel {
    /// Load a build-validated native artifact without runtime source compilation.
    /// Its reflected binding ABI remains authoritative; source kernels use the
    /// same public descriptors and executor. No filter-only kernel mode exists.
    ///
    /// # Errors
    /// Returns an error if the reflected workgroup or public binding interface is unsupported.
    pub fn precompiled(
        artifact: &'static crate::NativeShaderArtifact,
    ) -> Result<Self, ShaderError> {
        use ShaderBindingKind as K;
        let bindings = artifact
            .bindings
            .iter()
            .filter(|b| !b.internal)
            .map(|b| ShaderBinding {
                name: format!("slot_{}", b.slot),
                slot: b.slot,
                kind: match b.kind {
                    B::Uniform => K::Uniform(b.size),
                    B::Read => K::ReadBuffer,
                    B::Write => K::ReadWriteBuffer,
                    B::Texture => K::Texture,
                    B::TextureWrite => K::WriteTexture(texture_format(b.size)),
                    B::TextureReadWrite => K::ReadWriteTexture(texture_format(b.size)),
                    B::TextureArray => K::TextureArray,
                    B::TextureTable => K::TextureTable(b.count),
                    B::Sampler => K::Sampler,
                },
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

fn texture_format(size: u32) -> ShaderTextureFormat {
    match size {
        4 => ShaderTextureFormat::Rgba8Unorm,
        8 => ShaderTextureFormat::Rgba16Float,
        16 => ShaderTextureFormat::Rgba32Float,
        _ => unreachable!("build-validated writable texture size"),
    }
}
