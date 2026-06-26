use peniko::{
    Color,
    kurbo::{Affine, BezPath, Shape},
};

use crate::shared::{
    bd_record::BackdropRecord,
    bounds::{Bounds, PixelBounds},
    brush::Brush,
    draw_record::{DrawRecord, DrawTag},
    execution::{
        ClipStackEntry, Command, CommandList, CommandListId, ExecOp, ExecPlan, ROOT_COMMAND_LIST_ID,
    },
    fill::FillRule,
    layer::{Layer, LayerKind, blend::Blend, clip::Clip, opacity::Opacity},
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
            Bounds {
                x0: bounds.x0.floor() as i32,
                y0: bounds.y0.floor() as i32,
                x1: bounds.x1.ceil() as i32,
                y1: bounds.y1.ceil() as i32,
            },
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
            Bounds {
                x0: bounds.x0.floor() as i32,
                y0: bounds.y0.floor() as i32,
                x1: bounds.x1.ceil() as i32,
                y1: bounds.y1.ceil() as i32,
            },
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

    pub fn push_blend_layer(
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
            Bounds {
                x0: bounds.x0.floor() as i32,
                y0: bounds.y0.floor() as i32,
                x1: bounds.x1.ceil() as i32,
                y1: bounds.y1.ceil() as i32,
            },
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

    pub fn pop_layer(&mut self) -> Option<LayerKind> {
        self.ensure_command_root();
        let layer_kind = self.layer_stack.pop()?;
        if self.command_stack.len() > 1 {
            self.command_stack.pop();
        }
        Some(layer_kind)
    }

    fn push_path(
        &mut self,
        path: BezPath,
        brush: impl Into<Brush>,
        transform: Affine,
        rule: FillRule,
        tolerance: f64,
        bounds_override: Option<Bounds>,
    ) {
        self.ensure_command_root();
        let line_start = self.lines.len() as u32;
        let path_id = self.path_cnt;
        self.path_cnt += 1;
        let mut local_tile_cnt = 0;
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
            None => PixelBounds::from_path(&path, transform),
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
    }

    fn push_layer_path(
        &mut self,
        tag: DrawTag,
        path: BezPath,
        transform: Affine,
        rule: FillRule,
        tolerance: f64,
        bounds: Bounds,
    ) -> usize {
        let line_start = self.lines.len() as u32;
        let path_id = self.path_cnt;
        self.path_cnt += 1;
        let mut local_tile_cnt = 0;
        PathFlatten::new(&path, tolerance as f32, path_id, &mut local_tile_cnt)
            .flatten(&mut self.lines);
        let line_count = self.lines.len() as u32 - line_start;
        self.path_records.push(PathRecord {
            path_id,
            line_count,
            line_start,
            _pad: 0,
        });

        let pixel_bounds = PixelBounds {
            x0: bounds.x0,
            y0: bounds.y0,
            x1: bounds.x1,
            y1: bounds.y1,
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
            clip_stack_data: Vec::new(),
        };
        let mut clip_stack = Vec::new();
        self.compile_into(list_id, &mut ops, &mut plan, &mut clip_stack);
        plan.ops = ops;
        plan
    }

    fn compile_into(
        &self,
        list_id: CommandListId,
        ops: &mut Vec<ExecOp>,
        plan: &mut ExecPlan,
        clip_stack: &mut Vec<ClipStackEntry>,
    ) {
        let mut pending_batch: Option<(usize, usize)> = None;

        let flush_batch = |pending_batch: &mut Option<(usize, usize)>,
                           ops: &mut Vec<ExecOp>,
                           plan: &mut ExecPlan,
                           clip_stack: &[ClipStackEntry]| {
            let Some((start, end)) = pending_batch.take() else {
                return;
            };
            let clip_start = plan.clip_stack_data.len();
            plan.clip_stack_data.extend_from_slice(clip_stack);
            let clip_end = plan.clip_stack_data.len();

            ops.push(ExecOp::DrawBatch {
                draws: start..end,
                clip_stack: clip_start..clip_end,
            });
        };

        for command in &self.command_lists[list_id].commands {
            match command {
                Command::Draw(draw_ix) => match &mut pending_batch {
                    Some((start, end)) if *end == *draw_ix => {
                        *end = *draw_ix + 1;
                    }
                    Some(_) => {
                        flush_batch(&mut pending_batch, ops, plan, clip_stack);
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
                    flush_batch(&mut pending_batch, ops, plan, clip_stack);
                    if Self::can_fuse(layer) {
                        match layer {
                            Layer::Clip(_) | Layer::ClipSdf { .. } => {
                                let bounds = self.draw_bounds(*draw);
                                ops.push(ExecOp::BeginClip {
                                    draw: *draw,
                                    bounds,
                                });
                                clip_stack.push(*draw as u32);
                                self.compile_into(*children, ops, plan, clip_stack);
                                clip_stack.pop();
                                ops.push(ExecOp::EndClip);
                            }
                            Layer::Opacity(opacity) => {
                                let bounds = self.draw_bounds(*draw);
                                ops.push(ExecOp::BeginOpacity {
                                    draw: *draw,
                                    opacity: opacity.opacity,
                                    bounds,
                                });
                                self.compile_into(*children, ops, plan, clip_stack);
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
                                self.compile_into(*children, ops, plan, clip_stack);
                                ops.push(ExecOp::EndBlend {
                                    draw: *draw,
                                    mode: blend.mode,
                                    bounds,
                                });
                            }
                            _ => unreachable!(),
                        }
                    } else {
                        ops.push(ExecOp::OffscreenLayer {
                            layer: layer.clone(),
                            children: {
                                let mut child_ops = Vec::new();
                                self.compile_into(*children, &mut child_ops, plan, clip_stack);
                                child_ops
                            },
                        });
                    }
                }
            }
        }

        flush_batch(&mut pending_batch, ops, plan, clip_stack);
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
        Scene {
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
            width: 64,
            height: 64,
        }
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

    fn assert_clip_stack(plan: &ExecPlan, clip_stack: Range<usize>, expected: &[u32]) {
        let actual = &plan.clip_stack_data[clip_stack];
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
            None,
        );
        scene.push_blend_layer(
            rect_path(4.0, 4.0, 24.0, 24.0),
            Affine::IDENTITY,
            0.25,
            Blend::new(Mix::Multiply, Compose::SrcOver),
        );
        scene.push_path(
            rect_path(6.0, 6.0, 12.0, 12.0),
            Brush::Solid(rgb(0, 255, 0)),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.25,
            None,
        );
        scene.pop_layer();
        scene.push_path(
            rect_path(10.0, 10.0, 18.0, 18.0),
            Brush::Solid(rgb(0, 0, 255)),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.25,
            None,
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
            ExecOp::DrawBatch { draws, clip_stack } => {
                assert_eq!(draws.clone(), 1..2);
                assert_clip_stack(&plan, clip_stack.clone(), &[0]);
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
            ExecOp::DrawBatch { draws, clip_stack } => {
                assert_eq!(draws.clone(), 3..4);
                assert_clip_stack(&plan, clip_stack.clone(), &[0]);
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
            ExecOp::DrawBatch { draws, clip_stack } => {
                assert_eq!(draws.clone(), 4..5);
                assert_clip_stack(&plan, clip_stack.clone(), &[0]);
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
            None,
        );
        scene.push_blend_layer(
            rect_path(4.0, 4.0, 24.0, 24.0),
            Affine::IDENTITY,
            0.25,
            Blend::new(Mix::Screen, Compose::SrcOver),
        );
        scene.push_path(
            rect_path(6.0, 6.0, 12.0, 12.0),
            Brush::Solid(rgb(0, 255, 0)),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.25,
            None,
        );
        scene.pop_layer();
        scene.push_path(
            rect_path(10.0, 10.0, 18.0, 18.0),
            Brush::Solid(rgb(0, 0, 255)),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.25,
            None,
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
            ExecOp::DrawBatch { draws, clip_stack } => {
                assert_eq!(draws.clone(), 1..2);
                assert_clip_stack(&plan, clip_stack.clone(), &[]);
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
            ExecOp::DrawBatch { draws, clip_stack } => {
                assert_eq!(draws.clone(), 3..4);
                assert_clip_stack(&plan, clip_stack.clone(), &[]);
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
            ExecOp::DrawBatch { draws, clip_stack } => {
                assert_eq!(draws.clone(), 4..5);
                assert_clip_stack(&plan, clip_stack.clone(), &[]);
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
}
