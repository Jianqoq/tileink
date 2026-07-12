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
        let determinant = self.a * self.d - self.b * self.c;
        if !determinant.is_finite() || determinant.abs() <= f32::EPSILON {
            return None;
        }
        let inverse = determinant.recip();
        let a = self.d * inverse;
        let b = -self.b * inverse;
        let c = -self.c * inverse;
        let d = self.a * inverse;
        Some(Self {
            a,
            b,
            c,
            d,
            e: -(a * self.e + c * self.f),
            f: -(b * self.e + d * self.f),
        })
    }

    /// Matrix composition with kurbo semantics: `self * rhs` applies `rhs` first.
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
