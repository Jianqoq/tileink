use peniko::kurbo::Point;

use super::{SOLID_DIST, coverage_from_dist};
use crate::{TILE_SIZE, shared::bounds::Bounds};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Radius {
    pub top_left: f32,
    pub top_right: f32,
    pub bottom_left: f32,
    pub bottom_right: f32,
}

impl Radius {
    pub const ZERO: Self = Self::all(0.0);

    pub const fn all(radius: f32) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_left: radius,
            bottom_right: radius,
        }
    }

    pub(crate) fn is_zero(self) -> bool {
        const EPS: f32 = 1e-6;
        self.top_left <= EPS
            && self.top_right <= EPS
            && self.bottom_left <= EPS
            && self.bottom_right <= EPS
    }

    pub(crate) fn is_uniform(self) -> bool {
        const EPS: f32 = 1e-6;
        (self.top_left - self.top_right).abs() <= EPS
            && (self.top_left - self.bottom_left).abs() <= EPS
            && (self.top_left - self.bottom_right).abs() <= EPS
    }

    fn offset_corners(
        self,
        top_left: f32,
        top_right: f32,
        bottom_left: f32,
        bottom_right: f32,
    ) -> Self {
        Self {
            top_left: (self.top_left + top_left).max(0.0),
            top_right: (self.top_right + top_right).max(0.0),
            bottom_left: (self.bottom_left + bottom_left).max(0.0),
            bottom_right: (self.bottom_right + bottom_right).max(0.0),
        }
    }
}

/// Per-side full stroke widths for centered rectangle strokes.
///
/// The SDF renderer expands the outer edge and shrinks the inner edge by half
/// of each side width, matching the existing `Stroke::width` centered-stroke
/// semantics while allowing each side to differ.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrokeWidths {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl StrokeWidths {
    pub fn all(width: f32) -> Self {
        Self {
            top: width,
            right: width,
            bottom: width,
            left: width,
        }
    }

    pub(crate) fn clamped(self) -> Self {
        Self {
            top: self.top.max(0.0),
            right: self.right.max(0.0),
            bottom: self.bottom.max(0.0),
            left: self.left.max(0.0),
        }
    }

    pub(crate) fn is_empty(self) -> bool {
        let widths = self.clamped();
        widths.top == 0.0 && widths.right == 0.0 && widths.bottom == 0.0 && widths.left == 0.0
    }

    pub(crate) fn half(self) -> Self {
        let widths = self.clamped();
        Self {
            top: widths.top * 0.5,
            right: widths.right * 0.5,
            bottom: widths.bottom * 0.5,
            left: widths.left * 0.5,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub start: Point,
    pub end: Point,
    pub radius: Radius,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectShadowOptions {
    pub offset_x: f32,
    pub offset_y: f32,
    /// Exponential falloff distance in pixels. Four expand lengths cover more
    /// than 98% of the visible soft shadow while keeping tile bounds finite.
    pub expand: f32,
    pub intensity: f32,
}

impl RectShadowOptions {
    pub fn new(offset_x: f32, offset_y: f32, expand: f32, intensity: f32) -> Self {
        Self {
            offset_x,
            offset_y,
            expand,
            intensity,
        }
    }

    pub(crate) fn normalized(self) -> Option<Self> {
        let intensity = self.intensity.clamp(0.0, 1.0);
        (intensity > 0.0).then_some(Self {
            expand: self.expand.max(0.0),
            intensity,
            ..self
        })
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RectShadow {
    pub rect: Rect,
    pub options: RectShadowOptions,
}

#[derive(Clone, Copy)]
struct FineRect {
    tile_x0: i32,
    tile_y0: i32,
    pixel_bounds: Bounds,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

impl FineRect {
    fn row_start(self, y_px: i32) -> usize {
        (y_px - self.tile_y0) as usize * TILE_SIZE as usize
    }

    fn tile_ix(self, row: usize, x_px: i32) -> usize {
        row + (x_px - self.tile_x0) as usize
    }
}

impl Rect {
    pub(crate) fn axis_bounds(&self) -> (f64, f64, f64, f64) {
        (
            self.start.x.min(self.end.x),
            self.start.y.min(self.end.y),
            self.start.x.max(self.end.x),
            self.start.y.max(self.end.y),
        )
    }

    fn axis_bounds_f32(&self) -> (f32, f32, f32, f32) {
        let (x0, y0, x1, y1) = self.axis_bounds();
        (x0 as f32, y0 as f32, x1 as f32, y1 as f32)
    }

    pub(crate) fn tile_is_solid(&self, bounds: Bounds) -> bool {
        if bounds.x0 >= bounds.x1 || bounds.y0 >= bounds.y1 {
            return false;
        }

        let (x0, y0, x1, y1) = self.axis_bounds();
        if self.radius.is_zero() {
            return (bounds.x0 as f64) >= x0
                && (bounds.y0 as f64) >= y0
                && (bounds.x1 as f64) <= x1
                && (bounds.y1 as f64) <= y1;
        }

        tile_perimeter_is_solid(bounds, |x, y| self.distance(x, y))
    }

    pub(crate) fn distance(&self, x: f64, y: f64) -> f64 {
        f64::from(self.signed_distance(x as f32, y as f32))
    }

    pub(crate) fn fine_area(
        &self,
        area: &mut [f32; (TILE_SIZE * TILE_SIZE) as usize],
        tile_bounds: Bounds,
        pixel_bounds: Bounds,
    ) {
        let (x0, y0, x1, y1) = self.axis_bounds_f32();
        let rect = FineRect {
            tile_x0: tile_bounds.x0,
            tile_y0: tile_bounds.y0,
            pixel_bounds,
            x0,
            y0,
            x1,
            y1,
        };

        if self.radius.is_zero() {
            Self::fine_sharp_rect(area, rect);
        } else if self.radius.is_uniform() {
            Self::fine_uniform_round_rect(area, rect, self.radius.top_left);
        } else {
            self.fine_per_pixel(area, rect);
        }
    }

    fn signed_distance(&self, x: f32, y: f32) -> f32 {
        let (x0, y0, x1, y1) = self.axis_bounds_f32();
        let cx = (x0 + x1) * 0.5;
        let cy = (y0 + y1) * 0.5;
        let hx = (x1 - x0) * 0.5;
        let hy = (y1 - y0) * 0.5;
        let px = x - cx;
        let py = y - cy;

        let r = if px >= 0.0 {
            if py <= 0.0 {
                self.radius.top_right
            } else {
                self.radius.bottom_right
            }
        } else if py <= 0.0 {
            self.radius.top_left
        } else {
            self.radius.bottom_left
        };
        let r = r.min(hx).min(hy).max(0.0);

        let ax = px.abs();
        let ay = py.abs();
        if r <= 0.0 {
            let dx = ax - hx;
            let dy = ay - hy;
            return dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0);
        }

        let qx = ax - hx + r;
        let qy = ay - hy + r;
        qx.max(qy).min(0.0) + (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() - r
    }

    fn uniform_signed_distance(
        x: f32,
        y: f32,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        radius: f32,
    ) -> f32 {
        let cx = (x0 + x1) * 0.5;
        let cy = (y0 + y1) * 0.5;
        let hx = (x1 - x0) * 0.5;
        let hy = (y1 - y0) * 0.5;
        let r = radius.min(hx).min(hy).max(0.0);
        let ax = (x - cx).abs();
        let ay = (y - cy).abs();
        let qx = ax - hx + r;
        let qy = ay - hy + r;
        qx.max(qy).min(0.0) + (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() - r
    }

    fn fine_sharp_rect(area: &mut [f32; (TILE_SIZE * TILE_SIZE) as usize], rect: FineRect) {
        let FineRect { x0, y0, x1, y1, .. } = rect;
        let out_y0 = y0 - 0.5;
        let out_y1 = y1 + 0.5;
        let inner_y0 = y0 + 0.5;
        let inner_y1 = y1 - 0.5;
        let inner_x0 = x0 + 0.5;
        let inner_x1 = x1 - 0.5;
        let out_x0 = x0 - 0.5;
        let out_x1 = x1 + 0.5;
        let cx = (x0 + x1) * 0.5;
        let cy = (y0 + y1) * 0.5;
        let hx = (x1 - x0) * 0.5;
        let hy = (y1 - y0) * 0.5;

        for y_px in rect.pixel_bounds.y0..rect.pixel_bounds.y1 {
            let py = y_px as f32 + 0.5;
            if py < out_y0 || py > out_y1 {
                continue;
            }
            let row = rect.row_start(y_px);
            let inner_row = py >= inner_y0 && py <= inner_y1;
            let ay = (py - cy).abs();
            let dy = ay - hy;

            for x_px in rect.pixel_bounds.x0..rect.pixel_bounds.x1 {
                let px = x_px as f32 + 0.5;
                let ix = rect.tile_ix(row, x_px);
                if inner_row && px >= inner_x0 && px <= inner_x1 {
                    area[ix] = 1.0;
                } else if px < out_x0 || px > out_x1 {
                    area[ix] = 0.0;
                } else {
                    let ax = (px - cx).abs();
                    let dx = ax - hx;
                    let dist = dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0);
                    area[ix] = coverage_from_dist(dist);
                }
            }
        }
    }

    fn fine_uniform_round_rect(
        area: &mut [f32; (TILE_SIZE * TILE_SIZE) as usize],
        rect: FineRect,
        radius: f32,
    ) {
        let FineRect { x0, y0, x1, y1, .. } = rect;
        let hx = (x1 - x0) * 0.5;
        let hy = (y1 - y0) * 0.5;
        let r = radius.min(hx).min(hy).max(0.0);
        let out_y0 = y0 - 0.5;
        let out_y1 = y1 + 0.5;
        let out_x0 = x0 - 0.5;
        let out_x1 = x1 + 0.5;
        let inner_y0 = y0 + r + 0.5;
        let inner_y1 = y1 - r - 0.5;
        let inner_x0 = x0 + r + 0.5;
        let inner_x1 = x1 - r - 0.5;

        for y_px in rect.pixel_bounds.y0..rect.pixel_bounds.y1 {
            let py = y_px as f32 + 0.5;
            if py < out_y0 || py > out_y1 {
                continue;
            }
            let row = rect.row_start(y_px);
            let inner_row = py >= inner_y0 && py <= inner_y1;

            for x_px in rect.pixel_bounds.x0..rect.pixel_bounds.x1 {
                let px = x_px as f32 + 0.5;
                let ix = rect.tile_ix(row, x_px);
                if px < out_x0 || px > out_x1 {
                    area[ix] = 0.0;
                } else if inner_row && px >= inner_x0 && px <= inner_x1 {
                    area[ix] = 1.0;
                } else {
                    let dist = Self::uniform_signed_distance(px, py, x0, y0, x1, y1, radius);
                    area[ix] = coverage_from_dist(dist);
                }
            }
        }
    }

    fn fine_per_pixel(&self, area: &mut [f32; (TILE_SIZE * TILE_SIZE) as usize], rect: FineRect) {
        let FineRect { x0, y0, x1, y1, .. } = rect;
        let out_y0 = y0 - 0.5;
        let out_y1 = y1 + 0.5;
        let out_x0 = x0 - 0.5;
        let out_x1 = x1 + 0.5;

        for y_px in rect.pixel_bounds.y0..rect.pixel_bounds.y1 {
            let py = y_px as f32 + 0.5;
            if py < out_y0 || py > out_y1 {
                continue;
            }
            let row = rect.row_start(y_px);

            for x_px in rect.pixel_bounds.x0..rect.pixel_bounds.x1 {
                let px = x_px as f32 + 0.5;
                let ix = rect.tile_ix(row, x_px);
                if px < out_x0 || px > out_x1 {
                    area[ix] = 0.0;
                } else {
                    area[ix] = coverage_from_dist(self.signed_distance(px, py));
                }
            }
        }
    }
}

impl RectShadow {
    pub(crate) fn bounds(&self) -> Bounds {
        let (x0, y0, x1, y1) = self.rect.axis_bounds();
        let outset = self.options.expand * 4.0 + 1.0;
        Bounds {
            x0: (x0 + f64::from(self.options.offset_x) - f64::from(outset)).floor() as i32,
            y0: (y0 + f64::from(self.options.offset_y) - f64::from(outset)).floor() as i32,
            x1: (x1 + f64::from(self.options.offset_x) + f64::from(outset)).ceil() as i32,
            y1: (y1 + f64::from(self.options.offset_y) + f64::from(outset)).ceil() as i32,
        }
    }

    pub(crate) fn tile_is_solid(&self, _: Bounds) -> bool {
        false
    }

    pub(crate) fn fine_area(
        &self,
        area: &mut [f32; (TILE_SIZE * TILE_SIZE) as usize],
        tile_bounds: Bounds,
        pixel_bounds: Bounds,
    ) {
        let Some(options) = self.options.normalized() else {
            return;
        };
        let intensity = options.intensity;

        for y_px in pixel_bounds.y0..pixel_bounds.y1 {
            let py = y_px as f32 + 0.5 - options.offset_y;
            let row = (y_px - tile_bounds.y0) as usize * TILE_SIZE as usize;
            for x_px in pixel_bounds.x0..pixel_bounds.x1 {
                let px = x_px as f32 + 0.5 - options.offset_x;
                let dist = self.rect.signed_distance(px, py);
                let alpha = if options.expand <= 0.0 {
                    coverage_from_dist(dist) * intensity
                } else {
                    (-dist.max(0.0) / options.expand).exp() * intensity
                };
                area[row + (x_px - tile_bounds.x0) as usize] = alpha.clamp(0.0, 1.0);
            }
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RectStroke {
    pub rect: Rect,
    /// Full side widths; rasterization derives inner and outer SDF bounds from
    /// half of these values so uniform strokes match `kurbo::Stroke::width`.
    pub widths: StrokeWidths,
}

impl RectStroke {
    pub(crate) fn tile_is_solid(&self, _: Bounds) -> bool {
        false
    }

    pub(crate) fn fine_area(
        &self,
        area: &mut [f32; (TILE_SIZE * TILE_SIZE) as usize],
        tile_bounds: Bounds,
        pixel_bounds: Bounds,
    ) {
        if self.widths.is_empty() {
            return;
        }

        self.outer_rect().fine_area(area, tile_bounds, pixel_bounds);
        let Some(inner) = self.inner_rect() else {
            return;
        };

        let mut inner_area = [0.0; (TILE_SIZE * TILE_SIZE) as usize];
        inner.fine_area(&mut inner_area, tile_bounds, pixel_bounds);
        for (outer, inner) in area.iter_mut().zip(inner_area) {
            *outer = (*outer - inner).clamp(0.0, 1.0);
        }
    }

    fn outer_rect(&self) -> Rect {
        let (x0, y0, x1, y1) = self.rect.axis_bounds();
        let half = self.widths.half();
        Rect {
            start: Point::new(x0 - f64::from(half.left), y0 - f64::from(half.top)),
            end: Point::new(x1 + f64::from(half.right), y1 + f64::from(half.bottom)),
            radius: self.rect.radius.offset_corners(
                half.top.max(half.left),
                half.top.max(half.right),
                half.bottom.max(half.left),
                half.bottom.max(half.right),
            ),
        }
    }

    fn inner_rect(&self) -> Option<Rect> {
        let (x0, y0, x1, y1) = self.rect.axis_bounds();
        let half = self.widths.half();
        let (ix0, iy0, ix1, iy1) = (
            x0 + f64::from(half.left),
            y0 + f64::from(half.top),
            x1 - f64::from(half.right),
            y1 - f64::from(half.bottom),
        );
        (ix0 < ix1 && iy0 < iy1).then(|| Rect {
            start: Point::new(ix0, iy0),
            end: Point::new(ix1, iy1),
            radius: self.rect.radius.offset_corners(
                -half.top.max(half.left),
                -half.top.max(half.right),
                -half.bottom.max(half.left),
                -half.bottom.max(half.right),
            ),
        })
    }
}

/// For convex SDFs, solid coverage on the pixel perimeter implies solid interior.
pub(super) fn tile_perimeter_is_solid(
    bounds: Bounds,
    mut distance: impl FnMut(f64, f64) -> f64,
) -> bool {
    let w = bounds.x1 - bounds.x0;
    let h = bounds.y1 - bounds.y0;
    if w <= 0 || h <= 0 {
        return false;
    }

    for x in bounds.x0..bounds.x1 {
        let px = x as f64 + 0.5;
        if distance(px, bounds.y0 as f64 + 0.5) > SOLID_DIST {
            return false;
        }
        if h > 1 && distance(px, bounds.y1 as f64 - 0.5) > SOLID_DIST {
            return false;
        }
    }
    for y in (bounds.y0 + 1)..(bounds.y1 - 1) {
        let py = y as f64 + 0.5;
        if distance(bounds.x0 as f64 + 0.5, py) > SOLID_DIST {
            return false;
        }
        if w > 1 && distance(bounds.x1 as f64 - 0.5, py) > SOLID_DIST {
            return false;
        }
    }
    true
}
