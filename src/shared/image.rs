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
            pixels: vec![premul_color_to_rgba8_pack(clear); (width * height) as usize],
        }
    }

    pub fn rgba8_at(&self, x: u32, y: u32) -> [u8; 4] {
        unpack_rgba8(self.pixels[(y * self.width + x) as usize])
    }

    pub fn rgba8_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.pixels.len() * 4);
        for pixel in &self.pixels {
            bytes.extend_from_slice(&pixel.to_le_bytes());
        }
        bytes
    }
}

pub(crate) fn rgba8_pack(rgba: [u8; 4]) -> u32 {
    rgba[0] as u32 | (rgba[1] as u32) << 8 | (rgba[2] as u32) << 16 | (rgba[3] as u32) << 24
}

pub(crate) fn premul_color_to_rgba8_pack(color: Color) -> u32 {
    let [r, g, b, a] = color.premultiply().components;
    rgba8_pack([
        (r * 255.0 + 0.5) as u8,
        (g * 255.0 + 0.5) as u8,
        (b * 255.0 + 0.5) as u8,
        (a * 255.0 + 0.5) as u8,
    ])
}

pub(crate) fn unpack_rgba8(px: u32) -> [u8; 4] {
    [
        (px & 0xff) as u8,
        ((px >> 8) & 0xff) as u8,
        ((px >> 16) & 0xff) as u8,
        ((px >> 24) & 0xff) as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::{Image, premul_color_to_rgba8_pack, rgba8_pack};
    use peniko::Color;

    #[test]
    fn image_new_stores_premultiplied_clear_color() {
        let clear = Color::from_rgba8(255, 0, 0, 128);
        let image = Image::new(1, 1, clear);

        assert_eq!(image.pixels[0], premul_color_to_rgba8_pack(clear));
        assert_eq!(image.rgba8_at(0, 0), [128, 0, 0, 128]);
    }

    #[test]
    fn rgba8_bytes_returns_pixels_in_rgba_order() {
        let mut image = Image::new(2, 1, Color::TRANSPARENT);
        image.pixels = vec![rgba8_pack([1, 2, 3, 4]), rgba8_pack([5, 6, 7, 8])];

        assert_eq!(image.rgba8_bytes(), vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }
}
