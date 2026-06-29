use std::io::Cursor;

use peniko::kurbo::Affine;

use super::SvgError;
use crate::shared::{
    brush::PatternSampling,
    image::{Image as RasterImage, rgba8_pack},
    pixel::mul_div255,
};

pub(super) fn decode_png_image(data: &[u8]) -> Result<RasterImage, SvgError> {
    let mut decoder = png::Decoder::new(Cursor::new(data));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder
        .read_info()
        .map_err(|err| SvgError::unsupported(format!("invalid PNG image: {err}")))?;
    let mut bytes = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut bytes)
        .map_err(|err| SvgError::unsupported(format!("invalid PNG image: {err}")))?;
    let bytes = &bytes[..info.buffer_size()];

    let pixels = match info.color_type {
        png::ColorType::Rgba => bytes
            .chunks_exact(4)
            .map(|px| premul_rgba8_pack(px[0], px[1], px[2], px[3]))
            .collect(),
        png::ColorType::Rgb => bytes
            .chunks_exact(3)
            .map(|px| premul_rgba8_pack(px[0], px[1], px[2], 255))
            .collect(),
        png::ColorType::Grayscale => bytes
            .iter()
            .map(|&gray| premul_rgba8_pack(gray, gray, gray, 255))
            .collect(),
        png::ColorType::GrayscaleAlpha => bytes
            .chunks_exact(2)
            .map(|px| premul_rgba8_pack(px[0], px[0], px[0], px[1]))
            .collect(),
        png::ColorType::Indexed => {
            return Err(SvgError::unsupported("indexed PNG image"));
        }
    };

    Ok(RasterImage {
        width: info.width,
        height: info.height,
        pixels,
    })
}

pub(super) fn decode_encoded_image(
    data: &[u8],
    format: ::image::ImageFormat,
    feature: &str,
) -> Result<RasterImage, SvgError> {
    let image = ::image::load_from_memory_with_format(data, format)
        .map_err(|err| SvgError::unsupported(format!("invalid {feature}: {err}")))?
        .into_rgba8();
    let (width, height) = image.dimensions();
    let pixels = image
        .pixels()
        .map(|px| premul_rgba8_pack(px.0[0], px.0[1], px.0[2], px.0[3]))
        .collect();
    Ok(RasterImage {
        width,
        height,
        pixels,
    })
}

pub(super) fn svg_image_raster_size(transform: Affine, size: usvg::Size) -> (u32, u32) {
    let [xx, yx, xy, yy, _, _] = transform.as_coeffs();
    let scale_x = xx.hypot(yx).max(f64::EPSILON);
    let scale_y = xy.hypot(yy).max(f64::EPSILON);
    (
        (f64::from(size.width()) * scale_x).ceil().max(1.0) as u32,
        (f64::from(size.height()) * scale_y).ceil().max(1.0) as u32,
    )
}

pub(super) fn image_sampling(rendering: usvg::ImageRendering) -> PatternSampling {
    // SVG raster images are smooth by default; explicit speed/crisp/pixelated hints keep hard edges.
    match rendering {
        usvg::ImageRendering::OptimizeSpeed
        | usvg::ImageRendering::CrispEdges
        | usvg::ImageRendering::Pixelated => PatternSampling::Nearest,
        usvg::ImageRendering::OptimizeQuality
        | usvg::ImageRendering::Smooth
        | usvg::ImageRendering::HighQuality => PatternSampling::Bilinear,
    }
}

fn premul_rgba8_pack(r: u8, g: u8, b: u8, a: u8) -> u32 {
    rgba8_pack([mul_div255(r, a), mul_div255(g, a), mul_div255(b, a), a])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_image_raster_size_uses_axis_scale() {
        let size = usvg::Size::from_wh(10.0, 20.0).unwrap();

        assert_eq!(
            svg_image_raster_size(Affine::scale_non_uniform(2.0, 0.25), size),
            (20, 5)
        );
    }

    #[test]
    fn image_sampling_preserves_explicit_crisp_modes() {
        assert_eq!(
            image_sampling(usvg::ImageRendering::CrispEdges),
            PatternSampling::Nearest
        );
        assert_eq!(
            image_sampling(usvg::ImageRendering::Smooth),
            PatternSampling::Bilinear
        );
    }
}
