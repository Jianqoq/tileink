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
    /// subpixel edge ownership in the wgpu renderer. SDF primitives
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

    /// Adds an external image scaled into `rect` with explicit extend and sampling.
    ///
    /// The image is stored as a pattern brush, so it uses the same upload/sampling
    /// path as SVG raster images. Empty images, empty rectangles, and non-finite
    /// rectangles are ignored.
    pub fn push_image(
        &mut self,
        rect: Rect,
        image: impl Into<SharedRc<Image>>,
        extend: Extend,
        sampling: PatternSampling,
    ) -> Option<DrawId> {
        if !image_rect_is_valid(rect) {
            return None;
        }
        let image = image.into();
        if image.width == 0 || image.height == 0 {
            return None;
        }
        let key = self.register_scene_image(image)?;
        let brush = Brush::from_scene_image_key_with_options(key, rect, extend, sampling, 255)?;
        Some(self.push_rect(rect, Radius::ZERO, brush))
    }

    /// Adds a renderer-owned image resource scaled into `rect` with explicit extend and sampling.
    ///
    /// The scene only stores `key`; the wgpu renderer resolves the image through
    /// its resource table at render time.
    pub fn push_image_key(
        &mut self,
        rect: Rect,
        key: ImageKey,
        extend: Extend,
        sampling: PatternSampling,
    ) -> Option<DrawId> {
        let brush = Brush::from_image_key_with_options(key, rect, extend, sampling, 255)?;
        Some(self.push_rect(rect, Radius::ZERO, brush))
    }

    pub(crate) fn register_scene_image(
        &mut self,
        image: impl Into<SharedRc<Image>>,
    ) -> Option<ImageKey> {
        let image = image.into();
        let key = ImageKey::new(SharedRc::as_ptr(&image) as usize as u64);
        if self.scene_images.get(key).is_some() {
            return Some(key);
        }
        self.scene_images.insert(key, image).then_some(key)
    }

    pub(crate) fn scene_image_resources(&self) -> &ImageResourceStore {
        &self.scene_images
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
        options: RectShadowOptions,
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

    pub fn push_circle_shadow(
        &mut self,
        circle: Circle,
        options: RectShadowOptions,
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

    pub fn push_arc_shadow(
        &mut self,
        arc: SdfArc,
        options: RectShadowOptions,
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

    pub fn push_line_shadow(
        &mut self,
        line: SdfLine,
        options: RectShadowOptions,
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
        let clip = logical_rect_pixel_bounds(clip, self.scale_factor)?;
        self.push_text_layout_inner(layout, origin, Some(clip), brush.into())
    }

    fn push_text_layout_inner(
        &mut self,
        layout: &TextLayout,
        origin: Point,
        clip: Option<PixelBounds>,
        brush: Brush,
    ) -> Option<DrawId> {
        if layout.is_empty() {
            return None;
        }

        let layout_bounds = layout_bounds_at_scaled_origin(layout, origin, self.scale_factor);
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

        self.ensure_command_root();
        let glyph_start = self.text_glyphs.len() as u32;
        self.text_glyphs.extend(scene_glyphs_at_scaled_origin(
            layout,
            origin,
            self.scale_factor,
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
            transform: Default::default(),
            inverse_transform: Default::default(),
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
    #[allow(clippy::too_many_arguments)]
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
        x0: (rect.x0 * scale_factor as f64).floor() as i32,
        y0: (rect.y0 * scale_factor as f64).floor() as i32,
        x1: (rect.x1 * scale_factor as f64).ceil() as i32,
        y1: (rect.y1 * scale_factor as f64).ceil() as i32,
    })
}
