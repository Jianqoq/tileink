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
        let mut dense_tile_threshold = None;
        let mut affected_tile_work = 0u64;
        let mut previous_end = 0usize;
        for range in changed {
            let start = range.start.max(previous_end).min(draw_records.len());
            let end = range.end.min(draw_records.len());
            for (draw, record) in draw_records.iter().enumerate().take(end).skip(start) {
                let bbox = record.tile_bbox(tiles_size.0, tiles_size.1);
                if bbox != self.draw_bboxes[draw] {
                    // Paint-only edits need no density decision. Count complete CPU membership
                    // lazily so culled GPU counts cannot force a rebuild or add a viewport scan.
                    let threshold = *dense_tile_threshold.get_or_insert_with(|| {
                        if self.spatial_index_valid {
                            self.tile_refs
                                .iter()
                                .map(|refs| refs.len() as u64)
                                .sum::<u64>()
                        } else {
                            self.draw_bboxes
                                .iter()
                                .map(|bbox| u64::from(bbox.tile_count()))
                                .sum::<u64>()
                        }
                        .max(1)
                    });
                    affected_tile_work +=
                        u64::from(bbox.tile_count() + self.draw_bboxes[draw].tile_count());
                    if affected_tile_work >= threshold {
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
        if !self.spatial_index_valid || self.tiles_size != tiles_size {
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
        for &bbox in &self.occlusion.dirty_tiles {
            for_tile_in_bbox(bbox, tiles_size.0, |tile| {
                self.affected_tiles.insert(tile);
            });
        }
        for draw in changed
            .iter()
            .flat_map(std::clone::Clone::clone)
            .chain((0..self.occlusion.changed.len()).map(|index| self.occlusion.changed.get(index)))
        {
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
            } else if !self.occlusion.changed.is_empty() && self.occlusion.changed.contains(draw) {
                for_tile_in_bbox(old_bbox, tiles_size.0, |tile| {
                    self.affected_tiles.insert(tile);
                });
            }
        }
        if self.prefix_visibility
            && self.occlusion.single_batch
            && self.membership_changed.is_empty()
        {
            for index in 0..self.affected_tiles.len() {
                let tile = self.affected_tiles.get(index);
                self.refresh_record(tile);
                self.dirty_records.push(tile);
            }
            self.draw_bboxes.truncate(new_len);
            self.draw_ranks.truncate(new_len);
            self.draw_ptcl_capacities.truncate(new_len);
            self.occlusion.clear_changes(new_len);
            return true;
        }
        let membership_changed = &self.membership_changed;
        for index in 0..self.affected_tiles.len() {
            let tile = self.affected_tiles.get(index);
            self.tile_refs.retain(tile, |draw| {
                let draw = draw as usize;
                !membership_changed.contains(draw)
            });
        }
        for index in 0..self.membership_changed.len() {
            let draw = self.membership_changed.get(index);
            if draw >= new_len || self.draw_ranks[draw].path[0] == u128::MAX {
                continue;
            }
            for_tile_in_bbox(self.draw_bboxes[draw], tiles_size.0, |tile| {
                self.tile_refs.push(tile, draw as u32);
            });
        }
        if !self.membership_changed.is_empty() {
            for index in 0..self.affected_tiles.len() {
                let tile = self.affected_tiles.get(index);
                self.tile_refs[tile].sort_unstable_by(|a, b| {
                    self.draw_ranks[*a as usize].cmp(&self.draw_ranks[*b as usize])
                });
            }
        }
        if self.prefix_visibility == self.occlusion.single_batch {
            for index in 0..self.affected_tiles.len() {
                let tile = self.affected_tiles.get(index);
                self.tile_refs.release_empty(tile);
                self.rewrite_tile(tile);
            }
            if self.indices.maybe_compact() {
                self.dirty_indices.clear();
                self.dirty_records.clear();
                self.refresh_records();
                self.full_upload = true;
            }
        } else {
            self.prefix_visibility = self.occlusion.single_batch;
            self.rebuild_indices();
        }
        self.tile_refs.maybe_compact();
        self.draw_bboxes.truncate(new_len);
        self.draw_ranks.truncate(new_len);
        self.draw_ptcl_capacities.truncate(new_len);
        self.occlusion.clear_changes(new_len);
        true
    }
}
