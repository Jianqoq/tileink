use super::*;

impl Occlusion {
    /// Subtract a proven rectangle before visiting tile memberships. Remember one covering
    /// rectangle per contiguous batch; replacing it is conservative, never a union guess.
    /// This avoids scanning hidden tiles just to discover that their references are unnecessary.
    #[inline(never)]
    pub(in crate::shared::gpu_plan::tile_bins) fn for_each_visible(
        &self,
        draw_records: &[DrawRecord],
        ordered: &[u32],
        tiles_size: (u32, u32),
        mut visit: impl FnMut(usize, u32),
    ) -> usize {
        let mut batch = u32::MAX;
        let mut cover = EMPTY;
        let mut capacity = 0;
        for &draw in ordered.iter().rev() {
            let visibility = self.draws[draw as usize];
            if batch != visibility.batch || batch == u32::MAX {
                cover = EMPTY;
            }
            batch = visibility.batch;
            let record = &draw_records[draw as usize];
            let bbox = record.tile_bbox(tiles_size.0, tiles_size.1);
            let hidden = TileBbox {
                x0: bbox.x0.max(cover.x0),
                y0: bbox.y0.max(cover.y0),
                x1: bbox.x1.min(cover.x1),
                y1: bbox.y1.min(cover.y1),
            };
            capacity += draw_ptcl_capacity(record, bbox) - draw_ptcl_capacity(record, hidden);
            let mut visit = |tile| visit(tile, draw);
            if hidden.tile_count() == 0 {
                for_tile_in_bbox(bbox, tiles_size.0, &mut visit);
            } else {
                for region in [
                    TileBbox {
                        y1: hidden.y0,
                        ..bbox
                    },
                    TileBbox {
                        y0: hidden.y1,
                        ..bbox
                    },
                    TileBbox {
                        x1: hidden.x0,
                        y0: hidden.y0,
                        y1: hidden.y1,
                        ..bbox
                    },
                    TileBbox {
                        x0: hidden.x1,
                        y0: hidden.y0,
                        y1: hidden.y1,
                        ..bbox
                    },
                ] {
                    for_tile_in_bbox(region, tiles_size.0, &mut visit);
                }
            }
            if visibility.opaque.tile_count() > cover.tile_count() {
                cover = visibility.opaque;
            }
        }
        capacity
    }
}
