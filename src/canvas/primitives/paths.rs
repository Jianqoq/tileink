use super::*;

impl Canvas {
    pub fn push_arc(
        &mut self,
        arc: Arc,
        brush: impl Into<Brush>,
        rule: FillRule,
        tolerance: f64,
    ) -> DrawId {
        self.push_path(
            arc.to_path(tolerance),
            brush,
            Affine::IDENTITY,
            rule,
            tolerance,
        )
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "Drawing commands accept owned temporary stroke descriptors, including dash storage"
    )]
    pub fn push_stroke(
        &mut self,
        shape: impl Shape,
        stroke: Stroke,
        brush: impl Into<Brush>,
        transform: Affine,
        rule: FillRule,
        tolerance: f64,
    ) -> DrawId {
        let path = shape.to_path(tolerance);
        let outline = kurbo_stroke(path, &stroke, &StrokeOpts::default(), tolerance);
        let draw = self.push_path_inner_with_tag(
            outline,
            transform,
            rule,
            tolerance,
            PathPushOptions {
                bounds_override: None,
                brush: brush.into(),
                emit_draw_command: true,
                tag: DrawTag::Brush,
            },
        );
        self.draw_id_from_index(draw)
    }

    pub fn push_path(
        &mut self,
        path: BezPath,
        brush: impl Into<Brush>,
        transform: Affine,
        rule: FillRule,
        tolerance: f64,
    ) -> DrawId {
        let draw = self.push_path_inner(path, brush, transform, rule, tolerance, None);
        self.draw_id_from_index(draw)
    }
}
