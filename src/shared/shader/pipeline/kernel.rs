//! Native kernel declarations and build-compiled kernel identities.
use super::{ShaderError, ShaderSources, ShaderTextureFormat, validate};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaderBindingKind {
    Config,
    /// Arbitrary native constant block; byte size is checked by reflection.
    Uniform(u32),
    TextureArray,
    /// Sampled 2D texture descriptor table, with the exact declared count.
    TextureTable(u32),
    ReadBuffer,
    ReadWriteBuffer,
    Texture,
    WriteTexture(ShaderTextureFormat),
    /// In-place access after a preceding writer initialized the texture. Threads
    /// must not race reads/writes of neighbouring texels; use two textures then.
    ReadWriteTexture(ShaderTextureFormat),
    Sampler,
}
impl ShaderBindingKind {
    pub(crate) const fn writable(self) -> bool {
        matches!(
            self,
            Self::ReadWriteBuffer | Self::WriteTexture(_) | Self::ReadWriteTexture(_)
        )
    }
}

/// HLSL uses `name` and the matching b/t/u/s register in space0 (Vulkan set0).
///
/// Metal uses the matching buffer/texture/sampler index. Slots are unique across
/// namespaces for portability; 0..=29 is the common Metal resource slot range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderBinding {
    pub name: String,
    pub slot: u32,
    pub kind: ShaderBindingKind,
}

#[derive(Clone)]
pub struct ShaderKernel {
    pub(crate) code: ShaderKernelCode,
    pub(crate) entry: Rc<str>,
    pub(crate) workgroup: [u32; 3],
    pub(crate) bindings: Rc<[ShaderBinding]>,
}
#[derive(Clone)]
pub(crate) enum ShaderKernelCode {
    Source {
        sources: Rc<ShaderSources>,
        #[cfg(tileink_native_runtime)]
        native: Rc<std::cell::OnceCell<Rc<crate::native::runtime::program_shader::NativeSource>>>,
    },
    /// Build-validated binaries use the same public binding contract and pass
    /// executor, without runtime source compilation.
    #[cfg(tileink_native_runtime)]
    Compiled(&'static crate::NativeShaderArtifact),
}
impl std::fmt::Debug for ShaderKernelCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[allow(
                clippy::unnecessary_rest_pattern,
                reason = "Native builds also contain a cached source field"
            )]
            Self::Source { sources, .. } => sources.fmt(f),
            #[cfg(tileink_native_runtime)]
            Self::Compiled(artifact) => f
                .debug_tuple("Compiled")
                .field(&artifact.cache_key)
                .finish(),
        }
    }
}
impl std::fmt::Debug for ShaderKernel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Canvas execution fingerprints use Debug. Compilation is a cache event,
        // not a semantic edit, and must never invalidate a retained execution plan.
        f.debug_struct("ShaderKernel")
            .field("code", &self.code)
            .field("entry", &self.entry)
            .field("workgroup", &self.workgroup)
            .field("bindings", &self.bindings)
            .finish()
    }
}
impl ShaderKernel {
    #[must_use]
    pub fn entry(&self) -> &str {
        &self.entry
    }
    #[must_use]
    pub const fn workgroup(&self) -> [u32; 3] {
        self.workgroup
    }
    #[must_use]
    pub fn bindings(&self) -> &[ShaderBinding] {
        &self.bindings
    }

    ///
    /// # Errors
    /// Returns an error for missing source, an invalid entry/workgroup, or an invalid or conflicting binding declaration.
    pub fn new(
        sources: impl Into<Rc<ShaderSources>>,
        entry: &str,
        workgroup: [u32; 3],
        bindings: Vec<ShaderBinding>,
    ) -> Result<Self, ShaderError> {
        let sources = sources.into();
        validate::kernel(&sources, entry, workgroup, &bindings)?;
        Ok(Self {
            code: ShaderKernelCode::Source {
                sources,
                #[cfg(tileink_native_runtime)]
                native: Rc::new(std::cell::OnceCell::new()),
            },
            entry: entry.into(),
            workgroup,
            bindings: bindings.into(),
        })
    }
}
