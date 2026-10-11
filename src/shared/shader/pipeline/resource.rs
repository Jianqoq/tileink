//! Public resource contracts shared by canvas filters and native invocations.
use super::*;

/// Physical resource category for zero-copy invocation bindings. Native execution
/// checks the actual handle's owner, category, capacity, format and table aliases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaderResourceKind {
    Buffer,
    Texture(ShaderTextureFormat),
    TextureArray,
    TextureTable(u32),
    Sampler,
}

/// Metadata written into the standard ShaderConfig. This describes the ABI only;
/// declare retained input dependencies on ShaderPipeline separately.
#[derive(Clone, Copy, Debug)]
pub struct ShaderConfigDescriptor {
    pub extent: ShaderExtent,
    pub sample_radius: f32,
    pub uniform_words: u32,
}
impl ShaderConfigDescriptor {
    pub const fn new(extent: ShaderExtent) -> Self {
        Self {
            extent,
            sample_radius: 0.0,
            uniform_words: 0,
        }
    }
}
impl ShaderResource {
    pub const fn config(extent: ShaderExtent) -> Self {
        Self::Config(ShaderConfigDescriptor::new(extent))
    }
    pub(crate) fn kind(&self) -> ShaderResourceKind {
        use ShaderResourceKind as K;
        match self {
            Self::Config(_) | Self::Uniform(_) | Self::Buffer(_) | Self::ActiveTiles => K::Buffer,
            Self::Input(_) | Self::Output => K::Texture(ShaderTextureFormat::Rgba8Unorm),
            Self::Image { format, .. } | Self::Texture { format, .. } => K::Texture(*format),
            Self::Sampler(_) => K::Sampler,
            Self::External { kind, .. } => *kind,
        }
    }
}
