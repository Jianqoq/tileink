use peniko::kurbo::Affine;

use crate::shared::bounds::PixelBounds;

/// GPU ABI for a 2D affine transform.
///
/// Coefficients follow kurbo's `[a, b, c, d, e, f]` convention:
/// `(x, y) -> (a*x + c*y + e, b*x + d*y + f)`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct GpuAffine {
    pub(crate) a: f32,
    pub(crate) b: f32,
    pub(crate) c: f32,
    pub(crate) d: f32,
    pub(crate) e: f32,
    pub(crate) f: f32,
}

impl GpuAffine {
    pub(crate) const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// Converts a logical-space node transform to the physical coordinates stored by Canvas.
    pub(crate) fn from_logical(transform: Affine, scale: f32) -> Self {
        let [a, b, c, d, e, f] = transform.as_coeffs();
        Self {
            a: a as f32,
            b: b as f32,
            c: c as f32,
            d: d as f32,
            e: e as f32 * scale,
            f: f as f32 * scale,
        }
    }

    pub(crate) fn inverse(self) -> Option<Self> {
        // A small determinant is not singular: animation scales can legitimately approach zero.
        // Use f64 intermediates to avoid f32 determinant underflow/overflow, then require the
        // actual GPU coefficients and inverse to be representable (root-cause fix).
        let coefficients = [self.a, self.b, self.c, self.d, self.e, self.f];
        if !coefficients.into_iter().all(f32::is_finite) {
            return None;
        }
        let [a, b, c, d, e, f] = coefficients.map(f64::from);
        let determinant = a * d - b * c;
        if determinant == 0.0 {
            return None;
        }
        let result = Self {
            a: (d / determinant) as f32,
            b: (-b / determinant) as f32,
            c: (-c / determinant) as f32,
            d: (a / determinant) as f32,
            e: ((c * f - d * e) / determinant) as f32,
            f: ((b * e - a * f) / determinant) as f32,
        };
        [result.a, result.b, result.c, result.d, result.e, result.f]
            .into_iter()
            .all(f32::is_finite)
            .then_some(result)
    }

    /// Matrix composition with kurbo semantics: `self * rhs` applies `rhs` first.
    #[allow(
        clippy::suspicious_operation_groupings,
        reason = "Affine composition multiplies matrix rows by columns, including cross-component products"
    )]
    pub(crate) fn compose(self, rhs: Self) -> Self {
        Self {
            a: self.a * rhs.a + self.c * rhs.b,
            b: self.b * rhs.a + self.d * rhs.b,
            c: self.a * rhs.c + self.c * rhs.d,
            d: self.b * rhs.c + self.d * rhs.d,
            e: self.a * rhs.e + self.c * rhs.f + self.e,
            f: self.b * rhs.e + self.d * rhs.f + self.f,
        }
    }

    pub(crate) fn transform_point(self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    pub(crate) fn transform_bounds(self, bounds: PixelBounds) -> PixelBounds {
        let corners = [
            self.transform_point(bounds.x0 as f32, bounds.y0 as f32),
            self.transform_point(bounds.x1 as f32, bounds.y0 as f32),
            self.transform_point(bounds.x0 as f32, bounds.y1 as f32),
            self.transform_point(bounds.x1 as f32, bounds.y1 as f32),
        ];
        let (mut x0, mut y0, mut x1, mut y1) = (
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        );
        for (x, y) in corners {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        PixelBounds {
            x0: x0.floor() as i32,
            y0: y0.floor() as i32,
            x1: x1.ceil() as i32,
            y1: y1.ceil() as i32,
        }
    }
}

impl Default for GpuAffine {
    fn default() -> Self {
        Self::IDENTITY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_handles_small_large_and_reflected_gpu_scales() {
        for scale in [1e-30, 1e-12, 1e-4, 1.0, 1e20, f64::from(f32::MAX)] {
            for sign in [-1.0, 1.0] {
                let transform =
                    GpuAffine::from_logical(Affine::scale_non_uniform(scale, sign * scale), 1.0);
                let inverse = transform.inverse().unwrap();
                let product = inverse.compose(transform);
                assert!((product.a - 1.0).abs() < 1e-6, "scale={scale}");
                assert!((product.d - 1.0).abs() < 1e-6, "scale={scale}");
                assert_eq!(
                    (product.b, product.c, product.e, product.f),
                    (0.0, 0.0, 0.0, 0.0)
                );
            }
        }
    }

    #[test]
    fn inverse_rejects_nonfinite_singular_and_unrepresentable_results() {
        for transform in [
            Affine::scale(0.0),
            Affine::scale(1e-40),
            Affine::new([1.0, 1.0, 1.0, 1.0, 0.0, 0.0]),
            Affine::translate((f64::INFINITY, 0.0)),
            Affine::new([f64::NAN, 0.0, 0.0, 1.0, 0.0, 0.0]),
        ] {
            assert!(GpuAffine::from_logical(transform, 1.0).inverse().is_none());
        }
    }
}
