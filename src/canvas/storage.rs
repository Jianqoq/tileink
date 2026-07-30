use super::*;

impl Canvas {
    /// Clears all recorded content and retargets this allocation to a new logical surface.
    ///
    /// Vector capacities are retained, making this the preferred frame boundary for immediate
    /// renderers whose viewport can change. No recorded geometry survives the call, so changing
    /// the scale factor cannot leave mixed-scale records behind.
    pub fn reset_for_surface(
        &mut self,
        logical_width: u32,
        logical_height: u32,
        scale_factor: f32,
    ) {
        let scale_factor = valid_scale_factor(scale_factor);
        self.reset();
        self.logical_width = logical_width;
        self.logical_height = logical_height;
        self.scale_factor = scale_factor;
    }

    pub fn reset(&mut self) {
        self.lines.clear();
        self.path_records.clear();
        self.draw_records.clear();
        self.brush_blob.clear();
        self.sdf_blob.clear();
        self.sdf_shadow_blob.clear();
        self.text_glyphs.clear();
        self.text_runs.clear();
        self.scene_images.clear();
        if let Some(root) = self.command_lists.first_mut() {
            root.commands.clear();
            self.command_lists.truncate(1);
        } else {
            self.command_lists.push(CommandList::default());
        }
        self.root_commands = ROOT_COMMAND_LIST_ID;
        self.command_stack.clear();
        self.command_stack.push(self.root_commands);
        self.layer_stack.clear();
        self.invalidated_bounds.clear();
        self.invalidate_all = false;
        self.buffer_changes = None;
        self.plan_cache_key = None;
        self.compiled_plan = None;
        self.persistent_frame = None;
        self.painter_keys = None;
        self.stable_batch_ids = None;
        self.stable_batch_counts = None;
        self.retained_transform = GpuAffine::IDENTITY;
        self.path_cnt = 0;
        self.backdrop_pool_capacity = 0;
        self.tile_cnt = 0;
        self.draw_generation = self.draw_generation.wrapping_add(1);
    }

    pub(super) fn push_draw_record(&mut self, draw: DrawRecord) -> usize {
        let draw_ix = self.draw_records.len();
        self.draw_records.push(draw);
        draw_ix
    }

    pub(super) fn push_brush(&mut self, brush: Brush) -> (u32, u32) {
        self.push_physical_brush(self.physical_brush(brush))
    }

    pub(super) fn push_physical_brush(&mut self, brush: Brush) -> (u32, u32) {
        push_encoded_brush(&mut self.brush_blob, &brush)
    }

    pub(super) fn push_sdf(&mut self, sdf: Sdf) -> (u32, u32) {
        push_encoded_sdf(&mut self.sdf_blob, sdf)
    }

    pub(super) fn push_sdf_shadow(&mut self, sdf_shadow: SdfShadow) -> (u32, u32) {
        push_encoded_sdf_shadow(&mut self.sdf_shadow_blob, sdf_shadow)
    }

    pub(crate) fn draw_brush_for_record(&self, draw: &DrawRecord) -> Option<Brush> {
        decode_encoded_brush(&self.brush_blob, draw.brush_offset, draw.brush_len)
    }

    pub(crate) fn draw_sdf(&self, draw: &DrawRecord) -> Option<Sdf> {
        decode_sdf(&self.sdf_blob, draw.sdf_offset, draw.sdf_len)
    }

    pub(crate) fn draw_sdf_shadow(&self, draw: &DrawRecord) -> Option<SdfShadow> {
        decode_sdf_shadow(
            &self.sdf_shadow_blob,
            draw.sdf_shadow_offset,
            draw.sdf_shadow_len,
        )
    }

    pub(crate) fn width_in_tiles(&self) -> u32 {
        self.physical_width().div_ceil(crate::TILE_SIZE)
    }

    pub(crate) fn height_in_tiles(&self) -> u32 {
        self.physical_height().div_ceil(crate::TILE_SIZE)
    }

    pub(super) fn segment_capacity_for_path_lines(
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
}
