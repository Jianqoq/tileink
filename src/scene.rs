use peniko::kurbo::{Affine, BezPath, Shape};

use crate::shared::{
    bd_record::BackdropRecord,
    bounds::{Bounds, PixelBounds},
    brush::Brush,
    draw_record::DrawRecord,
    execution::{BatchState, Command, CommandList, CommandListId, ExecNode, ROOT_COMMAND_LIST_ID},
    fill::FillRule,
    layer::{Layer, LayerKind, clip::Clip},
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
            Command::Layer { layer, children } => Command::Layer {
                layer,
                children: children + child_list_offset,
            },
        }
    }

    pub fn push_clip_layer(&mut self, path: BezPath, transform: Affine, tolerance: f64) {
        self.ensure_command_root();
        let bounds = transform.transform_rect_bbox(path.bounding_box());
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
            .push(Command::Layer { layer, children });
        self.command_stack.push(children);
        self.layer_stack.push(LayerKind::Clip);
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
            brush: brush.into(),
            fill_rule: rule,
            pixel_bounds,
            solid_rect: false,
            opacity_depth: self
                .layer_stack
                .iter()
                .filter(|&&kind| kind == LayerKind::Opacity)
                .count() as u8,
            blend_depth: self
                .layer_stack
                .iter()
                .filter(|&&kind| kind == LayerKind::Blend)
                .count() as u8,
            clip_depth: self
                .layer_stack
                .iter()
                .filter(|&&kind| kind == LayerKind::Clip)
                .count() as u8,
            allow_solid_override: true,
        });
        self.current_command_list_mut()
            .commands
            .push(Command::Draw(draw_ix));
        self.bd_records.push(BackdropRecord {
            path_id,
            data_offset: backdrop_offset,
            tile_x0: tile_bbox.x0,
            tile_y0: tile_bbox.y0,
            tile_x1: tile_bbox.x1,
            tile_y1: tile_bbox.y1,
            segment_start,
            segment_capacity: local_tile_cnt,
            segment_count: 0,
        });
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

    fn batch_state_for_draw(&self, draw_ix: usize) -> BatchState {
        let draw = &self.draw_records[draw_ix];
        BatchState {
            clip_depth: draw.clip_depth,
            blend_depth: draw.blend_depth,
            opacity_depth: draw.opacity_depth,
        }
    }

    pub(crate) fn compile(&self, list_id: CommandListId) -> Vec<ExecNode> {
        let mut nodes = Vec::new();
        let mut pending_batch: Option<(usize, usize, BatchState)> = None;

        for command in &self.command_lists[list_id].commands {
            match command {
                Command::Draw(draw_ix) => {
                    let state = self.batch_state_for_draw(*draw_ix);
                    match &mut pending_batch {
                        Some((start, end, batch_state))
                            if *end == *draw_ix && *batch_state == state =>
                        {
                            let _ = start;
                            *end = *draw_ix + 1;
                        }
                        Some((start, end, batch_state)) => {
                            nodes.push(ExecNode::DrawBatch {
                                draws: *start..*end,
                                state: *batch_state,
                            });
                            pending_batch = Some((*draw_ix, *draw_ix + 1, state));
                        }
                        None => {
                            pending_batch = Some((*draw_ix, *draw_ix + 1, state));
                        }
                    }
                }
                Command::Layer { layer, children } => {
                    if let Some((start, end, batch_state)) = pending_batch.take() {
                        nodes.push(ExecNode::DrawBatch {
                            draws: start..end,
                            state: batch_state,
                        });
                    }
                    if Self::can_fuse(layer) {
                        nodes.extend(self.compile(*children));
                    } else {
                        nodes.push(ExecNode::OffscreenLayer {
                            layer: layer.clone(),
                            children: self.compile(*children),
                        });
                    }
                }
            }
        }

        if let Some((start, end, batch_state)) = pending_batch.take() {
            nodes.push(ExecNode::DrawBatch {
                draws: start..end,
                state: batch_state,
            });
        }

        nodes
    }

    fn can_fuse(layer: &Layer) -> bool {
        matches!(
            layer,
            Layer::Clip(_) | Layer::ClipSdf { .. } | Layer::Opacity { .. } | Layer::Blend { .. }
        )
    }
}
