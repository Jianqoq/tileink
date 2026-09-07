use std::io::Cursor;

use crate::Result;

#[derive(Debug, PartialEq)]
pub enum Difference {
    Dimensions {
        before: (u32, u32),
        after: (u32, u32),
    },
    Pixels {
        count: usize,
        first: (u32, u32),
        before: [u16; 4],
        after: [u16; 4],
    },
}

struct Image {
    size: (u32, u32),
    rgba: Vec<[u16; 4]>,
}

fn decode(bytes: &[u8]) -> Result<Image> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    // Expand palettes, low-bit grayscale and tRNS, but never strip 16-bit precision.
    // Raw RGBA comparison also preserves RGB under zero alpha; no color management is applied.
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info()?;
    if reader.info().animation_control.is_some() {
        return Err(
            "animated PNG is unsupported; comparing only its first frame would be incomplete"
                .into(),
        );
    }
    let mut buffer = vec![0; reader.output_buffer_size().ok_or("PNG is too large")?];
    let output = reader.next_frame(&mut buffer)?;
    reader.finish()?;
    let bytes_per_sample = match output.bit_depth {
        png::BitDepth::Eight => 1,
        png::BitDepth::Sixteen => 2,
        _ => return Err("PNG decoder did not expand packed samples".into()),
    };
    let mut rgba = Vec::new();
    for pixel in
        buffer[..output.buffer_size()].chunks_exact(output.color_type.samples() * bytes_per_sample)
    {
        let sample = |channel: usize| {
            if bytes_per_sample == 1 {
                u16::from(pixel[channel]) * 257
            } else {
                u16::from_be_bytes([pixel[channel * 2], pixel[channel * 2 + 1]])
            }
        };
        rgba.push(match output.color_type {
            png::ColorType::Grayscale => [sample(0), sample(0), sample(0), u16::MAX],
            png::ColorType::GrayscaleAlpha => [sample(0), sample(0), sample(0), sample(1)],
            png::ColorType::Rgb => [sample(0), sample(1), sample(2), u16::MAX],
            png::ColorType::Rgba => [sample(0), sample(1), sample(2), sample(3)],
            png::ColorType::Indexed => return Err("PNG decoder did not expand palette".into()),
        });
    }
    Ok(Image {
        size: (output.width, output.height),
        rgba,
    })
}

pub fn compare(before: &[u8], after: &[u8]) -> Result<Option<Difference>> {
    let before = decode(before).map_err(|error| format!("baseline: {error}"))?;
    let after = decode(after).map_err(|error| format!("working tree: {error}"))?;
    if before.size != after.size {
        return Ok(Some(Difference::Dimensions {
            before: before.size,
            after: after.size,
        }));
    }
    let mut different = before
        .rgba
        .iter()
        .zip(&after.rgba)
        .enumerate()
        .filter(|(_, (before, after))| before != after);
    Ok(different
        .next()
        .map(|(index, (old, new))| Difference::Pixels {
            count: 1 + different.count(),
            first: (index as u32 % before.size.0, index as u32 / before.size.0),
            before: *old,
            after: *new,
        }))
}
