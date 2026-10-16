use super::*;

impl Canvas {
    /// Adds a laid-out text run at `origin`.
    ///
    /// Text layout and glyph rasterization stay in [`TextContext`](crate::TextContext);
    /// the canvas stores only positioned glyph cache keys. This keeps cached UI text
    /// reusable across renderers while leaving transform-heavy glyph quads for a
    /// future atlas path instead of pretending bitmap glyphs support arbitrary affine
    /// transforms here.
    pub fn push_text_layout(
        &mut self,
        layout: &TextLayout,
        origin: Point,
        brush: impl Into<Brush>,
    ) -> Option<DrawId> {
        self.push_text_layout_inner(layout, origin, None, brush.into())
    }

    /// Adds a laid-out text run whose output is hard-clipped to `clip`.
    ///
    /// Unlike a clip layer, this restricts the text draw's exact fine-composition pixel domain. It
    /// therefore adds no layer command or clip geometry and is intended for rectangular overflow
    /// clipping of one text run. `clip` is expressed in the canvas's logical coordinate space.
    pub fn push_text_layout_clipped(
        &mut self,
        layout: &TextLayout,
        origin: Point,
        clip: Rect,
        brush: impl Into<Brush>,
    ) -> Option<DrawId> {
        let clip = logical_rect_pixel_bounds(clip, self.scale_factor())?;
        self.push_text_layout_inner(layout, origin, Some(clip), brush.into())
    }

    fn push_text_layout_inner(
        &mut self,
        layout: &TextLayout,
        origin: Point,
        clip: Option<PixelBounds>,
        brush: Brush,
    ) -> Option<DrawId> {
        if layout.is_empty() || !origin.is_finite() {
            return None;
        }

        let layout_bounds = layout_bounds_at_scaled_origin(layout, origin, self.scale_factor());
        if layout_bounds.is_empty() {
            return None;
        }
        let mut bounds = PixelBounds {
            x0: layout_bounds.x0,
            y0: layout_bounds.y0,
            x1: layout_bounds.x1,
            y1: layout_bounds.y1,
        };
        if let Some(clip) = clip {
            bounds = bounds.intersect(clip);
            if bounds.is_empty() {
                return None;
            }
        }

        let glyph_start = self.text_glyphs.len() as u32;
        self.text_glyphs.extend(scene_glyphs_at_scaled_origin(
            layout,
            origin,
            self.scale_factor(),
        ));
        let glyph_count = self.text_glyphs.len() as u32 - glyph_start;
        if glyph_count == 0 {
            return None;
        }
        let run_id = self.text_runs.len() as u32;
        let run = TextRun {
            glyph_start,
            glyph_count,
        };
        self.text_runs.push(run);
        let (brush_offset, brush_len) = self.push_brush(brush);
        let draw_ix = self.push_draw_record(DrawRecord {
            path_id: DrawRecord::NONE,
            glyph_run_id: run_id,
            sdf_offset: DrawRecord::NONE,
            sdf_len: 0,
            sdf_shadow_offset: DrawRecord::NONE,
            sdf_shadow_len: 0,
            brush_offset,
            brush_len,
            tag: DrawTag::Brush.into(),
            fill_rule: FillRule::NonZero.into(),
            pixel_bounds: bounds,
            local_pixel_bounds: bounds,
            solid_rect: 0,
            transform: GpuAffine::default(),
            inverse_transform: GpuAffine::default(),
        });
        self.current_command_list_mut()
            .commands
            .push(Command::Draw(draw_ix));
        Some(self.draw_id_from_index(draw_ix))
    }

    /// Adds a laid-out text run as vector outlines.
    ///
    /// Cosmic-text still owns shaping, fallback, and ligatures; this method asks
    /// `text_context` for each scalable swash outline and appends the outlines as
    /// path geometry tagged with text compositing semantics. Glyphs backed only
    /// by bitmap strikes do not have a vector outline, so keep
    /// [`push_text_layout`](Self::push_text_layout) for small hinted text and
    /// bitmap/color emoji.
    #[allow(
        clippy::too_many_arguments,
        reason = "Explicit geometry/resource parameters preserve the renderer and native ABI contract."
    )]
    pub fn push_text_layout_as_path(
        &mut self,
        text_context: &mut TextContext,
        font_system: &mut TextFontSystem,
        layout: &TextLayout,
        origin: Point,
        brush: impl Into<Brush>,
        transform: Affine,
        tolerance: f64,
    ) -> Option<DrawId> {
        if layout.glyphs().is_empty() {
            return None;
        }

        let path = text_context.layout_outline_path(font_system, layout, origin);
        if path.is_empty() {
            return None;
        }
        let draw = self.push_path_inner_with_tag(
            path,
            transform,
            FillRule::NonZero,
            tolerance,
            PathPushOptions {
                bounds_override: None,
                brush: brush.into(),
                emit_draw_command: true,
                tag: DrawTag::PathGlyph,
            },
        );
        Some(self.draw_id_from_index(draw))
    }
}

fn logical_rect_pixel_bounds(rect: Rect, scale_factor: f32) -> Option<PixelBounds> {
    let coords = [rect.x0, rect.y0, rect.x1, rect.y1];
    if coords.iter().any(|coord| !coord.is_finite()) || rect.x0 >= rect.x1 || rect.y0 >= rect.y1 {
        return None;
    }
    Some(PixelBounds {
        x0: (rect.x0 * f64::from(scale_factor)).floor() as i32,
        y0: (rect.y0 * f64::from(scale_factor)).floor() as i32,
        x1: (rect.x1 * f64::from(scale_factor)).ceil() as i32,
        y1: (rect.y1 * f64::from(scale_factor)).ceil() as i32,
    })
}
