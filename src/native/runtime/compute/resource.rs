use super::super::Result;

pub enum Resource {
    Buffer(Vec<u8>),
    Texture(Texture),
}
impl Resource {
    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::Buffer(bytes) => bytes,
            Self::Texture(texture) => &texture.bytes,
        }
    }
}

pub struct Texture {
    pub size: [u32; 2],
    pub bytes: Vec<u8>,
}
impl Texture {
    pub(super) fn new(size: [u32; 2], bytes: Vec<u8>) -> Result<Self> {
        let count = (size[0] as usize)
            .checked_mul(size[1] as usize)
            .and_then(|n| n.checked_mul(4));
        if size.contains(&0)
            || size.iter().any(|&n| n > i32::MAX as u32)
            || count != Some(bytes.len())
        {
            return Err("invalid RGBA8 texture dimensions or byte count".into());
        }
        Ok(Self { size, bytes })
    }
}
