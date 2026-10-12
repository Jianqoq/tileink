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
        for pages in &mut self.tile_pages {
            pages.clear();
        }
        self.free_pages.clear();
        self.flat_rebuild_work = 0;
        self.spatial_index_valid = true;
        self.records.resize(
            tile_count,
            TileDrawRecord {
                start: u32::MAX,
                end: 0,
            },
        );
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
        self.transient_occlusion = false;
        self.index_format = if self.occlusion.single_batch {
            IndexFormat::FlatPrefix
        } else {
            IndexFormat::Flat
        };
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
        self.build_flat_full_upload();
        self.dirty_records.clear();
        self.dirty_pages.clear();
        self.full_upload = true;
        self.occlusion.clear_changes(draw_records.len());
    }
}
