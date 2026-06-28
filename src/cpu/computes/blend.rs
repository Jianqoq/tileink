use peniko::BlendMode;

use crate::shared::{
    bounds::Bounds,
    image::Image,
    layer::blend::Blend,
    pixel::{pack_premul_rgba8, scale_premul_u8, src_over, unpack_premul_rgba8},
};

pub(crate) fn composite_src_over_masked_at(
    dst: &mut Image,
    content: &Image,
    mask: &Image,
    content_bounds: Bounds,
    dst_bounds: Bounds,
) {
    for y in 0..content.height {
        let dst_y = content_bounds.y0 + y as i32 - dst_bounds.y0;
        if dst_y < 0 || dst_y >= dst.height as i32 {
            continue;
        }
        for x in 0..content.width {
            let dst_x = content_bounds.x0 + x as i32 - dst_bounds.x0;
            if dst_x < 0 || dst_x >= dst.width as i32 {
                continue;
            }
            let ix = (y * content.width + x) as usize;
            let coverage = ((mask.pixels[ix] >> 24) & 0xff) as f32 / 255.0;
            if coverage <= 0.0 {
                continue;
            }
            let mut src_px = unpack_premul_rgba8(content.pixels[ix]);
            for c in &mut src_px {
                *c *= coverage;
            }
            let dst_ix = (dst_y as u32 * dst.width + dst_x as u32) as usize;
            let dst_px = unpack_premul_rgba8(dst.pixels[dst_ix]);
            dst.pixels[dst_ix] = pack_premul_rgba8(src_over(dst_px, src_px));
        }
    }
}

pub(crate) fn composite_blend_masked_at(
    dst: &mut Image,
    content: &Image,
    mask: &Image,
    content_bounds: Bounds,
    dst_bounds: Bounds,
    mode: BlendMode,
) {
    let blend = Blend::new(mode.mix, mode.compose);
    for y in 0..content.height {
        let dst_y = content_bounds.y0 + y as i32 - dst_bounds.y0;
        if dst_y < 0 || dst_y >= dst.height as i32 {
            continue;
        }
        for x in 0..content.width {
            let dst_x = content_bounds.x0 + x as i32 - dst_bounds.x0;
            if dst_x < 0 || dst_x >= dst.width as i32 {
                continue;
            }
            let ix = (y * content.width + x) as usize;
            let coverage = ((mask.pixels[ix] >> 24) & 0xff) as u8;
            if coverage == 0 {
                continue;
            }
            let src = scale_premul_u8(content.pixels[ix], coverage);
            if src >> 24 == 0 {
                continue;
            }
            let dst_ix = (dst_y as u32 * dst.width + dst_x as u32) as usize;
            dst.pixels[dst_ix] = blend.blend_pixel(src, dst.pixels[dst_ix]);
        }
    }
}
