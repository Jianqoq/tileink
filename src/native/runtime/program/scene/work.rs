//! Upload CPU tile/clip inputs into the reusable GPU work arena.
use super::*;
use crate::shared::gpu_coarse::{
    TileDrawRecord, coarse_work_tile_draw_index_word_offset,
    coarse_work_tile_draw_record_word_offset,
};

impl SceneCache {
    pub(super) fn upload_work(
        &mut self,
        batch: &mut ComputeBatch,
        canvas: &Canvas,
        prepared: &crate::render::prepare::PreparedPlan,
        lengths: GpuBufferLengths,
        work_words: usize,
        active: Option<&crate::render::damage_tiles::DamageTiles>,
    ) -> Result<(ResourceId, clip_tiles::ClipDispatch)> {
        // Scan/coarse dispatches initialize their GPU outputs. Upload only the CPU
        // tile bins and active list, rather than clearing and transferring the whole
        // work arena (including PTCL, glyph and emit scratch) on every frame.
        let record_base = coarse_work_tile_draw_record_word_offset(
            lengths.tile_count,
            lengths.coarse_ptcl_capacity,
            lengths.coarse_glyph_capacity,
        ) * size_of::<u32>();
        let index_base = coarse_work_tile_draw_index_word_offset(
            lengths.tile_count,
            lengths.coarse_ptcl_capacity,
            lengths.coarse_glyph_capacity,
        ) * size_of::<u32>();
        let mut clip_dispatch = clip_tiles::ClipDispatch::new(
            canvas,
            &prepared.plan,
            active,
            self.staging.tile_draw_bins.upload_records(),
            lengths,
            prepared.stack_depths.0,
        )?;
        let (bins_full, dirty_records, dirty_indices) =
            Rc::make_mut(&mut self.staging.tile_draw_bins).take_dirty();
        if !clip_dispatch.preallocated {
            self.buffers.clip_slot_snapshot = None;
        }
        let slot_bytes = bytemuck::cast_slice::<_, u8>(&clip_dispatch.slots);
        let slots_changed = self.buffers.clip_slot_snapshot.as_deref() != Some(slot_bytes);
        let mut prefix_updates = Vec::new();
        if !clip_dispatch.slots.is_empty() {
            prefix_updates.push((0, slot_bytes));
        }
        let records: &[u8] = bytemuck::cast_slice(self.staging.tile_draw_bins.upload_records());
        let indices: &[u8] = bytemuck::cast_slice(self.staging.tile_draw_bins.upload_indices());
        let mut tail_updates = Vec::new();
        if !clip_dispatch.kinds.is_empty() {
            let offset = crate::shared::gpu_coarse::coarse_work_fine_tile_kind_word_offset(
                lengths.tile_count,
                lengths.coarse_ptcl_capacity,
                lengths.coarse_glyph_capacity,
                lengths.tile_draw_index_count,
                lengths.tile_draw_chunk_count,
            ) * size_of::<u32>();
            tail_updates.push((
                offset,
                bytemuck::cast_slice::<u32, u8>(&clip_dispatch.kinds),
            ));
        }
        if let Some(active) = active {
            let offset = crate::shared::gpu_coarse::coarse_work_active_tile_list_word_offset(
                lengths.tile_count,
                lengths.coarse_ptcl_capacity,
                lengths.coarse_glyph_capacity,
                lengths.tile_draw_index_count,
                lengths.tile_draw_chunk_count,
            ) * size_of::<u32>();
            let bytes: &[u8] = bytemuck::cast_slice(active.list());
            if !bytes.is_empty() {
                tail_updates.push((offset, bytes));
            }
        }
        if !clip_dispatch.data.is_empty() {
            tail_updates.push((
                work_words * size_of::<u32>(),
                bytemuck::cast_slice::<u32, u8>(&clip_dispatch.data),
            ));
        }
        let mut full_bin_updates = Vec::new();
        if !records.is_empty() {
            full_bin_updates.push((record_base, records));
        }
        if !indices.is_empty() {
            full_bin_updates.push((index_base, indices));
        }
        let mut full_updates = prefix_updates.clone();
        full_updates.extend_from_slice(&full_bin_updates);
        full_updates.extend_from_slice(&tail_updates);
        let layout = (record_base, index_base, records.len(), indices.len());
        // The bin records and indices are read-only inputs to coarse/fine. An
        // immediate Canvas rebuilds the same bins each frame, but copying its
        // multi-megabyte list to the work buffer again is unnecessary. Metal's
        // render stages also read these bins without modifying them, so they use
        // the same accepted-snapshot reuse as the other native backends.
        let full_bin_upload = self.buffers.tile_bin_layout != Some(layout)
            || bins_full
                && !self.buffers.tile_bin_snapshot.as_ref().is_some_and(
                    |(old_records, old_indices)| old_records == records && old_indices == indices,
                );
        let mut dirty_updates = if slots_changed {
            prefix_updates
        } else {
            Vec::new()
        };
        if full_bin_upload {
            dirty_updates.extend_from_slice(&full_bin_updates);
        } else {
            dirty_updates.extend(tile_bin_dirty_updates(
                self.staging.tile_draw_bins.upload_records(),
                self.staging.tile_draw_bins.upload_indices(),
                record_base,
                index_base,
                &dirty_records,
                &dirty_indices,
            ));
        }
        dirty_updates.extend_from_slice(&tail_updates);
        let work = self.buffers.work.patches_delta(
            batch,
            (work_words + clip_dispatch.data.len()).max(1) * size_of::<u32>(),
            &full_updates,
            &dirty_updates,
        )?;
        if clip_dispatch.preallocated && slots_changed {
            self.buffers.clip_slot_snapshot = Some(slot_bytes.to_vec());
        }
        if full_bin_upload {
            let snapshot = self
                .buffers
                .tile_bin_snapshot
                .get_or_insert_with(Default::default);
            snapshot.0.clear();
            snapshot.0.extend_from_slice(records);
            snapshot.1.clear();
            snapshot.1.extend_from_slice(indices);
        } else if !dirty_records.is_empty() || !dirty_indices.is_empty() {
            self.buffers.tile_bin_snapshot = None;
        }
        self.buffers.tile_bin_layout = Some(layout);
        clip_dispatch.discard_upload_data();
        Rc::make_mut(&mut self.staging.tile_draw_bins).recycle_dirty(dirty_records, dirty_indices);
        Ok((work, clip_dispatch))
    }
}

/// Convert dirty tile records and exact index ranges into bounded GPU uploads.
/// The complete snapshot remains available when storage/layout changes or a batch fails.
pub(super) fn tile_bin_dirty_updates<'a>(
    records: &'a [TileDrawRecord],
    indices: &'a [u32],
    record_base: usize,
    index_base: usize,
    dirty_records: &[usize],
    dirty_indices: &[std::ops::Range<usize>],
) -> Vec<(usize, &'a [u8])> {
    use crate::render::upload::ranges::contiguous_index_runs;
    let mut updates = Vec::new();
    for run in contiguous_index_runs(dirty_records.iter().copied()) {
        let bytes = bytemuck::cast_slice(&records[run.clone()]);
        updates.push((record_base + run.start * size_of::<TileDrawRecord>(), bytes));
    }
    for words in dirty_indices {
        let bytes = bytemuck::cast_slice(&indices[words.clone()]);
        updates.push((index_base + words.start * size_of::<u32>(), bytes));
    }
    updates
}
