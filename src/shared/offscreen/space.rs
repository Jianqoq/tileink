//! Coordinate conversion for compact offscreen geometry and filter descriptors.
use super::*;

// Offscreen rendering reuses the main canvas data, but filter kernels and scratch
// images run in a local surface whose origin may be outside the canvas. Keeping the
// coordinate conversion in one type makes it clear which values move into local
// space and which values, such as fixed filter regions, stay in buffer space.
#[derive(Clone, Copy)]
pub(super) struct LocalSpace {
    pub(super) surface: Bounds,
}

impl LocalSpace {
    pub(super) const fn new(surface: Bounds) -> Self {
        Self { surface }
    }

    pub(super) fn line(self, line: Line) -> Line {
        let dx = -(self.surface.x0 as f32);
        let dy = -(self.surface.y0 as f32);
        Line {
            p0: [line.p0[0] + dx, line.p0[1] + dy],
            p1: [line.p1[0] + dx, line.p1[1] + dy],
            ..line
        }
    }

    // Saturate to the signed coordinate domain before clipping: wrapping a world
    // edge around a negative surface origin turns a full clip into an empty one.
    pub(super) const fn pixel_bounds(self, bounds: PixelBounds) -> PixelBounds {
        PixelBounds {
            x0: bounds.x0.saturating_sub(self.surface.x0),
            y0: bounds.y0.saturating_sub(self.surface.y0),
            x1: bounds.x1.saturating_sub(self.surface.x0),
            y1: bounds.y1.saturating_sub(self.surface.y0),
        }
    }

    pub(super) const fn bounds(self, bounds: Bounds) -> Bounds {
        Bounds::new(
            bounds.x0.saturating_sub(self.surface.x0),
            bounds.y0.saturating_sub(self.surface.y0),
            bounds.x1.saturating_sub(self.surface.x0),
            bounds.y1.saturating_sub(self.surface.y0),
        )
    }

    pub(super) fn rect(self, rect: Rect) -> Rect {
        let dx = f64::from(self.surface.x0);
        let dy = f64::from(self.surface.y0);
        Rect::new(rect.x0 - dx, rect.y0 - dy, rect.x1 - dx, rect.y1 - dy)
    }

    pub(super) fn transform(self, transform: Affine) -> Affine {
        Affine::translate((-f64::from(self.surface.x0), -f64::from(self.surface.y0))) * transform
    }

    pub(super) fn sdf(self, sdf: Sdf) -> Sdf {
        sdf.translated(self.surface.x0 as f32, self.surface.y0 as f32)
    }

    pub(super) fn sdf_shadow(self, sdf_shadow: SdfShadow) -> SdfShadow {
        sdf_shadow.translated(self.surface.x0 as f32, self.surface.y0 as f32)
    }

    pub(super) fn brush(self, brush: Brush) -> Brush {
        let ox = self.surface.x0 as f32;
        let oy = self.surface.y0 as f32;
        match brush {
            Brush::Solid(_) => brush,
            Brush::Linear(mut gradient) => {
                gradient.transform = self.brush_transform(gradient.transform);
                Brush::Linear(gradient)
            }
            Brush::Radial(mut gradient) => {
                gradient.transform = self.brush_transform(gradient.transform);
                Brush::Radial(gradient)
            }
            Brush::Sweep(mut gradient) => {
                gradient.center[0] -= ox;
                gradient.center[1] -= oy;
                Brush::Sweep(gradient)
            }
            Brush::FourCorner(mut gradient) => {
                gradient.bounds[0] -= ox;
                gradient.bounds[1] -= oy;
                gradient.bounds[2] -= ox;
                gradient.bounds[3] -= oy;
                Brush::FourCorner(gradient)
            }
            Brush::Mesh(mut gradient) => {
                gradient.transform = self.brush_transform(gradient.transform);
                Brush::Mesh(gradient)
            }
            Brush::Program(mut program) => {
                program.0.transform = self.brush_transform(program.0.transform);
                Brush::Program(program)
            }
            Brush::Pattern(mut pattern) => {
                pattern.transform = self.brush_transform(pattern.transform);
                Brush::Pattern(pattern)
            }
        }
    }

    pub(super) fn brush_transform(self, transform: [f32; 6]) -> [f32; 6] {
        let [a, b, c, d, e, f] = transform;
        let ox = self.surface.x0 as f32;
        let oy = self.surface.y0 as f32;
        [a, b, c, d, a * ox + c * oy + e, b * ox + d * oy + f]
    }

    pub(super) fn turbulence(self, turbulence: Turbulence) -> FilterPrimitiveKind {
        let mut turbulence = turbulence;
        turbulence.transform_x -= self.surface.x0 as f32;
        turbulence.transform_y -= self.surface.y0 as f32;
        turbulence.tile_x -= self.surface.x0 as f32;
        turbulence.tile_y -= self.surface.y0 as f32;
        FilterPrimitiveKind::Turbulence(turbulence)
    }
}
