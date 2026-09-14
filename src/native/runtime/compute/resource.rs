use super::super::Result;
use super::ResourceId;

pub enum Resource {
    Buffer(Vec<u8>),
    TextureTable(Vec<ResourceId>),
    Sampler(SamplerFilter),
    Texture(Texture),
}
impl Resource {
    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::Buffer(bytes) => bytes,
            Self::Sampler(_) | Self::TextureTable(_) => &[],
            Self::Texture(texture) => &texture.bytes,
        }
    }
}

pub struct Texture {
    pub size: [u32; 2],
    pub layers: u32,
    pub array: bool,
    pub bytes: Vec<u8>,
}
impl Texture {
    pub(super) fn new(size: [u32; 2], layers: u32, array: bool, bytes: Vec<u8>) -> Result<Self> {
        let count = (size[0] as usize)
            .checked_mul(size[1] as usize)
            .and_then(|n| n.checked_mul(layers as usize))
            .and_then(|n| n.checked_mul(4));
        if layers == 0
            || layers > u16::MAX as u32
            || (!array && layers != 1)
            || size.contains(&0)
            || size.iter().any(|&n| n > i32::MAX as u32)
            || count != Some(bytes.len())
        {
            return Err("invalid RGBA8 texture dimensions or byte count".into());
        }
        Ok(Self {
            size,
            layers,
            array,
            bytes,
        })
    }
}

/// Filtering for a single-mip clamp-to-edge sampler.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SamplerFilter {
    Nearest,
    Linear,
}
