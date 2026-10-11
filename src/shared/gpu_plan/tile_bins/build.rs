use super::*;

impl TileDrawBins {
    pub(super) fn reset(
        &mut self,
        draw_records: &[DrawRecord],
        draw_order: &[u32],
        painter_keys: Option<&[PainterKey]>,
        tiles_size: (u32, u32),
    ) {
        let tile_count = tiles_size.0 as usize * tiles_size.1 as usize;
        if !self.page_arena_valid {
            self.draw_indices.clear();
            for pages in &mut self.tile_pages {
                pages.clear();
            }
            self.free_pages.clear();
            self.active_pages = 0;
            self.page_arena_valid = true;
        }
        self.records.resize(
            tile_count,
            TileDrawRecord {
                start: u32::MAX,
                end: 0,
            },
        );
        self.records.fill(TileDrawRecord {
            start: u32::MAX,
            end: 0,
        });
        if self.tile_pages.len() > tile_count {
            for pages in self.tile_pages.drain(tile_count..) {
                self.active_pages -= pages.len();
                self.free_pages.extend(pages);
            }
        }
        self.tile_pages.resize_with(tile_count, Vec::new);
        self.tile_refs.resize_with(tile_count, Vec::new);
        for draws in &mut self.tile_refs {
            draws.clear();
        }
        self.draw_bboxes.clear();
        self.draw_bboxes.resize(
            draw_records.len(),
            TileBbox {
                x0: 0,
                y0: 0,
                x1: 0,
                y1: 0,
            },
        );
        self.draw_ranks.clear();
        self.draw_ranks
            .resize(draw_records.len(), PainterKey::inactive());
        self.draw_ptcl_capacities.clear();
        self.draw_ptcl_capacities.resize(draw_records.len(), 0);
        self.draw_ptcl_capacity = 0;
        self.tiles_size = tiles_size;
        self.dirty_records.clear();
        self.dirty_pages.clear();
        let mut stable_order = Vec::new();
        let ordered = if let Some(keys) = painter_keys {
            stable_order.extend(
                keys.iter()
                    .enumerate()
                    .filter_map(|(draw, key)| (key.path[0] != u128::MAX).then_some(draw as u32)),
            );
            stable_order.sort_unstable_by(|a, b| keys[*a as usize].cmp(&keys[*b as usize]));
            stable_order.as_slice()
        } else {
            draw_order
        };
        for (rank, &draw_ix) in ordered.iter().enumerate() {
            self.draw_ranks[draw_ix as usize] = painter_keys.map_or_else(
                || PainterKey {
                    path: std::rc::Rc::from([rank as u128]),
                    local: 0,
                },
                |keys| keys[draw_ix as usize].clone(),
            );
            let bbox = draw_records[draw_ix as usize].tile_bbox(tiles_size.0, tiles_size.1);
            self.draw_bboxes[draw_ix as usize] = bbox;
            let capacity = draw_ptcl_capacity(&draw_records[draw_ix as usize], bbox);
            self.draw_ptcl_capacities[draw_ix as usize] = capacity;
            self.draw_ptcl_capacity += capacity;
            for_tile_in_bbox(bbox, tiles_size.0, |tile| {
                self.tile_refs[tile].push(draw_ix);
            });
        }
        for tile in 0..tile_count {
            self.rewrite_tile(tile);
        }
        self.build_flat_full_upload();
        self.dirty_records.clear();
        self.dirty_pages.clear();
        self.full_upload = true;
    }

    /// Builds page-form tile bins for a one-shot immediate canvas without allocating persistent
    /// per-draw ranks, bboxes, or per-tile vectors. Immediate canvases are rebuilt every frame, so
    /// maintaining mutation indexes only adds CPU and allocation cost without enabling reuse.
    pub(in crate::shared::gpu_plan) fn reset_transient(
        &mut self,
        draw_records: &[DrawRecord],
        draw_order: &[u32],
        painter_keys: Option<&[PainterKey]>,
        tiles_size: (u32, u32),
        cursors: &mut Vec<u32>,
        preserve_draw_bboxes: bool,
    ) {
        let tile_count = tiles_size.0 as usize * tiles_size.1 as usize;
        self.records.clear();
        self.records.resize(
            tile_count,
            TileDrawRecord {
                start: u32::MAX,
                end: 0,
            },
        );
        self.draw_ptcl_capacity = 0;
        let mut stable_order = Vec::new();
        let ordered = if let Some(keys) = painter_keys {
            stable_order.extend(
                keys.iter()
                    .enumerate()
                    .filter_map(|(draw, key)| (key.path[0] != u128::MAX).then_some(draw as u32)),
            );
            stable_order.sort_unstable_by(|a, b| keys[*a as usize].cmp(&keys[*b as usize]));
            stable_order.as_slice()
        } else {
            draw_order
        };
        for &draw in ordered {
            let record = &draw_records[draw as usize];
            let bbox = record.tile_bbox(tiles_size.0, tiles_size.1);
            self.draw_ptcl_capacity += draw_ptcl_capacity(record, bbox);
            for_tile_in_bbox(bbox, tiles_size.0, |tile| {
                self.records[tile].end += 1;
            });
        }

        let mut next_index = 0u32;
        let mut chunk_count = 0usize;
        for record in &mut self.records {
            let count = record.end;
            record.start = if count == 0 {
                u32::MAX
            } else {
                assert!(next_index < TILE_DRAW_FLAT_FLAG);
                TILE_DRAW_FLAT_FLAG | next_index
            };
            next_index += count;
            chunk_count += count.div_ceil(COARSE_CHUNK_SIZE) as usize;
        }
        self.draw_indices.clear();
        self.draw_indices.resize(next_index as usize, u32::MAX);

        cursors.clear();
        cursors.resize(tile_count, 0);
        for &draw in ordered {
            let bbox = draw_records[draw as usize].tile_bbox(tiles_size.0, tiles_size.1);
            for_tile_in_bbox(bbox, tiles_size.0, |tile| {
                let ordinal = cursors[tile];
                let start = self.records[tile].start & !TILE_DRAW_FLAT_FLAG;
                self.draw_indices[(start + ordinal) as usize] = draw;
                cursors[tile] += 1;
            });
        }

        // Retain per-tile allocations across transient motion. Dropping them made the
        // first sparse frame allocate thousands of vectors as the spring settled.
        for pages in &mut self.tile_pages {
            pages.clear();
        }
        for refs in &mut self.tile_refs {
            refs.clear();
        }
        self.free_pages.clear();
        self.draw_bboxes.clear();
        if preserve_draw_bboxes {
            self.draw_bboxes.extend(
                draw_records
                    .iter()
                    .map(|draw| draw.tile_bbox(tiles_size.0, tiles_size.1)),
            );
        }
        self.draw_ranks.clear();
        self.draw_ptcl_capacities.clear();
        self.tiles_size = tiles_size;
        self.active_pages = chunk_count;
        self.page_arena_valid = false;
        self.flat_full_upload = false;
        self.full_upload_records.clear();
        self.full_upload_indices.clear();
        self.dirty_records.clear();
        self.dirty_pages.clear();
        self.full_upload = true;
    }
}
