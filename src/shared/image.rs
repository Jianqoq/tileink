use std::{
    fs::File,
    io::{self, BufWriter},
    path::Path,
};

use peniko::Color;

/// Premultiplied RGBA8 pixel buffer.
///
/// Pixels are packed little-endian RGBA with red in the low byte. RGB channels
/// are stored premultiplied by alpha because renderers composite directly from
/// this buffer.
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
            pixels: vec![premul_color_to_rgba8_pack(clear); pixel_len(width, height)],
        }
    }

    /// Builds an image from straight RGBA8 bytes in row-major order.
    ///
    /// External image decoders usually return straight-alpha RGBA. This
    /// constructor converts those pixels into the renderer's premultiplied
    /// storage format so the image can be used directly by pattern brushes and
    /// `Canvas::push_image`.
    pub fn from_rgba8(width: u32, height: u32, rgba: impl AsRef<[u8]>) -> Self {
        let rgba = rgba.as_ref();
        assert_eq!(
            rgba.len(),
            pixel_len(width, height) * 4,
            "rgba byte length must be width * height * 4"
        );
        let pixels = rgba
            .chunks_exact(4)
            .map(|px| premul_rgba8_pack(px[0], px[1], px[2], px[3]))
            .collect();
        Self {
            width,
            height,
            pixels,
        }
    }

    /// Builds an image from already-premultiplied packed RGBA8 pixels.
    pub fn from_premultiplied_rgba8(width: u32, height: u32, pixels: Vec<u32>) -> Self {
        assert_eq!(
            pixels.len(),
            pixel_len(width, height),
            "premultiplied pixel count must be width * height"
        );
        Self {
            width,
            height,
            pixels,
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

    fn straight_rgba8_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.pixels.len() * 4);
        for &pixel in &self.pixels {
            bytes.extend_from_slice(&unpack_straight_rgba8(pixel));
        }
        bytes
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), ImageSaveError> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }

        let file = File::create(path)?;
        let writer = BufWriter::new(file);
        let mut encoder = png::Encoder::new(writer, self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()?
            .write_image_data(&self.straight_rgba8_bytes())?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum ImageSaveError {
    Io(io::Error),
    Png(png::EncodingError),
}

impl std::fmt::Display for ImageSaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "image IO error: {err}"),
            Self::Png(err) => write!(f, "PNG encoding error: {err}"),
        }
    }
}

impl std::error::Error for ImageSaveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Png(err) => Some(err),
        }
    }
}

impl From<io::Error> for ImageSaveError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<png::EncodingError> for ImageSaveError {
    fn from(err: png::EncodingError) -> Self {
        Self::Png(err)
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

pub(crate) fn premul_rgba8_pack(r: u8, g: u8, b: u8, a: u8) -> u32 {
    rgba8_pack([
        crate::shared::pixel::mul_div255(r, a),
        crate::shared::pixel::mul_div255(g, a),
        crate::shared::pixel::mul_div255(b, a),
        a,
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

fn unpack_straight_rgba8(px: u32) -> [u8; 4] {
    let [r, g, b, a] = unpack_rgba8(px);
    if a == 0 {
        [0, 0, 0, 0]
    } else {
        [
            unpremultiply_u8(r, a),
            unpremultiply_u8(g, a),
            unpremultiply_u8(b, a),
            a,
        ]
    }
}

fn unpremultiply_u8(channel: u8, alpha: u8) -> u8 {
    ((u32::from(channel) * 255 + u32::from(alpha) / 2) / u32::from(alpha)).min(255) as u8
}

fn pixel_len(width: u32, height: u32) -> usize {
    width
        .checked_mul(height)
        .expect("image dimensions overflow u32 pixel count") as usize
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::BufReader, path::PathBuf};

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

    #[test]
    fn from_rgba8_premultiplies_external_straight_alpha_pixels() {
        let image = Image::from_rgba8(
            2,
            1,
            [
                255, 0, 0, 128, //
                0, 128, 255, 64,
            ],
        );

        assert_eq!(image.rgba8_at(0, 0), [128, 0, 0, 128]);
        assert_eq!(image.rgba8_at(1, 0), [0, 32, 64, 64]);
    }

    #[test]
    #[should_panic(expected = "rgba byte length must be width * height * 4")]
    fn from_rgba8_rejects_wrong_byte_count() {
        let _ = Image::from_rgba8(2, 1, [255, 0, 0, 255]);
    }

    #[test]
    fn save_writes_rgba_png_to_path() {
        let mut image = Image::new(2, 1, Color::TRANSPARENT);
        image.pixels = vec![rgba8_pack([64, 0, 0, 128]), rgba8_pack([0, 32, 64, 128])];
        let path = PathBuf::from("target/image-save-test/rgba.png");

        image.save(&path).unwrap();

        let decoder = png::Decoder::new(BufReader::new(File::open(path).unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let mut data = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut data).unwrap();

        assert_eq!((info.width, info.height), (2, 1));
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(
            &data[..info.buffer_size()],
            &[128, 0, 0, 128, 0, 64, 128, 128]
        );
    }
}
