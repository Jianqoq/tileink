use peniko::Color;

/// RGBA8 pixel buffer. Pixels are packed little-endian RGBA: red in the low byte.
#[derive(Clone, Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>,
}

impl Image {
    pub fn new(width: u32, height: u32, clear: Color) -> Self {
        Self {
            width,
            height,
            pixels: vec![rgba8_pack(clear.to_rgba8().to_u8_array()); (width * height) as usize],
        }
    }

    pub fn rgba8_at(&self, x: u32, y: u32) -> [u8; 4] {
        unpack_rgba8(self.pixels[(y * self.width + x) as usize])
    }
}

pub(crate) fn rgba8_pack(rgba: [u8; 4]) -> u32 {
    rgba[0] as u32 | (rgba[1] as u32) << 8 | (rgba[2] as u32) << 16 | (rgba[3] as u32) << 24
}

pub(crate) fn unpack_rgba8(px: u32) -> [u8; 4] {
    [
        (px & 0xff) as u8,
        ((px >> 8) & 0xff) as u8,
        ((px >> 16) & 0xff) as u8,
        ((px >> 24) & 0xff) as u8,
    ]
}
