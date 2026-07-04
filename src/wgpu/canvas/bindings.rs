use super::super::buffer::WgpuBuffer;
use super::{WgpuCoarseBuffers, WgpuScanBuffers, WgpuSceneBuffers};

impl WgpuSceneBuffers {
    #[cfg(test)]
    pub(crate) fn draw_records_capacity(&self) -> ::wgpu::BufferAddress {
        self.draw_records.capacity()
    }

    pub(crate) fn fine_bindings(&self) -> WgpuFineSceneBindings<'_> {
        WgpuFineSceneBindings {
            draw_records: self.draw_records.buffer(),
            sdf_blob: self.sdf_blob.buffer(),
            sdf_shadow_blob: self.sdf_shadow_blob.buffer(),
            brush_data: self.draw_brush_data.buffer(),
            brush_params: self.draw_brush_params.buffer(),
            brush_payloads: self.draw_brush_payloads.buffer(),
            image_resource_metadata: self.image_resource_metadata.buffer(),
            image_resource_pixels: self.image_resource_pixels.buffer(),
        }
    }

    pub(crate) fn image_resource_bindings(&self) -> (&::wgpu::Buffer, &::wgpu::Buffer) {
        (
            self.image_resource_metadata.buffer(),
            self.image_resource_pixels.buffer(),
        )
    }

    pub(crate) fn tile_fine_bindings<'a>(
        &'a self,
        scan: &'a WgpuScanBuffers,
        coarse: &'a WgpuCoarseBuffers,
        clip_spills: &'a WgpuBuffer,
        group_spills: &'a WgpuBuffer,
    ) -> WgpuTileFineBindings<'a> {
        WgpuTileFineBindings {
            fine: self.fine_bindings(),
            tile_range_starts: coarse.tile_ptcl_range_starts.buffer(),
            tile_range_ends: coarse.tile_ptcl_range_ends.buffer(),
            ptcl_tags: coarse.ptcl_tags.buffer(),
            ptcl_backdrops: coarse.ptcl_backdrops.buffer(),
            ptcl_fill_rules: coarse.ptcl_fill_rules.buffer(),
            ptcl_segment_starts: coarse.ptcl_segment_starts.buffer(),
            ptcl_segment_ends: coarse.ptcl_segment_ends.buffer(),
            ptcl_colors: coarse.ptcl_colors.buffer(),
            segments: scan.segments.buffer(),
            glyph_indices: coarse.glyph_indices.buffer(),
            glyph_image_ids: self.glyph_image_ids.buffer(),
            glyph_x: self.glyph_x.buffer(),
            glyph_y: self.glyph_y.buffer(),
            glyph_image_left: self.glyph_image_left.buffer(),
            glyph_image_top: self.glyph_image_top.buffer(),
            glyph_image_width: self.glyph_image_width.buffer(),
            glyph_image_height: self.glyph_image_height.buffer(),
            glyph_image_content: self.glyph_image_content.buffer(),
            glyph_image_data_offsets: self.glyph_image_data_offsets.buffer(),
            glyph_image_data: self.glyph_image_data.buffer(),
            clip_spills: clip_spills.buffer(),
            group_spills: group_spills.buffer(),
        }
    }

    pub(crate) fn cumsum_bindings<'a>(
        &'a self,
        scan: &'a WgpuScanBuffers,
    ) -> WgpuCumsumBindings<'a> {
        WgpuCumsumBindings {
            chunk_backdrop_offsets: self.cumsum_chunk_backdrop_offsets.buffer(),
            chunk_lens: self.cumsum_chunk_lens.buffer(),
            row_chunk_starts: self.cumsum_row_chunk_starts.buffer(),
            row_chunk_ends: self.cumsum_row_chunk_ends.buffer(),
            backdrops: scan.backdrops.buffer(),
            chunk_totals: scan.cumsum_chunk_totals.buffer(),
            chunk_offsets: scan.cumsum_chunk_offsets.buffer(),
        }
    }

    pub(crate) fn filter_bindings<'a>(
        &'a self,
        scan: &'a WgpuScanBuffers,
    ) -> WgpuFilterBindings<'a> {
        WgpuFilterBindings {
            draw_records: self.draw_records.buffer(),
            sdf_blob: self.sdf_blob.buffer(),
            sdf_shadow_blob: self.sdf_shadow_blob.buffer(),
            path_records: self.path_records.buffer(),
            backdrops: scan.backdrops.buffer(),
            segment_starts: scan.tile_segment_range_starts.buffer(),
            segment_ends: scan.tile_segment_range_ends.buffer(),
            segments: scan.segments.buffer(),
            layer_stack_tags: self.plan_layer_stack_tags.buffer(),
            layer_stack_draws: self.plan_layer_stack_draws.buffer(),
            layer_stack_payloads: self.plan_layer_stack_payloads.buffer(),
        }
    }

    pub(crate) fn scan_bindings<'a>(&'a self, scan: &'a WgpuScanBuffers) -> WgpuScanBindings<'a> {
        WgpuScanBindings {
            lines: self.lines.buffer(),
            path_records: self.path_records.buffer(),
            scan_chunk_backdrop_offsets: self.scan_chunk_backdrop_offsets.buffer(),
            scan_chunk_lens: self.scan_chunk_lens.buffer(),
            scan_chunk_range_starts: self.scan_chunk_range_starts.buffer(),
            scan_chunk_range_ends: self.scan_chunk_range_ends.buffer(),
            backdrops: scan.backdrops.buffer(),
            tile_segment_range_starts: scan.tile_segment_range_starts.buffer(),
            tile_segment_range_ends: scan.tile_segment_range_ends.buffer(),
            segment_tile_counts: scan.segment_tile_counts.buffer(),
            segment_tile_cursors: scan.segment_tile_cursors.buffer(),
            segment_bumps: scan.segment_bumps.buffer(),
            chunk_totals: scan.chunk_totals.buffer(),
            chunk_offsets: scan.chunk_offsets.buffer(),
            segments: scan.segments.buffer(),
        }
    }

    pub(crate) fn coarse_bindings<'a>(
        &'a self,
        scan: &'a WgpuScanBuffers,
        coarse: &'a WgpuCoarseBuffers,
    ) -> WgpuCoarseBindings<'a> {
        WgpuCoarseBindings {
            draw_records: self.draw_records.buffer(),
            glyph_run_starts: self.text_run_starts.buffer(),
            glyph_run_counts: self.text_run_counts.buffer(),
            glyph_image_ids: self.glyph_image_ids.buffer(),
            glyph_x: self.glyph_x.buffer(),
            glyph_y: self.glyph_y.buffer(),
            glyph_image_left: self.glyph_image_left.buffer(),
            glyph_image_top: self.glyph_image_top.buffer(),
            glyph_image_width: self.glyph_image_width.buffer(),
            glyph_image_height: self.glyph_image_height.buffer(),
            brush_data: self.draw_brush_data.buffer(),
            path_records: self.path_records.buffer(),
            backdrops: scan.backdrops.buffer(),
            segment_starts: scan.tile_segment_range_starts.buffer(),
            segment_ends: scan.tile_segment_range_ends.buffer(),
            layer_stack_tags: self.plan_layer_stack_tags.buffer(),
            layer_stack_draws: self.plan_layer_stack_draws.buffer(),
            layer_stack_payloads: self.plan_layer_stack_payloads.buffer(),
            tile_draw_range_starts: self.tile_draw_range_starts.buffer(),
            tile_draw_range_ends: self.tile_draw_range_ends.buffer(),
            tile_draw_indices: self.tile_draw_indices.buffer(),
            tile_ptcl_counts: coarse.tile_ptcl_counts.buffer(),
            tile_ptcl_range_starts: coarse.tile_ptcl_range_starts.buffer(),
            tile_ptcl_range_ends: coarse.tile_ptcl_range_ends.buffer(),
            tile_glyph_counts: coarse.tile_glyph_counts.buffer(),
            tile_glyph_range_starts: coarse.tile_glyph_range_starts.buffer(),
            tile_glyph_range_ends: coarse.tile_glyph_range_ends.buffer(),
            chunk_totals: coarse.chunk_totals.buffer(),
            chunk_offsets: coarse.chunk_offsets.buffer(),
            glyph_chunk_totals: coarse.glyph_chunk_totals.buffer(),
            glyph_chunk_offsets: coarse.glyph_chunk_offsets.buffer(),
            ptcl_tags: coarse.ptcl_tags.buffer(),
            ptcl_backdrops: coarse.ptcl_backdrops.buffer(),
            ptcl_fill_rules: coarse.ptcl_fill_rules.buffer(),
            ptcl_segment_starts: coarse.ptcl_segment_starts.buffer(),
            ptcl_segment_ends: coarse.ptcl_segment_ends.buffer(),
            ptcl_colors: coarse.ptcl_colors.buffer(),
            glyph_indices: coarse.glyph_indices.buffer(),
        }
    }
}

pub(crate) struct WgpuFineSceneBindings<'a> {
    pub(crate) draw_records: &'a ::wgpu::Buffer,
    pub(crate) sdf_blob: &'a ::wgpu::Buffer,
    pub(crate) sdf_shadow_blob: &'a ::wgpu::Buffer,
    pub(crate) brush_data: &'a ::wgpu::Buffer,
    pub(crate) brush_params: &'a ::wgpu::Buffer,
    pub(crate) brush_payloads: &'a ::wgpu::Buffer,
    pub(crate) image_resource_metadata: &'a ::wgpu::Buffer,
    pub(crate) image_resource_pixels: &'a ::wgpu::Buffer,
}

pub(crate) struct WgpuTileFineBindings<'a> {
    pub(crate) fine: WgpuFineSceneBindings<'a>,
    pub(crate) tile_range_starts: &'a ::wgpu::Buffer,
    pub(crate) tile_range_ends: &'a ::wgpu::Buffer,
    pub(crate) ptcl_tags: &'a ::wgpu::Buffer,
    pub(crate) ptcl_backdrops: &'a ::wgpu::Buffer,
    pub(crate) ptcl_fill_rules: &'a ::wgpu::Buffer,
    pub(crate) ptcl_segment_starts: &'a ::wgpu::Buffer,
    pub(crate) ptcl_segment_ends: &'a ::wgpu::Buffer,
    pub(crate) ptcl_colors: &'a ::wgpu::Buffer,
    pub(crate) segments: &'a ::wgpu::Buffer,
    pub(crate) glyph_indices: &'a ::wgpu::Buffer,
    pub(crate) glyph_image_ids: &'a ::wgpu::Buffer,
    pub(crate) glyph_x: &'a ::wgpu::Buffer,
    pub(crate) glyph_y: &'a ::wgpu::Buffer,
    pub(crate) glyph_image_left: &'a ::wgpu::Buffer,
    pub(crate) glyph_image_top: &'a ::wgpu::Buffer,
    pub(crate) glyph_image_width: &'a ::wgpu::Buffer,
    pub(crate) glyph_image_height: &'a ::wgpu::Buffer,
    pub(crate) glyph_image_content: &'a ::wgpu::Buffer,
    pub(crate) glyph_image_data_offsets: &'a ::wgpu::Buffer,
    pub(crate) glyph_image_data: &'a ::wgpu::Buffer,
    pub(crate) clip_spills: &'a ::wgpu::Buffer,
    pub(crate) group_spills: &'a ::wgpu::Buffer,
}

pub(crate) struct WgpuFilterBindings<'a> {
    pub(crate) draw_records: &'a ::wgpu::Buffer,
    pub(crate) sdf_blob: &'a ::wgpu::Buffer,
    pub(crate) sdf_shadow_blob: &'a ::wgpu::Buffer,
    pub(crate) path_records: &'a ::wgpu::Buffer,
    pub(crate) backdrops: &'a ::wgpu::Buffer,
    pub(crate) segment_starts: &'a ::wgpu::Buffer,
    pub(crate) segment_ends: &'a ::wgpu::Buffer,
    pub(crate) segments: &'a ::wgpu::Buffer,
    pub(crate) layer_stack_tags: &'a ::wgpu::Buffer,
    pub(crate) layer_stack_draws: &'a ::wgpu::Buffer,
    pub(crate) layer_stack_payloads: &'a ::wgpu::Buffer,
}

pub(crate) struct WgpuCumsumBindings<'a> {
    pub(crate) chunk_backdrop_offsets: &'a ::wgpu::Buffer,
    pub(crate) chunk_lens: &'a ::wgpu::Buffer,
    pub(crate) row_chunk_starts: &'a ::wgpu::Buffer,
    pub(crate) row_chunk_ends: &'a ::wgpu::Buffer,
    pub(crate) backdrops: &'a ::wgpu::Buffer,
    pub(crate) chunk_totals: &'a ::wgpu::Buffer,
    pub(crate) chunk_offsets: &'a ::wgpu::Buffer,
}

pub(crate) struct WgpuScanBindings<'a> {
    pub(crate) lines: &'a ::wgpu::Buffer,
    pub(crate) path_records: &'a ::wgpu::Buffer,
    pub(crate) scan_chunk_backdrop_offsets: &'a ::wgpu::Buffer,
    pub(crate) scan_chunk_lens: &'a ::wgpu::Buffer,
    pub(crate) scan_chunk_range_starts: &'a ::wgpu::Buffer,
    pub(crate) scan_chunk_range_ends: &'a ::wgpu::Buffer,
    pub(crate) backdrops: &'a ::wgpu::Buffer,
    pub(crate) tile_segment_range_starts: &'a ::wgpu::Buffer,
    pub(crate) tile_segment_range_ends: &'a ::wgpu::Buffer,
    pub(crate) segment_tile_counts: &'a ::wgpu::Buffer,
    pub(crate) segment_tile_cursors: &'a ::wgpu::Buffer,
    pub(crate) segment_bumps: &'a ::wgpu::Buffer,
    pub(crate) chunk_totals: &'a ::wgpu::Buffer,
    pub(crate) chunk_offsets: &'a ::wgpu::Buffer,
    pub(crate) segments: &'a ::wgpu::Buffer,
}

pub(crate) struct WgpuCoarseBindings<'a> {
    pub(crate) draw_records: &'a ::wgpu::Buffer,
    pub(crate) glyph_run_starts: &'a ::wgpu::Buffer,
    pub(crate) glyph_run_counts: &'a ::wgpu::Buffer,
    pub(crate) glyph_image_ids: &'a ::wgpu::Buffer,
    pub(crate) glyph_x: &'a ::wgpu::Buffer,
    pub(crate) glyph_y: &'a ::wgpu::Buffer,
    pub(crate) glyph_image_left: &'a ::wgpu::Buffer,
    pub(crate) glyph_image_top: &'a ::wgpu::Buffer,
    pub(crate) glyph_image_width: &'a ::wgpu::Buffer,
    pub(crate) glyph_image_height: &'a ::wgpu::Buffer,
    pub(crate) brush_data: &'a ::wgpu::Buffer,
    pub(crate) path_records: &'a ::wgpu::Buffer,
    pub(crate) backdrops: &'a ::wgpu::Buffer,
    pub(crate) segment_starts: &'a ::wgpu::Buffer,
    pub(crate) segment_ends: &'a ::wgpu::Buffer,
    pub(crate) layer_stack_tags: &'a ::wgpu::Buffer,
    pub(crate) layer_stack_draws: &'a ::wgpu::Buffer,
    pub(crate) layer_stack_payloads: &'a ::wgpu::Buffer,
    pub(crate) tile_draw_range_starts: &'a ::wgpu::Buffer,
    pub(crate) tile_draw_range_ends: &'a ::wgpu::Buffer,
    pub(crate) tile_draw_indices: &'a ::wgpu::Buffer,
    pub(crate) tile_ptcl_counts: &'a ::wgpu::Buffer,
    pub(crate) tile_ptcl_range_starts: &'a ::wgpu::Buffer,
    pub(crate) tile_ptcl_range_ends: &'a ::wgpu::Buffer,
    pub(crate) tile_glyph_counts: &'a ::wgpu::Buffer,
    pub(crate) tile_glyph_range_starts: &'a ::wgpu::Buffer,
    pub(crate) tile_glyph_range_ends: &'a ::wgpu::Buffer,
    pub(crate) chunk_totals: &'a ::wgpu::Buffer,
    pub(crate) chunk_offsets: &'a ::wgpu::Buffer,
    pub(crate) glyph_chunk_totals: &'a ::wgpu::Buffer,
    pub(crate) glyph_chunk_offsets: &'a ::wgpu::Buffer,
    pub(crate) ptcl_tags: &'a ::wgpu::Buffer,
    pub(crate) ptcl_backdrops: &'a ::wgpu::Buffer,
    pub(crate) ptcl_fill_rules: &'a ::wgpu::Buffer,
    pub(crate) ptcl_segment_starts: &'a ::wgpu::Buffer,
    pub(crate) ptcl_segment_ends: &'a ::wgpu::Buffer,
    pub(crate) ptcl_colors: &'a ::wgpu::Buffer,
    pub(crate) glyph_indices: &'a ::wgpu::Buffer,
}
