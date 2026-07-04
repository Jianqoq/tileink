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
            brush_blob: self.brush_blob.buffer(),
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
            coarse_work: coarse.work.buffer(),
            segments: scan.segments.buffer(),
            glyphs: self.glyphs.buffer(),
            glyph_images: self.glyph_images.buffer(),
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
            segment_ranges: scan.tile_segment_ranges.buffer(),
            segments: scan.segments.buffer(),
            layer_stack: self.plan_layer_stack.buffer(),
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
            tile_segment_ranges: scan.tile_segment_ranges.buffer(),
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
            text_blob: self.coarse_text_blob.buffer(),
            brush_blob: self.brush_blob.buffer(),
            path_records: self.path_records.buffer(),
            backdrops: scan.backdrops.buffer(),
            segment_ranges: scan.tile_segment_ranges.buffer(),
            layer_stack: self.plan_layer_stack.buffer(),
            coarse_work: coarse.work.buffer(),
            chunk_records: coarse.chunk_records.buffer(),
        }
    }
}

pub(crate) struct WgpuFineSceneBindings<'a> {
    pub(crate) draw_records: &'a ::wgpu::Buffer,
    pub(crate) sdf_blob: &'a ::wgpu::Buffer,
    pub(crate) sdf_shadow_blob: &'a ::wgpu::Buffer,
    pub(crate) brush_blob: &'a ::wgpu::Buffer,
    pub(crate) image_resource_metadata: &'a ::wgpu::Buffer,
    pub(crate) image_resource_pixels: &'a ::wgpu::Buffer,
}

pub(crate) struct WgpuTileFineBindings<'a> {
    pub(crate) fine: WgpuFineSceneBindings<'a>,
    pub(crate) coarse_work: &'a ::wgpu::Buffer,
    pub(crate) segments: &'a ::wgpu::Buffer,
    pub(crate) glyphs: &'a ::wgpu::Buffer,
    pub(crate) glyph_images: &'a ::wgpu::Buffer,
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
    pub(crate) segment_ranges: &'a ::wgpu::Buffer,
    pub(crate) segments: &'a ::wgpu::Buffer,
    pub(crate) layer_stack: &'a ::wgpu::Buffer,
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
    pub(crate) tile_segment_ranges: &'a ::wgpu::Buffer,
    pub(crate) segment_tile_counts: &'a ::wgpu::Buffer,
    pub(crate) segment_tile_cursors: &'a ::wgpu::Buffer,
    pub(crate) segment_bumps: &'a ::wgpu::Buffer,
    pub(crate) chunk_totals: &'a ::wgpu::Buffer,
    pub(crate) chunk_offsets: &'a ::wgpu::Buffer,
    pub(crate) segments: &'a ::wgpu::Buffer,
}

pub(crate) struct WgpuCoarseBindings<'a> {
    pub(crate) draw_records: &'a ::wgpu::Buffer,
    pub(crate) text_blob: &'a ::wgpu::Buffer,
    pub(crate) brush_blob: &'a ::wgpu::Buffer,
    pub(crate) path_records: &'a ::wgpu::Buffer,
    pub(crate) backdrops: &'a ::wgpu::Buffer,
    pub(crate) segment_ranges: &'a ::wgpu::Buffer,
    pub(crate) layer_stack: &'a ::wgpu::Buffer,
    pub(crate) coarse_work: &'a ::wgpu::Buffer,
    pub(crate) chunk_records: &'a ::wgpu::Buffer,
}
