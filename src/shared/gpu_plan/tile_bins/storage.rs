//! Compact GPU index allocation and exact dirty-word uploads, independent of GPU workgroup size.
use super::*;
use std::ops::Range;

impl TileDrawBins {
    pub(super) fn rewrite_tile(&mut self, tile: usize) {
        let cull = !self.prefix_visibility
            && self.occlusion.active
            && self.occlusion.filter_tile(
                tile,
                self.tiles_size.0,
                &self.tile_refs[tile],
                &mut self.visible_refs,
            );
        let refs = if cull {
            &self.visible_refs
        } else {
            &self.tile_refs[tile]
        };
        self.active_chunks -= self.indices[tile]
            .len()
            .div_ceil(COARSE_CHUNK_SIZE as usize);
        self.active_chunks += refs.len().div_ceil(COARSE_CHUNK_SIZE as usize);
        let dirty = self.indices.write(tile, refs);
        if !dirty.is_empty() {
            self.dirty_indices.push(dirty);
        }
        self.indices.release_empty(tile);
        self.refresh_record(tile);
        self.dirty_records.push(tile);
    }

    pub(super) fn rebuild_indices(&mut self) {
        self.dirty_indices.clear();
        self.dirty_records.clear();
        self.indices.clear(self.tile_refs.len());
        self.indices.values.reserve(self.tile_refs.values.len());
        self.active_chunks = 0;
        for tile in 0..self.tile_refs.len() {
            let cull = !self.prefix_visibility
                && self.occlusion.active
                && self.occlusion.filter_tile(
                    tile,
                    self.tiles_size.0,
                    &self.tile_refs[tile],
                    &mut self.visible_refs,
                );
            let refs = if cull {
                &self.visible_refs
            } else {
                &self.tile_refs[tile]
            };
            self.active_chunks += refs.len().div_ceil(COARSE_CHUNK_SIZE as usize);
            self.indices.append_exact(tile, refs);
        }
        self.refresh_records();
        self.full_upload = true;
    }

    pub(super) fn refresh_records(&mut self) {
        for tile in 0..self.records.len() {
            self.refresh_record(tile);
        }
    }

    pub(super) fn refresh_record(&mut self, tile: usize) {
        let refs = &self.indices[tile];
        let skip = if self.prefix_visibility {
            self.occlusion.visible_start(tile, self.tiles_size.0, refs)
        } else {
            0
        };
        self.records[tile] = TileDrawRecord {
            start: if refs.is_empty() {
                u32::MAX
            } else {
                self.indices.start(tile) + skip as u32
            },
            end: (refs.len() - skip) as u32,
        };
    }

    pub(crate) fn take_dirty(&mut self) -> (bool, Vec<usize>, Vec<Range<usize>>) {
        self.dirty_records.sort_unstable();
        self.dirty_records.dedup();
        self.dirty_indices.sort_unstable_by_key(|range| range.start);
        let mut count = 0;
        for index in 0..self.dirty_indices.len() {
            let range = self.dirty_indices[index].clone();
            if count > 0 && range.start <= self.dirty_indices[count - 1].end {
                self.dirty_indices[count - 1].end =
                    self.dirty_indices[count - 1].end.max(range.end);
            } else {
                self.dirty_indices[count] = range;
                count += 1;
            }
        }
        self.dirty_indices.truncate(count);
        (
            std::mem::take(&mut self.full_upload),
            std::mem::take(&mut self.dirty_records),
            std::mem::take(&mut self.dirty_indices),
        )
    }

    pub(crate) fn recycle_dirty(
        &mut self,
        mut records: Vec<usize>,
        mut indices: Vec<Range<usize>>,
    ) {
        debug_assert!(self.dirty_records.is_empty() && self.dirty_indices.is_empty());
        records.clear();
        indices.clear();
        if records.capacity() <= RETAINED_TILE_DIRTY_CAPACITY {
            self.dirty_records = records;
        }
        if indices.capacity() <= RETAINED_TILE_DIRTY_CAPACITY {
            self.dirty_indices = indices;
        }
    }

    pub(crate) const fn active_chunk_count(&self) -> usize {
        self.active_chunks
    }
    pub(crate) fn upload_records(&self) -> &[TileDrawRecord] {
        &self.records
    }
    pub(crate) fn upload_indices(&self) -> &[u32] {
        &self.indices.values
    }
    pub(crate) const fn upload_index_count(&self) -> usize {
        self.indices.values.len()
    }
}
