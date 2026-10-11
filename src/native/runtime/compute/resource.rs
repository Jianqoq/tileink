use super::super::Result;
use super::ResourceId;

pub enum Resource {
    Buffer(Vec<u8>),
    PersistentBuffer(super::BufferUpload),
    TextureTable(Vec<ResourceId>),
    Sampler(SamplerFilter),
    Texture(Texture),
}
impl Resource {
    /// Vulkan image transfers require offsets aligned to one texel block; raw
    /// buffer transfers require four bytes. Keep format-dependent packing shared.
    #[cfg(any(feature = "vulkan", test))]
    pub fn transfer_alignment(&self) -> usize {
        match self {
            Self::Texture(t) => t.format.bytes_per_pixel(),
            _ => 4,
        }
    }
    pub fn byte_len(&self) -> usize {
        match self {
            Self::PersistentBuffer(upload) => upload.buffer.state.size,
            Self::Texture(texture) => {
                texture.size[0] as usize
                    * texture.size[1] as usize
                    * texture.layers as usize
                    * texture.format.bytes_per_pixel()
            }
            _ => self.bytes().len(),
        }
    }
    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::Buffer(bytes) => bytes,
            Self::PersistentBuffer(upload) => &upload.bytes,
            Self::Sampler(_) | Self::TextureTable(_) => &[],
            Self::Texture(texture) => &texture.bytes,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn float_texture_byte_extent_and_transfer_alignment_include_precision() {
        use crate::ShaderTextureFormat as F;
        for (format, size) in [
            (F::Rgba8Unorm, 4),
            (F::Rgba16Float, 8),
            (F::Rgba32Float, 16),
        ] {
            let texture = Resource::Texture(
                Texture::with_format([3, 2], 1, false, format, vec![0; 6 * size].into()).unwrap(),
            );
            assert_eq!(texture.byte_len(), 6 * size);
            assert_eq!(texture.transfer_alignment(), size);
            assert!(
                Texture::with_format([3, 2], 1, false, format, vec![0; 6 * size - 1].into())
                    .is_err()
            );
        }
    }
}

pub struct Texture {
    pub format: crate::ShaderTextureFormat,
    pub size: [u32; 2],
    pub layers: u32,
    pub array: bool,
    pub bytes: TextureBytes,
    pub persistent: Option<crate::native::NativeTexture>,
}
impl Texture {
    pub(super) fn new(
        size: [u32; 2],
        layers: u32,
        array: bool,
        bytes: TextureBytes,
    ) -> Result<Self> {
        Self::with_format(
            size,
            layers,
            array,
            crate::ShaderTextureFormat::Rgba8Unorm,
            bytes,
        )
    }
    pub(super) fn with_format(
        size: [u32; 2],
        layers: u32,
        array: bool,
        format: crate::ShaderTextureFormat,
        bytes: TextureBytes,
    ) -> Result<Self> {
        let count = (size[0] as usize)
            .checked_mul(size[1] as usize)
            .and_then(|n| n.checked_mul(layers as usize))
            .and_then(|n| n.checked_mul(format.bytes_per_pixel()));
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
            format,
            layers,
            array,
            bytes,
            persistent: None,
        })
    }
}

/// Filtering for a single-mip clamp-to-edge sampler.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SamplerFilter {
    Nearest,
    Linear,
}

/// Retain immutable image pixels through submission without reinterpreting their
/// allocation ownership or copying a complete atlas into a second CPU buffer.
pub enum TextureBytes {
    Raw(Vec<u8>),
    Pixels(std::rc::Rc<Vec<u32>>),
}
impl From<Vec<u8>> for TextureBytes {
    fn from(bytes: Vec<u8>) -> Self {
        Self::Raw(bytes)
    }
}
impl std::ops::Deref for TextureBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Raw(bytes) => bytes,
            Self::Pixels(pixels) => bytemuck::cast_slice(pixels),
        }
    }
}
