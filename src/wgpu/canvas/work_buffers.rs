use crate::shared::{
    gpu_coarse::{CoarseChunkRecord, PtclRecord, TileCoarseRecord},
    gpu_plan::GpuBufferLengths,
    line_seg::LineSegment,
    tile_seg_range::TileSegmentRange,
};

use super::super::buffer::WgpuBuffer;

pub(crate) struct WgpuScanBuffers {
    pub(crate) backdrops: WgpuBuffer,
    pub(crate) tile_segment_ranges: WgpuBuffer,
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
            tile_segment_ranges: WgpuBuffer::new(device, "tileink wgpu scan tile segment ranges"),
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
        self.tile_segment_ranges.resize_uninit::<TileSegmentRange>(
            device,
            "tileink wgpu scan tile segment ranges",
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
    pub(crate) tile_records: WgpuBuffer,
    pub(crate) chunk_records: WgpuBuffer,
    pub(crate) ptcl_records: WgpuBuffer,
    pub(crate) glyph_indices: WgpuBuffer,
}

impl WgpuCoarseBuffers {
    pub(crate) fn new(device: &::wgpu::Device) -> Self {
        Self {
            tile_records: WgpuBuffer::new(device, "tileink wgpu coarse tile records"),
            chunk_records: WgpuBuffer::new(device, "tileink wgpu coarse chunk records"),
            ptcl_records: WgpuBuffer::new(device, "tileink wgpu coarse ptcl records"),
            glyph_indices: WgpuBuffer::new(device, "tileink wgpu coarse glyph indices"),
        }
    }

    pub(crate) fn prepare_outputs(&mut self, device: &::wgpu::Device, lengths: GpuBufferLengths) {
        self.tile_records.resize_uninit::<TileCoarseRecord>(
            device,
            "tileink wgpu coarse tile records",
            lengths.tile_count,
        );
        self.chunk_records.resize_uninit::<CoarseChunkRecord>(
            device,
            "tileink wgpu coarse chunk records",
            lengths.coarse_chunk_count,
        );
        self.ptcl_records.resize_uninit::<PtclRecord>(
            device,
            "tileink wgpu coarse ptcl records",
            lengths.coarse_ptcl_capacity,
        );
        self.glyph_indices.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse glyph indices",
            lengths.coarse_glyph_capacity,
        );
    }
}
