use crate::{
    shared::{
        brush::Brush,
        coverage::Coverage,
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
    let mut coverage = apply_rule(backdrop as f32, fill_rule).clamp(0.0, 1.0) * 255.0;
    for segment in segments {
        for segment_coverage in &segment.coverages {
            if let Some(alpha) = lookup_coverage_alpha(segment_coverage, x, y) {
                coverage = coverage.max(alpha as f32);
            }
        }
    }
    coverage.clamp(0.0, 255.0) as u8
}

fn lookup_coverage_alpha(coverage: &Coverage, x: u32, y: u32) -> Option<u8> {
    let pixel_ix = (y * TILE_SIZE + x) as u8;
    for &(ix, alpha) in coverage.alphas.iter().take(coverage.alpha_cnt as usize) {
        if ix == pixel_ix {
            return Some(alpha);
        }
    }
    None
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
        coverage::Coverage,
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
    fn pixel_coverage_consumes_segment_coverages() {
        let mut segment = LineSegment::default();
        segment.coverages[0] = Coverage {
            alphas: [(3, 200); 32],
            alpha_cnt: 1,
            is_left: false,
        };

        assert_eq!(pixel_coverage(&[segment], 0, FillRule::NonZero, 3, 0), 200);
        assert_eq!(pixel_coverage(&[segment], 0, FillRule::NonZero, 4, 0), 0);
    }
}
