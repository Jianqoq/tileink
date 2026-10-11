use super::*;

impl TileDrawBins {
    // Keep the update policy available for optimization in the buffer-length planner.
    #[inline]
    pub(in crate::shared::gpu_plan) fn spatial_change_is_dense(
        &self,
        draw_records: &[DrawRecord],
        tiles_size: (u32, u32),
        changed: &[std::ops::Range<usize>],
    ) -> bool {
        if self.tiles_size != tiles_size || draw_records.is_empty() || changed.is_empty() {
            return false;
        }
        if self.draw_bboxes.len() != draw_records.len() {
            return true;
        }
        // Compare changed draw/tile memberships with the work of rebuilding all bins.
        // A fixed fraction of viewport tiles classified local UI motion as dense, repeatedly
        // discarding the persistent index and causing a rebuild/upload spike as motion settled.
        let dense_tile_threshold = self
            .records
            .iter()
            .map(|record| u64::from(record.end))
            .sum::<u64>()
            .max(1);
        let mut affected_tile_work = 0u64;
        let mut previous_end = 0usize;
        for range in changed {
            let start = range.start.max(previous_end).min(draw_records.len());
            let end = range.end.min(draw_records.len());
            for (draw, record) in draw_records.iter().enumerate().take(end).skip(start) {
                let bbox = record.tile_bbox(tiles_size.0, tiles_size.1);
                if bbox != self.draw_bboxes[draw] {
                    affected_tile_work +=
                        u64::from(bbox.tile_count() + self.draw_bboxes[draw].tile_count());
                    if affected_tile_work >= dense_tile_threshold {
                        return true;
                    }
                }
            }
            previous_end = previous_end.max(end);
        }
        false
    }

    #[inline]
    pub(in crate::shared::gpu_plan) fn update_changed(
        &mut self,
        draw_records: &[DrawRecord],
        draw_order: &[u32],
        painter_keys: Option<&[PainterKey]>,
        tiles_size: (u32, u32),
        changed: &[std::ops::Range<usize>],
    ) -> bool {
        if !self.page_arena_valid || self.tiles_size != tiles_size {
            return false;
        }
        if painter_keys.is_none()
            && (self.draw_bboxes.len() != draw_records.len()
                || self.draw_ranks.len() != draw_records.len()
                || self.draw_ptcl_capacities.len() != draw_records.len())
        {
            return false;
        }
        debug_assert!(
            painter_keys.is_some()
                || draw_order
                    .iter()
                    .enumerate()
                    .all(|(rank, &draw)| self.draw_ranks[draw as usize].path[0] == rank as u128)
        );
        let old_len = self.draw_bboxes.len();
        let new_len = draw_records.len();
        let working_len = old_len.max(new_len);
        self.draw_bboxes.resize(
            working_len,
            TileBbox {
                x0: 0,
                y0: 0,
                x1: 0,
                y1: 0,
            },
        );
        self.draw_ranks.resize(working_len, PainterKey::inactive());
        self.draw_ptcl_capacities.resize(working_len, 0);
        self.visited_draws.begin(working_len);
        self.membership_changed.begin(working_len);
        let tile_count = tiles_size.0 as usize * tiles_size.1 as usize;
        self.affected_tiles.begin(tile_count);
        for draw in changed.iter().flat_map(|range| range.clone()) {
            if draw >= working_len {
                continue;
            }
            if !self.visited_draws.insert(draw) {
                continue;
            }
            let old_bbox = self.draw_bboxes[draw];
            let old_rank = self.draw_ranks[draw].clone();
            if draw < old_len {
                self.draw_ptcl_capacity -= self.draw_ptcl_capacities[draw];
            }
            if draw < new_len {
                let bbox = draw_records[draw].tile_bbox(tiles_size.0, tiles_size.1);
                self.draw_bboxes[draw] = bbox;
                if let Some(keys) = painter_keys {
                    self.draw_ranks[draw] = keys[draw].clone();
                }
                let capacity = draw_ptcl_capacity(&draw_records[draw], bbox);
                self.draw_ptcl_capacities[draw] = capacity;
                self.draw_ptcl_capacity += capacity;
            } else {
                self.draw_bboxes[draw] = TileBbox {
                    x0: 0,
                    y0: 0,
                    x1: 0,
                    y1: 0,
                };
                self.draw_ranks[draw] = PainterKey::inactive();
                self.draw_ptcl_capacities[draw] = 0;
            }
            if old_bbox != self.draw_bboxes[draw] || old_rank != self.draw_ranks[draw] {
                self.membership_changed.insert(draw);
                for_tile_in_bbox(old_bbox, tiles_size.0, |tile| {
                    self.affected_tiles.insert(tile);
                });
                for_tile_in_bbox(self.draw_bboxes[draw], tiles_size.0, |tile| {
                    self.affected_tiles.insert(tile);
                });
            }
        }
        let page_membership_changed = !self.affected_tiles.is_empty();
        let membership_changed = &self.membership_changed;
        for index in 0..self.affected_tiles.len() {
            let tile = self.affected_tiles.get(index);
            self.tile_refs[tile].retain(|draw| {
                let draw = *draw as usize;
                !membership_changed.contains(draw)
            });
        }
        for index in 0..self.membership_changed.len() {
            let draw = self.membership_changed.get(index);
            if draw >= new_len || self.draw_ranks[draw].path[0] == u128::MAX {
                continue;
            }
            for_tile_in_bbox(self.draw_bboxes[draw], tiles_size.0, |tile| {
                self.tile_refs[tile].push(draw as u32);
            });
        }
        for index in 0..self.affected_tiles.len() {
            let tile = self.affected_tiles.get(index);
            self.tile_refs[tile].sort_unstable_by(|a, b| {
                self.draw_ranks[*a as usize].cmp(&self.draw_ranks[*b as usize])
            });
            self.rewrite_tile(tile);
        }
        self.draw_bboxes.truncate(new_len);
        self.draw_ranks.truncate(new_len);
        self.draw_ptcl_capacities.truncate(new_len);
        if page_membership_changed && self.flat_full_upload {
            // The GPU currently contains the compact flat representation from the preceding
            // full upload. Keep it for content-only updates; switch once tile membership really
            // changes, uploading the already-maintained persistent page arena exactly once.
            self.flat_full_upload = false;
            self.full_upload_records.clear();
            self.full_upload_indices.clear();
            self.full_upload = true;
        }
        self.maybe_compact_pages();
        true
    }
}
