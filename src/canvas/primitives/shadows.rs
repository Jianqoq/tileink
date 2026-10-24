use super::*;

impl Canvas {
    /// Adds a soft SDF shadow for a rounded rectangle.
    ///
    /// This is intentionally a separate draw instead of a hidden side effect of
    /// `push_rect`: shadow order matters under clips, blend layers, filters, and
    /// overlapping content. Push the shadow before the rectangle when it should
    /// sit behind the rectangle.
    pub fn push_rect_shadow(
        &mut self,
        rect: Rect,
        radius: Radius,
        options: ShadowOptions,
        brush: impl Into<Brush>,
    ) -> Option<DrawId> {
        let options = options.normalized()?;
        let shadow = SdfRectShadow {
            rect: SdfRect {
                start: Point::new(rect.x0, rect.y0),
                end: Point::new(rect.x1, rect.y1),
                radius,
            },
            options,
        };
        let draw = self.push_sdf_shadow_draw(SdfShadow::Rect(shadow), brush);
        Some(self.draw_id_from_index(draw))
    }

    pub fn push_circle_shadow(
        &mut self,
        circle: Circle,
        options: ShadowOptions,
        brush: impl Into<Brush>,
    ) -> Option<DrawId> {
        let options = options.normalized()?;
        let shadow = SdfCircleShadow {
            circle: SdfCircle {
                center: circle.center,
                radius: circle.radius as f32,
            },
            options,
        };
        let draw = self.push_sdf_shadow_draw(SdfShadow::Circle(shadow), brush);
        Some(self.draw_id_from_index(draw))
    }

    pub fn push_arc_shadow(
        &mut self,
        arc: SdfArc,
        options: ShadowOptions,
        brush: impl Into<Brush>,
    ) -> Option<DrawId> {
        if arc.is_empty() {
            return None;
        }
        let options = options.normalized()?;
        let shadow = SdfArcShadow { arc, options };
        let draw = self.push_sdf_shadow_draw(SdfShadow::Rc(shadow), brush);
        Some(self.draw_id_from_index(draw))
    }

    pub fn push_line_shadow(
        &mut self,
        line: SdfLine,
        options: ShadowOptions,
        brush: impl Into<Brush>,
    ) -> Option<DrawId> {
        if line.is_empty() {
            return None;
        }
        let options = options.normalized()?;
        let shadow = SdfLineShadow { line, options };
        let draw = self.push_sdf_shadow_draw(SdfShadow::Line(shadow), brush);
        Some(self.draw_id_from_index(draw))
    }

    /// Adds a soft shadow around a complete analytic callout union.
    ///
    /// The shadow is a separate draw so callers retain painter-order control. Push it before the
    /// fill and stroke when it should sit behind the surface.
    pub fn push_callout_shadow(
        &mut self,
        callout: SdfCallout,
        options: ShadowOptions,
        brush: impl Into<Brush>,
    ) -> Option<DrawId> {
        if callout.is_empty() {
            return None;
        }
        let options = options.normalized()?;
        let draw = self.push_sdf_shadow_draw(
            SdfShadow::Callout(SdfCalloutShadow { callout, options }),
            brush,
        );
        Some(self.draw_id_from_index(draw))
    }
}
