use super::*;

impl Canvas {
    /// Adds a filled circle as exact SDF geometry instead of flattening it to path segments.
    pub fn push_circle(&mut self, circle: Circle, brush: impl Into<Brush>) -> DrawId {
        let draw = self.push_sdf_draw(
            Sdf::Circle(SdfCircle {
                center: circle.center,
                radius: circle.radius as f32,
            }),
            brush,
        );
        self.draw_id_from_index(draw)
    }

    pub fn push_circle_stroke(
        &mut self,
        circle: Circle,
        stroke: Stroke,
        brush: impl Into<Brush>,
    ) -> Option<DrawId> {
        if stroke.width <= 0.0 {
            return None;
        }
        if !stroke.dash_pattern.is_empty() {
            let draw = self.push_stroke(
                circle,
                stroke,
                brush,
                Affine::IDENTITY,
                FillRule::NonZero,
                0.1,
            );
            return Some(draw);
        }

        let half_width = (stroke.width * 0.5) as f32;
        let draw = self.push_sdf_draw(
            Sdf::CircleStroke(SdfCircleStroke {
                circle: SdfCircle {
                    center: circle.center,
                    radius: circle.radius as f32,
                },
                half_width,
            }),
            brush,
        );
        Some(self.draw_id_from_index(draw))
    }

    /// Adds a circular stroked arc as SDF geometry.
    ///
    /// This is separate from [`push_arc`](Self::push_arc), which preserves the
    /// existing path-backed kurbo arc semantics. Use this method when the arc is
    /// a stroke-like primitive and should avoid path flattening.
    pub fn push_sdf_arc(&mut self, arc: SdfArc, brush: impl Into<Brush>) -> Option<DrawId> {
        if arc.is_empty() {
            return None;
        }
        let draw = self.push_sdf_draw(Sdf::Rc(arc), brush);
        Some(self.draw_id_from_index(draw))
    }

    pub fn push_candlestick(&mut self, candle: SdfCandleStick, brush: impl Into<Brush>) -> DrawId {
        assert!(
            SdfCandleStick::valid_body_width(candle.body_width),
            "candlestick body width must be positive"
        );
        assert!(
            SdfCandleStick::valid_wick_width(candle.wick_width),
            "candlestick wick width must be positive"
        );
        let draw = self.push_sdf_draw(Sdf::CandleStick(candle), brush);
        self.draw_id_from_index(draw)
    }

    pub fn push_line(&mut self, line: SdfLine, brush: impl Into<Brush>) -> Option<DrawId> {
        if line.is_empty() {
            return None;
        }
        let draw = self.push_sdf_draw(Sdf::Line(line), brush);
        Some(self.draw_id_from_index(draw))
    }

    pub fn push_dash_line(&mut self, line: SdfDashLine, brush: impl Into<Brush>) -> Option<DrawId> {
        if line.is_empty() {
            return None;
        }
        let draw = self.push_sdf_draw(Sdf::DashLine(line), brush);
        Some(self.draw_id_from_index(draw))
    }

    /// Adds a filled, optionally rounded triangle as analytic SDF geometry.
    pub fn push_triangle(
        &mut self,
        triangle: SdfTriangle,
        brush: impl Into<Brush>,
    ) -> Option<DrawId> {
        if triangle.is_empty() {
            return None;
        }
        let draw = self.push_sdf_draw(Sdf::Triangle(triangle), brush);
        Some(self.draw_id_from_index(draw))
    }

    /// Adds a filled rounded body and directional tail as one analytic SDF union.
    pub fn push_callout(&mut self, callout: SdfCallout, brush: impl Into<Brush>) -> Option<DrawId> {
        if callout.is_empty() {
            return None;
        }
        let draw = self.push_sdf_draw(Sdf::Callout(callout), brush);
        Some(self.draw_id_from_index(draw))
    }

    /// Adds a centered stroke around a complete analytic callout union.
    pub fn push_callout_stroke(
        &mut self,
        stroke: SdfCalloutStroke,
        brush: impl Into<Brush>,
    ) -> Option<DrawId> {
        if stroke.is_empty() {
            return None;
        }
        let draw = self.push_sdf_draw(Sdf::CalloutStroke(stroke), brush);
        Some(self.draw_id_from_index(draw))
    }

    /// Adds a filled, optionally rounded five-point star as one analytic SDF draw.
    pub fn push_star(&mut self, star: SdfStar, brush: impl Into<Brush>) -> Option<DrawId> {
        if star.is_empty() {
            return None;
        }
        let draw = self.push_sdf_draw(Sdf::Star(star), brush);
        Some(self.draw_id_from_index(draw))
    }

    /// Adds a centered stroke around a rounded five-point star as one analytic SDF draw.
    pub fn push_star_stroke(
        &mut self,
        stroke: SdfStarStroke,
        brush: impl Into<Brush>,
    ) -> Option<DrawId> {
        if stroke.is_empty() {
            return None;
        }
        let draw = self.push_sdf_draw(Sdf::StarStroke(stroke), brush);
        Some(self.draw_id_from_index(draw))
    }
}
