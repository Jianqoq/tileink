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
        arc::{Arc as SdfArc, ArcShadow as SdfArcShadow},
        candlestick::CandleStick as SdfCandleStick,
        circle::{
            Circle as SdfCircle, CircleShadow as SdfCircleShadow, CircleStroke as SdfCircleStroke,
        },
        line::{Line as SdfLine, LineShadow as SdfLineShadow},
        rect::{
            Radius, Rect as SdfRect, RectShadow as SdfRectShadow, RectShadowOptions,
            RectStroke as SdfRectStroke, StrokeWidths,
        },
    },
};
use crate::text::{
    TextContext, TextLayout, TextRun, layout_bounds_at_origin, scene_glyphs_at_origin,
};

#[derive(Clone)]
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

mod scale;

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
        assert!(
            !filter.contains_rect_liquid_glass(),
            "RectLiquidGlass is a rounded-rectangle backdrop effect; use push_backdrop_layer with Region::Rect"
        );
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
        if filter.contains_rect_liquid_glass() {
            assert!(
                matches!(sample_region, Region::Rect { .. }),
                "RectLiquidGlass requires Region::Rect because it uses rounded-rectangle SDF normals"
            );
        }
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

    /// Adds a filled rectangle as SDF geometry with independent corner radii.
    ///
    /// This keeps rounded rectangles on the SDF path instead of flattening them
    /// to path segments, matching the SDF shadow/stroke APIs and preserving
    /// subpixel edge ownership in both CPU and CubeCL renderers.
    pub fn push_rect(
        &mut self,
        rect: Rect,
        radius: Radius,
        brush: impl Into<Brush>,
        rule: FillRule,
    ) {
        self.push_sdf_draw(
            Sdf::Rect(SdfRect {
                start: Point::new(rect.x0, rect.y0),
                end: Point::new(rect.x1, rect.y1),
                radius,
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
        rule: FillRule,
    ) {
        let Some(options) = options.normalized() else {
            return;
        };
        let shadow = SdfRectShadow {
            rect: SdfRect {
                start: Point::new(rect.x0, rect.y0),
                end: Point::new(rect.x1, rect.y1),
                radius,
            },
            options,
        };
        self.push_sdf_draw(Sdf::RectShadow(shadow), shadow.bounds(), brush, rule);
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

    pub fn push_circle_shadow(
        &mut self,
        circle: Circle,
        options: RectShadowOptions,
        brush: impl Into<Brush>,
        rule: FillRule,
    ) {
        let Some(options) = options.normalized() else {
            return;
        };
        let shadow = SdfCircleShadow {
            circle: SdfCircle {
                center: circle.center,
                radius: circle.radius as f32,
            },
            options,
        };
        self.push_sdf_draw(Sdf::CircleShadow(shadow), shadow.bounds(), brush, rule);
    }

    /// Adds a circular stroked arc as SDF geometry.
    ///
    /// This is separate from [`push_arc`](Self::push_arc), which preserves the
    /// existing path-backed kurbo arc semantics. Use this method when the arc is
    /// a stroke-like primitive and should avoid path flattening.
    pub fn push_sdf_arc(&mut self, arc: SdfArc, brush: impl Into<Brush>, rule: FillRule) {
        if arc.is_empty() {
            return;
        }
        self.push_sdf_draw(Sdf::Arc(arc), arc.bounds(), brush, rule);
    }

    pub fn push_arc_shadow(
        &mut self,
        arc: SdfArc,
        options: RectShadowOptions,
        brush: impl Into<Brush>,
        rule: FillRule,
    ) {
        if arc.is_empty() {
            return;
        }
        let Some(options) = options.normalized() else {
            return;
        };
        let shadow = SdfArcShadow { arc, options };
        self.push_sdf_draw(Sdf::ArcShadow(shadow), shadow.bounds(), brush, rule);
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

    pub fn push_line_shadow(
        &mut self,
        line: SdfLine,
        options: RectShadowOptions,
        brush: impl Into<Brush>,
        rule: FillRule,
    ) {
        if line.is_empty() {
            return;
        }
        let Some(options) = options.normalized() else {
            return;
        };
        let shadow = SdfLineShadow { line, options };
        self.push_sdf_draw(Sdf::LineShadow(shadow), shadow.bounds(), brush, rule);
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

    fn rebuild_backdrop_records(&mut self) {
        let mut path_bounds: Vec<Option<PixelBounds>> = vec![None; self.path_records.len()];
        for draw in &self.draw_records {
            if let Some(path_id) = draw.path_id
                && let Some(slot) = path_bounds.get_mut(path_id as usize)
            {
                *slot = Some(match *slot {
                    Some(bounds) => bounds.union(draw.pixel_bounds),
                    None => draw.pixel_bounds,
                });
            }
        }

        let mut data_offset = 0;
        let mut segment_start = 0;
        let width_in_tiles = self.width_in_tiles();
        let height_in_tiles = self.height_in_tiles();
        let mut records = Vec::with_capacity(self.bd_records.len());
        for record in &self.bd_records {
            let path_id = record.path_id as usize;
            let pixel_bounds = path_bounds
                .get(path_id)
                .and_then(|bounds| *bounds)
                .unwrap_or_else(|| self.path_pixel_bounds(path_id));
            let tile_bbox = pixel_bounds.tile_bbox(width_in_tiles, height_in_tiles);
            let data_len = tile_bbox.tile_count();
            let segment_capacity = self.path_segment_capacity(path_id, tile_bbox);
            records.push(BackdropRecord {
                path_id: record.path_id,
                data_offset,
                data_len,
                tile_x0: tile_bbox.x0,
                tile_y0: tile_bbox.y0,
                tile_x1: tile_bbox.x1,
                tile_y1: tile_bbox.y1,
                segment_start,
                segment_capacity,
                segment_count: 0,
            });
            data_offset += data_len;
            segment_start += segment_capacity;
        }

        self.bd_records = records;
        self.backdrop_pool_capacity = data_offset;
        self.tile_cnt = segment_start;
    }

    fn path_pixel_bounds(&self, path_id: usize) -> PixelBounds {
        let Some(record) = self.path_records.get(path_id) else {
            return PixelBounds {
                x0: 0,
                y0: 0,
                x1: 0,
                y1: 0,
            };
        };
        let lines = &self.lines
            [record.line_start as usize..(record.line_start + record.line_count) as usize];
        if lines.is_empty() {
            return PixelBounds {
                x0: 0,
                y0: 0,
                x1: 0,
                y1: 0,
            };
        }

        let mut x0 = f32::INFINITY;
        let mut y0 = f32::INFINITY;
        let mut x1 = f32::NEG_INFINITY;
        let mut y1 = f32::NEG_INFINITY;
        for line in lines {
            x0 = x0.min(line.p0[0]).min(line.p1[0]);
            y0 = y0.min(line.p0[1]).min(line.p1[1]);
            x1 = x1.max(line.p0[0]).max(line.p1[0]);
            y1 = y1.max(line.p0[1]).max(line.p1[1]);
        }
        PixelBounds {
            x0: x0.floor() as i32,
            y0: y0.floor() as i32,
            x1: x1.ceil() as i32,
            y1: y1.ceil() as i32,
        }
    }

    fn path_segment_capacity(
        &self,
        path_id: usize,
        tile_bbox: crate::shared::bounds::TileBbox,
    ) -> u32 {
        let Some(record) = self.path_records.get(path_id) else {
            return 0;
        };
        self.segment_capacity_for_path_lines(record.line_start, record.line_count, tile_bbox)
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
mod tests;
