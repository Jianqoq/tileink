use super::*;

impl Canvas {
    /// Adds a two-color checkerboard with a constant two-draw SDF representation.
    ///
    /// The first draw fills the complete rectangle and the second analytically covers one
    /// alternating cell parity. Invalid rectangles or cell sizes are ignored before either draw
    /// is recorded, so callers never observe a partially constructed checkerboard.
    pub fn push_checkerboard(
        &mut self,
        rect: Rect,
        cell_size: f32,
        first: impl Into<Brush>,
        second: impl Into<Brush>,
    ) -> Option<(DrawId, DrawId)> {
        if !image_rect_is_valid(rect) || !cell_size.is_finite() || cell_size <= 0.0 {
            return None;
        }
        let base = self.push_rect(rect, Radius::ZERO, second);
        let cells = self.push_sdf_draw(
            Sdf::Checkerboard(SdfCheckerboard::new(rect, cell_size)),
            first,
        );
        Some((base, self.draw_id_from_index(cells)))
    }

    /// Adds a filled rectangle as SDF geometry with independent corner radii.
    ///
    /// This keeps rounded rectangles on the SDF path instead of flattening them
    /// to path segments, matching the SDF shadow/stroke APIs and preserving
    /// subpixel edge ownership. SDF primitives
    /// have inherent coverage; use path APIs when fill-rule semantics matter.
    pub fn push_rect(&mut self, rect: Rect, radius: Radius, brush: impl Into<Brush>) -> DrawId {
        let draw = self.push_sdf_draw(
            Sdf::Rect(SdfRect {
                start: Point::new(rect.x0, rect.y0),
                end: Point::new(rect.x1, rect.y1),
                radius,
            }),
            brush,
        );
        self.draw_id_from_index(draw)
    }

    pub fn push_rect_stroke(
        &mut self,
        rect: Rect,
        radius: Radius,
        stroke: Stroke,
        brush: impl Into<Brush>,
    ) -> Option<DrawId> {
        if stroke.width <= 0.0 {
            return None;
        }
        if !stroke.dash_pattern.is_empty() {
            let path = Self::rounded_rect_path(rect, radius, 0.1);
            let outline = kurbo_stroke(path, &stroke, &StrokeOpts::default(), 0.1);
            let draw = self.push_path_inner_with_tag(
                outline,
                Affine::IDENTITY,
                FillRule::NonZero,
                0.1,
                PathPushOptions {
                    bounds_override: None,
                    brush: brush.into(),
                    emit_draw_command: true,
                    tag: DrawTag::Brush,
                },
            );
            return Some(self.draw_id_from_index(draw));
        }

        self.push_rect_stroke_widths(rect, radius, StrokeWidths::all(stroke.width as f32), brush)
    }

    /// Adds a rectangle stroke with independent per-side widths as SDF geometry.
    ///
    /// `widths` are full centered stroke widths. This path is meant for dense
    /// rectangle borders; dashed or arbitrary stroked shapes should use
    /// `push_stroke`, which expands through the path stroker.
    pub fn push_rect_stroke_widths(
        &mut self,
        rect: Rect,
        radius: Radius,
        widths: StrokeWidths,
        brush: impl Into<Brush>,
    ) -> Option<DrawId> {
        let widths = widths.clamped();
        if widths.is_empty() {
            return None;
        }

        let draw = self.push_sdf_draw(
            Sdf::RectStroke(SdfRectStroke {
                rect: SdfRect {
                    start: Point::new(rect.x0, rect.y0),
                    end: Point::new(rect.x1, rect.y1),
                    radius,
                },
                widths,
            }),
            brush,
        );
        Some(self.draw_id_from_index(draw))
    }
}
