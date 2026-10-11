use super::*;

impl Canvas {
    pub fn draw_count(&self) -> usize {
        self.draw_records.len()
    }

    pub fn draw_id_at(&self, index: usize) -> Option<DrawId> {
        (index < self.draw_records.len()).then(|| self.draw_id_from_index(index))
    }

    pub fn draw_brush(&self, draw: DrawId) -> Option<Brush> {
        self.draw_index(draw)
            .and_then(|index| self.draw_brush_for_record(&self.draw_records[index]))
    }

    /// Replaces a draw's brush in the semantic draw record.
    ///
    /// GPU upload data is derived from draw records during upload, so this
    /// mutation only updates the scene source of truth.
    pub fn set_draw_brush(&mut self, draw: DrawId, brush: impl Into<Brush>) -> bool {
        let Some(index) = self.draw_index(draw) else {
            return false;
        };
        let (brush_offset, brush_len) = self.push_brush(brush.into());
        self.draw_records[index].brush_offset = brush_offset;
        self.draw_records[index].brush_len = brush_len;
        true
    }

    pub fn set_draw_color(&mut self, draw: DrawId, color: Color) -> bool {
        self.set_draw_brush(draw, Brush::Solid(color))
    }

    pub fn draw_solid_color(&self, draw: DrawId) -> Option<Color> {
        self.draw_brush(draw).and_then(|brush| brush.solid_color())
    }

    pub(super) fn draw_id_from_index(&self, index: usize) -> DrawId {
        debug_assert!(index < self.draw_records.len());
        DrawId {
            index: index as u32,
            generation: self.draw_generation,
        }
    }

    pub(super) fn draw_index(&self, draw: DrawId) -> Option<usize> {
        if draw.generation != self.draw_generation {
            return None;
        }
        let index = draw.index as usize;
        (index < self.draw_records.len()).then_some(index)
    }

    pub(super) fn current_command_list_mut(&mut self) -> &mut CommandList {
        &mut self.command_lists[self.current_commands]
    }

    pub(super) fn push_child_command_list(&mut self) -> CommandListId {
        let children = self.command_lists.len();
        self.command_lists.push(CommandList::default());
        children
    }

    pub(super) fn push_layer_command(&mut self, layer: Layer) {
        let kind = match &layer {
            Layer::Clip { .. } => LayerKind::Clip,
            Layer::ClipSdf { .. } => LayerKind::ClipSdf,
            Layer::Isolate { .. } => LayerKind::Isolate,
            Layer::Opacity { .. } => LayerKind::Opacity,
            Layer::Blend { .. } => LayerKind::Blend,
            Layer::Filter { .. } => LayerKind::Filter,
            Layer::Backdrop { .. } => LayerKind::Backdrop,
        };
        let children = self.push_child_command_list();
        self.current_command_list_mut()
            .commands
            .push(Command::Layer {
                retained: None,
                layer,
                children,
            });
        self.layer_stack.push(OpenLayer {
            parent: self.current_commands,
            kind,
        });
        self.current_commands = children;
    }

    pub(super) fn push_mask_command(&mut self, layer: Mask, mask_commands: CommandListId) {
        let content = self.push_child_command_list();
        self.current_command_list_mut()
            .commands
            .push(Command::MaskLayer {
                retained: None,
                layer,
                content,
                mask: mask_commands,
            });
        self.layer_stack.push(OpenLayer {
            parent: self.current_commands,
            kind: LayerKind::Mask,
        });
        self.current_commands = content;
    }
}
