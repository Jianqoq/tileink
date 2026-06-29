use peniko::{
    Color,
    kurbo::{Point, Shape},
};

use crate::{
    cpu::computes::fine::combine_alpha,
    shared::{
        bounds::Bounds,
        image::{Image, rgba8_pack},
        layer::{mask::MaskKind, region::Region},
        pixel::{coverage_f32_to_u8, opacity_f32_to_u8},
        sdf::{Sdf, rect::Rect as SdfRect},
    },
};

pub(super) fn region_bounds(region: &Region) -> Bounds {
    match region {
        Region::Rect { rect, .. } => rect_bounds(*rect),
        Region::Path {
            path, transform, ..
        } => {
            let rect = transform.transform_rect_bbox(path.bounding_box());
            rect_bounds(rect)
        }
    }
}

pub(super) fn svg_mask_coverage(source: &Image, kind: MaskKind) -> Image {
    let mut mask = Image::new(source.width, source.height, Color::TRANSPARENT);
    for (dst, &src) in mask.pixels.iter_mut().zip(&source.pixels) {
        let alpha = match kind {
            MaskKind::Alpha => ((src >> 24) & 0xff) as u8,
            MaskKind::Luminance => svg_luminance_mask_alpha(src),
        };
        *dst = rgba8_pack([alpha, alpha, alpha, alpha]);
    }
    mask
}

pub(super) fn apply_opacity_to_mask(mask: &mut Image, opacity: f32) {
    let opacity = opacity_f32_to_u8(opacity);
    if opacity == 255 {
        return;
    }
    for px in &mut mask.pixels {
        let alpha = combine_alpha(((*px >> 24) & 0xff) as u8, opacity);
        *px = rgba8_pack([alpha, alpha, alpha, alpha]);
    }
}

pub(super) fn copy_image_region(source: &Image, bounds: Bounds, source_bounds: Bounds) -> Image {
    let mut image = Image::new(bounds.width(), bounds.height(), Color::TRANSPARENT);
    for y in 0..image.height {
        let src_y = bounds.y0 + y as i32 - source_bounds.y0;
        if src_y < 0 || src_y >= source.height as i32 {
            continue;
        }
        for x in 0..image.width {
            let src_x = bounds.x0 + x as i32 - source_bounds.x0;
            if src_x < 0 || src_x >= source.width as i32 {
                continue;
            }
            let src_ix = (src_y as u32 * source.width + src_x as u32) as usize;
            image.pixels[(y * image.width + x) as usize] = source.pixels[src_ix];
        }
    }
    image
}

pub(super) fn rasterize_region_mask(region: &Region, bounds: Bounds) -> Image {
    match region {
        Region::Rect { rect, radius } => {
            let sdf = SdfRect {
                start: Point::new(rect.x0, rect.y0),
                end: Point::new(rect.x1, rect.y1),
                radius: *radius,
            };
            rasterize_sdf_mask(&Sdf::Rect(sdf), rect_bounds(*rect), bounds)
        }
        Region::Path {
            path, transform, ..
        } => {
            let mut image = Image::new(bounds.width(), bounds.height(), Color::TRANSPARENT);
            let path = *transform * path;
            for y in 0..image.height {
                let py = bounds.y0 as f64 + y as f64 + 0.5;
                for x in 0..image.width {
                    let px = bounds.x0 as f64 + x as f64 + 0.5;
                    if path.contains(Point::new(px, py)) {
                        let ix = (y * image.width + x) as usize;
                        image.pixels[ix] = rgba8_pack([255, 255, 255, 255]);
                    }
                }
            }
            image
        }
    }
}

pub(super) fn rasterize_sdf_mask(sdf: &Sdf, sdf_bounds: Bounds, bounds: Bounds) -> Image {
    let mut image = Image::new(bounds.width(), bounds.height(), Color::TRANSPARENT);
    let paint_bounds = sdf_bounds.intersect(bounds);
    if paint_bounds.is_empty() {
        return image;
    }

    let tile_x0 = paint_bounds.x0.div_euclid(crate::TILE_SIZE as i32);
    let tile_y0 = paint_bounds.y0.div_euclid(crate::TILE_SIZE as i32);
    let tile_x1 =
        (paint_bounds.x1 + crate::TILE_SIZE as i32 - 1).div_euclid(crate::TILE_SIZE as i32);
    let tile_y1 =
        (paint_bounds.y1 + crate::TILE_SIZE as i32 - 1).div_euclid(crate::TILE_SIZE as i32);

    for tile_y in tile_y0..tile_y1 {
        for tile_x in tile_x0..tile_x1 {
            let tile_bounds = Bounds::new(
                tile_x * crate::TILE_SIZE as i32,
                tile_y * crate::TILE_SIZE as i32,
                (tile_x + 1) * crate::TILE_SIZE as i32,
                (tile_y + 1) * crate::TILE_SIZE as i32,
            );
            let pixel_bounds = tile_bounds.intersect(paint_bounds);
            if pixel_bounds.is_empty() {
                continue;
            }

            if sdf.tile_is_solid(pixel_bounds) {
                write_sdf_mask_tile(&mut image, bounds, pixel_bounds, |_, _| 255);
                continue;
            }

            let mut area = [0.0; crate::BLOCK_SIZE as usize];
            sdf.fine_area(&mut area, tile_bounds, pixel_bounds);
            write_sdf_mask_tile(&mut image, bounds, pixel_bounds, |global_x, global_y| {
                let tile_ix = (global_y - tile_bounds.y0) as usize * crate::TILE_SIZE as usize
                    + (global_x - tile_bounds.x0) as usize;
                coverage_f32_to_u8(area[tile_ix])
            });
        }
    }

    image
}

fn svg_luminance_mask_alpha(px: u32) -> u8 {
    let a = (px >> 24) & 0xff;
    if a == 0 {
        return 0;
    }
    let r = px & 0xff;
    let g = (px >> 8) & 0xff;
    let b = (px >> 16) & 0xff;
    let straight_r = (r * 255 + a / 2) / a;
    let straight_g = (g * 255 + a / 2) / a;
    let straight_b = (b * 255 + a / 2) / a;
    ((2126 * straight_r + 7152 * straight_g + 722 * straight_b) * a / (10_000 * 255)) as u8
}

fn write_sdf_mask_tile(
    image: &mut Image,
    bounds: Bounds,
    pixel_bounds: Bounds,
    mut alpha_at: impl FnMut(i32, i32) -> u8,
) {
    for global_y in pixel_bounds.y0..pixel_bounds.y1 {
        let local_y = (global_y - bounds.y0) as u32;
        for global_x in pixel_bounds.x0..pixel_bounds.x1 {
            let alpha = alpha_at(global_x, global_y);
            if alpha == 0 {
                continue;
            }
            let local_x = (global_x - bounds.x0) as u32;
            let ix = (local_y * image.width + local_x) as usize;
            image.pixels[ix] = rgba8_pack([alpha, alpha, alpha, alpha]);
        }
    }
}

fn rect_bounds(rect: peniko::kurbo::Rect) -> Bounds {
    Bounds::new(
        rect.x0.min(rect.x1).floor() as i32,
        rect.y0.min(rect.y1).floor() as i32,
        rect.x0.max(rect.x1).ceil() as i32,
        rect.y0.max(rect.y1).ceil() as i32,
    )
}

#[cfg(test)]
mod tests {
    use peniko::kurbo::Rect;

    use super::*;
    use crate::{Radius, shared::image::unpack_rgba8};

    #[test]
    fn svg_luminance_mask_alpha_uses_unpremultiplied_rgb_and_alpha() {
        let source = Image {
            width: 1,
            height: 1,
            pixels: vec![rgba8_pack([128, 0, 0, 128])],
        };

        let mask = svg_mask_coverage(&source, MaskKind::Luminance);

        assert_eq!(unpack_rgba8(mask.pixels[0]), [27, 27, 27, 27]);
    }

    #[test]
    fn rasterize_region_mask_clips_to_requested_bounds() {
        let mask = rasterize_region_mask(
            &Region::rect(Rect::new(8.0, 0.0, 16.0, 16.0), Radius::all(0.0)),
            Bounds::new(0, 0, 16, 16),
        );

        assert_eq!(unpack_rgba8(mask.pixels[8]), [255, 255, 255, 255]);
        assert_eq!(unpack_rgba8(mask.pixels[7]), [0, 0, 0, 0]);
    }
}
