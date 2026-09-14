use std::{fs::File, io::BufWriter, path::Path};

use tileink::Image;

use crate::Result;

#[derive(Debug, PartialEq, Eq)]
pub struct Difference {
    pub pixels: usize,
    pub max_channel_delta: u8,
    pub first: Option<(u32, u32, [u8; 4], [u8; 4])>,
}

pub fn compare(before: &Image, after: &Image) -> Result<Difference> {
    validate(before)?;
    validate(after)?;
    if (before.width, before.height) != (after.width, after.height) {
        return Err("render target dimensions differ".into());
    }
    let mut difference = Difference {
        pixels: 0,
        max_channel_delta: 0,
        first: None,
    };
    let width = before.width as usize;
    for (index, (&before, &after)) in before.pixels.iter().zip(&after.pixels).enumerate() {
        if before == after {
            continue;
        }
        difference.pixels += 1;
        let before = before.to_le_bytes();
        let after = after.to_le_bytes();
        difference.first.get_or_insert((
            (index % width) as u32,
            (index / width) as u32,
            before,
            after,
        ));
        for channel in 0..4 {
            difference.max_channel_delta = difference
                .max_channel_delta
                .max(before[channel].abs_diff(after[channel]));
        }
    }
    Ok(difference)
}

fn validate(image: &Image) -> Result<()> {
    let expected = u64::from(image.width) * u64::from(image.height);
    if image.width == 0 || image.height == 0 || image.pixels.len() as u64 != expected {
        return Err("invalid RGBA image dimensions or pixel count".into());
    }
    Ok(())
}

/// Save the actual premultiplied target bytes. Image::save unpremultiplies pixels,
/// which would hide differences in transparent RGB and alter the parity evidence.
pub fn save_raw(image: &Image, path: &Path) -> Result<()> {
    validate(image)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = File::options().write(true).create_new(true).open(path)?;
    encode_raw(image, BufWriter::new(file))
}

fn encode_raw(image: &Image, writer: impl std::io::Write) -> Result<()> {
    let mut encoder = png::Encoder::new(writer, image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let bytes: Vec<u8> = image
        .pixels
        .iter()
        .flat_map(|pixel| pixel.to_le_bytes())
        .collect();
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&bytes)?;
    // Drop discards IEND/flush errors; failed evidence must never be certified.
    writer.finish()?;
    Ok(())
}

/// Opaque diagnostic PNG: each RGB channel shows the larger of its own error
/// and the alpha error, so an alpha-only mismatch remains visible.
pub fn save_diff(before: &Image, after: &Image, path: &Path) -> Result<()> {
    compare(before, after)?;
    let pixels = before
        .pixels
        .iter()
        .zip(&after.pixels)
        .map(|(before, after)| {
            let a = before.to_le_bytes();
            let b = after.to_le_bytes();
            let alpha = a[3].abs_diff(b[3]);
            u32::from_le_bytes([
                a[0].abs_diff(b[0]).max(alpha),
                a[1].abs_diff(b[1]).max(alpha),
                a[2].abs_diff(b[2]).max(alpha),
                255,
            ])
        })
        .collect();
    save_raw(
        &Image {
            width: before.width,
            height: before.height,
            pixels,
        },
        path,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(pixels: &[[u8; 4]]) -> Image {
        Image {
            width: pixels.len() as u32,
            height: 1,
            pixels: pixels.iter().copied().map(u32::from_le_bytes).collect(),
        }
    }

    #[test]
    fn png_final_flush_failure_is_reported() {
        struct FailedFlush;
        impl std::io::Write for FailedFlush {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::Error::other("final flush failed"))
            }
        }
        assert!(encode_raw(&image(&[[0, 0, 0, 255]]), FailedFlush).is_err());
    }

    #[test]
    fn equal_pixels_have_zero_difference() {
        let frame = image(&[[17, 3, 2, 0], [0, 127, 0, 127]]);
        assert_eq!(
            compare(&frame, &frame).unwrap(),
            Difference {
                pixels: 0,
                max_channel_delta: 0,
                first: None
            }
        );
    }

    #[test]
    fn every_channel_including_transparent_rgb_is_compared() {
        for channel in 0..4 {
            let before = image(&[[0; 4], [0; 4]]);
            let mut pixel = [0; 4];
            pixel[channel] = 1;
            let after = image(&[[0; 4], pixel]);
            assert_eq!(
                compare(&before, &after).unwrap(),
                Difference {
                    pixels: 1,
                    max_channel_delta: 1,
                    first: Some((1, 0, [0; 4], pixel))
                }
            );
        }
    }

    #[test]
    fn counts_pixels_not_channels_and_reports_largest_delta() {
        let before = image(&[[1, 2, 3, 4], [0; 4]]);
        let after = image(&[[255, 3, 4, 5], [0, 0, 2, 0]]);
        let difference = compare(&before, &after).unwrap();
        assert_eq!(difference.pixels, 2);
        assert_eq!(difference.max_channel_delta, 254);
    }

    #[test]
    fn rejects_truncated_or_differently_shaped_images() {
        let before = image(&[[0; 4], [1; 4]]);
        let mut after = before.clone();
        after.pixels.pop();
        assert!(compare(&before, &after).is_err());
        after = before.clone();
        after.width = 1;
        after.height = 2;
        assert!(compare(&before, &after).is_err());
        assert!(compare(&image(&[]), &image(&[])).is_err());
    }

    #[test]
    fn raw_png_preserves_premultiplied_and_transparent_channels() -> Result<()> {
        let path = std::env::temp_dir().join(format!(
            "tileink-parity-{}-{}.png",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        let frame = image(&[[13, 9, 3, 0], [31, 17, 1, 63]]);
        save_raw(&frame, &path)?;
        assert!(
            save_raw(&frame, &path).is_err(),
            "baseline evidence must not be overwritten"
        );
        let data = std::fs::read(&path)?;
        std::fs::remove_file(&path)?;
        let mut decoder = png::Decoder::new(std::io::Cursor::new(data)).read_info()?;
        let mut bytes = vec![0; decoder.output_buffer_size().unwrap()];
        decoder.next_frame(&mut bytes)?;
        assert_eq!(bytes, vec![13, 9, 3, 0, 31, 17, 1, 63]);
        Ok(())
    }
}
