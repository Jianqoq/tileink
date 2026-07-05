use peniko::BlendMode;

use crate::{
    TILE_SIZE,
    shared::{
        bounds::Bounds,
        brush::Brush,
        fill::FillRule,
        image_resource::ImageResourceStore,
        layer::blend::Blend,
        line_seg::LineSegment,
        pixel::{
            TextCoverageParams, TileBuffer, coverage_f32_to_u8, pack_premul_rgba8, scale_premul_u8,
            src_over_mask_linear_auto_u8, src_over_mask_linear_auto_with_params_u8,
            src_over_premul_u8, src_over_subpixel_mask_linear_auto_u8,
            src_over_subpixel_mask_linear_auto_with_params_u8, src_over_subpixel_mask_u8,
            unpack_premul_rgba8,
        },
        sdf::{Sdf, SdfShadow},
    },
    text::{PreparedGlyphContent, PreparedTextData, TextCompositeMode},
};

const FINE_AREA_EPSILON: f32 = 1.0e-6;

#[inline]
fn apply_rule(value: f32, fill_rule: FillRule) -> f32 {
    match fill_rule {
        FillRule::EvenOdd => (value - 2.0 * (0.5 * value).round()).abs(),
        FillRule::NonZero => value.abs().min(1.0),
    }
}

#[inline]
fn coverage_to_alpha(value: f32, fill_rule: FillRule) -> u8 {
    (apply_rule(value, fill_rule).clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

#[inline]
pub(crate) fn combine_alpha(a: u8, b: u8) -> u8 {
    ((a as u32 * b as u32 + 127) / 255) as u8
}

#[inline]
#[cfg(test)]
fn segment_coverage_at(segment: &LineSegment, x: u32, y: u32) -> f32 {
    let delta_x = segment.p1x - segment.p0x;
    let delta_y = segment.p1y - segment.p0y;
    let row_y = y as f32;
    let local_y = segment.p0y - row_y;
    let y0 = local_y.clamp(0.0, 1.0);
    let y1 = (local_y + delta_y).clamp(0.0, 1.0);
    let dy = y0 - y1;
    let y_edge = delta_x.signum() * (row_y - segment.y_edge + 1.0).clamp(0.0, 1.0);

    if dy == 0.0 {
        return y_edge;
    }

    let recip = 1.0 / delta_y;
    let t0 = (y0 - local_y) * recip;
    let t1 = (y1 - local_y) * recip;
    let sx0 = segment.p0x + t0 * delta_x;
    let sx1 = segment.p0x + t1 * delta_x;
    let pixel_x = x as f32;
    let xmin = sx0.min(sx1) - pixel_x;
    let xmax = sx0.max(sx1) - pixel_x;
    if xmax - xmin <= FINE_AREA_EPSILON {
        return y_edge + (1.0 - xmin).clamp(0.0, 1.0) * dy;
    }
    let a_min = xmin.min(1.0) - FINE_AREA_EPSILON;
    let b = xmax.min(1.0);
    let c = b.max(0.0);
    let d = a_min.max(0.0);
    let a = (b + 0.5 * (d * d - c * c) - a_min) / (xmax - a_min);
    y_edge + a * dy
}

#[inline]
fn segment_row_parts(segment: &LineSegment, y: u32) -> (f32, f32, f32, f32) {
    let delta_x = segment.p1x - segment.p0x;
    let delta_y = segment.p1y - segment.p0y;
    let row_y = y as f32;
    let local_y = segment.p0y - row_y;
    let y0 = local_y.clamp(0.0, 1.0);
    let y1 = (local_y + delta_y).clamp(0.0, 1.0);
    let dy = y0 - y1;
    let y_edge = delta_x.signum() * (row_y - segment.y_edge + 1.0).clamp(0.0, 1.0);

    if dy == 0.0 {
        return (y_edge, dy, 0.0, 0.0);
    }

    let recip = 1.0 / delta_y;
    let t0 = (y0 - local_y) * recip;
    let t1 = (y1 - local_y) * recip;
    let sx0 = segment.p0x + t0 * delta_x;
    let sx1 = segment.p0x + t1 * delta_x;

    (y_edge, dy, sx0.min(sx1), sx0.max(sx1))
}

#[inline]
fn segment_area_at(xmin: f32, xmax: f32, x: u32) -> f32 {
    let pixel_x = x as f32;
    let xmin = xmin - pixel_x;
    let xmax = xmax - pixel_x;
    if xmax - xmin <= FINE_AREA_EPSILON {
        return (1.0 - xmin).clamp(0.0, 1.0);
    }
    let a_min = xmin.min(1.0) - FINE_AREA_EPSILON;
    let b = xmax.min(1.0);
    let c = b.max(0.0);
    let d = a_min.max(0.0);
    (b + 0.5 * (d * d - c * c) - a_min) / (xmax - a_min)
}

fn build_row_coverages(segments: &[LineSegment], backdrop: i32, y: u32) -> [f32; 16] {
    let mut base = backdrop as f32;
    let mut diff = [0.0f32; 17];
    let mut partial = [0.0f32; 16];

    for segment in segments {
        let (y_edge, dy, xmin, xmax) = segment_row_parts(segment, y);
        base += y_edge;

        if dy == 0.0 {
            continue;
        }

        let full_start = (xmax.ceil() as i32).clamp(0, TILE_SIZE as i32) as usize;
        if full_start < TILE_SIZE as usize {
            diff[full_start] += dy;
        }

        let partial_start = (xmin.floor() as i32).clamp(0, TILE_SIZE as i32) as u32;
        let partial_end = (xmax.ceil() as i32).clamp(0, TILE_SIZE as i32) as u32;
        for x in partial_start..partial_end {
            partial[x as usize] += segment_area_at(xmin, xmax, x) * dy;
        }
    }

    let mut out = [0.0f32; 16];
    let mut running = 0.0;
    for x in 0..TILE_SIZE as usize {
        running += diff[x];
        out[x] = base + running + partial[x];
    }
    out
}

#[cfg(test)]
pub(crate) fn pixel_coverage(
    segments: &[LineSegment],
    backdrop: i32,
    fill_rule: FillRule,
    x: u32,
    y: u32,
) -> u8 {
    let mut coverage = backdrop as f32;
    for segment in segments {
        coverage += segment_coverage_at(segment, x, y);
    }
    coverage_to_alpha(coverage, fill_rule)
}

pub(crate) fn build_tile_alpha(
    segments: &[LineSegment],
    backdrop: i32,
    fill_rule: FillRule,
) -> [u8; 256] {
    let fill_alpha = coverage_to_alpha(backdrop as f32, fill_rule);
    let mut tile_alpha = [fill_alpha; 256];
    if segments.is_empty() {
        return tile_alpha;
    }

    for y in 0..TILE_SIZE {
        let row = build_row_coverages(segments, backdrop, y);
        let row_start = (y * TILE_SIZE) as usize;
        for x in 0..TILE_SIZE as usize {
            tile_alpha[row_start + x] = coverage_to_alpha(row[x], fill_rule);
        }
    }
    tile_alpha
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn rasterize_tile_buffer_into(
    tile: &mut TileBuffer,
    tile_x: u32,
    tile_y: u32,
    segments: &[LineSegment],
    backdrop: i32,
    fill_rule: FillRule,
    brush: &Brush,
    clip_mask: &[u8; 256],
    image_resources: Option<&ImageResourceStore>,
) {
    rasterize_path_tile_buffer_into(
        tile,
        tile_x,
        tile_y,
        segments,
        backdrop,
        fill_rule,
        brush,
        clip_mask,
        image_resources,
        false,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn rasterize_path_glyph_tile_buffer_into(
    tile: &mut TileBuffer,
    tile_x: u32,
    tile_y: u32,
    segments: &[LineSegment],
    backdrop: i32,
    fill_rule: FillRule,
    brush: &Brush,
    clip_mask: &[u8; 256],
    image_resources: Option<&ImageResourceStore>,
) {
    rasterize_path_tile_buffer_into(
        tile,
        tile_x,
        tile_y,
        segments,
        backdrop,
        fill_rule,
        brush,
        clip_mask,
        image_resources,
        true,
    );
}

#[allow(clippy::too_many_arguments)]
fn rasterize_path_tile_buffer_into(
    tile: &mut TileBuffer,
    tile_x: u32,
    tile_y: u32,
    segments: &[LineSegment],
    backdrop: i32,
    fill_rule: FillRule,
    brush: &Brush,
    clip_mask: &[u8; 256],
    image_resources: Option<&ImageResourceStore>,
    linear_text_coverage: bool,
) {
    let base_x = tile_x * TILE_SIZE;
    let base_y = tile_y * TILE_SIZE;
    let tile_alpha = build_tile_alpha(segments, backdrop, fill_rule);
    for y in 0..TILE_SIZE {
        let row_start = (y * TILE_SIZE) as usize;
        for x in 0..TILE_SIZE as usize {
            let ix = row_start + x;
            let alpha = combine_alpha(tile_alpha[ix], clip_mask[ix]);
            if alpha == 0 {
                continue;
            }
            let color = brush.sample_with_resources(
                (base_x + x as u32) as f32 + 0.5,
                (base_y + y) as f32 + 0.5,
                image_resources,
            );
            tile[ix] = if linear_text_coverage {
                src_over_mask_linear_auto_u8(tile[ix], color, alpha)
            } else {
                src_over_premul_u8(tile[ix], scale_premul_u8(color, alpha))
            };
        }
    }
}

pub(crate) fn rasterize_sdf_tile_buffer_into(
    tile: &mut TileBuffer,
    tile_x: u32,
    tile_y: u32,
    sdf: &Sdf,
    brush: &Brush,
    clip_mask: &[u8; 256],
    image_resources: Option<&ImageResourceStore>,
) {
    rasterize_sdf_area_tile_buffer_into(
        tile,
        tile_x,
        tile_y,
        brush,
        clip_mask,
        image_resources,
        |area, bounds| {
            sdf.fine_area(area, bounds, bounds);
        },
    );
}

pub(crate) fn rasterize_sdf_shadow_tile_buffer_into(
    tile: &mut TileBuffer,
    tile_x: u32,
    tile_y: u32,
    sdf_shadow: &SdfShadow,
    brush: &Brush,
    clip_mask: &[u8; 256],
    image_resources: Option<&ImageResourceStore>,
) {
    rasterize_sdf_area_tile_buffer_into(
        tile,
        tile_x,
        tile_y,
        brush,
        clip_mask,
        image_resources,
        |area, bounds| {
            sdf_shadow.fine_area(area, bounds, bounds);
        },
    );
}

fn rasterize_sdf_area_tile_buffer_into(
    tile: &mut TileBuffer,
    tile_x: u32,
    tile_y: u32,
    brush: &Brush,
    clip_mask: &[u8; 256],
    image_resources: Option<&ImageResourceStore>,
    fine_area: impl FnOnce(&mut [f32; crate::BLOCK_SIZE as usize], Bounds),
) {
    let base_x = (tile_x * TILE_SIZE) as i32;
    let base_y = (tile_y * TILE_SIZE) as i32;
    let tile_bounds = Bounds::new(
        base_x,
        base_y,
        base_x + TILE_SIZE as i32,
        base_y + TILE_SIZE as i32,
    );
    let mut area = [0.0; crate::BLOCK_SIZE as usize];
    fine_area(&mut area, tile_bounds);

    for y in 0..TILE_SIZE {
        let row_start = (y * TILE_SIZE) as usize;
        for x in 0..TILE_SIZE as usize {
            let ix = row_start + x;
            let alpha = combine_alpha(coverage_f32_to_u8(area[ix]), clip_mask[ix]);
            if alpha == 0 {
                continue;
            }
            let src = scale_premul_u8(
                brush.sample_with_resources(
                    (base_x + x as i32) as f32 + 0.5,
                    (base_y + y as i32) as f32 + 0.5,
                    image_resources,
                ),
                alpha,
            );
            tile[ix] = src_over_premul_u8(tile[ix], src);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn rasterize_glyphs_tile_buffer_into(
    tile: &mut TileBuffer,
    tile_x: u32,
    tile_y: u32,
    glyph_ids: &[u32],
    brush: &Brush,
    text: &PreparedTextData,
    clip_mask: &[u8; 256],
    image_resources: Option<&ImageResourceStore>,
) {
    let base_x = (tile_x * TILE_SIZE) as i32;
    let base_y = (tile_y * TILE_SIZE) as i32;
    let tile_x1 = base_x + TILE_SIZE as i32;
    let tile_y1 = base_y + TILE_SIZE as i32;

    for &glyph_id in glyph_ids {
        let Some(glyph) = text.glyph(glyph_id) else {
            continue;
        };
        let Some(image_id) = glyph.image else {
            continue;
        };
        let Some(image) = text.image(image_id) else {
            continue;
        };

        let glyph_x0 = glyph.x + image.left;
        let glyph_y0 = glyph.y - image.top;
        let glyph_x1 = glyph_x0 + image.width as i32;
        let glyph_y1 = glyph_y0 + image.height as i32;
        let x0 = glyph_x0.max(base_x);
        let y0 = glyph_y0.max(base_y);
        let x1 = glyph_x1.min(tile_x1);
        let y1 = glyph_y1.min(tile_y1);
        if x0 >= x1 || y0 >= y1 {
            continue;
        }

        for global_y in y0..y1 {
            let local_y = (global_y - base_y) as usize;
            let image_y = (global_y - glyph_y0) as usize;
            for global_x in x0..x1 {
                let local_x = (global_x - base_x) as usize;
                let image_x = (global_x - glyph_x0) as usize;
                let tile_ix = local_y * TILE_SIZE as usize + local_x;
                let clip = clip_mask[tile_ix];
                if clip == 0 {
                    continue;
                }

                let src = match image.content {
                    PreparedGlyphContent::Mask => {
                        let image_ix = image_y * image.width as usize + image_x;
                        let alpha = combine_alpha(image.data[image_ix], clip);
                        if alpha == 0 {
                            continue;
                        }
                        let color = brush.sample_with_resources(
                            global_x as f32 + 0.5,
                            global_y as f32 + 0.5,
                            image_resources,
                        );
                        match image.composite_mode {
                            TextCompositeMode::Linear => {
                                tile[tile_ix] = src_over_mask_linear_auto_with_params_u8(
                                    tile[tile_ix],
                                    color,
                                    alpha,
                                    image.coverage_params,
                                );
                                continue;
                            }
                            TextCompositeMode::Srgb => {}
                        }
                        scale_premul_u8(color, alpha)
                    }
                    PreparedGlyphContent::Color => {
                        let image_ix = (image_y * image.width as usize + image_x) * 4;
                        let a = image.data[image_ix + 3];
                        if a == 0 {
                            continue;
                        }
                        let src = scale_premul_u8(
                            crate::shared::image::rgba8_pack([
                                crate::shared::pixel::mul_div255(image.data[image_ix], a),
                                crate::shared::pixel::mul_div255(image.data[image_ix + 1], a),
                                crate::shared::pixel::mul_div255(image.data[image_ix + 2], a),
                                a,
                            ]),
                            clip,
                        );
                        match image.composite_mode {
                            TextCompositeMode::Linear => {
                                tile[tile_ix] = src_over_mask_linear_auto_with_params_u8(
                                    tile[tile_ix],
                                    src,
                                    clip,
                                    image.coverage_params,
                                );
                                continue;
                            }
                            TextCompositeMode::Srgb => {}
                        }
                        src
                    }
                    PreparedGlyphContent::SubpixelMask => {
                        let image_ix = (image_y * image.width as usize + image_x) * 3;
                        let mask = [
                            image.data[image_ix],
                            image.data[image_ix + 1],
                            image.data[image_ix + 2],
                        ];
                        let color = brush.sample_with_resources(
                            global_x as f32 + 0.5,
                            global_y as f32 + 0.5,
                            image_resources,
                        );
                        let out = match image.composite_mode {
                            TextCompositeMode::Srgb => {
                                src_over_subpixel_mask_u8(tile[tile_ix], color, mask, clip)
                            }
                            TextCompositeMode::Linear => {
                                if image.coverage_params == TextCoverageParams::DEFAULT {
                                    src_over_subpixel_mask_linear_auto_u8(
                                        tile[tile_ix],
                                        color,
                                        mask,
                                        clip,
                                    )
                                } else {
                                    src_over_subpixel_mask_linear_auto_with_params_u8(
                                        tile[tile_ix],
                                        color,
                                        mask,
                                        clip,
                                        image.coverage_params,
                                    )
                                }
                            }
                        };
                        tile[tile_ix] = out;
                        continue;
                    }
                };
                tile[tile_ix] = src_over_premul_u8(tile[tile_ix], src);
            }
        }
    }
}

pub(crate) fn composite_color_tile_buffer_into(
    tile: &mut TileBuffer,
    color: u32,
    clip_mask: &[u8; 256],
) {
    if (color >> 24) as u8 == 0 {
        return;
    }
    for (dst, &mask) in tile.iter_mut().zip(clip_mask) {
        if mask == 0 {
            continue;
        }
        *dst = src_over_premul_u8(*dst, scale_premul_u8(color, mask));
    }
}

pub(crate) fn composite_opacity_group_tile(
    parent: &mut TileBuffer,
    group: &TileBuffer,
    layer_alpha: &[u8; 256],
    parent_clip_mask: &[u8; 256],
    opacity: u8,
) {
    for ix in 0..parent.len() {
        let alpha = combine_alpha(
            combine_alpha(layer_alpha[ix], parent_clip_mask[ix]),
            opacity,
        );
        if alpha == 0 {
            continue;
        }
        parent[ix] = src_over_premul_u8(parent[ix], scale_premul_u8(group[ix], alpha));
    }
}

pub(crate) fn composite_blend_group_tile(
    parent: &mut TileBuffer,
    group: &TileBuffer,
    layer_alpha: &[u8; 256],
    parent_clip_mask: &[u8; 256],
    mode: BlendMode,
) {
    let blend = Blend::new(mode.mix, mode.compose);
    for ix in 0..parent.len() {
        let alpha = combine_alpha(layer_alpha[ix], parent_clip_mask[ix]);
        if alpha == 0 {
            continue;
        }
        let src = unpack_premul_rgba8(scale_premul_u8(group[ix], alpha));
        if src[3] == 0.0 {
            continue;
        }
        parent[ix] = pack_premul_rgba8(blend.blend(src, unpack_premul_rgba8(parent[ix])));
    }
}

#[cfg(test)]
mod tests {
    use peniko::Color;

    use super::{
        build_tile_alpha, composite_color_tile_buffer_into, pixel_coverage,
        rasterize_glyphs_tile_buffer_into, rasterize_path_glyph_tile_buffer_into,
        rasterize_tile_buffer_into,
    };
    use crate::{
        TextCoverageParams,
        shared::{
            brush::Brush,
            fill::FillRule,
            image::{rgba8_pack, unpack_rgba8},
            line_seg::LineSegment,
            pixel::{TileBuffer, premul_f32_to_u32, src_over_mask_linear_auto_u8},
        },
        text::{
            PreparedGlyph, PreparedGlyphContent, PreparedGlyphImage, PreparedTextData,
            TextCompositeMode,
        },
    };

    #[test]
    fn pixel_coverage_uses_backdrop_when_tile_has_no_segments() {
        assert_eq!(pixel_coverage(&[], 0, FillRule::NonZero, 0, 0), 0);
        assert_eq!(pixel_coverage(&[], -1, FillRule::NonZero, 8, 8), 255);
        assert_eq!(pixel_coverage(&[], 2, FillRule::NonZero, 15, 15), 255);
        assert_eq!(pixel_coverage(&[], 1, FillRule::EvenOdd, 3, 4), 255);
        assert_eq!(pixel_coverage(&[], 2, FillRule::EvenOdd, 3, 4), 0);
    }

    #[test]
    fn pixel_coverage_consumes_segment_geometry() {
        let segment = LineSegment {
            p0x: 4.0,
            p0y: 0.0,
            p1x: 12.0,
            p1y: 16.0,
            y_edge: 1.0e9,
        };

        assert_eq!(pixel_coverage(&[segment], 0, FillRule::NonZero, 0, 4), 0);
        assert!(pixel_coverage(&[segment], 0, FillRule::NonZero, 8, 4) > 0);
    }

    #[test]
    fn pixel_coverage_handles_vertical_edges_without_cancellation() {
        let segment = LineSegment {
            p0x: 0.75,
            p0y: 16.0,
            p1x: 0.75,
            p1y: 0.0,
            y_edge: 1.0e9,
        };

        assert_eq!(pixel_coverage(&[segment], 0, FillRule::NonZero, 0, 2), 64);
    }

    #[test]
    fn build_tile_alpha_uses_segment_geometry() {
        let segment = LineSegment {
            p0x: 4.0,
            p0y: 0.0,
            p1x: 12.0,
            p1y: 16.0,
            y_edge: 1.0e9,
        };

        let alpha = build_tile_alpha(&[segment], 0, FillRule::NonZero);

        assert!(alpha[4 * 16 + 8] > 0);
    }

    #[test]
    fn build_tile_alpha_matches_pixel_coverage_reference() {
        let segments = [
            LineSegment {
                p0x: 4.0,
                p0y: 0.0,
                p1x: 12.0,
                p1y: 16.0,
                y_edge: 1.0e9,
            },
            LineSegment {
                p0x: 15.0,
                p0y: 2.0,
                p1x: 1.0,
                p1y: 14.0,
                y_edge: 1.0e9,
            },
            LineSegment {
                p0x: -2.0,
                p0y: 7.0,
                p1x: 18.0,
                p1y: 9.0,
                y_edge: 1.0e9,
            },
            LineSegment {
                p0x: 6.0,
                p0y: 0.0,
                p1x: 6.0,
                p1y: 16.0,
                y_edge: 1.0e9,
            },
        ];

        for rule in [FillRule::NonZero, FillRule::EvenOdd] {
            let alpha = build_tile_alpha(&segments, 1, rule);
            for y in 0..16 {
                for x in 0..16 {
                    let pixel_ix = y * 16 + x;
                    assert_eq!(
                        alpha[pixel_ix],
                        pixel_coverage(&segments, 1, rule, x as u32, y as u32),
                        "mismatch at ({x}, {y}) with {rule:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn composite_color_preserves_source_alpha_on_opaque_clip() {
        let mut tile: TileBuffer = [rgba8_pack([248, 249, 251, 255]); 256];
        let color = premul_f32_to_u32(Color::from_rgba8(37, 143, 93, 230).premultiply().components);

        composite_color_tile_buffer_into(&mut tile, color, &[255; 256]);

        assert_eq!(tile[0], rgba8_pack([57, 153, 109, 255]));
    }

    #[test]
    fn path_glyph_rasterization_uses_text_coverage_compositing() {
        let clip_mask = [128; 256];
        let mut normal_path = [rgba8_pack([255, 255, 255, 255]); 256];
        let mut path_glyph = normal_path;
        let brush = Brush::Solid(Color::BLACK);

        rasterize_tile_buffer_into(
            &mut normal_path,
            0,
            0,
            &[],
            1,
            FillRule::NonZero,
            &brush,
            &clip_mask,
            None,
        );
        rasterize_path_glyph_tile_buffer_into(
            &mut path_glyph,
            0,
            0,
            &[],
            1,
            FillRule::NonZero,
            &brush,
            &clip_mask,
            None,
        );

        let normal = unpack_rgba8(normal_path[0]);
        let glyph = unpack_rgba8(path_glyph[0]);
        let expected = unpack_rgba8(src_over_mask_linear_auto_u8(
            rgba8_pack([255, 255, 255, 255]),
            rgba8_pack([0, 0, 0, 255]),
            128,
        ));
        assert_ne!(glyph, normal);
        assert_eq!(glyph, expected);
        assert_eq!(glyph[3], 255);
    }

    #[test]
    fn rasterize_glyphs_preserves_subpixel_mask_channels() {
        let text = PreparedTextData::from_test_parts(
            vec![PreparedGlyph {
                image: Some(0),
                x: 0,
                y: 0,
            }],
            Vec::new(),
            vec![PreparedGlyphImage {
                content: PreparedGlyphContent::SubpixelMask,
                composite_mode: TextCompositeMode::Linear,
                coverage_params: TextCoverageParams::DEFAULT,
                left: 0,
                top: 0,
                width: 1,
                height: 1,
                data: vec![255, 0, 0],
            }],
        );
        let mut tile = [rgba8_pack([0, 0, 0, 255]); 256];

        rasterize_glyphs_tile_buffer_into(
            &mut tile,
            0,
            0,
            &[0],
            &Brush::Solid(Color::WHITE),
            &text,
            &[255; 256],
            None,
        );

        assert_eq!(unpack_rgba8(tile[0]), [255, 0, 0, 255]);
    }
}
