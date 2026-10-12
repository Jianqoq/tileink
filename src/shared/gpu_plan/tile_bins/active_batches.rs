//! Exact active-batch queries own scratch independently of shared spatial snapshots.
use super::TileDrawBins;
use crate::render::damage_tiles::DamageTiles;
use crate::{canvas::Canvas, shared::execution::ExecPlan};

#[derive(Default)]
pub(crate) struct ActiveBatches {
    marks: Vec<u32>,
    generation: u32,
    batches: Vec<u32>,
}

impl ActiveBatches {
    /// Bins, membership and damage describe the same scene. Scene upload validates
    /// damage dimensions once; every generated tile/draw index is in that domain.
    pub(crate) fn collect(
        &mut self,
        bins: &TileDrawBins,
        canvas: &Canvas,
        plan: &ExecPlan,
        active: &DamageTiles,
    ) -> Vec<u32> {
        self.begin();
        let painter = canvas.retained_painter();
        let ids = canvas.raster_batch_ids(plan);
        if active.len() as usize > canvas.draw_records.len() {
            // Dense damage used to revisit a draw once per dirty tile in its bbox.
            // Reverse the exact membership query and stop at a batch's first hit.
            if let Some(painter) = painter {
                for (draw, key) in painter.keys.iter().enumerate() {
                    if key.path[0] != u128::MAX {
                        self.mark_if_intersects(draw, ids, canvas, active);
                    }
                }
            } else {
                for &draw in plan.draw_order.iter() {
                    self.mark_if_intersects(draw as usize, ids, canvas, active);
                }
            }
        } else {
            self.collect_tiles(bins, active.list(), ids);
        }
        self.finish()
    }

    fn begin(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.marks.fill(0);
            self.generation = 1;
        }
        self.batches.clear();
    }

    fn mark(&mut self, batch: u32) {
        if batch == u32::MAX {
            return;
        }
        let index = batch as usize;
        if index >= self.marks.len() {
            self.marks.resize(index + 1, 0);
        }
        if self.marks[index] != self.generation {
            self.marks[index] = self.generation;
            self.batches.push(batch);
        }
    }

    fn mark_if_intersects(
        &mut self,
        draw: usize,
        ids: &[u32],
        canvas: &Canvas,
        active: &DamageTiles,
    ) {
        let batch = ids[draw];
        if batch == u32::MAX || self.marks.get(batch as usize) == Some(&self.generation) {
            return;
        }
        // Match bin construction's tile-rounded bounds, including conservative
        // membership for degenerate pixel rectangles inside a tile.
        let (width, height) = active.dimensions();
        let bbox = canvas.draw_records[draw].tile_bbox(width, height);
        if active.intersects_tile_rect(bbox.x0, bbox.y0, bbox.x1, bbox.y1) {
            self.mark(batch);
        }
    }

    fn collect_tiles(&mut self, bins: &TileDrawBins, tiles: &[u32], ids: &[u32]) {
        let mut mark = |draw| self.mark(ids[draw as usize]);
        if bins.spatial_index_valid {
            for &draw in tiles
                .iter()
                .flat_map(|&tile| &bins.tile_refs[tile as usize])
            {
                mark(draw);
            }
        } else {
            for &tile in tiles {
                bins.for_each_tile_draw(tile as usize, &mut mark);
            }
        }
    }

    fn finish(&mut self) -> Vec<u32> {
        self.batches.sort_unstable();
        self.batches.clone()
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod producer_tests;
