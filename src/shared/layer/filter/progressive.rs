use peniko::kurbo::{Point, Vec2};

/// Spatially varying, isotropic blur for UI content and backdrop layers.
///
/// Positions and standard deviation use logical canvas pixels. Before `start`
/// the image is unchanged; after `end` it keeps `max_std_dev`. Between them the
/// projected position follows smoothstep. Reversing the endpoints reverses the
/// effect. Coincident endpoints mean a uniform maximum blur.
///
/// The GPU uses a calibrated Gaussian scale pyramid and variance interpolation,
/// not an exact Gaussian and not a crossfade with one maximally blurred image.
/// Filtering uses premultiplied RGBA in the enclosing filter's working space,
/// with transparent samples outside its source domain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProgressiveBlur {
    pub start: Point,
    pub end: Point,
    /// Nonnegative standard deviation. The device-space limit is 65536 pixels.
    /// Nonfinite/negative values and nonfinite endpoints fail render validation.
    pub max_std_dev: f32,
    /// Accuracy/cost policy, independent of the requested blur strength.
    pub quality: ProgressiveBlurQuality,
}

/// Sampling policy for progressive blur. Both policies preserve fine detail by
/// evaluating subpixel sigma directly and delaying pyramid downsampling.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProgressiveBlurQuality {
    /// Two levels per octave, with Gaussian support of at least three sigma.
    #[default]
    Balanced,
    /// Three levels per octave, later downsampling and four-sigma pyramid kernels.
    High,
}

impl ProgressiveBlur {
    pub const fn new(start: Point, end: Point, max_std_dev: f32) -> Self {
        Self {
            start,
            end,
            max_std_dev,
            quality: ProgressiveBlurQuality::Balanced,
        }
    }

    pub const fn with_quality(mut self, quality: ProgressiveBlurQuality) -> Self {
        self.quality = quality;
        self
    }

    pub(crate) fn translated(self, offset: Vec2) -> Self {
        Self {
            start: self.start + offset,
            end: self.end + offset,
            ..self
        }
    }

    pub(crate) fn scaled(self, scale: f64) -> Self {
        Self {
            start: Point::new(self.start.x * scale, self.start.y * scale),
            end: Point::new(self.end.x * scale, self.end.y * scale),
            max_std_dev: self.max_std_dev * scale as f32,
            ..self
        }
    }

    pub(crate) fn projection(self) -> Option<[f32; 4]> {
        let delta = self.end - self.start;
        let length2 = delta.hypot2();
        if !self.max_std_dev.is_finite()
            || !(0.0..=65536.0).contains(&self.max_std_dev)
            || !length2.is_finite()
        {
            return None;
        }
        let projection = if length2 == 0.0 {
            // A zero vector is an explicit uniform-blur sentinel in the shader.
            [self.start.x as f32, self.start.y as f32, 0.0, 0.0]
        } else {
            [
                self.start.x as f32,
                self.start.y as f32,
                (delta.x / length2) as f32,
                (delta.y / length2) as f32,
            ]
        };
        projection
            .iter()
            .all(|v| v.is_finite())
            .then_some(projection)
    }

    pub(crate) fn sample_outset(self) -> i32 {
        if self.max_std_dev <= 0.0 || self.projection().is_none() {
            return 0;
        }
        // Cover the support of the upper bracketing pyramid level, including
        // reconstruction. A Gaussian's usual 3*sigma halo is insufficient here.
        (20.0 * self.max_std_dev + 8.0).ceil() as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_supports_reverse_diagonal_and_uniform_gradients() {
        let blur = ProgressiveBlur::new(Point::new(10.0, 20.0), Point::new(30.0, 40.0), 8.0);
        let p = blur.projection().unwrap();
        assert_eq!(p, [10.0, 20.0, 0.025, 0.025]);
        assert_eq!(
            ProgressiveBlur::new(blur.end, blur.start, 8.0)
                .projection()
                .unwrap()[2],
            -0.025
        );
        assert_eq!(
            ProgressiveBlur::new(blur.start, blur.start, 8.0)
                .projection()
                .unwrap()[2..],
            [0.0, 0.0]
        );
    }

    #[test]
    fn invalid_parameters_are_rejected_and_zero_has_no_halo() {
        for sigma in [-1.0, f32::NAN, f32::INFINITY, 65537.0] {
            assert!(
                ProgressiveBlur::new(Point::ZERO, Point::new(0.0, 1.0), sigma)
                    .projection()
                    .is_none()
            );
        }
        assert!(
            ProgressiveBlur::new(Point::new(f64::NAN, 0.0), Point::ZERO, 1.0)
                .projection()
                .is_none()
        );
        assert_eq!(
            ProgressiveBlur::new(Point::ZERO, Point::ZERO, 0.0).sample_outset(),
            0
        );
    }

    #[test]
    fn quality_is_independent_of_strength_and_survives_coordinate_changes() {
        let blur = ProgressiveBlur::new(Point::ZERO, Point::new(4.0, 8.0), 2.0);
        assert_eq!(blur.quality, ProgressiveBlurQuality::Balanced);
        let high = blur.with_quality(ProgressiveBlurQuality::High);
        assert_eq!(blur.projection(), high.projection());
        assert_eq!(high.scaled(2.0).quality, ProgressiveBlurQuality::High);
        assert_eq!(
            high.translated(Vec2::new(2.0, 4.0)).quality,
            ProgressiveBlurQuality::High
        );
        assert_eq!(high.max_std_dev, blur.max_std_dev);
    }
}
