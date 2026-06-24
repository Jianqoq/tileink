use crate::{
    shared::{
        brush::Brush,
        fill::FillRule,
        line_seg::LineSegment,
        pixel::{scale_premul_u8, src_over_premul_u8},
    },
    TILE_SIZE,
};

#[inline]
fn apply_rule(value: f32, fill_rule: FillRule) -> f32 {
    match fill_rule {
        FillRule::EvenOdd => (value - 2.0 * (0.5 * value).round()).abs(),
        FillRule::NonZero => value.abs().min(1.0),
    }
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
        let p0 = segment.point0;
        let p1 = segment.point1;
        let delta = (p1.0 - p0.0, p1.1 - p0.1);
        let row_y = y as f32;
        let local_y = p0.1 - row_y;
        let y0 = local_y.clamp(0.0, 1.0);
        let y1 = (local_y + delta.1).clamp(0.0, 1.0);
        let dy = y0 - y1;
        let y_edge = delta.0.signum() * (row_y - segment.y_edge + 1.0).clamp(0.0, 1.0);
        if dy != 0.0 {
            let recip = 1.0 / delta.1;
            let t0 = (y0 - local_y) * recip;
            let t1 = (y1 - local_y) * recip;
            let sx0 = p0.0 + t0 * delta.0;
            let sx1 = p0.0 + t1 * delta.0;
            let xmin = sx0.min(sx1) - x as f32;
            let xmax = sx0.max(sx1) - x as f32;
            let a_min = xmin.min(1.0) - 1e-6;
            let b = xmax.min(1.0);
            let c = b.max(0.0);
            let d = a_min.max(0.0);
            let a = (b + 0.5 * (d * d - c * c) - a_min) / (xmax - a_min);
            coverage += y_edge + a * dy;
        } else {
            coverage += y_edge;
        }
    }
    (apply_rule(coverage, fill_rule).clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn rasterize_tile(
    image: &mut [u32],
    image_width: u32,
    image_height: u32,
    tile_x: u32,
    tile_y: u32,
    segments: &[LineSegment],
    backdrop: i32,
    fill_rule: FillRule,
    brush: &Brush,
) {
    let base_x = tile_x * TILE_SIZE;
    let base_y = tile_y * TILE_SIZE;
    let tile_width = (image_width - base_x).min(TILE_SIZE);
    let tile_height = (image_height - base_y).min(TILE_SIZE);
    for local_y in 0..tile_height {
        let global_y = base_y + local_y;
        for local_x in 0..tile_width {
            let alpha = pixel_coverage(segments, backdrop, fill_rule, local_x, local_y);
            if alpha == 0 {
                continue;
            }
            let global_x = base_x + local_x;
            let src = scale_premul_u8(brush.sample(global_x as f32 + 0.5, global_y as f32 + 0.5), alpha);
            let pixel_ix = (global_y * image_width + global_x) as usize;
            image[pixel_ix] = src_over_premul_u8(image[pixel_ix], src);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::pixel_coverage;
    use crate::shared::{
        fill::FillRule,
    };

    #[test]
    fn pixel_coverage_uses_backdrop_when_tile_has_no_segments() {
        assert_eq!(pixel_coverage(&[], 0, FillRule::NonZero, 0, 0), 0);
        assert_eq!(pixel_coverage(&[], -1, FillRule::NonZero, 8, 8), 255);
        assert_eq!(pixel_coverage(&[], 2, FillRule::NonZero, 15, 15), 255);
        assert_eq!(pixel_coverage(&[], 1, FillRule::EvenOdd, 3, 4), 255);
        assert_eq!(pixel_coverage(&[], 2, FillRule::EvenOdd, 3, 4), 0);
    }
}
