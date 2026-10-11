//! Build-time native shader products. Loading these bytes does not invoke DXC.
//! Driver-specific pipeline creation/caching belongs to the native adapters.

#[derive(Clone, Copy, Debug)]
pub struct ShaderArtifact<'a> {
    pub format: &'a str,
    pub entry: &'a str,
    pub cache_key: &'a str,
    pub workgroup: [u32; 3],
    pub(crate) bindings: &'a [Binding],
    #[cfg(feature = "metal")]
    pub(crate) uniforms: &'a [UniformLayout],
    pub bytes: &'a [u8],
}
pub type NativeShaderArtifact = ShaderArtifact<'static>;

include!(concat!(env!("OUT_DIR"), "/tileink_native_artifacts.rs"));

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BindingKind {
    Uniform,
    Read,
    Write,
    Texture,
    TextureWrite,
    TextureReadWrite,
    TextureArray,
    TextureTable,
    Sampler,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Binding {
    pub slot: u32,
    pub kind: BindingKind,
    pub size: u32,
    pub count: u32,
    pub internal: bool,
}

impl BindingKind {
    pub(crate) fn writable(self) -> bool {
        matches!(
            self,
            Self::Write | Self::TextureWrite | Self::TextureReadWrite
        )
    }
}

/// Scalar offsets are flattened so equivalent vector/scalar MSL declarations
/// are checked against the same host byte layout without requiring field names.
#[cfg(feature = "metal")]
#[derive(Clone, Copy, Debug)]
pub(crate) struct UniformLayout {
    pub slot: u32,
    pub fields: &'static [(u32, u8)],
}
