use crate::shared::{
    gpu_coarse::{
        CoarseChunkRecord, coarse_work_active_tile_list_word_offset, coarse_work_word_len,
    },
    gpu_plan::{GpuBufferLengths, GpuCumsumPlan},
    line_seg::LineSegment,
    tile_seg_range::TileSegmentRange,
};

#[cfg(test)]
use crate::shared::gpu_coarse::{
    FineTileKind, PTCL_RECORD_WORDS, PtclRecord, TILE_COARSE_RECORD_WORDS, TileCoarseRecord,
    coarse_work_fine_tile_kind_word_offset, coarse_work_ptcl_word_offset,
};

use std::sync::Mutex;

use super::super::buffer::WgpuBuffer;
use super::bindings::WgpuCoarseBindingKey;

const COARSE_BIND_GROUP_CACHE_SLOTS: usize = 256;

#[derive(Clone)]
pub(crate) struct WgpuCoarseBindGroups {
    pub(crate) count: ::wgpu::BindGroup,
    pub(crate) prefix: ::wgpu::BindGroup,
    pub(crate) emit: ::wgpu::BindGroup,
}

#[derive(Default)]
struct WgpuCoarseBindGroupCache {
    key: Option<WgpuCoarseBindingKey>,
    slots: Vec<Option<WgpuCoarseBindGroups>>,
}

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
    pub(crate) active_indices: WgpuBuffer,
    pub(crate) active_cumsum_chunk_backdrop_offsets: WgpuBuffer,
    pub(crate) active_cumsum_chunk_lens: WgpuBuffer,
    pub(crate) active_cumsum_row_chunk_starts: WgpuBuffer,
    pub(crate) active_cumsum_row_chunk_ends: WgpuBuffer,
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
            active_indices: WgpuBuffer::new(device, "tileink wgpu scan active indices"),
            active_cumsum_chunk_backdrop_offsets: WgpuBuffer::new(
                device,
                "tileink wgpu active cumsum chunk backdrop offsets",
            ),
            active_cumsum_chunk_lens: WgpuBuffer::new(
                device,
                "tileink wgpu active cumsum chunk lengths",
            ),
            active_cumsum_row_chunk_starts: WgpuBuffer::new(
                device,
                "tileink wgpu active cumsum row chunk starts",
            ),
            active_cumsum_row_chunk_ends: WgpuBuffer::new(
                device,
                "tileink wgpu active cumsum row chunk ends",
            ),
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

    pub(crate) fn upload_active_indices(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        indices: &[u32],
    ) {
        self.active_indices.upload_cached(
            device,
            queue,
            "tileink wgpu scan active indices",
            indices,
        );
    }

    pub(crate) fn upload_active_cumsum_plan(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        plan: &GpuCumsumPlan,
    ) {
        self.active_cumsum_chunk_backdrop_offsets.upload_cached(
            device,
            queue,
            "tileink wgpu active cumsum chunk backdrop offsets",
            &plan.chunk_backdrop_offsets,
        );
        self.active_cumsum_chunk_lens.upload_cached(
            device,
            queue,
            "tileink wgpu active cumsum chunk lengths",
            &plan.chunk_lens,
        );
        self.active_cumsum_row_chunk_starts.upload_cached(
            device,
            queue,
            "tileink wgpu active cumsum row chunk starts",
            &plan.row_chunk_starts,
        );
        self.active_cumsum_row_chunk_ends.upload_cached(
            device,
            queue,
            "tileink wgpu active cumsum row chunk ends",
            &plan.row_chunk_ends,
        );
    }
}

pub(crate) struct WgpuCoarseBuffers {
    pub(crate) work: WgpuBuffer,
    pub(crate) chunk_records: WgpuBuffer,
    pub(crate) tile_bin_layout: Option<(u64, usize, usize, usize)>,
    pub(crate) tile_bin_staging: WgpuBuffer,
    pub(crate) tile_bin_staging_words: Vec<u32>,
    pub(crate) pending_tile_bin_copies: Vec<(u64, u64, u64)>,
    bind_group_cache: Mutex<WgpuCoarseBindGroupCache>,
}

impl WgpuCoarseBuffers {
    pub(crate) fn new(device: &::wgpu::Device) -> Self {
        Self {
            work: WgpuBuffer::new(device, "tileink wgpu coarse work"),
            chunk_records: WgpuBuffer::new(device, "tileink wgpu coarse chunk records"),
            tile_bin_layout: None,
            tile_bin_staging: WgpuBuffer::new(device, "tileink wgpu tile bin staging"),
            tile_bin_staging_words: Vec::new(),
            pending_tile_bin_copies: Vec::new(),
            bind_group_cache: Mutex::new(WgpuCoarseBindGroupCache::default()),
        }
    }

    pub(crate) fn encode_pending_tile_bin_copies(&mut self, encoder: &mut ::wgpu::CommandEncoder) {
        for (source, target, size) in self.pending_tile_bin_copies.drain(..) {
            encoder.copy_buffer_to_buffer(
                self.tile_bin_staging.buffer(),
                source,
                self.work.buffer(),
                target,
                size,
            );
        }
    }

    pub(crate) fn cached_bind_groups(
        &self,
        key: WgpuCoarseBindingKey,
        slot: usize,
        create: impl FnOnce() -> WgpuCoarseBindGroups,
    ) -> WgpuCoarseBindGroups {
        // Config offsets repeat from zero in each command batch. Cache the common slots, while
        // bounding driver objects for pathological plans with thousands of independent batches.
        if slot >= COARSE_BIND_GROUP_CACHE_SLOTS {
            return create();
        }
        let mut cache = self.bind_group_cache.lock().unwrap();
        if cache.key != Some(key) {
            cache.key = Some(key);
            cache.slots.clear();
        }
        if cache.slots.len() <= slot {
            cache.slots.resize_with(slot + 1, || None);
        }
        cache.slots[slot].get_or_insert_with(create).clone()
    }

    pub(crate) fn prepare_outputs(&mut self, device: &::wgpu::Device, lengths: GpuBufferLengths) {
        self.work.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse work",
            coarse_work_word_len(
                lengths.tile_count,
                lengths.coarse_ptcl_capacity,
                lengths.coarse_glyph_capacity,
                lengths.tile_draw_index_count,
                lengths.tile_draw_chunk_count,
            ),
        );
        self.chunk_records.resize_uninit::<CoarseChunkRecord>(
            device,
            "tileink wgpu coarse chunk records",
            lengths.coarse_chunk_count,
        );
    }

    pub(crate) fn upload_active_tiles(
        &mut self,
        queue: &::wgpu::Queue,
        lengths: GpuBufferLengths,
        tiles: &[u32],
    ) {
        let offset = coarse_work_active_tile_list_word_offset(
            lengths.tile_count,
            lengths.coarse_ptcl_capacity,
            lengths.coarse_glyph_capacity,
            lengths.tile_draw_index_count,
            lengths.tile_draw_chunk_count,
        );
        self.work.write_at(queue, (offset * 4) as u64, tiles);
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
            .as_chunks::<TILE_COARSE_RECORD_WORDS>()
            .0
            .iter()
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
            .as_chunks::<PTCL_RECORD_WORDS>()
            .0
            .iter()
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

    #[cfg(test)]
    pub(crate) fn read_fine_tile_kinds(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        lengths: GpuBufferLengths,
    ) -> Vec<FineTileKind> {
        let offset = coarse_work_fine_tile_kind_word_offset(
            lengths.tile_count,
            lengths.coarse_ptcl_capacity,
            lengths.coarse_glyph_capacity,
            lengths.tile_draw_index_count,
            lengths.tile_draw_chunk_count,
        );
        self.work
            .read::<u32>(device, queue, offset + lengths.tile_count)
            .into_iter()
            .skip(offset)
            .map(FineTileKind::from_word)
            .collect()
    }
}
