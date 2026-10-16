//! Fixed vertex primitives share the path fitter's edge and stroke rules without path allocation.
use super::{Affine, PixelGrid, Point, Stroke, Vec2, fit_edge, uniform_axis_scale};

impl PixelGrid {
    /// Fits a closed polygon's horizontal/vertical edges to pixel boundaries.
    ///
    /// This follows [`Self::fill_path`] using fixed vertex storage, including its opt-out for
    /// nonuniform, rotated or skewed transforms. Empty and degenerate polygons are supported.
    #[must_use]
    pub fn fill_polygon<const N: usize>(self, points: [Point; N], transform: Affine) -> [Point; N] {
        self.fit_vertices(points, transform, 0.0, true)
    }

    /// Fits a line and stroke width with the same device-pixel phase as [`Self::stroke_path`].
    ///
    /// The endpoints remain typed throughout fitting; no temporary path or topology conversion
    /// is needed for SDF lines. Unsupported transforms and nonpositive widths retain their input.
    #[must_use]
    pub fn stroke_line(
        self,
        points: [Point; 2],
        stroke: &Stroke,
        transform: Affine,
    ) -> ([Point; 2], Stroke) {
        let Some((stroke, phase)) = self.fitted_stroke(stroke, transform) else {
            return (points, stroke.clone());
        };
        (self.fit_vertices(points, transform, phase, false), stroke)
    }

    fn fit_vertices<const N: usize>(
        self,
        points: [Point; N],
        transform: Affine,
        phase: f64,
        closed: bool,
    ) -> [Point; N] {
        if uniform_axis_scale(transform).is_none() {
            return points;
        }
        let device = Affine::scale(self.scale) * transform;
        let inverse = device.inverse();
        let vertices = points.map(|point| device * point);
        if !inverse.is_finite() || vertices.iter().any(|point| !point.is_finite()) {
            return points;
        }
        let mut shifts = [Vec2::ZERO; N];
        for end in 1..N {
            fit_edge(
                vertices[end - 1],
                vertices[end],
                &mut shifts,
                end - 1,
                end,
                phase,
            );
        }
        if closed && N > 0 {
            fit_edge(vertices[N - 1], vertices[0], &mut shifts, N - 1, 0, phase);
        }
        std::array::from_fn(|index| inverse * (vertices[index] + shifts[index]))
    }
}

#[cfg(test)]
mod tests;
