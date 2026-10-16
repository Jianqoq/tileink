use super::*;

impl Canvas {
    /// Clears all recorded content and retargets this allocation to a new logical surface.
    ///
    /// Vector capacities are retained, making this the preferred frame boundary for immediate
    /// renderers whose viewport can change. No recorded geometry survives the call, so changing
    /// the scale factor cannot leave mixed-scale records behind.
    ///
    /// # Panics
    /// Panics if the scale is nonpositive/nonfinite or the resulting surface extent is unrepresentable.
    pub fn reset_for_surface(
        &mut self,
        logical_width: u32,
        logical_height: u32,
        scale_factor: f32,
    ) {
        #[allow(
            clippy::expect_used,
            reason = "The authored reset requires a representable canvas surface before mutation"
        )]
        let surface = CanvasSurface::from_logical((logical_width, logical_height), scale_factor)
            .expect("valid canvas surface");
        self.reset_to_surface(surface);
    }

    /// Clears drawing while retaining allocations and the exact validated viewport.
    pub fn reset_to_surface(&mut self, surface: CanvasSurface) {
        self.reset();
        self.set_surface_extent(surface);
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
        self.command_lists[ROOT_COMMAND_LIST_ID].commands.clear();
        self.command_lists.truncate(1);
        self.current_commands = ROOT_COMMAND_LIST_ID;
        self.layer_stack.clear();
        self.invalidated_bounds.clear();
        self.invalidate_all = false;
        self.buffer_changes = None;
        self.plan_cache_key = None;
        self.materialization_key = None;
        self.compiled_plan = None;
        self.persistent_frame = None;
        self.painter = PainterMetadata::default();
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
        self.push_physical_brush(&self.physical_brush(brush))
    }

    pub(super) fn push_physical_brush(&mut self, brush: &Brush) -> (u32, u32) {
        push_encoded_brush(&mut self.brush_blob, brush)
    }

    pub(super) fn push_sdf(&mut self, sdf: Sdf) -> (u32, u32) {
        push_encoded_sdf(&mut self.sdf_blob, sdf)
    }

    pub(super) fn push_sdf_shadow(&mut self, sdf_shadow: SdfShadow) -> (u32, u32) {
        push_encoded_sdf_shadow(&mut self.sdf_shadow_blob, sdf_shadow)
    }

    #[allow(
        clippy::expect_used,
        reason = "Present canvas records are encoder-owned; decode failure indicates internal corruption"
    )]
    pub(crate) fn draw_brush_for_record(&self, draw: &DrawRecord) -> Option<Brush> {
        // NONE represents absence. A present record is encoder-owned and must
        // decode; treating corruption as absence silently drops paint on append.
        (draw.brush_offset != DrawRecord::NONE).then(|| {
            decode_encoded_brush(&self.brush_blob, draw.brush_offset, draw.brush_len)
                .expect("encoded canvas brush")
        })
    }

    #[allow(
        clippy::expect_used,
        reason = "Present canvas records are encoder-owned; decode failure indicates internal corruption"
    )]
    pub(crate) fn draw_sdf(&self, draw: &DrawRecord) -> Option<Sdf> {
        (draw.sdf_offset != DrawRecord::NONE).then(|| {
            decode_sdf(&self.sdf_blob, draw.sdf_offset, draw.sdf_len).expect("encoded canvas SDF")
        })
    }

    #[allow(
        clippy::expect_used,
        reason = "Present canvas records are encoder-owned; decode failure indicates internal corruption"
    )]
    pub(crate) fn draw_sdf_shadow(&self, draw: &DrawRecord) -> Option<SdfShadow> {
        (draw.sdf_shadow_offset != DrawRecord::NONE).then(|| {
            decode_sdf_shadow(
                &self.sdf_shadow_blob,
                draw.sdf_shadow_offset,
                draw.sdf_shadow_len,
            )
            .expect("encoded canvas SDF shadow")
        })
    }

    pub(crate) const fn width_in_tiles(&self) -> u32 {
        self.physical_width().div_ceil(crate::TILE_SIZE)
    }

    pub(crate) const fn height_in_tiles(&self) -> u32 {
        self.physical_height().div_ceil(crate::TILE_SIZE)
    }

    pub(super) fn segment_capacity_for_path_lines(
        &self,
        line_start: u32,
        line_count: u32,
        tile_bbox: crate::shared::bounds::TileBbox,
    ) -> u32 {
        self.lines[line_start as usize..(line_start + line_count) as usize]
            .iter()
            .fold(0u32, |capacity, &line| {
                capacity.saturating_add(line_scanned_tile_count(line, tile_bbox))
            })
    }
}
