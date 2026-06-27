use peniko::BlendMode;

use crate::{
    TILE_SIZE,
    shared::{
        brush::Brush,
        fill::FillRule,
        layer::blend::Blend,
        line_seg::LineSegment,
        pixel::{
            TileBuffer, pack_premul_rgba8, scale_premul_u8, src_over_premul_u8, unpack_premul_rgba8,
        },
    },
};

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
    let p0 = segment.point0;
    let p1 = segment.point1;
    let delta_x = p1.0 - p0.0;
    let delta_y = p1.1 - p0.1;
    let row_y = y as f32;
    let local_y = p0.1 - row_y;
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
    let sx0 = p0.0 + t0 * delta_x;
    let sx1 = p0.0 + t1 * delta_x;
    let pixel_x = x as f32;
    let xmin = sx0.min(sx1) - pixel_x;
    let xmax = sx0.max(sx1) - pixel_x;
    let a_min = xmin.min(1.0) - 1.0e-6;
    let b = xmax.min(1.0);
    let c = b.max(0.0);
    let d = a_min.max(0.0);
    let a = (b + 0.5 * (d * d - c * c) - a_min) / (xmax - a_min);
    y_edge + a * dy
}

#[inline]
fn segment_row_parts(segment: &LineSegment, y: u32) -> (f32, f32, f32, f32) {
    let p0 = segment.point0;
    let p1 = segment.point1;
    let delta_x = p1.0 - p0.0;
    let delta_y = p1.1 - p0.1;
    let row_y = y as f32;
    let local_y = p0.1 - row_y;
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
    let sx0 = p0.0 + t0 * delta_x;
    let sx1 = p0.0 + t1 * delta_x;

    (y_edge, dy, sx0.min(sx1), sx0.max(sx1))
}

#[inline]
fn segment_area_at(xmin: f32, xmax: f32, x: u32) -> f32 {
    let pixel_x = x as f32;
    let xmin = xmin - pixel_x;
    let xmax = xmax - pixel_x;
    let a_min = xmin.min(1.0) - 1.0e-6;
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
            let src = scale_premul_u8(
                brush.sample((base_x + x as u32) as f32 + 0.5, (base_y + y) as f32 + 0.5),
                alpha,
            );
            tile[ix] = src_over_premul_u8(tile[ix], src);
        }
    }
}

pub(crate) fn composite_color_tile_buffer_into(
    tile: &mut TileBuffer,
    color: u32,
    clip_mask: &[u8; 256],
) {
    let color_alpha = (color >> 24) as u8;
    if color_alpha == 0 {
        return;
    }
    for (dst, &mask) in tile.iter_mut().zip(clip_mask) {
        let alpha = combine_alpha(color_alpha, mask);
        if alpha == 0 {
            continue;
        }
        *dst = src_over_premul_u8(*dst, scale_premul_u8(color, alpha));
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
    use super::{build_tile_alpha, pixel_coverage};
    use crate::shared::{fill::FillRule, line_seg::LineSegment};

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
            point0: (4.0, 0.0),
            point1: (12.0, 16.0),
            y_edge: 1.0e9,
            ..LineSegment::default()
        };

        assert_eq!(pixel_coverage(&[segment], 0, FillRule::NonZero, 0, 4), 0);
        assert!(pixel_coverage(&[segment], 0, FillRule::NonZero, 8, 4) > 0);
    }

    #[test]
    fn build_tile_alpha_uses_segment_geometry() {
        let segment = LineSegment {
            point0: (4.0, 0.0),
            point1: (12.0, 16.0),
            y_edge: 1.0e9,
            ..LineSegment::default()
        };

        let alpha = build_tile_alpha(&[segment], 0, FillRule::NonZero);

        assert!(alpha[4 * 16 + 8] > 0);
    }

    #[test]
    fn build_tile_alpha_matches_pixel_coverage_reference() {
        let segments = [
            LineSegment {
                point0: (4.0, 0.0),
                point1: (12.0, 16.0),
                y_edge: 1.0e9,
                ..LineSegment::default()
            },
            LineSegment {
                point0: (15.0, 2.0),
                point1: (1.0, 14.0),
                y_edge: 1.0e9,
                ..LineSegment::default()
            },
            LineSegment {
                point0: (-2.0, 7.0),
                point1: (18.0, 9.0),
                y_edge: 1.0e9,
                ..LineSegment::default()
            },
            LineSegment {
                point0: (6.0, 0.0),
                point1: (6.0, 16.0),
                y_edge: 1.0e9,
                ..LineSegment::default()
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
}
