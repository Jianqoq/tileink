use crate::shared::{
    gpu_coarse::{CoarseChunkRecord, coarse_work_word_len},
    gpu_plan::GpuBufferLengths,
    line_seg::LineSegment,
    tile_seg_range::TileSegmentRange,
};

#[cfg(test)]
use crate::shared::gpu_coarse::{
    PTCL_RECORD_WORDS, PtclRecord, TILE_COARSE_RECORD_WORDS, TileCoarseRecord,
    coarse_work_ptcl_word_offset,
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
    pub(crate) work: WgpuBuffer,
    pub(crate) chunk_records: WgpuBuffer,
}

impl WgpuCoarseBuffers {
    pub(crate) fn new(device: &::wgpu::Device) -> Self {
        Self {
            work: WgpuBuffer::new(device, "tileink wgpu coarse work"),
            chunk_records: WgpuBuffer::new(device, "tileink wgpu coarse chunk records"),
        }
    }

    pub(crate) fn prepare_outputs(&mut self, device: &::wgpu::Device, lengths: GpuBufferLengths) {
        self.work.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse work",
            coarse_work_word_len(
                lengths.tile_count,
                lengths.coarse_ptcl_capacity,
                lengths.coarse_glyph_capacity,
            ),
        );
        self.chunk_records.resize_uninit::<CoarseChunkRecord>(
            device,
            "tileink wgpu coarse chunk records",
            lengths.coarse_chunk_count,
        );
    }

    #[cfg(test)]
    pub(crate) fn read_tile_records(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        tile_count: usize,
    ) -> Vec<TileCoarseRecord> {
        self.work
            .read::<u32>(device, queue, tile_count * TILE_COARSE_RECORD_WORDS)
            .chunks_exact(TILE_COARSE_RECORD_WORDS)
            .map(|words| TileCoarseRecord {
                ptcl_count: words[0],
                ptcl_start: words[1],
                ptcl_end: words[2],
                glyph_count: words[3],
                glyph_start: words[4],
                glyph_end: words[5],
            })
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn read_ptcl_records(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        tile_count: usize,
        len: usize,
    ) -> Vec<PtclRecord> {
        let offset = coarse_work_ptcl_word_offset(tile_count);
        self.work
            .read::<u32>(device, queue, offset + len * PTCL_RECORD_WORDS)
            .chunks_exact(PTCL_RECORD_WORDS)
            .skip(offset / PTCL_RECORD_WORDS)
            .map(|words| PtclRecord {
                tag: words[0],
                backdrop: words[1] as i32,
                fill_rule: words[2],
                segment_start: words[3],
                segment_end: words[4],
                color: words[5],
            })
            .collect()
    }
}
