use peniko::{
    Color, Compose, Mix,
    kurbo::{
        Affine, Arc, BezPath, Circle, Point, Rect, Shape, Stroke, StrokeOpts,
        stroke as kurbo_stroke,
    },
};

use crate::shared::{
    bd_record::BackdropRecord,
    bounds::{Bounds, PixelBounds},
    brush::Brush,
    draw_record::{DrawRecord, DrawTag},
    execution::{
        Command, CommandList, CommandListId, ExecOp, ExecPlan, LayerStackEntry,
        ROOT_COMMAND_LIST_ID,
    },
    fill::FillRule,
    layer::{
        Layer, LayerKind, blend::Blend, filter::Filter, mask::Mask, opacity::Opacity,
        region::Region,
    },
    line::Line,
    path::PathRecord,
    path_flatten::PathFlatten,
    scan_line::line_scanned_tile_count,
    sdf::{
        Sdf,
        candlestick::CandleStick as SdfCandleStick,
        circle::{Circle as SdfCircle, CircleStroke as SdfCircleStroke},
        line::Line as SdfLine,
        rect::{Radius, Rect as SdfRect, RectStroke as SdfRectStroke, StrokeWidths},
    },
};
use crate::text::{
    TextContext, TextLayout, TextRun, layout_bounds_at_origin, scene_glyphs_at_origin,
};

pub struct Scene {
    pub(crate) lines: Vec<Line>,
    pub(crate) path_records: Vec<PathRecord>,
    pub(crate) draw_records: Vec<DrawRecord>,
    pub(crate) text_glyphs: Vec<crate::text::SceneGlyph>,
    pub(crate) text_runs: Vec<TextRun>,
    pub(crate) bd_records: Vec<BackdropRecord>,
    pub(crate) command_lists: Vec<CommandList>,
    root_commands: CommandListId,
    command_stack: Vec<CommandListId>,
    layer_stack: Vec<LayerKind>,
    pub(crate) path_cnt: u32,
    pub(crate) backdrop_pool_capacity: u32,
    pub(crate) tile_cnt: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

struct PathPushOptions {
    bounds_override: Option<Bounds>,
    tag: DrawTag,
}

impl Scene {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            lines: Vec::new(),
            path_records: Vec::new(),
            draw_records: Vec::new(),
            text_glyphs: Vec::new(),
            text_runs: Vec::new(),
            bd_records: Vec::new(),
            command_lists: vec![CommandList::default()],
            root_commands: ROOT_COMMAND_LIST_ID,
            command_stack: vec![ROOT_COMMAND_LIST_ID],
            layer_stack: Vec::new(),
            path_cnt: 0,
            backdrop_pool_capacity: 0,
            tile_cnt: 0,
            width,
            height,
        }
    }

    fn ensure_command_root(&mut self) {
        if self.command_lists.is_empty() {
            self.command_lists.push(CommandList::default());
        }
        self.root_commands = ROOT_COMMAND_LIST_ID;
        if self.command_stack.is_empty() {
            self.command_stack.push(self.root_commands);
        }
    }

    fn current_command_list_id(&self) -> CommandListId {
        self.command_stack
            .last()
            .copied()
            .unwrap_or(self.root_commands)
    }

    fn current_command_list_mut(&mut self) -> &mut CommandList {
        let id = self.current_command_list_id();
        &mut self.command_lists[id]
    }

    pub fn merge(&mut self, mut other: Scene) {
        self.ensure_command_root();
        other.ensure_command_root();

        assert!(
            self.width == other.width && self.height == other.height,
            "scene merge requires matching dimensions"
        );
        assert!(
            self.command_stack.len() == 1 && self.layer_stack.is_empty(),
            "cannot merge into a scene with unclosed layers"
        );
        assert!(
            other.command_stack.len() == 1 && other.layer_stack.is_empty(),
            "cannot merge a scene with unclosed layers"
        );

        let line_offset = self.lines.len() as u32;
        let path_offset = self.path_cnt;
        let draw_offset = self.draw_records.len();
        let glyph_offset = self.text_glyphs.len() as u32;
        let text_run_offset = self.text_runs.len() as u32;
        let backdrop_offset = self.backdrop_pool_capacity;
        let tile_offset = self.tile_cnt;

        for line in &mut other.lines {
            line.path_id = line.path_id.saturating_add(path_offset);
        }
        self.lines.extend(other.lines);

        for record in &mut other.path_records {
            record.path_id = record.path_id.saturating_add(path_offset);
            record.line_start = record.line_start.saturating_add(line_offset);
        }
        self.path_records.extend(other.path_records);

        for draw in &mut other.draw_records {
            if let Some(path_id) = &mut draw.path_id {
                *path_id = path_id.saturating_add(path_offset);
            }
            if let Some(glyph_run_id) = &mut draw.glyph_run_id {
                *glyph_run_id = glyph_run_id.saturating_add(text_run_offset);
            }
        }
        self.draw_records.extend(other.draw_records);

        for run in &mut other.text_runs {
            run.glyph_start = run.glyph_start.saturating_add(glyph_offset);
        }
        self.text_glyphs.extend(other.text_glyphs);
        self.text_runs.extend(other.text_runs);

        for record in &mut other.bd_records {
            record.path_id = record.path_id.saturating_add(path_offset);
            record.data_offset = record.data_offset.saturating_add(backdrop_offset);
            record.segment_start = record.segment_start.saturating_add(tile_offset);
        }
        self.bd_records.extend(other.bd_records);

        let command_list_offset = self.command_lists.len();
        let mut remapped_root_commands =
            Vec::with_capacity(other.command_lists[other.root_commands].commands.len());
        for command in other.command_lists[other.root_commands].commands.drain(..) {
            remapped_root_commands.push(Self::remap_command(
                command,
                draw_offset,
                command_list_offset.saturating_sub(1),
            ));
        }

        for (list_ix, mut list) in other.command_lists.into_iter().enumerate() {
            if list_ix == other.root_commands {
                continue;
            }
            for command in &mut list.commands {
                *command = Self::remap_command(
                    std::mem::replace(command, Command::Draw(0)),
                    draw_offset,
                    command_list_offset.saturating_sub(1),
                );
            }
            self.command_lists.push(list);
        }

        self.command_lists[self.root_commands]
            .commands
            .extend(remapped_root_commands);
        self.path_cnt = self.path_cnt.saturating_add(other.path_cnt);
        self.backdrop_pool_capacity = self
            .backdrop_pool_capacity
            .saturating_add(other.backdrop_pool_capacity);
        self.tile_cnt = self.tile_cnt.saturating_add(other.tile_cnt);
    }

    fn remap_command(command: Command, draw_offset: usize, child_list_offset: usize) -> Command {
        match command {
            Command::Draw(draw_ix) => Command::Draw(draw_ix + draw_offset),
            Command::Layer {
                draw,
                layer,
                children,
            } => Command::Layer {
                draw: draw + draw_offset,
                layer,
                children: children + child_list_offset,
            },
            Command::MaskLayer {
                layer,
                content,
                mask,
            } => Command::MaskLayer {
                layer,
                content: content + child_list_offset,
                mask: mask + child_list_offset,
            },
        }
    }

    fn append_scene_as_command_list(&mut self, mut other: Scene) -> CommandListId {
        self.ensure_command_root();
        other.ensure_command_root();

        assert!(
            self.width == other.width && self.height == other.height,
            "scene append requires matching dimensions"
        );
        assert!(
            other.command_stack.len() == 1 && other.layer_stack.is_empty(),
            "cannot append a scene with unclosed layers"
        );

        let line_offset = self.lines.len() as u32;
        let path_offset = self.path_cnt;
        let draw_offset = self.draw_records.len();
        let glyph_offset = self.text_glyphs.len() as u32;
        let text_run_offset = self.text_runs.len() as u32;
        let backdrop_offset = self.backdrop_pool_capacity;
        let tile_offset = self.tile_cnt;

        for line in &mut other.lines {
            line.path_id = line.path_id.saturating_add(path_offset);
        }
        self.lines.extend(other.lines);

        for record in &mut other.path_records {
            record.path_id = record.path_id.saturating_add(path_offset);
            record.line_start = record.line_start.saturating_add(line_offset);
        }
        self.path_records.extend(other.path_records);

        for draw in &mut other.draw_records {
            if let Some(path_id) = &mut draw.path_id {
                *path_id = path_id.saturating_add(path_offset);
            }
            if let Some(glyph_run_id) = &mut draw.glyph_run_id {
                *glyph_run_id = glyph_run_id.saturating_add(text_run_offset);
            }
        }
        self.draw_records.extend(other.draw_records);

        for run in &mut other.text_runs {
            run.glyph_start = run.glyph_start.saturating_add(glyph_offset);
        }
        self.text_glyphs.extend(other.text_glyphs);
        self.text_runs.extend(other.text_runs);

        for record in &mut other.bd_records {
            record.path_id = record.path_id.saturating_add(path_offset);
            record.data_offset = record.data_offset.saturating_add(backdrop_offset);
            record.segment_start = record.segment_start.saturating_add(tile_offset);
        }
        self.bd_records.extend(other.bd_records);

        let command_list_offset = self.command_lists.len();
        let root_commands = other.root_commands;
        let path_cnt = other.path_cnt;
        let backdrop_pool_capacity = other.backdrop_pool_capacity;
        let tile_cnt = other.tile_cnt;
        for mut list in other.command_lists {
            for command in &mut list.commands {
                *command = Self::remap_command(
                    std::mem::replace(command, Command::Draw(0)),
                    draw_offset,
                    command_list_offset,
                );
            }
            self.command_lists.push(list);
        }

        self.path_cnt = self.path_cnt.saturating_add(path_cnt);
        self.backdrop_pool_capacity = self
            .backdrop_pool_capacity
            .saturating_add(backdrop_pool_capacity);
        self.tile_cnt = self.tile_cnt.saturating_add(tile_cnt);
        command_list_offset + root_commands
    }

    pub fn push_clip_layer(
        &mut self,
        path: BezPath,
        transform: Affine,
        rule: FillRule,
        tolerance: f64,
    ) {
        self.ensure_command_root();
        let draw = self.push_layer_path(DrawTag::Clip, path.clone(), transform, rule, tolerance);
        let layer = Layer::Clip;
        let children = self.command_lists.len();
        self.command_lists.push(CommandList::default());
        self.current_command_list_mut()
            .commands
            .push(Command::Layer {
                draw,
                layer,
                children,
            });
        self.command_stack.push(children);
        self.layer_stack.push(LayerKind::Clip);
    }

    /// Adds a rounded/sharp rectangle clip that is rasterized directly from an SDF.
    ///
    /// This avoids flattening simple rounded clips into path segments while keeping
    /// the SDF geometry as the source of truth until render time.
    pub fn push_clip_sdf_rect_layer(&mut self, rect: Rect, radius: Radius) {
        self.ensure_command_root();
        let layer = Layer::ClipSdf {
            sdf: Sdf::Rect(SdfRect {
                start: Point::new(rect.x0, rect.y0),
                end: Point::new(rect.x1, rect.y1),
                radius,
            }),
            bounds: Self::rect_bounds(rect),
        };
        let children = self.command_lists.len();
        self.command_lists.push(CommandList::default());
        self.current_command_list_mut()
            .commands
            .push(Command::Layer {
                draw: 0,
                layer,
                children,
            });
        self.command_stack.push(children);
        self.layer_stack.push(LayerKind::ClipSdf);
    }

    /// Starts an isolated source-over group.
    ///
    /// This is the renderer primitive for SVG/CSS `isolation:isolate` without
    /// opacity, blending, or filtering. The children are composited into a
    /// transparent offscreen buffer first, then the group is composited back
    /// through the supplied layer path and any outer clips.
    pub fn push_isolate_layer(&mut self, path: BezPath, transform: Affine, tolerance: f64) {
        self.ensure_command_root();
        let draw = self.push_layer_path(
            DrawTag::Isolate,
            path.clone(),
            transform,
            FillRule::NonZero,
            tolerance,
        );
        let children = self.command_lists.len();
        self.command_lists.push(CommandList::default());
        self.current_command_list_mut()
            .commands
            .push(Command::Layer {
                draw,
                layer: Layer::Isolate,
                children,
            });
        self.command_stack.push(children);
        self.layer_stack.push(LayerKind::Isolate);
    }

    pub fn push_opacity_layer(
        &mut self,
        path: BezPath,
        transform: Affine,
        tolerance: f64,
        opacity: f32,
    ) {
        self.ensure_command_root();
        let draw = self.push_layer_path(
            DrawTag::Opacity,
            path.clone(),
            transform,
            FillRule::NonZero,
            tolerance,
        );
        let layer = Layer::Opacity(Opacity { opacity });
        let children = self.command_lists.len();
        self.command_lists.push(CommandList::default());
        self.current_command_list_mut()
            .commands
            .push(Command::Layer {
                draw,
                layer,
                children,
            });
        self.command_stack.push(children);
        self.layer_stack.push(LayerKind::Opacity);
    }

    pub(crate) fn push_blend_layer_inner(
        &mut self,
        path: BezPath,
        transform: Affine,
        tolerance: f64,
        blend: Blend,
    ) {
        self.ensure_command_root();
        let draw = self.push_layer_path(
            DrawTag::Blend,
            path.clone(),
            transform,
            FillRule::NonZero,
            tolerance,
        );
        let layer = Layer::Blend(Blend { mode: blend.mode });
        let children = self.command_lists.len();
        self.command_lists.push(CommandList::default());
        self.current_command_list_mut()
            .commands
            .push(Command::Layer {
                draw,
                layer,
                children,
            });
        self.command_stack.push(children);
        self.layer_stack.push(LayerKind::Blend);
    }

    pub fn push_blend_layer(
        &mut self,
        path: BezPath,
        transform: Affine,
        tolerance: f64,
        mix: Mix,
        compose: Compose,
    ) {
        self.push_blend_layer_inner(path, transform, tolerance, Blend::new(mix, compose));
    }

    /// Starts a masked group using `mask_scene` as the mask source.
    ///
    /// The mask source is rendered isolated, converted to either alpha or
    /// luminance coverage, clipped to `mask.region`, then applied to this
    /// layer's content before compositing through any outer clips.
    pub fn push_mask_layer(&mut self, mask_scene: Scene, mask: Mask) {
        self.ensure_command_root();
        let mask_commands = self.append_scene_as_command_list(mask_scene);
        let content = self.command_lists.len();
        self.command_lists.push(CommandList::default());
        self.current_command_list_mut()
            .commands
            .push(Command::MaskLayer {
                layer: mask,
                content,
                mask: mask_commands,
            });
        self.command_stack.push(content);
        self.layer_stack.push(LayerKind::Mask);
    }

    /// Adds an offscreen filter group sampled from `sample_region`.
    ///
    /// Filters derive their final output bounds from this region. Blur and
    /// drop-shadow expand it internally so their output is not clipped back to
    /// the original geometry.
    pub fn push_filter_layer(&mut self, filter: Filter, sample_region: Region) {
        self.ensure_command_root();
        let children = self.command_lists.len();
        self.command_lists.push(CommandList::default());
        self.current_command_list_mut()
            .commands
            .push(Command::Layer {
                draw: 0,
                layer: Layer::Filter {
                    filter,
                    sample_region,
                },
                children,
            });
        self.command_stack.push(children);
        self.layer_stack.push(LayerKind::Filter);
    }

    /// Adds a backdrop filter group sampled from the already-rendered target.
    ///
    /// The filter samples pixels behind this layer from `sample_region`, clips
    /// the filtered backdrop back to that region, then renders this layer's
    /// children normally on top.
    pub fn push_backdrop_layer(&mut self, filter: Filter, sample_region: Region) {
        self.ensure_command_root();
        let children = self.command_lists.len();
        self.command_lists.push(CommandList::default());
        self.current_command_list_mut()
            .commands
            .push(Command::Layer {
                draw: 0,
                layer: Layer::Backdrop {
                    filter,
                    sample_region,
                },
                children,
            });
        self.command_stack.push(children);
        self.layer_stack.push(LayerKind::Backdrop);
    }

    pub fn pop_layer(&mut self) -> Option<LayerKind> {
        self.ensure_command_root();
        let layer_kind = self.layer_stack.pop()?;
        if self.command_stack.len() > 1 {
            self.command_stack.pop();
        }
        Some(layer_kind)
    }

    pub fn push_rect(&mut self, rect: Rect, brush: impl Into<Brush>, rule: FillRule) {
        self.push_sdf_draw(
            Sdf::Rect(SdfRect {
                start: Point::new(rect.x0, rect.y0),
                end: Point::new(rect.x1, rect.y1),
                radius: Radius::all(0.0),
            }),
            Self::rect_bounds(rect),
            brush,
            rule,
        );
    }

    pub fn push_rect_stroke(
        &mut self,
        rect: Rect,
        radius: Radius,
        stroke: Stroke,
        brush: impl Into<Brush>,
        rule: FillRule,
    ) {
        if stroke.width <= 0.0 {
            return;
        }
        if !stroke.dash_pattern.is_empty() {
            let path = Self::rounded_rect_path(rect, radius, 0.1);
            let outline = kurbo_stroke(path, &stroke, &StrokeOpts::default(), 0.1);
            self.push_path_inner(outline, brush, Affine::IDENTITY, rule, 0.1, None);
            return;
        }

        self.push_rect_stroke_widths(
            rect,
            radius,
            StrokeWidths::all(stroke.width as f32),
            brush,
            rule,
        );
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
        rule: FillRule,
    ) {
        let widths = widths.clamped();
        if widths.is_empty() {
            return;
        }

        self.push_sdf_draw(
            Sdf::RectStroke(SdfRectStroke {
                rect: SdfRect {
                    start: Point::new(rect.x0, rect.y0),
                    end: Point::new(rect.x1, rect.y1),
                    radius,
                },
                widths,
            }),
            Self::rect_bounds_outsets(rect, widths),
            brush,
            rule,
        );
    }

    fn rect_bounds(rect: Rect) -> Bounds {
        Bounds {
            x0: rect.x0.min(rect.x1).floor() as i32,
            y0: rect.y0.min(rect.y1).floor() as i32,
            x1: rect.x0.max(rect.x1).ceil() as i32,
            y1: rect.y0.max(rect.y1).ceil() as i32,
        }
    }

    fn rect_bounds_outset(rect: Rect, outset: f64) -> Bounds {
        Bounds {
            x0: (rect.x0.min(rect.x1) - outset).floor() as i32,
            y0: (rect.y0.min(rect.y1) - outset).floor() as i32,
            x1: (rect.x0.max(rect.x1) + outset).ceil() as i32,
            y1: (rect.y0.max(rect.y1) + outset).ceil() as i32,
        }
    }

    fn rect_bounds_outsets(rect: Rect, widths: StrokeWidths) -> Bounds {
        let half = widths.half();
        Bounds {
            x0: (rect.x0.min(rect.x1) - f64::from(half.left)).floor() as i32,
            y0: (rect.y0.min(rect.y1) - f64::from(half.top)).floor() as i32,
            x1: (rect.x0.max(rect.x1) + f64::from(half.right)).ceil() as i32,
            y1: (rect.y0.max(rect.y1) + f64::from(half.bottom)).ceil() as i32,
        }
    }

    /// Adds a filled circle as exact SDF geometry instead of flattening it to path segments.
    pub fn push_circle(&mut self, circle: Circle, brush: impl Into<Brush>, rule: FillRule) {
        let rect = circle.bounding_box();
        self.push_sdf_draw(
            Sdf::Circle(SdfCircle {
                center: circle.center,
                radius: circle.radius as f32,
            }),
            Self::rect_bounds(rect),
            brush,
            rule,
        );
    }

    pub fn push_circle_stroke(
        &mut self,
        circle: Circle,
        stroke: Stroke,
        brush: impl Into<Brush>,
        rule: FillRule,
    ) {
        if stroke.width <= 0.0 {
            return;
        }
        if !stroke.dash_pattern.is_empty() {
            self.push_stroke(circle, stroke, brush, Affine::IDENTITY, rule, 0.1);
            return;
        }

        let half_width = (stroke.width * 0.5) as f32;
        self.push_sdf_draw(
            Sdf::CircleStroke(SdfCircleStroke {
                circle: SdfCircle {
                    center: circle.center,
                    radius: circle.radius as f32,
                },
                half_width,
            }),
            Self::rect_bounds_outset(circle.bounding_box(), f64::from(half_width)),
            brush,
            rule,
        );
    }

    pub fn push_candlestick(
        &mut self,
        candle: SdfCandleStick,
        brush: impl Into<Brush>,
        rule: FillRule,
    ) {
        assert!(
            SdfCandleStick::valid_body_width(candle.body_width),
            "candlestick body width must be a positive odd number"
        );
        self.push_sdf_draw(Sdf::CandleStick(candle), candle.bounds(), brush, rule);
    }

    pub fn push_line(&mut self, line: SdfLine, brush: impl Into<Brush>, rule: FillRule) {
        if line.is_empty() {
            return;
        }
        self.push_sdf_draw(Sdf::Line(line), line.bounds(), brush, rule);
    }

    pub fn push_arc(&mut self, arc: Arc, brush: impl Into<Brush>, rule: FillRule, tolerance: f64) {
        self.push_path(
            arc.to_path(tolerance),
            brush,
            Affine::IDENTITY,
            rule,
            tolerance,
        );
    }

    pub fn push_stroke(
        &mut self,
        shape: impl Shape,
        stroke: Stroke,
        brush: impl Into<Brush>,
        transform: Affine,
        rule: FillRule,
        tolerance: f64,
    ) {
        let path = shape.to_path(tolerance);
        let outline = kurbo_stroke(path, &stroke, &StrokeOpts::default(), tolerance);
        self.push_path_inner(outline, brush, transform, rule, tolerance, None);
    }

    pub fn push_path(
        &mut self,
        path: BezPath,
        brush: impl Into<Brush>,
        transform: Affine,
        rule: FillRule,
        tolerance: f64,
    ) {
        self.push_path_inner(path, brush, transform, rule, tolerance, None);
    }

    /// Adds a laid-out text run at `origin`.
    ///
    /// Text layout and glyph rasterization stay in [`TextContext`](crate::TextContext);
    /// the scene stores only positioned glyph cache keys. This keeps cached UI text
    /// reusable across renderers while leaving transform-heavy glyph quads for a
    /// future atlas path instead of pretending bitmap glyphs support arbitrary affine
    /// transforms here.
    pub fn push_text_layout(
        &mut self,
        layout: &TextLayout,
        origin: Point,
        brush: impl Into<Brush>,
    ) {
        if layout.is_empty() {
            return;
        }

        self.ensure_command_root();
        let glyph_start = self.text_glyphs.len() as u32;
        self.text_glyphs
            .extend(scene_glyphs_at_origin(layout, origin));
        let glyph_count = self.text_glyphs.len() as u32 - glyph_start;
        if glyph_count == 0 {
            return;
        }

        let run_id = self.text_runs.len() as u32;
        self.text_runs.push(TextRun {
            glyph_start,
            glyph_count,
        });
        let bounds = layout_bounds_at_origin(layout, origin);
        let draw_ix = self.draw_records.len();
        self.draw_records.push(DrawRecord {
            path_id: None,
            glyph_run_id: Some(run_id),
            sdf: None,
            tag: DrawTag::Brush,
            brush: brush.into(),
            fill_rule: FillRule::NonZero,
            pixel_bounds: PixelBounds {
                x0: bounds.x0,
                y0: bounds.y0,
                x1: bounds.x1,
                y1: bounds.y1,
            },
            solid_rect: false,
        });
        self.current_command_list_mut()
            .commands
            .push(Command::Draw(draw_ix));
    }

    /// Adds a laid-out text run as vector outlines.
    ///
    /// Cosmic-text still owns shaping, fallback, and ligatures; this method asks
    /// `text_context` for each scalable swash outline and appends the outlines as
    /// path geometry tagged with text compositing semantics. Glyphs backed only
    /// by bitmap strikes do not have a vector outline, so keep
    /// [`push_text_layout`](Self::push_text_layout) for small hinted text and
    /// bitmap/color emoji.
    pub fn push_text_layout_as_path(
        &mut self,
        text_context: &mut TextContext,
        layout: &TextLayout,
        origin: Point,
        brush: impl Into<Brush>,
        transform: Affine,
        tolerance: f64,
    ) {
        if layout.glyphs().is_empty() {
            return;
        }

        let path = text_context.layout_outline_path(layout, origin);
        if path.is_empty() {
            return;
        }
        self.push_path_inner_with_tag(
            path,
            brush,
            transform,
            FillRule::NonZero,
            tolerance,
            PathPushOptions {
                bounds_override: None,
                tag: DrawTag::PathGlyph,
            },
        );
    }

    fn rounded_rect_path(rect: Rect, radius: Radius, tolerance: f64) -> BezPath {
        if radius.is_zero() {
            rect.to_path(tolerance)
        } else {
            peniko::kurbo::RoundedRect::new(
                rect.x0,
                rect.y0,
                rect.x1,
                rect.y1,
                (
                    radius.top_left as f64,
                    radius.top_right as f64,
                    radius.bottom_right as f64,
                    radius.bottom_left as f64,
                ),
            )
            .to_path(tolerance)
        }
    }

    fn transform_path(path: BezPath, transform: Affine) -> BezPath {
        if transform == Affine::IDENTITY {
            path
        } else {
            transform * path
        }
    }

    fn pixel_bounds_for_transformed_path(path: &BezPath) -> PixelBounds {
        let rect = path.bounding_box();
        PixelBounds {
            x0: rect.x0.floor() as i32,
            y0: rect.y0.floor() as i32,
            x1: rect.x1.ceil() as i32,
            y1: rect.y1.ceil() as i32,
        }
    }

    fn push_path_inner(
        &mut self,
        path: BezPath,
        brush: impl Into<Brush>,
        transform: Affine,
        rule: FillRule,
        tolerance: f64,
        bounds_override: Option<Bounds>,
    ) -> usize {
        self.push_path_inner_with_tag(
            path,
            brush,
            transform,
            rule,
            tolerance,
            PathPushOptions {
                bounds_override,
                tag: DrawTag::Brush,
            },
        )
    }

    fn push_path_inner_with_tag(
        &mut self,
        path: BezPath,
        brush: impl Into<Brush>,
        transform: Affine,
        rule: FillRule,
        tolerance: f64,
        options: PathPushOptions,
    ) -> usize {
        self.ensure_command_root();
        let line_start = self.lines.len() as u32;
        let path_id = self.path_cnt;
        self.path_cnt += 1;
        let path = Self::transform_path(path, transform);
        PathFlatten::new(&path, tolerance as f32, path_id).flatten(&mut self.lines);
        let line_count = self.lines.len() as u32 - line_start;
        self.path_records.push(PathRecord {
            path_id,
            line_count,
            line_start,
            _pad: 0,
        });
        let pixel_bounds = match options.bounds_override {
            Some(bounds) => PixelBounds {
                x0: bounds.x0,
                y0: bounds.y0,
                x1: bounds.x1,
                y1: bounds.y1,
            },
            None => Self::pixel_bounds_for_transformed_path(&path),
        };
        let tile_bbox = pixel_bounds.tile_bbox(self.width_in_tiles(), self.height_in_tiles());
        let tile_stride = tile_bbox.tile_stride();
        let tile_height = tile_bbox.tile_height();
        let backdrop_len = tile_stride * tile_height;
        let local_tile_cnt =
            self.segment_capacity_for_path_lines(line_start, line_count, tile_bbox);

        let backdrop_offset = self.backdrop_pool_capacity;
        self.backdrop_pool_capacity += backdrop_len;
        let segment_start = self.tile_cnt;
        self.tile_cnt += local_tile_cnt;

        let draw_ix = self.draw_records.len();
        self.draw_records.push(DrawRecord {
            path_id: Some(path_id),
            glyph_run_id: None,
            sdf: None,
            tag: options.tag,
            brush: brush.into(),
            fill_rule: rule,
            pixel_bounds,
            solid_rect: false,
        });
        self.current_command_list_mut()
            .commands
            .push(Command::Draw(draw_ix));
        self.bd_records.push(BackdropRecord {
            path_id,
            data_offset: backdrop_offset,
            data_len: backdrop_len,
            tile_x0: tile_bbox.x0,
            tile_y0: tile_bbox.y0,
            tile_x1: tile_bbox.x1,
            tile_y1: tile_bbox.y1,
            segment_start,
            segment_capacity: local_tile_cnt,
            segment_count: 0,
        });
        draw_ix
    }

    fn push_layer_path(
        &mut self,
        tag: DrawTag,
        path: BezPath,
        transform: Affine,
        rule: FillRule,
        tolerance: f64,
    ) -> usize {
        let line_start = self.lines.len() as u32;
        let path_id = self.path_cnt;
        self.path_cnt += 1;
        let path = Self::transform_path(path, transform);
        PathFlatten::new(&path, tolerance as f32, path_id).flatten(&mut self.lines);
        let line_count = self.lines.len() as u32 - line_start;
        self.path_records.push(PathRecord {
            path_id,
            line_count,
            line_start,
            _pad: 0,
        });

        let pixel_bounds = Self::pixel_bounds_for_transformed_path(&path);
        let tile_bbox = pixel_bounds.tile_bbox(self.width_in_tiles(), self.height_in_tiles());
        let tile_stride = tile_bbox.tile_stride();
        let tile_height = tile_bbox.tile_height();
        let backdrop_len = tile_stride * tile_height;
        let local_tile_cnt =
            self.segment_capacity_for_path_lines(line_start, line_count, tile_bbox);

        let backdrop_offset = self.backdrop_pool_capacity;
        self.backdrop_pool_capacity += backdrop_len;
        let segment_start = self.tile_cnt;
        self.tile_cnt += local_tile_cnt;

        let draw_ix = self.draw_records.len();
        self.draw_records.push(DrawRecord {
            path_id: Some(path_id),
            glyph_run_id: None,
            sdf: None,
            tag,
            brush: Brush::Solid(Color::TRANSPARENT),
            fill_rule: rule,
            pixel_bounds,
            solid_rect: false,
        });
        self.bd_records.push(BackdropRecord {
            path_id,
            data_offset: backdrop_offset,
            data_len: backdrop_len,
            tile_x0: tile_bbox.x0,
            tile_y0: tile_bbox.y0,
            tile_x1: tile_bbox.x1,
            tile_y1: tile_bbox.y1,
            segment_start,
            segment_capacity: local_tile_cnt,
            segment_count: 0,
        });
        draw_ix
    }

    fn push_sdf_draw(
        &mut self,
        sdf: Sdf,
        bounds: Bounds,
        brush: impl Into<Brush>,
        rule: FillRule,
    ) -> usize {
        self.ensure_command_root();
        let draw_ix = self.draw_records.len();
        self.draw_records.push(DrawRecord {
            path_id: None,
            glyph_run_id: None,
            sdf: Some(sdf),
            tag: DrawTag::Brush,
            brush: brush.into(),
            fill_rule: rule,
            pixel_bounds: PixelBounds {
                x0: bounds.x0,
                y0: bounds.y0,
                x1: bounds.x1,
                y1: bounds.y1,
            },
            solid_rect: false,
        });
        self.current_command_list_mut()
            .commands
            .push(Command::Draw(draw_ix));
        draw_ix
    }

    pub fn reset(&mut self) {
        self.lines.clear();
        self.path_records.clear();
        self.draw_records.clear();
        self.text_glyphs.clear();
        self.text_runs.clear();
        self.bd_records.clear();
        self.command_lists.clear();
        self.command_lists.push(CommandList::default());
        self.root_commands = ROOT_COMMAND_LIST_ID;
        self.command_stack.clear();
        self.command_stack.push(self.root_commands);
        self.layer_stack.clear();
        self.path_cnt = 0;
        self.backdrop_pool_capacity = 0;
        self.tile_cnt = 0;
    }

    pub(crate) fn width_in_tiles(&self) -> u32 {
        self.width.div_ceil(crate::TILE_SIZE)
    }

    pub(crate) fn height_in_tiles(&self) -> u32 {
        self.height.div_ceil(crate::TILE_SIZE)
    }

    fn segment_capacity_for_path_lines(
        &self,
        line_start: u32,
        line_count: u32,
        tile_bbox: crate::shared::bounds::TileBbox,
    ) -> u32 {
        let tiles_size = (self.width_in_tiles(), self.height_in_tiles());
        self.lines[line_start as usize..(line_start + line_count) as usize]
            .iter()
            .fold(0u32, |capacity, &line| {
                capacity.saturating_add(line_scanned_tile_count(line, tile_bbox, tiles_size))
            })
    }

    pub(crate) fn compile(&self, list_id: CommandListId) -> ExecPlan {
        let mut ops = Vec::new();
        let mut plan = ExecPlan {
            ops: Vec::new(),
            layer_stack_data: Vec::new(),
        };
        let mut layer_stack = Vec::new();
        self.compile_into(list_id, &mut ops, &mut plan, &mut layer_stack);
        plan.ops = ops;
        plan
    }

    fn compile_into(
        &self,
        list_id: CommandListId,
        ops: &mut Vec<ExecOp>,
        plan: &mut ExecPlan,
        layer_stack: &mut Vec<LayerStackEntry>,
    ) {
        let mut pending_batch: Option<(usize, usize)> = None;

        let flush_batch = |pending_batch: &mut Option<(usize, usize)>,
                           ops: &mut Vec<ExecOp>,
                           plan: &mut ExecPlan,
                           layer_stack: &[LayerStackEntry]| {
            let Some((start, end)) = pending_batch.take() else {
                return;
            };
            let layer_start = plan.layer_stack_data.len();
            plan.layer_stack_data.extend_from_slice(layer_stack);
            let layer_end = plan.layer_stack_data.len();

            ops.push(ExecOp::DrawBatch {
                draws: start..end,
                layer_stack: layer_start..layer_end,
            });
        };

        for command in &self.command_lists[list_id].commands {
            match command {
                Command::Draw(draw_ix) => match &mut pending_batch {
                    Some((start, end)) if *end == *draw_ix => {
                        *end = *draw_ix + 1;
                    }
                    Some(_) => {
                        flush_batch(&mut pending_batch, ops, plan, layer_stack);
                        pending_batch = Some((*draw_ix, *draw_ix + 1));
                    }
                    None => {
                        pending_batch = Some((*draw_ix, *draw_ix + 1));
                    }
                },
                Command::Layer {
                    draw,
                    layer,
                    children,
                } => {
                    flush_batch(&mut pending_batch, ops, plan, layer_stack);
                    if self.can_fuse(layer, *children) {
                        match layer {
                            Layer::Clip => {
                                ops.push(ExecOp::BeginClip);
                                layer_stack.push(LayerStackEntry::Clip { draw: *draw as u32 });
                                self.compile_into(*children, ops, plan, layer_stack);
                                layer_stack.pop();
                                ops.push(ExecOp::EndClip);
                            }
                            Layer::Opacity(opacity) => {
                                ops.push(ExecOp::BeginOpacity);
                                layer_stack.push(LayerStackEntry::Opacity {
                                    draw: *draw as u32,
                                    opacity: opacity.opacity,
                                });
                                self.compile_into(*children, ops, plan, layer_stack);
                                layer_stack.pop();
                                ops.push(ExecOp::EndOpacity);
                            }
                            Layer::Blend(blend) => {
                                ops.push(ExecOp::BeginBlend);
                                layer_stack.push(LayerStackEntry::Blend {
                                    draw: *draw as u32,
                                    mode: blend.mode,
                                });
                                self.compile_into(*children, ops, plan, layer_stack);
                                layer_stack.pop();
                                ops.push(ExecOp::EndBlend);
                            }
                            _ => unreachable!(),
                        }
                    } else {
                        let stack_start = plan.layer_stack_data.len();
                        plan.layer_stack_data.extend_from_slice(layer_stack);
                        let stack_end = plan.layer_stack_data.len();
                        ops.push(ExecOp::OffscreenLayer {
                            draw: *draw,
                            layer: layer.clone(),
                            outer_stack: stack_start..stack_end,
                            children: {
                                let mut child_ops = Vec::new();
                                let mut child_layer_stack = Vec::new();
                                self.compile_into(
                                    *children,
                                    &mut child_ops,
                                    plan,
                                    &mut child_layer_stack,
                                );
                                child_ops
                            },
                        });
                    }
                }
                Command::MaskLayer {
                    layer,
                    content,
                    mask,
                } => {
                    flush_batch(&mut pending_batch, ops, plan, layer_stack);
                    let stack_start = plan.layer_stack_data.len();
                    plan.layer_stack_data.extend_from_slice(layer_stack);
                    let stack_end = plan.layer_stack_data.len();
                    ops.push(ExecOp::OffscreenMaskLayer {
                        layer: layer.clone(),
                        outer_stack: stack_start..stack_end,
                        content: {
                            let mut child_ops = Vec::new();
                            let mut child_layer_stack = Vec::new();
                            self.compile_into(
                                *content,
                                &mut child_ops,
                                plan,
                                &mut child_layer_stack,
                            );
                            child_ops
                        },
                        mask: {
                            let mut mask_ops = Vec::new();
                            let mut mask_layer_stack = Vec::new();
                            self.compile_into(*mask, &mut mask_ops, plan, &mut mask_layer_stack);
                            mask_ops
                        },
                    });
                }
            }
        }

        flush_batch(&mut pending_batch, ops, plan, layer_stack);
    }

    fn can_fuse(&self, layer: &Layer, children: CommandListId) -> bool {
        match layer {
            Layer::Clip => true,
            // Group opacity and blend must wrap the composited child subtree.
            // If a child opens its own offscreen layer, keeping the group fused
            // would apply it to separate fragments before they are combined.
            Layer::Opacity(_) | Layer::Blend(_) => !self.command_list_contains_offscreen(children),
            _ => false,
        }
    }

    fn command_list_contains_offscreen(&self, list_id: CommandListId) -> bool {
        self.command_lists[list_id]
            .commands
            .iter()
            .any(|command| match command {
                Command::Draw(_) => false,
                Command::Layer {
                    layer, children, ..
                } => {
                    !matches!(layer, Layer::Clip | Layer::Opacity(_) | Layer::Blend(_))
                        || self.command_list_contains_offscreen(*children)
                }
                Command::MaskLayer { .. } => true,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ops::Range;

    use crate::shared::layer::mask::MaskKind;
    use peniko::{BlendMode, Compose, Mix, kurbo::PathEl};

    fn test_scene() -> Scene {
        Scene::new(64, 64)
    }

    fn rect_path(x0: f64, y0: f64, x1: f64, y1: f64) -> BezPath {
        BezPath::from_vec(vec![
            PathEl::MoveTo((x0, y0).into()),
            PathEl::LineTo((x1, y0).into()),
            PathEl::LineTo((x1, y1).into()),
            PathEl::LineTo((x0, y1).into()),
            PathEl::ClosePath,
        ])
    }

    fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color::from_rgb8(r, g, b)
    }

    fn assert_layer_stack(
        plan: &ExecPlan,
        layer_stack: Range<usize>,
        expected: &[LayerStackEntry],
    ) {
        let actual = &plan.layer_stack_data[layer_stack];
        assert_eq!(actual.len(), expected.len());
        assert_eq!(actual, expected);
    }

    #[test]
    fn compile_lowers_clip_blend_batches_in_user_order() {
        let mut scene = test_scene();
        scene.push_clip_layer(
            rect_path(0.0, 0.0, 32.0, 32.0),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.25,
        );
        scene.push_path(
            rect_path(2.0, 2.0, 8.0, 8.0),
            Brush::Solid(rgb(255, 0, 0)),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.25,
        );
        scene.push_blend_layer(
            rect_path(4.0, 4.0, 24.0, 24.0),
            Affine::IDENTITY,
            0.25,
            Mix::Multiply,
            Compose::SrcOver,
        );
        scene.push_path(
            rect_path(6.0, 6.0, 12.0, 12.0),
            Brush::Solid(rgb(0, 255, 0)),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.25,
        );
        scene.pop_layer();
        scene.push_path(
            rect_path(10.0, 10.0, 18.0, 18.0),
            Brush::Solid(rgb(0, 0, 255)),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.25,
        );
        scene.pop_layer();

        let plan = scene.compile(ROOT_COMMAND_LIST_ID);
        assert_eq!(plan.ops.len(), 7, "{:#?}", plan.ops);

        match &plan.ops[0] {
            ExecOp::BeginClip => {}
            op => panic!("expected BeginClip, got {op:#?}"),
        }
        match &plan.ops[1] {
            ExecOp::DrawBatch { draws, layer_stack } => {
                assert_eq!(draws.clone(), 1..2);
                assert_layer_stack(
                    &plan,
                    layer_stack.clone(),
                    &[LayerStackEntry::Clip { draw: 0 }],
                );
            }
            op => panic!("expected first DrawBatch, got {op:#?}"),
        }
        match &plan.ops[2] {
            ExecOp::BeginBlend => {}
            op => panic!("expected BeginBlend, got {op:#?}"),
        }
        match &plan.ops[3] {
            ExecOp::DrawBatch { draws, layer_stack } => {
                assert_eq!(draws.clone(), 3..4);
                assert_layer_stack(
                    &plan,
                    layer_stack.clone(),
                    &[
                        LayerStackEntry::Clip { draw: 0 },
                        LayerStackEntry::Blend {
                            draw: 2,
                            mode: BlendMode::new(Mix::Multiply, Compose::SrcOver),
                        },
                    ],
                );
            }
            op => panic!("expected second DrawBatch, got {op:#?}"),
        }
        match &plan.ops[4] {
            ExecOp::EndBlend => {}
            op => panic!("expected EndBlend, got {op:#?}"),
        }
        match &plan.ops[5] {
            ExecOp::DrawBatch { draws, layer_stack } => {
                assert_eq!(draws.clone(), 4..5);
                assert_layer_stack(
                    &plan,
                    layer_stack.clone(),
                    &[LayerStackEntry::Clip { draw: 0 }],
                );
            }
            op => panic!("expected third DrawBatch, got {op:#?}"),
        }
        match &plan.ops[6] {
            ExecOp::EndClip => {}
            op => panic!("expected EndClip, got {op:#?}"),
        }
    }

    #[test]
    fn compile_keeps_opacity_group_alive_across_nested_batches() {
        let mut scene = test_scene();
        scene.push_opacity_layer(rect_path(0.0, 0.0, 32.0, 32.0), Affine::IDENTITY, 0.25, 0.5);
        scene.push_path(
            rect_path(2.0, 2.0, 8.0, 8.0),
            Brush::Solid(rgb(255, 0, 0)),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.25,
        );
        scene.push_blend_layer(
            rect_path(4.0, 4.0, 24.0, 24.0),
            Affine::IDENTITY,
            0.25,
            Mix::Screen,
            Compose::SrcOver,
        );
        scene.push_path(
            rect_path(6.0, 6.0, 12.0, 12.0),
            Brush::Solid(rgb(0, 255, 0)),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.25,
        );
        scene.pop_layer();
        scene.push_path(
            rect_path(10.0, 10.0, 18.0, 18.0),
            Brush::Solid(rgb(0, 0, 255)),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.25,
        );
        scene.pop_layer();

        let plan = scene.compile(ROOT_COMMAND_LIST_ID);
        assert_eq!(plan.ops.len(), 7, "{:#?}", plan.ops);

        match &plan.ops[0] {
            ExecOp::BeginOpacity => {}
            op => panic!("expected BeginOpacity, got {op:#?}"),
        }
        match &plan.ops[1] {
            ExecOp::DrawBatch { draws, layer_stack } => {
                assert_eq!(draws.clone(), 1..2);
                assert_layer_stack(
                    &plan,
                    layer_stack.clone(),
                    &[LayerStackEntry::Opacity {
                        draw: 0,
                        opacity: 0.5,
                    }],
                );
            }
            op => panic!("expected first DrawBatch, got {op:#?}"),
        }
        match &plan.ops[2] {
            ExecOp::BeginBlend => {}
            op => panic!("expected BeginBlend, got {op:#?}"),
        }
        match &plan.ops[3] {
            ExecOp::DrawBatch { draws, layer_stack } => {
                assert_eq!(draws.clone(), 3..4);
                assert_layer_stack(
                    &plan,
                    layer_stack.clone(),
                    &[
                        LayerStackEntry::Opacity {
                            draw: 0,
                            opacity: 0.5,
                        },
                        LayerStackEntry::Blend {
                            draw: 2,
                            mode: BlendMode::new(Mix::Screen, Compose::SrcOver),
                        },
                    ],
                );
            }
            op => panic!("expected second DrawBatch, got {op:#?}"),
        }
        match &plan.ops[4] {
            ExecOp::EndBlend => {}
            op => panic!("expected EndBlend, got {op:#?}"),
        }
        match &plan.ops[5] {
            ExecOp::DrawBatch { draws, layer_stack } => {
                assert_eq!(draws.clone(), 4..5);
                assert_layer_stack(
                    &plan,
                    layer_stack.clone(),
                    &[LayerStackEntry::Opacity {
                        draw: 0,
                        opacity: 0.5,
                    }],
                );
            }
            op => panic!("expected third DrawBatch, got {op:#?}"),
        }
        match &plan.ops[6] {
            ExecOp::EndOpacity => {}
            op => panic!("expected EndOpacity, got {op:#?}"),
        }
    }

    #[test]
    fn compile_keeps_sdf_clip_as_sdf_offscreen_layer() {
        let mut scene = test_scene();
        scene.push_clip_sdf_rect_layer(Rect::new(4.0, 4.0, 32.0, 32.0), Radius::all(6.0));
        scene.push_path(
            rect_path(0.0, 0.0, 40.0, 40.0),
            Brush::Solid(rgb(255, 0, 0)),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
        scene.pop_layer();

        let plan = scene.compile(ROOT_COMMAND_LIST_ID);
        assert_eq!(scene.draw_records.len(), 1);
        assert_eq!(plan.ops.len(), 1, "{:#?}", plan.ops);
        match &plan.ops[0] {
            ExecOp::OffscreenLayer {
                draw,
                layer:
                    Layer::ClipSdf {
                        sdf: Sdf::Rect(rect),
                        bounds,
                    },
                outer_stack,
                children,
            } => {
                assert_eq!(*draw, 0);
                assert_eq!(rect.radius.top_left, 6.0);
                assert_eq!(*bounds, Bounds::new(4, 4, 32, 32));
                assert!(outer_stack.is_empty());
                match children.as_slice() {
                    [ExecOp::DrawBatch { draws, layer_stack }] => {
                        assert_eq!(draws.clone(), 0..1);
                        assert!(layer_stack.is_empty());
                    }
                    ops => panic!("expected one child draw batch, got {ops:#?}"),
                }
            }
            op => panic!("expected ClipSdf offscreen layer, got {op:#?}"),
        }
    }

    #[test]
    fn compile_keeps_opacity_with_offscreen_child_isolated() {
        let mut scene = test_scene();
        scene.push_opacity_layer(rect_path(0.0, 0.0, 48.0, 48.0), Affine::IDENTITY, 0.0, 0.5);
        scene.push_filter_layer(
            Filter::Opacity(1.0),
            Region::rect(Rect::new(0.0, 0.0, 48.0, 48.0), Radius::all(0.0)),
        );
        scene.push_path(
            rect_path(8.0, 8.0, 40.0, 40.0),
            Brush::Solid(rgb(0, 0, 255)),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
        scene.pop_layer();
        scene.pop_layer();

        let plan = scene.compile(ROOT_COMMAND_LIST_ID);
        assert_eq!(plan.ops.len(), 1, "{:#?}", plan.ops);
        match &plan.ops[0] {
            ExecOp::OffscreenLayer {
                draw,
                layer: Layer::Opacity(opacity),
                outer_stack,
                children,
            } => {
                assert_eq!(*draw, 0);
                assert_eq!(opacity.opacity, 0.5);
                assert!(outer_stack.is_empty());
                assert!(matches!(
                    children.as_slice(),
                    [ExecOp::OffscreenLayer {
                        layer: Layer::Filter { .. },
                        ..
                    }]
                ));
            }
            op => panic!("expected isolated opacity offscreen layer, got {op:#?}"),
        }
    }

    #[test]
    fn compile_keeps_blend_with_offscreen_child_isolated() {
        let mut scene = test_scene();
        scene.push_blend_layer(
            rect_path(0.0, 0.0, 48.0, 48.0),
            Affine::IDENTITY,
            0.0,
            Mix::Multiply,
            Compose::SrcOver,
        );
        scene.push_filter_layer(
            Filter::Opacity(1.0),
            Region::rect(Rect::new(0.0, 0.0, 48.0, 48.0), Radius::all(0.0)),
        );
        scene.push_path(
            rect_path(8.0, 8.0, 40.0, 40.0),
            Brush::Solid(rgb(0, 255, 0)),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
        scene.pop_layer();
        scene.pop_layer();

        let plan = scene.compile(ROOT_COMMAND_LIST_ID);
        assert_eq!(plan.ops.len(), 1, "{:#?}", plan.ops);
        match &plan.ops[0] {
            ExecOp::OffscreenLayer {
                draw,
                layer: Layer::Blend(blend),
                outer_stack,
                children,
            } => {
                assert_eq!(*draw, 0);
                assert_eq!(blend.mode, BlendMode::new(Mix::Multiply, Compose::SrcOver));
                assert!(outer_stack.is_empty());
                assert!(matches!(
                    children.as_slice(),
                    [ExecOp::OffscreenLayer {
                        layer: Layer::Filter { .. },
                        ..
                    }]
                ));
            }
            op => panic!("expected isolated blend offscreen layer, got {op:#?}"),
        }
    }

    #[test]
    fn compile_keeps_isolate_as_offscreen_layer() {
        let mut scene = test_scene();
        scene.push_isolate_layer(rect_path(0.0, 0.0, 48.0, 48.0), Affine::IDENTITY, 0.0);
        scene.push_path(
            rect_path(8.0, 8.0, 40.0, 40.0),
            Brush::Solid(rgb(255, 0, 0)),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
        scene.pop_layer();

        let plan = scene.compile(ROOT_COMMAND_LIST_ID);
        assert_eq!(plan.ops.len(), 1, "{:#?}", plan.ops);
        match &plan.ops[0] {
            ExecOp::OffscreenLayer {
                draw,
                layer: Layer::Isolate,
                outer_stack,
                children,
            } => {
                assert_eq!(*draw, 0);
                assert!(outer_stack.is_empty());
                match children.as_slice() {
                    [ExecOp::DrawBatch { draws, layer_stack }] => {
                        assert_eq!(draws.clone(), 1..2);
                        assert!(layer_stack.is_empty());
                    }
                    ops => panic!("expected one isolate child batch, got {ops:#?}"),
                }
            }
            op => panic!("expected isolate offscreen layer, got {op:#?}"),
        }
    }

    #[test]
    fn compile_keeps_mask_content_and_mask_isolated() {
        let mut scene = test_scene();
        let mut mask_scene = test_scene();
        mask_scene.push_rect(
            Rect::new(0.0, 0.0, 32.0, 64.0),
            Brush::Solid(rgb(255, 255, 255)),
            FillRule::NonZero,
        );
        scene.push_mask_layer(
            mask_scene,
            Mask {
                region: Region::rect(Rect::new(0.0, 0.0, 64.0, 64.0), Radius::all(0.0)),
                kind: MaskKind::Alpha,
            },
        );
        scene.push_rect(
            Rect::new(0.0, 0.0, 64.0, 64.0),
            Brush::Solid(rgb(255, 0, 0)),
            FillRule::NonZero,
        );
        scene.pop_layer();

        let plan = scene.compile(ROOT_COMMAND_LIST_ID);
        assert_eq!(plan.ops.len(), 1, "{:#?}", plan.ops);
        match &plan.ops[0] {
            ExecOp::OffscreenMaskLayer {
                layer,
                outer_stack,
                content,
                mask,
            } => {
                assert!(outer_stack.is_empty());
                assert_eq!(layer.kind, MaskKind::Alpha);
                match content.as_slice() {
                    [ExecOp::DrawBatch { draws, layer_stack }] => {
                        assert_eq!(draws.clone(), 1..2);
                        assert!(layer_stack.is_empty());
                    }
                    ops => panic!("expected one mask content batch, got {ops:#?}"),
                }
                match mask.as_slice() {
                    [ExecOp::DrawBatch { draws, layer_stack }] => {
                        assert_eq!(draws.clone(), 0..1);
                        assert!(layer_stack.is_empty());
                    }
                    ops => panic!("expected one mask source batch, got {ops:#?}"),
                }
            }
            op => panic!("expected mask offscreen layer, got {op:#?}"),
        }
    }

    #[test]
    fn push_rect_records_sdf_rect_without_path_storage() {
        let mut scene = test_scene();
        scene.push_rect(
            Rect::new(2.0, 3.0, 18.0, 19.0),
            Brush::Solid(rgb(255, 0, 0)),
            FillRule::NonZero,
        );

        assert_eq!(scene.draw_records.len(), 1);
        assert!(scene.path_records.is_empty());
        assert!(scene.bd_records.is_empty());
        let draw = &scene.draw_records[0];
        assert_eq!(
            draw.pixel_bounds,
            PixelBounds {
                x0: 2,
                y0: 3,
                x1: 18,
                y1: 19,
            }
        );
        assert_eq!(draw.tag, DrawTag::Brush);
        assert!(draw.path_id.is_none());
        assert!(!draw.solid_rect);
        match draw.sdf {
            Some(Sdf::Rect(rect)) => {
                assert_eq!(rect.axis_bounds(), (2.0, 3.0, 18.0, 19.0));
                assert!(rect.radius.is_zero());
            }
            sdf => panic!("expected rect SDF, got {sdf:?}"),
        }
    }

    #[test]
    fn push_circle_records_sdf_circle_without_path_storage() {
        let mut scene = test_scene();
        scene.push_circle(
            Circle::new((16.0, 20.0), 8.0),
            Brush::Solid(rgb(255, 0, 0)),
            FillRule::NonZero,
        );

        assert_eq!(scene.draw_records.len(), 1);
        assert!(scene.path_records.is_empty());
        assert!(scene.bd_records.is_empty());
        let draw = &scene.draw_records[0];
        assert_eq!(
            draw.pixel_bounds,
            PixelBounds {
                x0: 8,
                y0: 12,
                x1: 24,
                y1: 28,
            }
        );
        match draw.sdf {
            Some(Sdf::Circle(circle)) => {
                assert_eq!(circle.center, Point::new(16.0, 20.0));
                assert_eq!(circle.radius, 8.0);
            }
            sdf => panic!("expected circle SDF, got {sdf:?}"),
        }
    }

    #[test]
    fn push_candlestick_records_sdf_without_path_storage() {
        let mut scene = test_scene();
        scene.push_candlestick(
            SdfCandleStick::new(16.5, 4.0, 28.0, 10.0, 22.0, 7),
            Brush::Solid(rgb(255, 0, 0)),
            FillRule::NonZero,
        );

        assert_eq!(scene.draw_records.len(), 1);
        assert!(scene.path_records.is_empty());
        assert!(scene.bd_records.is_empty());
        assert_eq!(
            scene.draw_records[0].pixel_bounds,
            PixelBounds {
                x0: 13,
                y0: 4,
                x1: 20,
                y1: 28,
            }
        );
        match scene.draw_records[0].sdf {
            Some(Sdf::CandleStick(candle)) => {
                assert_eq!(candle.center_x, 16.5);
                assert_eq!(candle.body_width, 7);
            }
            sdf => panic!("expected candlestick SDF, got {sdf:?}"),
        }
    }

    #[test]
    fn push_line_records_sdf_without_path_storage() {
        let mut scene = test_scene();
        scene.push_line(
            SdfLine::new(
                Point::new(8.0, 16.5),
                Point::new(24.0, 16.5),
                1.0,
                crate::shared::sdf::line::LineCap::Butt,
            ),
            Brush::Solid(rgb(255, 0, 0)),
            FillRule::NonZero,
        );

        assert_eq!(scene.draw_records.len(), 1);
        assert!(scene.path_records.is_empty());
        assert!(scene.bd_records.is_empty());
        assert_eq!(
            scene.draw_records[0].pixel_bounds,
            PixelBounds {
                x0: 7,
                y0: 16,
                x1: 25,
                y1: 17,
            }
        );
        match scene.draw_records[0].sdf {
            Some(Sdf::Line(line)) => assert_eq!(line.width, 1.0),
            sdf => panic!("expected line SDF, got {sdf:?}"),
        }
    }

    #[test]
    fn push_rect_stroke_records_sdf_without_path_storage() {
        let mut scene = test_scene();
        scene.push_rect_stroke(
            Rect::new(10.0, 12.0, 30.0, 36.0),
            Radius::all(4.0),
            Stroke::new(6.0),
            Brush::Solid(rgb(255, 0, 0)),
            FillRule::NonZero,
        );

        assert_eq!(scene.draw_records.len(), 1);
        assert!(scene.path_records.is_empty());
        assert!(scene.bd_records.is_empty());
        let draw = &scene.draw_records[0];
        assert_eq!(
            draw.pixel_bounds,
            PixelBounds {
                x0: 7,
                y0: 9,
                x1: 33,
                y1: 39,
            }
        );
        assert!(draw.path_id.is_none());
        match draw.sdf {
            Some(Sdf::RectStroke(stroke)) => {
                assert_eq!(stroke.rect.axis_bounds(), (10.0, 12.0, 30.0, 36.0));
                assert_eq!(stroke.rect.radius.top_left, 4.0);
                assert_eq!(stroke.widths, StrokeWidths::all(6.0));
            }
            sdf => panic!("expected rect stroke SDF, got {sdf:?}"),
        }
    }

    #[test]
    fn push_rect_stroke_widths_records_per_side_sdf_widths() {
        let mut scene = test_scene();
        let widths = StrokeWidths {
            top: 2.0,
            right: 6.0,
            bottom: 10.0,
            left: 4.0,
        };
        scene.push_rect_stroke_widths(
            Rect::new(10.0, 12.0, 30.0, 36.0),
            Radius::all(4.0),
            widths,
            Brush::Solid(rgb(255, 0, 0)),
            FillRule::NonZero,
        );

        assert_eq!(scene.draw_records.len(), 1);
        assert!(scene.path_records.is_empty());
        assert!(scene.bd_records.is_empty());
        let draw = &scene.draw_records[0];
        assert_eq!(
            draw.pixel_bounds,
            PixelBounds {
                x0: 8,
                y0: 11,
                x1: 33,
                y1: 41,
            }
        );
        match draw.sdf {
            Some(Sdf::RectStroke(stroke)) => {
                assert_eq!(stroke.rect.axis_bounds(), (10.0, 12.0, 30.0, 36.0));
                assert_eq!(stroke.widths, widths);
            }
            sdf => panic!("expected rect stroke SDF, got {sdf:?}"),
        }
    }

    #[test]
    fn push_circle_stroke_records_sdf_without_path_storage() {
        let mut scene = test_scene();
        scene.push_circle_stroke(
            Circle::new((24.0, 20.0), 10.0),
            Stroke::new(4.0),
            Brush::Solid(rgb(255, 0, 0)),
            FillRule::NonZero,
        );

        assert_eq!(scene.draw_records.len(), 1);
        assert!(scene.path_records.is_empty());
        assert!(scene.bd_records.is_empty());
        let draw = &scene.draw_records[0];
        assert_eq!(
            draw.pixel_bounds,
            PixelBounds {
                x0: 12,
                y0: 8,
                x1: 36,
                y1: 32,
            }
        );
        match draw.sdf {
            Some(Sdf::CircleStroke(stroke)) => {
                assert_eq!(stroke.circle.center, Point::new(24.0, 20.0));
                assert_eq!(stroke.circle.radius, 10.0);
                assert_eq!(stroke.half_width, 2.0);
            }
            sdf => panic!("expected circle stroke SDF, got {sdf:?}"),
        }
    }

    #[test]
    fn push_sdf_stroke_with_zero_width_is_noop() {
        let mut scene = test_scene();
        scene.push_rect_stroke(
            Rect::new(10.0, 12.0, 30.0, 36.0),
            Radius::all(0.0),
            Stroke::new(0.0),
            Brush::Solid(rgb(255, 0, 0)),
            FillRule::NonZero,
        );
        scene.push_circle_stroke(
            Circle::new((24.0, 20.0), 10.0),
            Stroke::new(0.0),
            Brush::Solid(rgb(255, 0, 0)),
            FillRule::NonZero,
        );

        assert!(scene.draw_records.is_empty());
        assert!(scene.path_records.is_empty());
        assert!(scene.bd_records.is_empty());
    }

    #[test]
    fn push_dashed_circle_stroke_uses_path_storage() {
        let mut scene = test_scene();
        scene.push_circle_stroke(
            Circle::new((24.0, 20.0), 10.0),
            Stroke::new(4.0).with_dashes(0.0, [4.0, 4.0]),
            Brush::Solid(rgb(255, 0, 0)),
            FillRule::NonZero,
        );

        assert_eq!(scene.draw_records.len(), 1);
        assert_eq!(scene.path_records.len(), 1);
        assert_eq!(scene.bd_records.len(), 1);
        assert!(scene.draw_records[0].path_id.is_some());
        assert!(scene.draw_records[0].sdf.is_none());
    }

    #[test]
    fn push_arc_adds_draw_and_path_record() {
        let mut scene = test_scene();
        scene.push_arc(
            Arc::new((16.0, 16.0), (8.0, 6.0), 0.0, std::f64::consts::PI, 0.0),
            Brush::Solid(rgb(0, 255, 0)),
            FillRule::NonZero,
            0.25,
        );

        assert_eq!(scene.draw_records.len(), 1);
        assert_eq!(scene.path_records.len(), 1);
        assert_eq!(scene.bd_records.len(), 1);
        assert_eq!(scene.draw_records[0].tag, DrawTag::Brush);
        assert!(!scene.draw_records[0].solid_rect);
    }

    #[test]
    fn push_stroke_expands_shape_to_fill_path() {
        let mut scene = test_scene();
        scene.push_stroke(
            Rect::new(10.0, 10.0, 20.0, 20.0),
            Stroke::new(4.0),
            Brush::Solid(rgb(255, 0, 0)),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.1,
        );

        assert_eq!(scene.draw_records.len(), 1);
        let bounds = scene.draw_records[0].pixel_bounds;
        assert!(bounds.x0 <= 8);
        assert!(bounds.y0 <= 8);
        assert!(bounds.x1 >= 22);
        assert!(bounds.y1 >= 22);
        assert_eq!(scene.draw_records[0].fill_rule, FillRule::NonZero);
        assert!(!scene.draw_records[0].solid_rect);
    }

    #[test]
    fn push_path_flattens_transformed_geometry() {
        let mut scene = test_scene();
        scene.push_path(
            rect_path(0.0, 0.0, 10.0, 10.0),
            Brush::Solid(rgb(255, 0, 0)),
            Affine::translate((8.0, 4.0)),
            FillRule::NonZero,
            0.25,
        );

        assert_eq!(
            scene.draw_records[0].pixel_bounds,
            PixelBounds {
                x0: 8,
                y0: 4,
                x1: 18,
                y1: 14,
            }
        );
        assert!(scene.lines.iter().all(|line| {
            [line.p0, line.p1].into_iter().all(|point| {
                point[0] >= 8.0 && point[0] <= 18.0 && point[1] >= 4.0 && point[1] <= 14.0
            })
        }));
    }

    #[test]
    fn push_path_reserves_segment_capacity_from_scan_tile_count() {
        let mut path = BezPath::new();
        path.move_to((8.0, 8.0));
        path.line_to((9.0, 12.0));

        let mut scene = test_scene();
        scene.push_path(
            path,
            Brush::Solid(rgb(255, 0, 0)),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.25,
        );

        let record = scene.bd_records[0];
        let tile_bbox = crate::shared::bounds::TileBbox {
            x0: record.tile_x0,
            y0: record.tile_y0,
            x1: record.tile_x1,
            y1: record.tile_y1,
        };
        let expected = scene
            .lines
            .iter()
            .map(|&line| {
                line_scanned_tile_count(
                    line,
                    tile_bbox,
                    (scene.width_in_tiles(), scene.height_in_tiles()),
                )
            })
            .sum::<u32>();

        assert_eq!(record.segment_capacity, expected);
        assert_eq!(scene.tile_cnt, expected);
        assert!(expected > 0);
        assert!(expected < 20);
    }

    #[test]
    fn push_layer_path_flattens_transformed_geometry() {
        let mut scene = test_scene();
        scene.push_clip_layer(
            rect_path(0.0, 0.0, 10.0, 10.0),
            Affine::translate((12.0, 6.0)),
            FillRule::NonZero,
            0.25,
        );

        assert_eq!(
            scene.draw_records[0].pixel_bounds,
            PixelBounds {
                x0: 12,
                y0: 6,
                x1: 22,
                y1: 16,
            }
        );
        assert!(scene.lines.iter().all(|line| {
            [line.p0, line.p1].into_iter().all(|point| {
                point[0] >= 12.0 && point[0] <= 22.0 && point[1] >= 6.0 && point[1] <= 16.0
            })
        }));
    }
}
