//! Opt-in pixel fitting for small UI glyphs, after their complete scale is known.
use peniko::kurbo::{Affine, BezPath, PathEl, Point, Stroke, Vec2};

mod primitives;

/// Physical pixel grid for vector glyphs whose final origin is on a device-pixel boundary.
///
/// The scale includes the surface DPI and any uniform ancestor scale. Pixel fitting changes
/// only axis-aligned edges and stroke width; diagonal and curved segments retain antialiasing.
/// Do not use this for plots, illustrations, or geometry whose subpixel positions carry meaning.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelGrid {
    scale: f64,
}

impl PixelGrid {
    #[must_use]
    pub fn new(scale: f64) -> Option<Self> {
        (scale.is_finite() && scale > 0.0).then_some(Self { scale })
    }

    /// Fits filled horizontal/vertical edges to pixel boundaries.
    #[must_use]
    pub fn fill_path(self, path: &BezPath, transform: Affine) -> BezPath {
        self.fit(path, transform, 0.0)
    }

    /// Fits stroke width to whole pixels and axis-aligned centerlines to the corresponding
    /// whole/half-pixel phase. Nonuniform, rotated and skewed path transforms remain untouched.
    #[must_use]
    pub fn stroke_path(
        self,
        path: &BezPath,
        stroke: &Stroke,
        transform: Affine,
    ) -> (BezPath, Stroke) {
        let Some((fitted, phase)) = self.fitted_stroke(stroke, transform) else {
            return (path.clone(), stroke.clone());
        };
        (self.fit(path, transform, phase), fitted)
    }

    fn fitted_stroke(self, stroke: &Stroke, transform: Affine) -> Option<(Stroke, f64)> {
        let (width, phase) = self.fitted_width(stroke.width, transform)?;
        let mut fitted = stroke.clone();
        fitted.width = width;
        Some((fitted, phase))
    }

    /// Fits width alone for curved/SDF primitives whose center geometry must remain untouched.
    #[must_use]
    pub fn stroke_width(self, width: f64, transform: Affine) -> f64 {
        self.fitted_width(width, transform)
            .map_or(width, |(width, _)| width)
    }

    fn fitted_width(self, width: f64, transform: Affine) -> Option<(f64, f64)> {
        let scale = uniform_axis_scale(transform)?;
        let device_scale = scale * self.scale;
        let physical_width = width * device_scale;
        if !physical_width.is_finite() || physical_width <= 0.0 || !device_scale.is_finite() {
            return None;
        }
        let pixels = physical_width.round().max(1.0);
        let width = pixels / device_scale;
        if !width.is_finite() {
            return None;
        }
        Some((width, (pixels % 2.0) * 0.5))
    }

    fn fit(self, path: &BezPath, transform: Affine, phase: f64) -> BezPath {
        if uniform_axis_scale(transform).is_none() {
            return path.clone();
        }
        let device = Affine::scale(self.scale) * transform;
        let transformed = device * path;
        if !transformed.is_finite() || !device.inverse().is_finite() {
            return path.clone();
        }
        let mut elements = transformed.elements().to_vec();
        let mut shifts = vec![Vec2::ZERO; elements.len()];
        let mut current = None;
        let mut start = None;
        for (index, element) in elements.iter().enumerate() {
            match *element {
                PathEl::MoveTo(_) => start = Some(index),
                PathEl::LineTo(_) | PathEl::ClosePath => {
                    let end = if matches!(element, PathEl::ClosePath) {
                        start
                    } else {
                        Some(index)
                    };
                    if let (Some(a), Some(b)) = (current, end)
                        && let (Some(start), Some(end)) =
                            (endpoint(elements[a]), endpoint(elements[b]))
                    {
                        fit_edge(start, end, &mut shifts, a, b, phase);
                    }
                }
                _ => {}
            }
            current = if matches!(element, PathEl::ClosePath) {
                start
            } else {
                Some(index)
            };
        }
        let mut previous_shift = Vec2::ZERO;
        let mut start_shift = Vec2::ZERO;
        for (element, shift) in elements.iter_mut().zip(shifts) {
            *element = match *element {
                PathEl::MoveTo(p) => {
                    start_shift = shift;
                    PathEl::MoveTo(p + shift)
                }
                PathEl::LineTo(p) => PathEl::LineTo(p + shift),
                PathEl::QuadTo(c, p) => {
                    PathEl::QuadTo(c + (previous_shift + shift) * 0.5, p + shift)
                }
                PathEl::CurveTo(a, b, p) => {
                    PathEl::CurveTo(a + previous_shift, b + shift, p + shift)
                }
                PathEl::ClosePath => PathEl::ClosePath,
            };
            previous_shift = if matches!(element, PathEl::ClosePath) {
                start_shift
            } else {
                shift
            };
        }
        device.inverse() * BezPath::from_vec(elements)
    }
}

/// A stroke can have one device width only when both axes have the same absolute scale.
#[must_use]
pub fn uniform_axis_scale(transform: Affine) -> Option<f64> {
    let [a, b, c, d, e, f] = transform.as_coeffs();
    let scale = a.abs();
    ([a, b, c, d, e, f].iter().all(|v| v.is_finite())
        && scale > f64::EPSILON
        && b.abs() <= f64::EPSILON
        && c.abs() <= f64::EPSILON
        && (scale - d.abs()).abs() <= scale * 1e-9)
        .then_some(scale)
}

const fn endpoint(element: PathEl) -> Option<Point> {
    match element {
        PathEl::MoveTo(p) | PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => {
            Some(p)
        }
        PathEl::ClosePath => None,
    }
}

fn fit_edge(start: Point, end: Point, shifts: &mut [Vec2], a: usize, b: usize, phase: f64) {
    let snap = |v: f64| (v - phase).round() + phase - v;
    if (start.x - end.x).abs() <= 1e-7 {
        let dx = snap(start.x);
        shifts[a].x = dx;
        shifts[b].x = dx;
    }
    if (start.y - end.y).abs() <= 1e-7 {
        let dy = snap(start.y);
        shifts[a].y = dy;
        shifts[b].y = dy;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn path() -> BezPath {
        BezPath::from_svg("M2.2 4.2H16.2V18.2H2.2Z").unwrap()
    }
    fn integral(value: f64) {
        assert!((value - value.round()).abs() < 1e-8, "{value}");
    }

    #[test]
    fn stroke_edges_fit_odd_and_even_device_widths_at_fractional_dpi_and_zoom() {
        for dpi in [1.0, 1.25, 1.5, 2.0, 3.0] {
            for zoom in [0.8, 1.0, 1.25, 2.0] {
                for width in [0.75, 1.0, 1.5, 2.0] {
                    let grid = PixelGrid::new(dpi).unwrap();
                    let transform = Affine::translate((0.3, 0.7)) * Affine::scale(zoom);
                    let (fitted, stroke) =
                        grid.stroke_path(&path(), &Stroke::new(width), transform);
                    let device = Affine::scale(dpi) * transform * fitted;
                    let half = stroke.width * zoom * dpi * 0.5;
                    integral(half * 2.0);
                    for point in device.elements().iter().filter_map(|e| endpoint(*e)) {
                        integral(point.x - half);
                        integral(point.x + half);
                        integral(point.y - half);
                        integral(point.y + half);
                    }
                }
            }
        }
    }

    #[test]
    fn fills_close_on_pixel_edges_and_reflections_are_supported() {
        for scale in [1.0, 1.25, 2.0] {
            let grid = PixelGrid::new(scale).unwrap();
            let transform = Affine::translate((20.3, 0.7)) * Affine::scale_non_uniform(-1.0, 1.0);
            let fitted = Affine::scale(scale) * transform * grid.fill_path(&path(), transform);
            for point in fitted.elements().iter().filter_map(|e| endpoint(*e)) {
                integral(point.x);
                integral(point.y);
            }
        }
    }

    #[test]
    fn curves_and_diagonals_preserve_shape_and_antialiasing() {
        let path = BezPath::from_svg("M2.2 4.2L8.7 9.1C10.1 3.7 15.2 1.6 19.3 7.4").unwrap();
        let grid = PixelGrid::new(1.25).unwrap();
        let fitted = grid.fill_path(&path, Affine::IDENTITY);
        for (actual, expected) in fitted
            .elements()
            .iter()
            .filter_map(|e| endpoint(*e))
            .zip(path.elements().iter().filter_map(|e| endpoint(*e)))
        {
            assert!(actual.distance(expected) < 1e-8);
        }
    }

    #[test]
    fn adjusted_curve_endpoints_keep_their_tangent_handles() {
        let path = BezPath::from_svg("M2.2 4.2H8.2C10.2 4.2 12.2 7.3 12.2 9.3V18.3").unwrap();
        let fitted = PixelGrid::new(1.0)
            .unwrap()
            .fill_path(&path, Affine::IDENTITY);
        let PathEl::CurveTo(a, b, p) = fitted.elements()[2] else {
            panic!()
        };
        assert_eq!(a, Point::new(10.2, 4.0));
        assert_eq!(b, Point::new(12.0, 7.3));
        assert_eq!(p, Point::new(12.0, 9.3));
    }

    #[test]
    fn zero_width_and_extreme_coordinates_do_not_create_visible_or_nonfinite_geometry() {
        let grid = PixelGrid::new(2.0).unwrap();
        for width in [0.0, -1.0, f64::INFINITY] {
            let (actual, stroke) = grid.stroke_path(&path(), &Stroke::new(width), Affine::IDENTITY);
            assert_eq!(actual, path());
            assert_eq!(stroke.width, width);
        }
        let grid = PixelGrid::new(f64::MAX).unwrap();
        assert_eq!(grid.fill_path(&path(), Affine::IDENTITY), path());
        let (actual, stroke) = grid.stroke_path(&path(), &Stroke::new(2.0), Affine::IDENTITY);
        assert_eq!(actual, path());
        assert_eq!(stroke.width, 2.0);
    }

    #[test]
    fn rotation_skew_and_nonuniform_scale_do_not_deform_artwork() {
        let grid = PixelGrid::new(2.0).unwrap();
        for transform in [
            Affine::rotate(0.3),
            Affine::skew(0.2, 0.0),
            Affine::scale_non_uniform(2.0, 1.0),
        ] {
            assert_eq!(grid.fill_path(&path(), transform), path());
            let original = Stroke::new(1.25);
            let (actual, stroke) = grid.stroke_path(&path(), &original, transform);
            assert_eq!(actual, path());
            assert_eq!(stroke.width, original.width);
        }
        for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(PixelGrid::new(scale).is_none());
        }
    }
}
