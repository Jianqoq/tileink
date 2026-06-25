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

fn build_tile_alpha(segments: &[LineSegment], backdrop: i32, fill_rule: FillRule) -> [u8; 256] {
    let mut tile_alpha = [0u8; 256];
    for y in 0..TILE_SIZE {
        let row_start = (y * TILE_SIZE) as usize;
        for x in 0..TILE_SIZE {
            tile_alpha[row_start + x as usize] = pixel_coverage(segments, backdrop, fill_rule, x, y);
        }
    }
    tile_alpha
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
    let tile_alpha = build_tile_alpha(segments, backdrop, fill_rule);
    for local_y in 0..tile_height {
        let global_y = base_y + local_y;
        let row_start = (local_y * TILE_SIZE) as usize;
        let row = &tile_alpha[row_start..row_start + tile_width as usize];
        let mut local_x = 0usize;
        while local_x < row.len() {
            if row[local_x] == 0 {
                local_x += 1;
                continue;
            }
            let span_start = local_x;
            while local_x < row.len() && row[local_x] != 0 {
                local_x += 1;
            }
            for span_x in span_start..local_x {
                let alpha = row[span_x];
                let global_x = base_x + span_x as u32;
                let src = scale_premul_u8(
                    brush.sample(global_x as f32 + 0.5, global_y as f32 + 0.5),
                    alpha,
                );
                let pixel_ix = (global_y * image_width + global_x) as usize;
                image[pixel_ix] = src_over_premul_u8(image[pixel_ix], src);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::pixel_coverage;
    use crate::shared::{
        fill::FillRule,
        line_seg::LineSegment,
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
            point0: (4.0, 0.0),
            point1: (12.0, 16.0),
            y_edge: 1.0e9,
            ..LineSegment::default()
        };

        assert_eq!(pixel_coverage(&[segment], 0, FillRule::NonZero, 0, 4), 0);
        assert!(pixel_coverage(&[segment], 0, FillRule::NonZero, 8, 4) > 0);
    }
}
