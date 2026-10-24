//! Conservative visibility proofs before coarse count/emit. CPU spatial membership stays complete.
use super::*;
use crate::shared::{draw_record::DrawTag, gpu_layout::brush::GPU_BRUSH_SOLID};

const EMPTY: TileBbox = TileBbox {
    x0: 0,
    y0: 0,
    x1: 0,
    y1: 0,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DrawVisibility {
    batch: u32,
    opaque: TileBbox,
}

impl Default for DrawVisibility {
    fn default() -> Self {
        Self {
            batch: u32::MAX,
            opaque: EMPTY,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Batch {
    first: u32,
    enabled: bool,
    has_occluder: bool,
    clip: TileBbox,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Occlusion {
    draws: Vec<DrawVisibility>,
    batches: Vec<Batch>,
    candidates: Vec<(usize, TileBbox)>,
    pub(super) changed: DenseIndexSet,
    pub(super) dirty_tiles: Vec<TileBbox>,
    pub(super) active: bool,
    pub(super) single_batch: bool,
}

impl Occlusion {
    pub(super) fn update(&mut self, canvas: &Canvas, plan: &ExecPlan) {
        let was_active = self.active;
        let ids = canvas.raster_batch_ids(plan);
        let keys = canvas.retained_painter().map(|p| p.keys.as_slice());
        let batch_count = ids
            .iter()
            .copied()
            .filter(|&id| id != u32::MAX)
            .max()
            .map_or(0, |id| id as usize + 1);
        self.batches.clear();
        self.batches.resize(
            batch_count,
            Batch {
                first: u32::MAX,
                enabled: true,
                has_occluder: false,
                clip: EMPTY,
            },
        );
        let mut visit = |draw: u32| {
            let id = ids[draw as usize];
            if id == u32::MAX {
                return;
            }
            let batch = &mut self.batches[id as usize];
            batch.enabled &= matches!(
                canvas.draw_records[draw as usize].tag(),
                DrawTag::Brush | DrawTag::PathGlyph
            );
            if batch.first == u32::MAX
                || keys.is_some_and(|keys| keys[draw as usize] < keys[batch.first as usize])
            {
                batch.first = draw;
            }
        };
        if let Some(keys) = keys {
            for (draw, key) in keys.iter().enumerate() {
                if key.path[0] != u128::MAX {
                    visit(draw as u32);
                }
            }
        } else {
            for &draw in plan.draw_order.iter() {
                visit(draw);
            }
        }
        let len = canvas.draw_records.len();
        self.candidates.clear();
        for (index, draw) in canvas.draw_records.iter().enumerate() {
            let id = ids[index];
            if id == u32::MAX || keys.is_some_and(|keys| keys[index].path[0] == u128::MAX) {
                continue;
            }
            let batch = &mut self.batches[id as usize];
            // A batch's first draw has no predecessor to hide. Reject it before proving clips:
            // ordinary background/text batches must not decode every ancestor clip each frame.
            if !batch.enabled || batch.first == index as u32 {
                continue;
            }
            let bbox = opaque_tiles(canvas, draw);
            if bbox.tile_count() != 0 {
                self.candidates.push((index, bbox));
                batch.has_occluder = true;
            }
        }
        batch_clips(canvas, plan, &plan.ops, &mut self.batches);
        let mut candidates = self.candidates.iter().peekable();
        self.draws
            .resize(self.draws.len().max(len), DrawVisibility::default());
        self.changed.begin(self.draws.len());
        self.dirty_tiles.clear();
        self.active = false;
        for (index, previous) in self.draws.iter_mut().enumerate() {
            let mut current = DrawVisibility::default();
            if index < len && keys.is_none_or(|keys| keys[index].path[0] != u128::MAX) {
                let id = ids[index];
                if id != u32::MAX && self.batches[id as usize].enabled {
                    current.batch = id;
                    current.opaque = candidates
                        .next_if(|(draw, _)| *draw == index)
                        .map_or(EMPTY, |&(_, bbox)| {
                            intersection(bbox, self.batches[id as usize].clip)
                        });
                }
            }
            self.active |= current.opaque.tile_count() != 0;
            if previous.batch != current.batch {
                self.changed.insert(index);
            }
            if previous.opaque != current.opaque {
                self.dirty_tiles.extend(
                    [previous.opaque, current.opaque]
                        .into_iter()
                        .filter(|bbox| bbox.tile_count() != 0),
                );
            }
            *previous = current;
        }
        self.draws.truncate(len);
        // Batch IDs are uploaded independently. Without an old or new visibility proof, their
        // changes cannot alter tile references and must not force spatial rewrites/full uploads.
        if !was_active && !self.active {
            self.changed.begin(len);
        }
        self.single_batch = self
            .batches
            .iter()
            .filter(|batch| batch.first != u32::MAX)
            .count()
            == 1
            && self.batches.iter().all(|batch| batch.enabled);
    }

    pub(super) fn filter_tile(
        &self,
        tile: usize,
        width: u32,
        refs: &[u32],
        visible: &mut Vec<u32>,
    ) -> bool {
        visible.clear();
        let mut covered = u32::MAX;
        let mut removed = false;
        let x = tile as u32 % width;
        let y = tile as u32 / width;
        for (index, &draw) in refs.iter().enumerate().rev() {
            let visibility = self.draws[draw as usize];
            if visibility.batch != u32::MAX && visibility.batch == covered {
                // Most application tiles have no removable prefix. Start copying only when a
                // proof actually erases a predecessor, retaining the already visited suffix.
                if !removed {
                    visible.extend(refs[index + 1..].iter().rev().copied());
                    removed = true;
                }
                continue;
            }
            if removed {
                visible.push(draw);
            }
            let bbox = visibility.opaque;
            covered = if x >= bbox.x0 && x < bbox.x1 && y >= bbox.y0 && y < bbox.y1 {
                visibility.batch
            } else {
                u32::MAX
            };
        }
        visible.reverse();
        removed
    }

    /// Existing tile membership already has painter order. Searching it directly avoids scanning
    /// unrelated occluders and then binary-searching their ranks on every opacity edit.
    pub(super) fn visible_start(&self, tile: usize, width: u32, refs: &[u32]) -> usize {
        if !self.active {
            return 0;
        }
        let x = tile as u32 % width;
        let y = tile as u32 / width;
        refs.iter()
            .rposition(|&draw| {
                let bbox = self.draws[draw as usize].opaque;
                x >= bbox.x0 && x < bbox.x1 && y >= bbox.y0 && y < bbox.y1
            })
            .unwrap_or(0)
    }

    pub(super) fn clear_changes(&mut self, len: usize) {
        self.changed.begin(len);
        self.dirty_tiles.clear();
    }
}

mod geometry;
use geometry::{batch_clips, intersection, opaque_tiles};

mod transient;

#[cfg(test)]
mod tests;
