//! Exact active-batch queries own scratch independently of shared spatial snapshots.
use super::{Canvas, ExecPlan, TileDrawBins};
use crate::render::damage_tiles::DamageTiles;

#[derive(Default)]
pub(crate) struct ActiveBatches {
    marks: Vec<u32>,
    generation: u32,
    batches: Vec<u32>,
}

impl ActiveBatches {
    pub(crate) fn collect(
        &mut self,
        bins: &TileDrawBins,
        canvas: &Canvas,
        plan: &ExecPlan,
        active: &DamageTiles,
    ) -> Vec<u32> {
        self.begin();
        let ids = canvas
            .stable_batch_ids
            .as_deref()
            .unwrap_or(&plan.draw_batch_ids);
        if active.len() as usize > canvas.draw_records.len() {
            // Dense damage used to revisit a draw once per dirty tile in its bbox.
            // Reverse the exact membership query and stop at a batch's first hit.
            if let Some(keys) = &canvas.painter_keys {
                for (draw, key) in keys.iter().enumerate() {
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
        let Some(&batch) = ids.get(draw) else {
            return;
        };
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
        let mut mark = |draw| {
            if let Some(&batch) = ids.get(draw as usize) {
                self.mark(batch);
            }
        };
        if bins.page_arena_valid {
            for &draw in tiles
                .iter()
                .filter_map(|&tile| bins.tile_refs.get(tile as usize))
                .flatten()
            {
                mark(draw);
            }
        } else {
            for &tile in tiles {
                if (tile as usize) < bins.records.len() {
                    bins.for_each_tile_draw(tile as usize, &mut mark);
                }
            }
        }
    }

    fn finish(&mut self) -> Vec<u32> {
        self.batches.sort_unstable();
        self.batches.clone()
    }

    #[cfg(test)]
    pub(super) fn query_tiles(
        &mut self,
        bins: &TileDrawBins,
        tiles: &[u32],
        ids: &[u32],
    ) -> Vec<u32> {
        self.begin();
        self.collect_tiles(bins, tiles, ids);
        self.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::*;
    use crate::{Bounds, Radius};
    use peniko::{Color, kurbo::Rect};

    #[test]
    fn dense_damage_matches_exact_bin_membership_including_inactive_and_degenerate_draws() {
        let size = (65 * crate::TILE_SIZE + 3, 5 * crate::TILE_SIZE + 7);
        let mut canvas = Canvas::new(size.0, size.1, 1.0);
        for rect in [
            Rect::new(0.0, 0.0, 1043.0, 83.0),
            Rect::new(100.0, 0.0, 116.0, 16.0),
            Rect::new(1041.0, 81.0, 1055.0, 90.0),
            Rect::new(-32.0, -32.0, -16.0, -16.0),
            Rect::new(0.0, 0.0, 1043.0, 83.0),
            Rect::new(0.0, 0.0, 1043.0, 83.0),
            Rect::new(8.0, 8.0, 12.0, 12.0),
        ] {
            canvas.push_rect(rect, Radius::ZERO, Color::BLACK);
        }
        canvas.draw_records[6].pixel_bounds = PixelBounds {
            x0: 8,
            y0: 8,
            x1: 8,
            y1: 8,
        };
        let ids = [7, 3, 11, 13, 17, u32::MAX, 23];
        canvas.stable_batch_ids = Some(ids.to_vec());
        for keyed in [false, true] {
            if keyed {
                let mut keys = (0..ids.len())
                    .map(|draw| crate::canvas::PainterKey {
                        path: std::rc::Rc::from([draw as u128]),
                        local: 0,
                    })
                    .collect::<Vec<_>>();
                keys[4] = crate::canvas::PainterKey::inactive();
                canvas.painter_keys = Some(keys);
            }
            let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
            for flat in [false, true] {
                let mut bins = build_tile_draw_bins(&canvas);
                if flat {
                    bins.reset_transient(
                        &canvas.draw_records,
                        &plan.draw_order,
                        canvas.painter_keys.as_deref(),
                        (66, 6),
                        &mut Vec::new(),
                        false,
                    );
                }
                let mut query = ActiveBatches::default();
                let full = DamageTiles::full(size);
                let expected = if keyed {
                    vec![3, 7, 11, 23]
                } else {
                    vec![3, 7, 11, 17, 23]
                };
                assert_eq!(query.collect(&bins, &canvas, &plan, &full), expected);
                let mut holes = DamageTiles::new(size);
                holes.add_bounds(Bounds::new(1041, 81, 1043, 83));
                holes.add_bounds(Bounds::new(256, 20, 768, 48));
                let mut sparse = DamageTiles::new(size);
                sparse.add_bounds(Bounds::new(8, 8, 9, 9));
                for active in [full, holes, sparse, DamageTiles::new(size)] {
                    let expected = query.query_tiles(&bins, active.list(), &ids);
                    assert_eq!(query.collect(&bins, &canvas, &plan, &active), expected);
                }
                query.generation = u32::MAX;
                query.marks.fill(1);
                assert_eq!(
                    query.collect(&bins, &canvas, &plan, &DamageTiles::full(size)),
                    if keyed {
                        vec![3, 7, 11, 23]
                    } else {
                        vec![3, 7, 11, 17, 23]
                    }
                );
            }
        }
    }

    #[test]
    fn active_batches_preserve_exact_sets_for_flat_and_paged_bins() {
        let mut canvas = Canvas::new(crate::TILE_SIZE * 3, crate::TILE_SIZE, 1.0);
        canvas.push_rect(Rect::new(0.0, 0.0, 32.0, 16.0), Radius::ZERO, Color::BLACK);
        canvas.push_rect(Rect::new(16.0, 0.0, 32.0, 16.0), Radius::ZERO, Color::WHITE);
        canvas.push_rect(Rect::new(32.0, 0.0, 48.0, 16.0), Radius::ZERO, Color::BLACK);
        // More than one page in the first tile; the final draw has a distinct batch.
        for _ in 0..=COARSE_CHUNK_SIZE {
            canvas.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::ZERO, Color::BLACK);
        }
        let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
        let mut ids = vec![7; canvas.draw_records.len()];
        ids[1] = 3;
        ids[2] = u32::MAX;
        *ids.last_mut().unwrap() = 11;
        for flat in [false, true] {
            let mut bins = build_tile_draw_bins(&canvas);
            let mut query = ActiveBatches::default();
            if flat {
                bins.reset_transient(
                    &canvas.draw_records,
                    &plan.draw_order,
                    None,
                    (3, 1),
                    &mut Vec::new(),
                    false,
                );
            }
            assert_eq!(bins.page_arena_valid, !flat);
            assert_eq!(
                query.query_tiles(&bins, &[1, 0, 1, 2, 99], &ids),
                [3, 7, 11]
            );
            assert_eq!(query.query_tiles(&bins, &[1], &ids), [3, 7]);
            assert!(query.query_tiles(&bins, &[2, 99], &ids).is_empty());
            assert!(query.query_tiles(&bins, &[], &ids).is_empty());
            assert!(query.query_tiles(&bins, &[0], &[]).is_empty());
            assert_eq!(query.query_tiles(&bins, &[0, 1], &ids[..1]), [7]);
            query.generation = u32::MAX;
            query.marks.fill(1);
            assert_eq!(query.query_tiles(&bins, &[0, 1], &ids), [3, 7, 11]);
        }
    }
    #[test]
    fn active_batch_ids_deduplicate_with_reusable_generation_marks() {
        let mut canvas = Canvas::new(crate::TILE_SIZE * 3, crate::TILE_SIZE, 1.0);
        canvas.push_rect(
            Rect::new(0.0, 0.0, 32.0, 16.0),
            crate::Radius::ZERO,
            Color::BLACK,
        );
        canvas.push_rect(
            Rect::new(16.0, 0.0, 32.0, 16.0),
            crate::Radius::ZERO,
            Color::WHITE,
        );
        canvas.push_rect(
            Rect::new(32.0, 0.0, 48.0, 16.0),
            crate::Radius::ZERO,
            Color::BLACK,
        );
        let bins = build_tile_draw_bins(&canvas);
        let mut query = ActiveBatches::default();
        let batches = [7, 3, u32::MAX];

        assert_eq!(query.query_tiles(&bins, &[0, 1], &batches), [3, 7]);
        assert!(query.query_tiles(&bins, &[2], &batches).is_empty());
        assert_eq!(query.query_tiles(&bins, &[1], &batches), [3, 7]);
    }
}
