use super::*;

impl TileDrawBins {
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
        self.transient_occlusion = self.occlusion.active;
        let chunk_count = if self.transient_occlusion {
            self.fill_transient_occluded(draw_records, ordered, tiles_size, cursors)
        } else {
            self.fill_transient_plain(draw_records, ordered, tiles_size, cursors)
        };

        // Retain per-tile allocations across transient motion. Dropping them made the
        // first sparse frame allocate thousands of vectors as the spring settled.
        for pages in &mut self.tile_pages {
            pages.clear();
        }
        for refs in &mut self.tile_refs {
            refs.clear();
        }
        self.free_pages.clear();
        self.flat_rebuild_work = 0;
        self.draw_bboxes.clear();
        if preserve_draw_bboxes || self.transient_occlusion {
            self.draw_bboxes
                .extend(draw_records.iter().enumerate().map(|(index, draw)| {
                    // Retained arenas can contain retired slots. Density decisions and
                    // source queries must describe live membership, not their stale geometry.
                    if painter_keys.is_some_and(|keys| keys[index].path[0] == u128::MAX) {
                        TileBbox {
                            x0: 0,
                            y0: 0,
                            x1: 0,
                            y1: 0,
                        }
                    } else {
                        draw.tile_bbox(tiles_size.0, tiles_size.1)
                    }
                }));
        }
        self.draw_ranks.clear();
        self.draw_ptcl_capacities.clear();
        self.tiles_size = tiles_size;
        self.active_pages = chunk_count;
        self.spatial_index_valid = false;
        self.index_format = IndexFormat::Flat;
        self.dirty_records.clear();
        self.dirty_pages.clear();
        self.full_upload = true;
        self.occlusion.clear_changes(draw_records.len());
    }
    // Keep the ordinary tile loops separate from the visibility traversal. Joining both
    // strategies in one function prevented LLVM from vectorizing the unchanged hot path.
    #[inline(never)]
    fn fill_transient_plain(
        &mut self,
        draw_records: &[DrawRecord],
        ordered: &[u32],
        tiles_size: (u32, u32),
        cursors: &mut Vec<u32>,
    ) -> usize {
        for &draw in ordered {
            let record = &draw_records[draw as usize];
            let bbox = record.tile_bbox(tiles_size.0, tiles_size.1);
            self.draw_ptcl_capacity += draw_ptcl_capacity(record, bbox);
            for_tile_in_bbox(bbox, tiles_size.0, |tile| {
                self.records[tile].end += 1;
            });
        }
        let chunks = self.allocate_transient(cursors);
        for &draw in ordered {
            let bbox = draw_records[draw as usize].tile_bbox(tiles_size.0, tiles_size.1);
            for_tile_in_bbox(bbox, tiles_size.0, |tile| {
                let ordinal = cursors[tile];
                let start = self.records[tile].start & !TILE_DRAW_FLAT_FLAG;
                self.draw_indices[(start + ordinal) as usize] = draw;
                cursors[tile] += 1;
            });
        }
        chunks
    }

    fn fill_transient_occluded(
        &mut self,
        draw_records: &[DrawRecord],
        ordered: &[u32],
        tiles_size: (u32, u32),
        cursors: &mut Vec<u32>,
    ) -> usize {
        self.draw_ptcl_capacity =
            self.occlusion
                .for_each_visible(draw_records, ordered, tiles_size, |tile, _| {
                    self.records[tile].end += 1;
                });
        let chunks = self.allocate_transient(cursors);
        for (cursor, record) in cursors.iter_mut().zip(&self.records) {
            *cursor = record.end;
        }
        self.occlusion
            .for_each_visible(draw_records, ordered, tiles_size, |tile, draw| {
                cursors[tile] -= 1;
                let start = self.records[tile].start & !TILE_DRAW_FLAT_FLAG;
                self.draw_indices[(start + cursors[tile]) as usize] = draw;
            });
        chunks
    }

    fn allocate_transient(&mut self, cursors: &mut Vec<u32>) -> usize {
        let mut next_index = 0u32;
        let mut chunks = 0usize;
        for record in &mut self.records {
            let count = record.end;
            record.start = if count == 0 {
                u32::MAX
            } else {
                assert!(next_index < TILE_DRAW_FLAT_FLAG);
                TILE_DRAW_FLAT_FLAG | next_index
            };
            next_index += count;
            chunks += count.div_ceil(COARSE_CHUNK_SIZE) as usize;
        }
        self.draw_indices.clear();
        self.draw_indices.resize(next_index as usize, u32::MAX);
        cursors.clear();
        cursors.resize(self.records.len(), 0);
        chunks
    }
}
