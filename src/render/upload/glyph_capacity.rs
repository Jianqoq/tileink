//! CPU dependency tracking for coarse glyph capacity, shared by GPU adapters.
use super::ranges::{changed_ranges, indices_from_ranges, push_dirty_index, replace_or_push};
use crate::{
    canvas::{Canvas, SceneBufferChanges},
    shared::{
        dense_set::DenseIndexSet, draw_record::DrawRecord, gpu_plan::coarse_glyph_capacity_for_draw,
    },
    text::{AtlasSignature, PreparedTextChanges, PreparedTextData},
};
use std::collections::HashSet;

#[cfg(feature = "bench-internals")]
mod benchmark;
#[cfg(feature = "bench-internals")]
pub use benchmark::{GlyphCapacityBenchmark, GlyphCapacityBenchmarkCase};

#[derive(Default)]
pub(crate) struct GlyphCapacityCache {
    capacities: Vec<usize>,
    draw_records: Vec<DrawRecord>,
    draw_runs: Vec<Option<u32>>,
    run_draws: Vec<HashSet<usize>>,
    run_glyph_ranges: Vec<std::ops::Range<usize>>,
    glyph_runs: Vec<u32>,
    affected_draws: DenseIndexSet,
    total: usize,
    tiles_size: (u32, u32),
    atlas_signature: AtlasSignature,
    initialized: bool,
}

impl GlyphCapacityCache {
    pub(crate) fn update(
        &mut self,
        canvas: &Canvas,
        text: Option<&PreparedTextData>,
        changes: Option<&SceneBufferChanges>,
        flat_text_changes: Option<&PreparedTextChanges>,
    ) -> usize {
        let Some(text) = text else {
            *self = Self::default();
            return 0;
        };
        let tiles_size = (canvas.width_in_tiles(), canvas.height_in_tiles());
        let glyph_changes_empty = changes
            .map(|changes| changes.glyphs.is_empty())
            .or_else(|| flat_text_changes.map(|changes| changes.glyphs().is_empty()))
            .unwrap_or(false);
        let resource_only_change =
            text.atlas_signature() != self.atlas_signature && glyph_changes_empty;
        if !self.initialized || self.tiles_size != tiles_size || resource_only_change {
            self.rebuild(canvas, text);
            return self.total;
        }
        if let Some(changes) = changes {
            self.update_ranges(
                canvas,
                text,
                &changes.glyphs,
                &changes.text_runs,
                &changes.draws,
            );
        } else if let Some(text_changes) = flat_text_changes {
            let draw_changes = self.flat_draw_changes(canvas);
            self.update_ranges(
                canvas,
                text,
                text_changes.glyphs(),
                text_changes.runs(),
                &draw_changes,
            );
        } else {
            self.rebuild(canvas, text);
            return self.total;
        }
        self.atlas_signature = text.atlas_signature();
        self.total
    }

    fn rebuild(&mut self, canvas: &Canvas, text: &PreparedTextData) {
        self.capacities.clear();
        self.capacities.resize(canvas.draw_records.len(), 0);
        self.draw_records.clear();
        self.draw_records.extend_from_slice(&canvas.draw_records);
        self.draw_runs.clear();
        self.draw_runs.resize(canvas.draw_records.len(), None);
        self.run_draws.clear();
        self.run_draws
            .resize_with(canvas.text_runs.len(), HashSet::new);
        self.run_glyph_ranges.clear();
        self.run_glyph_ranges.resize(0, 0..0);
        self.run_glyph_ranges.extend(
            canvas
                .text_runs
                .iter()
                .map(|run| run_glyph_range(*run, canvas.text_glyphs.len())),
        );
        self.glyph_runs.clear();
        self.glyph_runs.resize(canvas.text_glyphs.len(), u32::MAX);
        for (run, range) in self.run_glyph_ranges.iter().enumerate() {
            self.glyph_runs[range.clone()].fill(run as u32);
        }
        self.total = 0;
        for draw in 0..canvas.draw_records.len() {
            let run = canvas.draw_records[draw].glyph_run_id();
            self.draw_runs[draw] = run;
            if let Some(run) = run.filter(|run| (*run as usize) < self.run_draws.len()) {
                self.run_draws[run as usize].insert(draw);
            }
            let capacity = coarse_glyph_capacity_for_draw(canvas, Some(text), draw);
            self.capacities[draw] = capacity;
            self.total += capacity;
        }
        self.tiles_size = (canvas.width_in_tiles(), canvas.height_in_tiles());
        self.atlas_signature = text.atlas_signature();
        self.initialized = true;
    }

    fn flat_draw_changes(&self, canvas: &Canvas) -> Vec<std::ops::Range<usize>> {
        let shared_len = self.draw_records.len().min(canvas.draw_records.len());
        let mut changes = Vec::new();
        for index in 0..shared_len {
            if bytemuck::bytes_of(&self.draw_records[index])
                != bytemuck::bytes_of(&canvas.draw_records[index])
            {
                push_dirty_index(&mut changes, index);
            }
        }
        if canvas.draw_records.len() > shared_len {
            if let Some(last) = changes.last_mut()
                && last.end == shared_len
            {
                last.end = canvas.draw_records.len();
            } else {
                changes.push(shared_len..canvas.draw_records.len());
            }
        }
        changes
    }

    fn update_ranges(
        &mut self,
        canvas: &Canvas,
        text: &PreparedTextData,
        glyph_changes: &[std::ops::Range<usize>],
        run_changes: &[std::ops::Range<usize>],
        draw_changes: &[std::ops::Range<usize>],
    ) {
        let old_draw_len = self.draw_runs.len();
        self.affected_draws
            .begin(old_draw_len.max(canvas.draw_records.len()));

        if canvas.text_glyphs.len() < self.glyph_runs.len() {
            for &run in &self.glyph_runs[canvas.text_glyphs.len()..] {
                add_run_draws(&self.run_draws, run, &mut self.affected_draws);
            }
        }
        self.glyph_runs.resize(canvas.text_glyphs.len(), u32::MAX);

        if canvas.text_runs.len() < self.run_glyph_ranges.len() {
            for run in canvas.text_runs.len()..self.run_glyph_ranges.len() {
                for &draw in &self.run_draws[run] {
                    self.affected_draws.insert(draw);
                }
                let range = self.run_glyph_ranges[run].clone();
                for glyph in
                    range.start.min(self.glyph_runs.len())..range.end.min(self.glyph_runs.len())
                {
                    if self.glyph_runs[glyph] == run as u32 {
                        self.glyph_runs[glyph] = u32::MAX;
                    }
                }
            }
        }
        self.run_draws.truncate(canvas.text_runs.len());
        self.run_glyph_ranges.truncate(canvas.text_runs.len());
        self.run_draws
            .resize_with(canvas.text_runs.len(), HashSet::new);
        self.run_glyph_ranges.resize(canvas.text_runs.len(), 0..0);

        for run in indices_from_ranges(run_changes, canvas.text_runs.len()) {
            for &draw in &self.run_draws[run] {
                self.affected_draws.insert(draw);
            }
            let old = self.run_glyph_ranges[run].clone();
            for glyph in old.start.min(self.glyph_runs.len())..old.end.min(self.glyph_runs.len()) {
                if self.glyph_runs[glyph] == run as u32 {
                    self.glyph_runs[glyph] = u32::MAX;
                }
            }
            let new = run_glyph_range(canvas.text_runs[run], canvas.text_glyphs.len());
            self.glyph_runs[new.clone()].fill(run as u32);
            self.run_glyph_ranges[run] = new;
        }

        if canvas.draw_records.len() < self.draw_runs.len() {
            for draw in canvas.draw_records.len()..self.draw_runs.len() {
                self.total -= self.capacities[draw];
                if let Some(run) = self.draw_runs[draw]
                    && let Some(draws) = self.run_draws.get_mut(run as usize)
                {
                    draws.remove(&draw);
                }
            }
        }
        self.draw_records.truncate(canvas.draw_records.len());
        self.draw_runs.resize(canvas.draw_records.len(), None);
        self.capacities.resize(canvas.draw_records.len(), 0);
        let changed_draw_ranges =
            changed_ranges(Some(draw_changes), old_draw_len, canvas.draw_records.len());
        let changed_draws = indices_from_ranges(&changed_draw_ranges, canvas.draw_records.len());
        for draw in changed_draws {
            self.affected_draws.insert(draw);
            replace_or_push(&mut self.draw_records, draw, canvas.draw_records[draw]);
            if let Some(run) = self.draw_runs[draw]
                && let Some(draws) = self.run_draws.get_mut(run as usize)
            {
                draws.remove(&draw);
            }
            let run = canvas.draw_records[draw].glyph_run_id();
            self.draw_runs[draw] = run;
            if let Some(run) = run.filter(|run| (*run as usize) < self.run_draws.len()) {
                self.run_draws[run as usize].insert(draw);
            }
        }

        for glyph in indices_from_ranges(glyph_changes, canvas.text_glyphs.len()) {
            add_run_draws(
                &self.run_draws,
                self.glyph_runs[glyph],
                &mut self.affected_draws,
            );
        }
        for index in 0..self.affected_draws.len() {
            let draw = self.affected_draws.get(index);
            if draw >= canvas.draw_records.len() {
                continue;
            }
            self.total -= self.capacities[draw];
            self.capacities[draw] = coarse_glyph_capacity_for_draw(canvas, Some(text), draw);
            self.total += self.capacities[draw];
        }
    }

    #[cfg(any(test, feature = "bench-internals"))]
    fn update_incremental(
        &mut self,
        canvas: &Canvas,
        text: &PreparedTextData,
        changes: &SceneBufferChanges,
    ) {
        self.update_ranges(
            canvas,
            text,
            &changes.glyphs,
            &changes.text_runs,
            &changes.draws,
        );
    }
}

fn add_run_draws(run_draws: &[HashSet<usize>], run: u32, affected: &mut DenseIndexSet) {
    if let Some(run_draws) = run_draws.get(run as usize) {
        for &draw in run_draws {
            affected.insert(draw);
        }
    }
}

fn run_glyph_range(run: crate::text::TextRun, glyph_len: usize) -> std::ops::Range<usize> {
    let start = (run.glyph_start as usize).min(glyph_len);
    let end = (run.glyph_start.saturating_add(run.glyph_count) as usize).min(glyph_len);
    start..end
}

#[cfg(test)]
pub(crate) mod tests;
