use crate::{
    TILE_SIZE,
    shared::{
        bounds::Bounds,
        brush::Brush,
        fill::FillRule,
        line_seg::LineSegment,
        pixel::{MASK_OPAQUE, scale_premul_u8, src_over_premul_u8},
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
    (apply_rule(coverage, fill_rule).clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

pub(crate) fn build_tile_alpha(
    segments: &[LineSegment],
    backdrop: i32,
    fill_rule: FillRule,
) -> [u8; 256] {
    let fill_alpha = (apply_rule(backdrop as f32, fill_rule).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    let mut tile_alpha = [fill_alpha; 256];
    if segments.is_empty() {
        return tile_alpha;
    }

    for pixel_ix in 0..tile_alpha.len() {
        let x = (pixel_ix % TILE_SIZE as usize) as u32;
        let y = (pixel_ix / TILE_SIZE as usize) as u32;
        tile_alpha[pixel_ix] = pixel_coverage(segments, backdrop, fill_rule, x, y);
    }
    tile_alpha
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn rasterize_tile_into(
    image: &mut [u32],
    image_width: u32,
    image_height: u32,
    origin_x: i32,
    origin_y: i32,
    tile_x: u32,
    tile_y: u32,
    segments: &[LineSegment],
    backdrop: i32,
    fill_rule: FillRule,
    brush: &Brush,
) {
    let base_x = tile_x * TILE_SIZE;
    let base_y = tile_y * TILE_SIZE;
    let target_x0 = origin_x;
    let target_y0 = origin_y;
    let target_x1 = origin_x + image_width as i32;
    let target_y1 = origin_y + image_height as i32;
    let global_x0 = base_x as i32;
    let global_y0 = base_y as i32;
    let global_x1 = global_x0 + TILE_SIZE as i32;
    let global_y1 = global_y0 + TILE_SIZE as i32;
    let clip_x0 = global_x0.max(target_x0);
    let clip_y0 = global_y0.max(target_y0);
    let clip_x1 = global_x1.min(target_x1);
    let clip_y1 = global_y1.min(target_y1);
    if clip_x0 >= clip_x1 || clip_y0 >= clip_y1 {
        return;
    }

    let tile_alpha = build_tile_alpha(segments, backdrop, fill_rule);
    for global_y in clip_y0..clip_y1 {
        let tile_local_y = (global_y - global_y0) as u32;
        let row_start = (tile_local_y * TILE_SIZE) as usize;
        for global_x in clip_x0..clip_x1 {
            let tile_local_x = (global_x - global_x0) as usize;
            let alpha = tile_alpha[row_start + tile_local_x];
            if alpha == 0 {
                continue;
            }
            let src = scale_premul_u8(
                brush.sample(global_x as f32 + 0.5, global_y as f32 + 0.5),
                alpha,
            );
            let local_x = (global_x - target_x0) as u32;
            let local_y = (global_y - target_y0) as u32;
            let pixel_ix = (local_y * image_width + local_x) as usize;
            image[pixel_ix] = src_over_premul_u8(image[pixel_ix], src);
        }
    }
}

pub(crate) fn composite_color_tile_into(
    image: &mut [u32],
    image_width: u32,
    image_height: u32,
    origin_x: i32,
    origin_y: i32,
    tile_x: u32,
    tile_y: u32,
    color: u32,
) {
    let global_tile = Bounds::from_tile_coords(
        tile_x,
        tile_y,
        origin_x.saturating_add_unsigned(image_width) as u32,
        origin_y.saturating_add_unsigned(image_height) as u32,
    );
    let target = Bounds::new(
        origin_x,
        origin_y,
        origin_x + image_width as i32,
        origin_y + image_height as i32,
    );
    let bounds = global_tile.intersect(target);
    if bounds.is_empty() {
        return;
    }

    let alpha = (color >> 24) as u8;
    if alpha == 0 {
        return;
    }

    for y in bounds.y0..bounds.y1 {
        let local_y = (y - origin_y) as u32;
        let local_x0 = (bounds.x0 - origin_x) as u32;
        let local_x1 = (bounds.x1 - origin_x) as u32;
        let row_start = (local_y * image_width + local_x0) as usize;
        let row_end = (local_y * image_width + local_x1) as usize;
        let row = &mut image[row_start..row_end];
        if alpha == MASK_OPAQUE {
            row.fill(color);
            continue;
        }
        for dst in row {
            *dst = src_over_premul_u8(*dst, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{build_tile_alpha, pixel_coverage};
    use crate::shared::{fill::FillRule, line_seg::LineSegment, pixel::TileMask};

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
    fn build_tile_alpha_recomputes_pixels_outside_edge_mask() {
        let mut edges = TileMask::new();
        edges.set(0);
        let segment = LineSegment {
            point0: (4.0, 0.0),
            point1: (12.0, 16.0),
            y_edge: 1.0e9,
            edges,
            ..LineSegment::default()
        };

        let alpha = build_tile_alpha(&[segment], 0, FillRule::NonZero);

        assert!(alpha[4 * 16 + 8] > 0);
    }
}
