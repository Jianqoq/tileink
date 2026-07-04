use crate::shared::{gpu_plan::GpuBufferLengths, line_seg::LineSegment};

use super::super::buffer::WgpuBuffer;

fn packed_u8_len(len: usize) -> usize {
    len.div_ceil(4)
}

pub(crate) struct WgpuScanBuffers {
    pub(crate) backdrops: WgpuBuffer,
    pub(crate) tile_segment_range_starts: WgpuBuffer,
    pub(crate) tile_segment_range_ends: WgpuBuffer,
    pub(crate) segments: WgpuBuffer,
    pub(crate) segment_tile_counts: WgpuBuffer,
    pub(crate) segment_tile_cursors: WgpuBuffer,
    pub(crate) segment_bumps: WgpuBuffer,
    pub(crate) chunk_totals: WgpuBuffer,
    pub(crate) chunk_offsets: WgpuBuffer,
    pub(crate) cumsum_chunk_totals: WgpuBuffer,
    pub(crate) cumsum_chunk_offsets: WgpuBuffer,
}

impl WgpuScanBuffers {
    pub(crate) fn new(device: &::wgpu::Device) -> Self {
        Self {
            backdrops: WgpuBuffer::new(device, "tileink wgpu scan backdrops"),
            tile_segment_range_starts: WgpuBuffer::new(
                device,
                "tileink wgpu scan tile segment range starts",
            ),
            tile_segment_range_ends: WgpuBuffer::new(
                device,
                "tileink wgpu scan tile segment range ends",
            ),
            segments: WgpuBuffer::new(device, "tileink wgpu scan segments"),
            segment_tile_counts: WgpuBuffer::new(device, "tileink wgpu scan segment tile counts"),
            segment_tile_cursors: WgpuBuffer::new(device, "tileink wgpu scan segment tile cursors"),
            segment_bumps: WgpuBuffer::new(device, "tileink wgpu scan segment bumps"),
            chunk_totals: WgpuBuffer::new(device, "tileink wgpu scan chunk totals"),
            chunk_offsets: WgpuBuffer::new(device, "tileink wgpu scan chunk offsets"),
            cumsum_chunk_totals: WgpuBuffer::new(device, "tileink wgpu scan cumsum chunk totals"),
            cumsum_chunk_offsets: WgpuBuffer::new(device, "tileink wgpu scan cumsum chunk offsets"),
        }
    }

    pub(crate) fn prepare_outputs(&mut self, device: &::wgpu::Device, lengths: GpuBufferLengths) {
        self.backdrops.resize_uninit::<i32>(
            device,
            "tileink wgpu scan backdrops",
            lengths.backdrop_len,
        );
        self.tile_segment_range_starts.resize_uninit::<u32>(
            device,
            "tileink wgpu scan tile segment range starts",
            lengths.backdrop_len,
        );
        self.tile_segment_range_ends.resize_uninit::<u32>(
            device,
            "tileink wgpu scan tile segment range ends",
            lengths.backdrop_len,
        );
        self.segments.resize_uninit::<LineSegment>(
            device,
            "tileink wgpu scan segments",
            lengths.segment_capacity,
        );
        self.segment_tile_counts.resize_uninit::<u32>(
            device,
            "tileink wgpu scan segment tile counts",
            lengths.backdrop_len,
        );
        self.segment_tile_cursors.resize_uninit::<u32>(
            device,
            "tileink wgpu scan segment tile cursors",
            lengths.backdrop_len,
        );
        self.segment_bumps.resize_uninit::<u32>(
            device,
            "tileink wgpu scan segment bumps",
            lengths.path_count,
        );
        self.chunk_totals.resize_uninit::<u32>(
            device,
            "tileink wgpu scan chunk totals",
            lengths.scan_chunk_count,
        );
        self.chunk_offsets.resize_uninit::<u32>(
            device,
            "tileink wgpu scan chunk offsets",
            lengths.scan_chunk_count,
        );
        self.cumsum_chunk_totals.resize_uninit::<i32>(
            device,
            "tileink wgpu scan cumsum chunk totals",
            lengths.cumsum_chunk_count,
        );
        self.cumsum_chunk_offsets.resize_uninit::<i32>(
            device,
            "tileink wgpu scan cumsum chunk offsets",
            lengths.cumsum_chunk_count,
        );
    }
}

pub(crate) struct WgpuCoarseBuffers {
    pub(crate) tile_ptcl_range_starts: WgpuBuffer,
    pub(crate) tile_ptcl_range_ends: WgpuBuffer,
    pub(crate) tile_ptcl_counts: WgpuBuffer,
    pub(crate) tile_glyph_range_starts: WgpuBuffer,
    pub(crate) tile_glyph_range_ends: WgpuBuffer,
    pub(crate) tile_glyph_counts: WgpuBuffer,
    pub(crate) chunk_totals: WgpuBuffer,
    pub(crate) chunk_offsets: WgpuBuffer,
    pub(crate) glyph_chunk_totals: WgpuBuffer,
    pub(crate) glyph_chunk_offsets: WgpuBuffer,
    pub(crate) ptcl_tags: WgpuBuffer,
    pub(crate) ptcl_backdrops: WgpuBuffer,
    pub(crate) ptcl_fill_rules: WgpuBuffer,
    pub(crate) ptcl_segment_starts: WgpuBuffer,
    pub(crate) ptcl_segment_ends: WgpuBuffer,
    pub(crate) ptcl_colors: WgpuBuffer,
    pub(crate) glyph_indices: WgpuBuffer,
}

impl WgpuCoarseBuffers {
    pub(crate) fn new(device: &::wgpu::Device) -> Self {
        Self {
            tile_ptcl_range_starts: WgpuBuffer::new(
                device,
                "tileink wgpu coarse tile ptcl range starts",
            ),
            tile_ptcl_range_ends: WgpuBuffer::new(
                device,
                "tileink wgpu coarse tile ptcl range ends",
            ),
            tile_ptcl_counts: WgpuBuffer::new(device, "tileink wgpu coarse tile ptcl counts"),
            tile_glyph_range_starts: WgpuBuffer::new(
                device,
                "tileink wgpu coarse tile glyph range starts",
            ),
            tile_glyph_range_ends: WgpuBuffer::new(
                device,
                "tileink wgpu coarse tile glyph range ends",
            ),
            tile_glyph_counts: WgpuBuffer::new(device, "tileink wgpu coarse tile glyph counts"),
            chunk_totals: WgpuBuffer::new(device, "tileink wgpu coarse chunk totals"),
            chunk_offsets: WgpuBuffer::new(device, "tileink wgpu coarse chunk offsets"),
            glyph_chunk_totals: WgpuBuffer::new(device, "tileink wgpu coarse glyph chunk totals"),
            glyph_chunk_offsets: WgpuBuffer::new(device, "tileink wgpu coarse glyph chunk offsets"),
            ptcl_tags: WgpuBuffer::new(device, "tileink wgpu coarse ptcl tags"),
            ptcl_backdrops: WgpuBuffer::new(device, "tileink wgpu coarse ptcl backdrops"),
            ptcl_fill_rules: WgpuBuffer::new(device, "tileink wgpu coarse ptcl fill rules"),
            ptcl_segment_starts: WgpuBuffer::new(device, "tileink wgpu coarse ptcl segment starts"),
            ptcl_segment_ends: WgpuBuffer::new(device, "tileink wgpu coarse ptcl segment ends"),
            ptcl_colors: WgpuBuffer::new(device, "tileink wgpu coarse ptcl colors"),
            glyph_indices: WgpuBuffer::new(device, "tileink wgpu coarse glyph indices"),
        }
    }

    pub(crate) fn prepare_outputs(&mut self, device: &::wgpu::Device, lengths: GpuBufferLengths) {
        self.tile_ptcl_range_starts.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse tile ptcl range starts",
            lengths.tile_count,
        );
        self.tile_ptcl_range_ends.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse tile ptcl range ends",
            lengths.tile_count,
        );
        self.tile_ptcl_counts.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse tile ptcl counts",
            lengths.tile_count,
        );
        self.tile_glyph_range_starts.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse tile glyph range starts",
            lengths.tile_count,
        );
        self.tile_glyph_range_ends.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse tile glyph range ends",
            lengths.tile_count,
        );
        self.tile_glyph_counts.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse tile glyph counts",
            lengths.tile_count,
        );
        self.chunk_totals.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse chunk totals",
            lengths.coarse_chunk_count,
        );
        self.chunk_offsets.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse chunk offsets",
            lengths.coarse_chunk_count,
        );
        self.glyph_chunk_totals.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse glyph chunk totals",
            lengths.coarse_chunk_count,
        );
        self.glyph_chunk_offsets.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse glyph chunk offsets",
            lengths.coarse_chunk_count,
        );
        self.ptcl_tags.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse ptcl tags",
            packed_u8_len(lengths.coarse_ptcl_capacity),
        );
        self.ptcl_backdrops.resize_uninit::<i32>(
            device,
            "tileink wgpu coarse ptcl backdrops",
            lengths.coarse_ptcl_capacity,
        );
        self.ptcl_fill_rules.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse ptcl fill rules",
            lengths.coarse_ptcl_capacity,
        );
        self.ptcl_segment_starts.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse ptcl segment starts",
            lengths.coarse_ptcl_capacity,
        );
        self.ptcl_segment_ends.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse ptcl segment ends",
            lengths.coarse_ptcl_capacity,
        );
        self.ptcl_colors.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse ptcl colors",
            lengths.coarse_ptcl_capacity,
        );
        self.glyph_indices.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse glyph indices",
            lengths.coarse_glyph_capacity,
        );
    }
}
