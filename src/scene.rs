use peniko::{
    Color, Compose, Mix,
    kurbo::{Affine, Arc, BezPath, Rect, Shape, Stroke, StrokeOpts, stroke as kurbo_stroke},
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
        Layer, LayerKind, blend::Blend, clip::Clip, filter::Filter, opacity::Opacity,
        region::Region,
    },
    line::Line,
    path::PathRecord,
    path_flatten::PathFlatten,
};

pub struct Scene {
    pub(crate) lines: Vec<Line>,
    pub(crate) path_records: Vec<PathRecord>,
    pub(crate) draw_records: Vec<DrawRecord>,
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

impl Scene {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            lines: Vec::new(),
            path_records: Vec::new(),
            draw_records: Vec::new(),
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

    fn draw_bounds(&self, draw_ix: usize) -> Bounds {
        let bounds = self.draw_records[draw_ix].pixel_bounds;
        Bounds::new(bounds.x0, bounds.y0, bounds.x1, bounds.y1)
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
        }
        self.draw_records.extend(other.draw_records);

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
        }
    }

    pub fn push_clip_layer(&mut self, path: BezPath, transform: Affine, tolerance: f64) {
        self.ensure_command_root();
        let bounds = transform.transform_rect_bbox(path.bounding_box());
        let draw = self.push_layer_path(
            DrawTag::Clip,
            path.clone(),
            transform,
            FillRule::NonZero,
            tolerance,
        );
        let layer = Layer::Clip(Clip {
            path,
            bounds: Bounds {
                x0: bounds.x0.floor() as i32,
                y0: bounds.y0.floor() as i32,
                x1: bounds.x1.ceil() as i32,
                y1: bounds.y1.ceil() as i32,
            },
            transform,
            tolerance,
        });
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

    pub fn push_opacity_layer(
        &mut self,
        path: BezPath,
        transform: Affine,
        tolerance: f64,
        opacity: f32,
    ) {
        self.ensure_command_root();
        let bounds = transform.transform_rect_bbox(path.bounding_box());
        let draw = self.push_layer_path(
            DrawTag::Opacity,
            path.clone(),
            transform,
            FillRule::NonZero,
            tolerance,
        );
        let layer = Layer::Opacity(Opacity {
            path,
            bounds: Bounds {
                x0: bounds.x0.floor() as i32,
                y0: bounds.y0.floor() as i32,
                x1: bounds.x1.ceil() as i32,
                y1: bounds.y1.ceil() as i32,
            },
            transform,
            tolerance,
            opacity,
        });
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
        let bounds = transform.transform_rect_bbox(path.bounding_box());
        let draw = self.push_layer_path(
            DrawTag::Blend,
            path.clone(),
            transform,
            FillRule::NonZero,
            tolerance,
        );
        let layer = Layer::Blend(Blend {
            path,
            bounds: Bounds {
                x0: bounds.x0.floor() as i32,
                y0: bounds.y0.floor() as i32,
                x1: bounds.x1.ceil() as i32,
                y1: bounds.y1.ceil() as i32,
            },
            transform,
            tolerance,
            mode: blend.mode,
        });
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

    /// Adds an offscreen filter group clipped by `region`.
    ///
    /// The CPU renderer uses this for real filter semantics instead of baking
    /// example-specific filtered pixels into fixtures.
    pub fn push_filter_layer(&mut self, filter: Filter, region: Region) {
        self.ensure_command_root();
        let children = self.command_lists.len();
        self.command_lists.push(CommandList::default());
        self.current_command_list_mut()
            .commands
            .push(Command::Layer {
                draw: 0,
                layer: Layer::Filter { filter, region },
                children,
            });
        self.command_stack.push(children);
        self.layer_stack.push(LayerKind::Filter);
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
        let bounds = Bounds {
            x0: rect.x0.min(rect.x1).floor() as i32,
            y0: rect.y0.min(rect.y1).floor() as i32,
            x1: rect.x0.max(rect.x1).ceil() as i32,
            y1: rect.y0.max(rect.y1).ceil() as i32,
        };
        let draw = self.push_path_inner(
            rect.to_path(0.0),
            brush,
            Affine::IDENTITY,
            rule,
            0.0,
            Some(bounds),
        );
        if let Some(draw) = self.draw_records.get_mut(draw) {
            draw.solid_rect = true;
        }
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
        self.ensure_command_root();
        let line_start = self.lines.len() as u32;
        let path_id = self.path_cnt;
        self.path_cnt += 1;
        let mut local_tile_cnt = 0;
        let path = Self::transform_path(path, transform);
        PathFlatten::new(&path, tolerance as f32, path_id, &mut local_tile_cnt)
            .flatten(&mut self.lines);
        let line_count = self.lines.len() as u32 - line_start;
        self.path_records.push(PathRecord {
            path_id,
            line_count,
            line_start,
            _pad: 0,
        });
        let pixel_bounds = match bounds_override {
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

        let backdrop_offset = self.backdrop_pool_capacity;
        self.backdrop_pool_capacity += backdrop_len;
        let segment_start = self.tile_cnt;
        self.tile_cnt += local_tile_cnt;

        let draw_ix = self.draw_records.len();
        self.draw_records.push(DrawRecord {
            path_id: Some(path_id),
            tag: DrawTag::Brush,
            brush: brush.into(),
            fill_rule: rule,
            pixel_bounds,
            solid_rect: false,
            allow_solid_override: true,
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
        let mut local_tile_cnt = 0;
        let path = Self::transform_path(path, transform);
        PathFlatten::new(&path, tolerance as f32, path_id, &mut local_tile_cnt)
            .flatten(&mut self.lines);
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

        let backdrop_offset = self.backdrop_pool_capacity;
        self.backdrop_pool_capacity += backdrop_len;
        let segment_start = self.tile_cnt;
        self.tile_cnt += local_tile_cnt;

        let draw_ix = self.draw_records.len();
        self.draw_records.push(DrawRecord {
            path_id: Some(path_id),
            tag,
            brush: Brush::Solid(Color::TRANSPARENT),
            fill_rule: rule,
            pixel_bounds,
            solid_rect: false,
            allow_solid_override: false,
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

    pub fn reset(&mut self) {
        self.lines.clear();
        self.path_records.clear();
        self.draw_records.clear();
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
                    if Self::can_fuse(layer) {
                        match layer {
                            Layer::Clip(_) | Layer::ClipSdf { .. } => {
                                let bounds = self.draw_bounds(*draw);
                                ops.push(ExecOp::BeginClip {
                                    draw: *draw,
                                    bounds,
                                });
                                layer_stack.push(LayerStackEntry::Clip { draw: *draw as u32 });
                                self.compile_into(*children, ops, plan, layer_stack);
                                layer_stack.pop();
                                ops.push(ExecOp::EndClip);
                            }
                            Layer::Opacity(opacity) => {
                                let bounds = self.draw_bounds(*draw);
                                ops.push(ExecOp::BeginOpacity {
                                    draw: *draw,
                                    opacity: opacity.opacity,
                                    bounds,
                                });
                                layer_stack.push(LayerStackEntry::Opacity {
                                    draw: *draw as u32,
                                    opacity: opacity.opacity,
                                });
                                self.compile_into(*children, ops, plan, layer_stack);
                                layer_stack.pop();
                                ops.push(ExecOp::EndOpacity {
                                    draw: *draw,
                                    opacity: opacity.opacity,
                                    bounds,
                                });
                            }
                            Layer::Blend(blend) => {
                                let bounds = self.draw_bounds(*draw);
                                ops.push(ExecOp::BeginBlend {
                                    draw: *draw,
                                    mode: blend.mode,
                                    bounds,
                                });
                                layer_stack.push(LayerStackEntry::Blend {
                                    draw: *draw as u32,
                                    mode: blend.mode,
                                });
                                self.compile_into(*children, ops, plan, layer_stack);
                                layer_stack.pop();
                                ops.push(ExecOp::EndBlend {
                                    draw: *draw,
                                    mode: blend.mode,
                                    bounds,
                                });
                            }
                            _ => unreachable!(),
                        }
                    } else {
                        let stack_start = plan.layer_stack_data.len();
                        plan.layer_stack_data.extend_from_slice(layer_stack);
                        let stack_end = plan.layer_stack_data.len();
                        ops.push(ExecOp::OffscreenLayer {
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
            }
        }

        flush_batch(&mut pending_batch, ops, plan, layer_stack);
    }

    fn can_fuse(layer: &Layer) -> bool {
        matches!(
            layer,
            Layer::Clip(_) | Layer::ClipSdf { .. } | Layer::Opacity(_) | Layer::Blend(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ops::Range;

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
        scene.push_clip_layer(rect_path(0.0, 0.0, 32.0, 32.0), Affine::IDENTITY, 0.25);
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
            ExecOp::BeginClip { draw, bounds } => {
                assert_eq!(*draw, 0);
                assert_eq!(*bounds, Bounds::new(0, 0, 32, 32));
            }
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
            ExecOp::BeginBlend { draw, mode, bounds } => {
                assert_eq!(*draw, 2);
                assert_eq!(*mode, BlendMode::new(Mix::Multiply, Compose::SrcOver));
                assert_eq!(*bounds, Bounds::new(4, 4, 24, 24));
            }
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
            ExecOp::EndBlend { draw, mode, bounds } => {
                assert_eq!(*draw, 2);
                assert_eq!(*mode, BlendMode::new(Mix::Multiply, Compose::SrcOver));
                assert_eq!(*bounds, Bounds::new(4, 4, 24, 24));
            }
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
            ExecOp::BeginOpacity {
                draw,
                opacity,
                bounds,
            } => {
                assert_eq!(*draw, 0);
                assert_eq!(*opacity, 0.5);
                assert_eq!(*bounds, Bounds::new(0, 0, 32, 32));
            }
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
            ExecOp::BeginBlend { draw, mode, bounds } => {
                assert_eq!(*draw, 2);
                assert_eq!(*mode, BlendMode::new(Mix::Screen, Compose::SrcOver));
                assert_eq!(*bounds, Bounds::new(4, 4, 24, 24));
            }
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
            ExecOp::EndBlend { draw, mode, bounds } => {
                assert_eq!(*draw, 2);
                assert_eq!(*mode, BlendMode::new(Mix::Screen, Compose::SrcOver));
                assert_eq!(*bounds, Bounds::new(4, 4, 24, 24));
            }
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
            ExecOp::EndOpacity {
                draw,
                opacity,
                bounds,
            } => {
                assert_eq!(*draw, 0);
                assert_eq!(*opacity, 0.5);
                assert_eq!(*bounds, Bounds::new(0, 0, 32, 32));
            }
            op => panic!("expected EndOpacity, got {op:#?}"),
        }
    }

    #[test]
    fn push_rect_marks_solid_rect_and_sets_bounds() {
        let mut scene = test_scene();
        scene.push_rect(
            Rect::new(2.0, 3.0, 18.0, 19.0),
            Brush::Solid(rgb(255, 0, 0)),
            FillRule::NonZero,
        );

        assert_eq!(scene.draw_records.len(), 1);
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
        assert!(draw.solid_rect);
        assert_eq!(draw.tag, DrawTag::Brush);
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
    fn push_layer_path_flattens_transformed_geometry() {
        let mut scene = test_scene();
        scene.push_clip_layer(
            rect_path(0.0, 0.0, 10.0, 10.0),
            Affine::translate((12.0, 6.0)),
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
