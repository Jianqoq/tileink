use std::collections::HashSet;

use crate::{
    canvas::{Canvas, SceneBufferChanges},
    shared::{
        dense_set::DenseIndexSet,
        draw_record::DrawRecord,
        execution::{ExecPlan, LayerStackEntry},
        gpu_brush::GpuBrushUpload,
        gpu_coarse::LayerStackRecord,
        gpu_coarse::{
            coarse_work_tile_draw_index_word_offset, coarse_work_tile_draw_record_word_offset,
        },
        gpu_plan::{
            CoarseBinningStats, GpuBufferLengths, GpuCumsumPlan, GpuLengthOverrides,
            GpuScanChunkRange, PersistentPathPlans, TILE_DRAW_PAGE_WORDS, TileDrawBins,
            coarse_glyph_capacity_for_draw,
        },
        gpu_text::{GlyphImageRecord, GlyphRecord, GlyphRunRecord, text_blob_word_len},
        gpu_types::{
            GPU_GLYPH_COLOR, GPU_GLYPH_LINEAR_COLOR, GPU_GLYPH_LINEAR_MASK,
            GPU_GLYPH_LINEAR_SUBPIXEL_MASK, GPU_GLYPH_MASK, GPU_GLYPH_SUBPIXEL_MASK,
            GPU_LAYER_BLEND, GPU_LAYER_CLIP, GPU_LAYER_OPACITY,
        },
        image::rgba8_pack,
        image_resource::GpuImageResourceUpload,
        pixel::{mul_div255, opacity_f32_to_u8},
    },
    text::{
        AtlasSignature, PreparedGlyphContent, PreparedTextChanges, PreparedTextData,
        TextCompositeMode,
    },
};

use super::super::buffer::WgpuBuffer;
use super::super::profile::profile_cpu;
use super::{
    WgpuCoarseBuffers, WgpuSceneBuffers, create_image_resource_atlas_texture,
    create_image_resource_atlas_view, create_image_resource_texture,
};

#[cfg(feature = "bench-internals")]
mod benchmark;
#[cfg(feature = "bench-internals")]
pub use benchmark::{GlyphCapacityBenchmark, GlyphCapacityBenchmarkCase};

#[derive(Default)]
pub(crate) struct WgpuSceneUploadStaging {
    text: TextUpload,
    path_plans: PersistentPathPlans,
    glyph_capacity: GlyphCapacityCache,
    coarse_ptcl_capacity: usize,
    coarse_ptcl_underused_frames: u16,
    coarse_glyph_capacity: usize,
    coarse_glyph_underused_frames: u16,
    tile_draw_bins: TileDrawBins,
    tile_draw_cursors: Vec<u32>,
    layer_stack: Vec<LayerStackRecord>,
    scene_brush_blob: Vec<u32>,
    resource_brush_draws: Vec<bool>,
    resource_brush_draw_count: usize,
    resource_brush_draws_initialized: bool,
    image_resource_generation: Option<u64>,
    paint_blob: Vec<u32>,
    paint_layout: (usize, usize, usize),
}

impl WgpuSceneUploadStaging {
    pub(crate) fn coarse_binning_stats(&self, tiles: &[u32]) -> CoarseBinningStats {
        self.tile_draw_bins.coarse_binning_stats(tiles)
    }

    pub(crate) fn scan_ranges(&self) -> &[GpuScanChunkRange] {
        self.path_plans.scan_ranges()
    }

    fn update_resource_brush_draws(&mut self, canvas: &Canvas) -> bool {
        let full = !self.resource_brush_draws_initialized
            || canvas.buffer_changes.is_none()
            || canvas
                .buffer_changes
                .as_ref()
                .is_some_and(|changes| changes.full_scene_sync);
        if full {
            self.resource_brush_draws.clear();
            self.resource_brush_draws
                .resize(canvas.draw_records.len(), false);
            self.resource_brush_draw_count = 0;
            for (index, draw) in canvas.draw_records.iter().enumerate() {
                let resource = GpuBrushUpload::draw_uses_resource_brush(draw, &canvas.brush_blob);
                self.resource_brush_draws[index] = resource;
                self.resource_brush_draw_count += resource as usize;
            }
            self.resource_brush_draws_initialized = true;
        } else {
            if self.resource_brush_draws.len() > canvas.draw_records.len() {
                self.resource_brush_draw_count -= self.resource_brush_draws
                    [canvas.draw_records.len()..]
                    .iter()
                    .filter(|resource| **resource)
                    .count();
                self.resource_brush_draws
                    .truncate(canvas.draw_records.len());
            } else {
                self.resource_brush_draws
                    .resize(canvas.draw_records.len(), false);
            }
            for index in canvas
                .buffer_changes
                .as_ref()
                .unwrap()
                .draws
                .iter()
                .flat_map(|range| {
                    range.start.min(canvas.draw_records.len())
                        ..range.end.min(canvas.draw_records.len())
                })
            {
                let resource = GpuBrushUpload::draw_uses_resource_brush(
                    &canvas.draw_records[index],
                    &canvas.brush_blob,
                );
                if self.resource_brush_draws[index] != resource {
                    if resource {
                        self.resource_brush_draw_count += 1;
                    } else {
                        self.resource_brush_draw_count -= 1;
                    }
                    self.resource_brush_draws[index] = resource;
                }
            }
        }
        self.resource_brush_draw_count != 0
    }

    pub(crate) fn active_batch_ids(&mut self, tiles: &[u32], draw_batch_ids: &[u32]) -> Vec<u32> {
        self.tile_draw_bins.active_batch_ids(tiles, draw_batch_ids)
    }

    pub(crate) fn draws_in_bounds(
        &self,
        bounds: crate::shared::bounds::Bounds,
        plan: &ExecPlan,
    ) -> Vec<u32> {
        self.tile_draw_bins
            .draws_in_bounds(bounds, &plan.draw_order)
    }

    pub(crate) fn build_lengths(
        &mut self,
        canvas: &Canvas,
        text: Option<&PreparedTextData>,
        plan: &ExecPlan,
        reused_plan: bool,
        cached_stack_depths: Option<(usize, usize)>,
        flat_text_changes: Option<&PreparedTextChanges>,
    ) -> GpuBufferLengths {
        // Lengths and tile draw bins must describe the same scene; building both here avoids
        // recounting every draw/tile intersection later in prepare.
        let path_counts = self.path_plans.update(
            canvas,
            canvas
                .buffer_changes
                .as_ref()
                .map(|changes| changes.paths.as_slice()),
        );
        let glyph_capacity = self.glyph_capacity.update(
            canvas,
            text,
            canvas.buffer_changes.as_ref(),
            flat_text_changes,
        );
        let mut lengths = GpuBufferLengths::from_scene_with_text_and_tile_draw_bins(
            canvas,
            text,
            plan,
            &mut self.tile_draw_bins,
            &mut self.tile_draw_cursors,
            reused_plan,
            GpuLengthOverrides {
                path_plan_counts: Some(path_counts),
                coarse_glyph_capacity: Some(glyph_capacity),
                cached_stack_depths,
                ..Default::default()
            },
        );
        if canvas.buffer_changes.is_some() {
            lengths.coarse_ptcl_capacity = stable_work_capacity(
                &mut self.coarse_ptcl_capacity,
                &mut self.coarse_ptcl_underused_frames,
                lengths.coarse_ptcl_capacity,
            );
            lengths.coarse_glyph_capacity = stable_work_capacity(
                &mut self.coarse_glyph_capacity,
                &mut self.coarse_glyph_underused_frames,
                lengths.coarse_glyph_capacity,
            );
        } else {
            self.coarse_ptcl_capacity = lengths.coarse_ptcl_capacity;
            self.coarse_ptcl_underused_frames = 0;
            self.coarse_glyph_capacity = lengths.coarse_glyph_capacity;
            self.coarse_glyph_underused_frames = 0;
        }
        lengths
    }
}

const WORK_CAPACITY_SHRINK_DELAY: u16 = 120;

fn stable_work_capacity(capacity: &mut usize, underused_frames: &mut u16, live: usize) -> usize {
    if live > *capacity {
        *capacity = live.saturating_add(live / 2).max(live);
        *underused_frames = 0;
    } else if capacity.saturating_mul(10) > live.saturating_mul(18) {
        // Shrinking immediately makes alternating layer depth or glyph workloads move every
        // following work-buffer section twice per pair of frames. Require sustained low usage so
        // temporary topology changes retain stable offsets while genuinely smaller scenes still
        // release excess capacity.
        *underused_frames = underused_frames.saturating_add(1);
        if *underused_frames >= WORK_CAPACITY_SHRINK_DELAY {
            *capacity = live.saturating_add(live / 2).max(live);
            *underused_frames = 0;
        }
    } else {
        *underused_frames = 0;
    }
    *capacity
}

fn grow_paint_layout(
    current: (usize, usize, usize),
    required: (usize, usize, usize),
) -> (usize, usize, usize) {
    let grow = |capacity: usize, live: usize| {
        if live == 0 {
            0
        } else if live > capacity || capacity.saturating_mul(10) > live.saturating_mul(18) {
            live.checked_div(256)
                .and_then(|pages| pages.checked_add(1))
                .and_then(|pages| pages.checked_mul(256))
                .unwrap_or(usize::MAX)
        } else {
            capacity
        }
    };
    (
        grow(current.0, required.0),
        grow(current.1, required.1),
        grow(current.2, required.2),
    )
}

#[derive(Default)]
struct GlyphCapacityCache {
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
    fn update(
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

fn replace_or_push<T>(values: &mut Vec<T>, index: usize, value: T) {
    if index < values.len() {
        values[index] = value;
    } else {
        debug_assert_eq!(index, values.len());
        values.push(value);
    }
}

fn push_dirty_index(ranges: &mut Vec<std::ops::Range<usize>>, index: usize) {
    if let Some(last) = ranges.last_mut()
        && last.end == index
    {
        last.end += 1;
    } else {
        ranges.push(index..index + 1);
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

fn indices_from_ranges(
    ranges: &[std::ops::Range<usize>],
    len: usize,
) -> impl Iterator<Item = usize> + '_ {
    debug_assert!(ranges.windows(2).all(|pair| pair[0].end < pair[1].start));
    ranges
        .iter()
        .flat_map(move |range| range.start.min(len)..range.end.min(len))
}

#[derive(Default)]
struct TextUpload {
    runs: Vec<GlyphRunRecord>,
    glyphs: Vec<GlyphRecord>,
    images: Vec<GlyphImageRecord>,
    image_data: Vec<u32>,
    atlas_signature: AtlasSignature,
    atlas_dirty: bool,
    coarse_blob: Vec<u32>,
    fine_blob: Vec<u32>,
    dirty_runs: Vec<std::ops::Range<usize>>,
    dirty_coarse: Vec<std::ops::Range<usize>>,
    dirty_fine: Vec<std::ops::Range<usize>>,
    fine_image_base: u32,
    fine_image_data_base: u32,
}

impl TextUpload {
    fn refill(
        &mut self,
        canvas: &Canvas,
        text: Option<&PreparedTextData>,
        current_atlas_signature: AtlasSignature,
        changes: Option<&SceneBufferChanges>,
        flat_changes: Option<&PreparedTextChanges>,
    ) {
        let Some(text) = text else {
            self.clear();
            return;
        };
        self.dirty_runs.clear();
        self.dirty_coarse.clear();
        self.dirty_fine.clear();
        let old_run_len = self.runs.len();
        let old_glyph_len = self.glyphs.len();
        let incremental = changes.is_some() || flat_changes.is_some();
        let changed_runs = changes
            .map(|changes| changes.text_runs.as_slice())
            .or_else(|| flat_changes.map(PreparedTextChanges::runs));
        let changed_glyphs = changes
            .map(|changes| changes.glyphs.as_slice())
            .or_else(|| flat_changes.map(PreparedTextChanges::glyphs));
        self.runs
            .resize(canvas.text_runs.len(), GlyphRunRecord::default());
        self.glyphs
            .resize(canvas.text_glyphs.len(), GlyphRecord::default());
        let run_ranges = changed_ranges(changed_runs, old_run_len, self.runs.len());
        let glyph_ranges = changed_ranges(changed_glyphs, old_glyph_len, self.glyphs.len());
        for range in &run_ranges {
            for index in range.clone() {
                let run = canvas.text_runs[index];
                self.runs[index] = GlyphRunRecord {
                    glyph_start: run.glyph_start,
                    glyph_count: run.glyph_count,
                };
            }
        }
        for range in &glyph_ranges {
            for index in range.clone() {
                let glyph = canvas.text_glyphs[index];
                self.glyphs[index] = GlyphRecord {
                    image_id: text
                        .image_id_for_cache_key(glyph.cache_key)
                        .unwrap_or(u32::MAX),
                    x: glyph.x,
                    y: glyph.y,
                };
            }
        }
        self.dirty_runs.extend(run_ranges.iter().cloned());
        let atlas_signature = text.atlas_signature();
        self.atlas_dirty = atlas_signature != current_atlas_signature;
        self.atlas_signature = atlas_signature;
        if self.atlas_dirty {
            self.rebuild_images(text);
        }

        let layout_changed = !incremental
            || old_run_len != self.runs.len()
            || old_glyph_len != self.glyphs.len()
            || self.atlas_dirty;
        if layout_changed {
            self.rebuild_blobs();
        } else {
            let run_words = std::mem::size_of::<GlyphRunRecord>() / 4;
            let glyph_words = std::mem::size_of::<GlyphRecord>() / 4;
            let coarse_glyph_base = self.runs.len() * run_words;
            patch_pod_ranges(
                &mut self.coarse_blob,
                0,
                &self.runs,
                &run_ranges,
                &mut self.dirty_coarse,
            );
            patch_pod_ranges(
                &mut self.coarse_blob,
                coarse_glyph_base,
                &self.glyphs,
                &glyph_ranges,
                &mut self.dirty_coarse,
            );
            patch_pod_ranges(
                &mut self.fine_blob,
                0,
                &self.glyphs,
                &glyph_ranges,
                &mut self.dirty_fine,
            );
            debug_assert_eq!(glyph_words, 3);
        }
    }

    fn rebuild_images(&mut self, text: &PreparedTextData) {
        self.images.clear();
        self.image_data.clear();
        for image in text.images() {
            let data_offset = self.image_data.len() as u32;
            let content = match image.content {
                PreparedGlyphContent::Mask => {
                    self.image_data
                        .extend(image.data.iter().map(|&alpha| alpha as u32));
                    match image.composite_mode {
                        TextCompositeMode::Srgb => GPU_GLYPH_MASK,
                        TextCompositeMode::Linear => GPU_GLYPH_LINEAR_MASK,
                    }
                }
                PreparedGlyphContent::Color => {
                    for pixel in image.data.chunks_exact(4) {
                        let a = pixel[3];
                        self.image_data.push(rgba8_pack([
                            mul_div255(pixel[0], a),
                            mul_div255(pixel[1], a),
                            mul_div255(pixel[2], a),
                            a,
                        ]));
                    }
                    match image.composite_mode {
                        TextCompositeMode::Srgb => GPU_GLYPH_COLOR,
                        TextCompositeMode::Linear => GPU_GLYPH_LINEAR_COLOR,
                    }
                }
                PreparedGlyphContent::SubpixelMask => {
                    for pixel in image.data.chunks_exact(3) {
                        self.image_data
                            .push(rgba8_pack([pixel[0], pixel[1], pixel[2], 0]));
                    }
                    match image.composite_mode {
                        TextCompositeMode::Srgb => GPU_GLYPH_SUBPIXEL_MASK,
                        TextCompositeMode::Linear => GPU_GLYPH_LINEAR_SUBPIXEL_MASK,
                    }
                }
            };
            self.images.push(GlyphImageRecord {
                left: image.left,
                top: image.top,
                width: image.width,
                height: image.height,
                content,
                data_offset,
            });
        }
    }

    fn rebuild_blobs(&mut self) {
        self.coarse_blob.clear();
        self.coarse_blob.reserve(text_blob_word_len(
            self.runs.len(),
            self.glyphs.len(),
            self.images.len(),
        ));
        self.coarse_blob
            .extend_from_slice(bytemuck::cast_slice(&self.runs));
        self.coarse_blob
            .extend_from_slice(bytemuck::cast_slice(&self.glyphs));
        self.coarse_blob
            .extend_from_slice(bytemuck::cast_slice(&self.images));
        self.dirty_coarse.push(0..self.coarse_blob.len());

        self.fine_image_base = (self.glyphs.len() * std::mem::size_of::<GlyphRecord>() / 4) as u32;
        self.fine_image_data_base = self.fine_image_base
            + (self.images.len() * std::mem::size_of::<GlyphImageRecord>() / 4) as u32;
        self.fine_blob.clear();
        self.fine_blob
            .reserve(self.fine_image_data_base as usize + self.image_data.len());
        self.fine_blob
            .extend_from_slice(bytemuck::cast_slice(&self.glyphs));
        self.fine_blob
            .extend_from_slice(bytemuck::cast_slice(&self.images));
        self.fine_blob.extend_from_slice(&self.image_data);
        self.dirty_fine.push(0..self.fine_blob.len());
    }

    fn clear(&mut self) {
        self.runs.clear();
        self.glyphs.clear();
        self.images.clear();
        self.image_data.clear();
        self.atlas_signature = AtlasSignature::default();
        self.atlas_dirty = false;
        self.coarse_blob.clear();
        self.fine_blob.clear();
        self.dirty_runs.clear();
        self.dirty_coarse.clear();
        self.dirty_fine.clear();
        self.fine_image_base = 0;
        self.fine_image_data_base = 0;
    }
}

fn changed_ranges(
    ranges: Option<&[std::ops::Range<usize>]>,
    old_len: usize,
    new_len: usize,
) -> Vec<std::ops::Range<usize>> {
    let Some(ranges) = ranges else {
        return (!new_len.eq(&0))
            .then_some(0..new_len)
            .into_iter()
            .collect();
    };
    let result = ranges
        .iter()
        .map(|range| range.start.min(new_len)..range.end.min(new_len))
        .filter(|range| !range.is_empty())
        .collect::<Vec<_>>();
    if new_len > old_len {
        merge_sorted_dirty_ranges(&result, std::slice::from_ref(&(old_len..new_len)))
    } else {
        result
    }
}

fn patch_pod_ranges<T: bytemuck::Pod>(
    blob: &mut [u32],
    word_base: usize,
    values: &[T],
    ranges: &[std::ops::Range<usize>],
    dirty: &mut Vec<std::ops::Range<usize>>,
) {
    let words_per_item = std::mem::size_of::<T>() / 4;
    let words: &[u32] = bytemuck::cast_slice(values);
    for range in ranges {
        let source = range.start * words_per_item..range.end * words_per_item;
        let target = source.start + word_base..source.end + word_base;
        blob[target.clone()].copy_from_slice(&words[source]);
        dirty.push(target);
    }
}

fn patch_u32_ranges(
    target: &mut [u32],
    base: usize,
    source: &[u32],
    ranges: &[std::ops::Range<usize>],
) {
    for range in ranges {
        target[range.start + base..range.end + base].copy_from_slice(&source[range.clone()]);
    }
}

fn word_offset(words: usize) -> ::wgpu::BufferAddress {
    words as ::wgpu::BufferAddress * std::mem::size_of::<u32>() as ::wgpu::BufferAddress
}

fn contiguous_index_runs(indices: impl IntoIterator<Item = usize>) -> Vec<std::ops::Range<usize>> {
    let mut indices = indices.into_iter();
    let Some(first) = indices.next() else {
        return Vec::new();
    };
    let mut runs = Vec::new();
    let (mut start, mut end) = (first, first + 1);
    for index in indices {
        if index == end {
            end += 1;
        } else {
            runs.push(start..end);
            start = index;
            end = index + 1;
        }
    }
    runs.push(start..end);
    runs
}

fn merge_sorted_dirty_ranges(
    left: &[std::ops::Range<usize>],
    right: &[std::ops::Range<usize>],
) -> Vec<std::ops::Range<usize>> {
    let mut merged = Vec::<std::ops::Range<usize>>::with_capacity(left.len() + right.len());
    let (mut left_index, mut right_index) = (0, 0);
    while left_index < left.len() || right_index < right.len() {
        let range = if right_index == right.len()
            || (left_index < left.len() && left[left_index].start <= right[right_index].start)
        {
            let range = left[left_index].clone();
            left_index += 1;
            range
        } else {
            let range = right[right_index].clone();
            right_index += 1;
            range
        };
        if range.is_empty() {
            continue;
        }
        if let Some(previous) = merged.last_mut()
            && range.start <= previous.end
        {
            previous.end = previous.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    merged
}

fn upload_coarse_text_blob(
    device: &::wgpu::Device,
    queue: &::wgpu::Queue,
    scatter: &mut super::super::buffer::WgpuRangeScatter,
    buffer: &mut WgpuBuffer,
    text: &TextUpload,
) -> usize {
    buffer.upload_ranges(
        device,
        queue,
        scatter,
        "tileink wgpu canvas coarse text blob",
        &text.coarse_blob,
        &text.dirty_coarse,
    )
}

fn upload_fine_text_blob(
    device: &::wgpu::Device,
    queue: &::wgpu::Queue,
    scatter: &mut super::super::buffer::WgpuRangeScatter,
    buffer: &mut WgpuBuffer,
    text: &TextUpload,
) -> (u32, u32, usize) {
    let uploaded = buffer.upload_ranges(
        device,
        queue,
        scatter,
        "tileink wgpu canvas fine text blob",
        &text.fine_blob,
        &text.dirty_fine,
    );
    (text.fine_image_base, text.fine_image_data_base, uploaded)
}

fn encode_layer_payload(entry: LayerStackEntry) -> u32 {
    match entry {
        LayerStackEntry::Clip { .. } => 0,
        LayerStackEntry::Opacity { opacity, .. } => opacity_f32_to_u8(opacity) as u32,
        LayerStackEntry::Blend { mode, .. } => mode.mix as u32 | ((mode.compose as u32) << 8),
    }
}

fn layer_stack_record(entry: LayerStackEntry) -> LayerStackRecord {
    LayerStackRecord {
        tag: match entry {
            LayerStackEntry::Clip { .. } => GPU_LAYER_CLIP,
            LayerStackEntry::Opacity { .. } => GPU_LAYER_OPACITY,
            LayerStackEntry::Blend { .. } => GPU_LAYER_BLEND,
        },
        draw: match entry {
            LayerStackEntry::Clip { draw }
            | LayerStackEntry::Opacity { draw, .. }
            | LayerStackEntry::Blend { draw, .. } => draw,
        },
        payload: encode_layer_payload(entry),
    }
}

impl WgpuSceneBuffers {
    pub(crate) fn upload_image_resources(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        upload: &GpuImageResourceUpload,
        force_all: bool,
    ) {
        let page_size = upload.atlas_page_size().max(1);
        let page_count = upload.atlas_page_count().max(1);
        let atlas_capacity = grow_image_resource_atlas_capacity(
            self.image_resource_atlas_size,
            (page_size, page_size, page_count),
            device.limits().max_texture_dimension_2d,
            device.limits().max_texture_array_layers,
        );
        if self.image_resource_atlas_size != atlas_capacity {
            self.image_resource_atlas = create_image_resource_atlas_texture(
                device,
                atlas_capacity.0,
                atlas_capacity.1,
                atlas_capacity.2,
            );
            self.image_resource_atlas_view =
                create_image_resource_atlas_view(&self.image_resource_atlas);
            self.image_resource_atlas_size = atlas_capacity;
            self.image_resource_binding_generation =
                self.image_resource_binding_generation.wrapping_add(1);
        }
        for page in upload.atlas_pages() {
            if !force_all && !page.dirty {
                continue;
            }
            if page.pixels.is_empty() {
                continue;
            }
            queue.write_texture(
                ::wgpu::TexelCopyTextureInfo {
                    texture: &self.image_resource_atlas,
                    mip_level: 0,
                    origin: ::wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: page.index,
                    },
                    aspect: ::wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(&page.pixels),
                ::wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(page.size * std::mem::size_of::<u32>() as u32),
                    rows_per_image: Some(page.size),
                },
                ::wgpu::Extent3d {
                    width: page.size,
                    height: page.size,
                    depth_or_array_layers: 1,
                },
            );
        }
        self.upload_image_resource_textures(device, queue, upload, force_all);
    }

    fn upload_image_resource_textures(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        upload: &GpuImageResourceUpload,
        force_all: bool,
    ) {
        for texture in upload.textures() {
            let index = texture.index as usize;
            let recreate = self
                .image_resource_textures
                .get(index)
                .is_none_or(|existing| {
                    let size = existing.size();
                    size.width != texture.width || size.height != texture.height
                });
            if recreate {
                while self.image_resource_textures.len() <= index {
                    self.image_resource_textures
                        .push(create_image_resource_texture(
                            device,
                            "tileink wgpu image resource texture",
                            1,
                            1,
                        ));
                    self.image_resource_texture_views.push(
                        self.image_resource_textures
                            .last()
                            .expect("pushed texture")
                            .create_view(&::wgpu::TextureViewDescriptor::default()),
                    );
                    self.image_resource_binding_generation =
                        self.image_resource_binding_generation.wrapping_add(1);
                }
                self.image_resource_textures[index] = create_image_resource_texture(
                    device,
                    "tileink wgpu image resource texture",
                    texture.width,
                    texture.height,
                );
                self.image_resource_texture_views[index] = self.image_resource_textures[index]
                    .create_view(&::wgpu::TextureViewDescriptor::default());
                self.image_resource_binding_generation =
                    self.image_resource_binding_generation.wrapping_add(1);
            }
            if force_all || texture.dirty || recreate {
                queue.write_texture(
                    self.image_resource_textures[index].as_image_copy(),
                    bytemuck::cast_slice(&texture.pixels),
                    ::wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(texture.width * std::mem::size_of::<u32>() as u32),
                        rows_per_image: Some(texture.height),
                    },
                    ::wgpu::Extent3d {
                        width: texture.width,
                        height: texture.height,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn upload(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        canvas: &Canvas,
        _lengths: GpuBufferLengths,
        plan: &ExecPlan,
        text: Option<&PreparedTextData>,
        image_resources: Option<&GpuImageResourceUpload>,
        staging: &mut WgpuSceneUploadStaging,
        upload_plan: bool,
        flat_text_changes: Option<&PreparedTextChanges>,
    ) -> usize {
        let mut uploaded = 0;
        // Keep canvas upload profiling split between CPU-side plan construction and queue uploads.
        uploaded += profile_cpu("prepare.upload_scene.upload_draw_records", || {
            let mut bytes = self.upload_draw_records(
                device,
                queue,
                &canvas.draw_records,
                canvas
                    .buffer_changes
                    .as_ref()
                    .map(|changes| changes.draws.as_slice()),
            );
            if let Some(batch_ids) = &canvas.stable_batch_ids {
                let ranges = canvas
                    .buffer_changes
                    .as_ref()
                    .map_or_else(Vec::new, |changes| {
                        merge_sorted_dirty_ranges(&changes.draws, &changes.painter)
                    });
                bytes += self.draw_batch_ids.upload_ranges(
                    device,
                    queue,
                    &mut self.range_scatter,
                    "tileink wgpu canvas stable draw batch ids",
                    batch_ids,
                    &ranges,
                );
            } else if upload_plan {
                bytes += self.draw_batch_ids.upload_cached(
                    device,
                    queue,
                    "tileink wgpu canvas draw batch ids",
                    &plan.draw_batch_ids,
                );
            }
            bytes
        });
        uploaded += profile_cpu("prepare.upload_scene.upload_scene_records", || {
            self.upload_scene_records(device, queue, canvas, image_resources, staging)
        });
        if upload_plan {
            uploaded += profile_cpu("prepare.upload_scene.upload_layer_stack", || {
                self.upload_plan_layer_stack(device, queue, &plan.layer_stack_data, staging)
            });
        } else if let Some(ranges) = canvas
            .buffer_changes
            .as_ref()
            .map(|changes| changes.plan_layer_stack.as_slice())
            .filter(|ranges| !ranges.is_empty())
        {
            uploaded += profile_cpu("prepare.upload_scene.upload_layer_stack", || {
                self.upload_plan_layer_stack_ranges(
                    device,
                    queue,
                    &plan.layer_stack_data,
                    ranges,
                    staging,
                )
            });
        }
        uploaded += profile_cpu("prepare.upload_scene.upload_text", || {
            self.upload_text(device, queue, canvas, text, staging, flat_text_changes)
        });
        let path_dirty = staging.path_plans.take_dirty();
        uploaded += profile_cpu("prepare.upload_scene.upload_scan_plan", || {
            self.upload_scan_plan(device, queue, &staging.path_plans, &path_dirty)
        });
        uploaded += profile_cpu("prepare.upload_scene.upload_cumsum_plan", || {
            self.upload_cumsum_plan(device, queue, staging.path_plans.cumsum_plan(), &path_dirty)
        });
        staging.path_plans.recycle_dirty(path_dirty);
        self.range_scatter.submit(queue);
        uploaded
    }

    fn upload_scene_records(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        canvas: &Canvas,
        image_resources: Option<&GpuImageResourceUpload>,
        staging: &mut WgpuSceneUploadStaging,
    ) -> usize {
        let geometry = profile_cpu("prepare.upload_scene.records.geometry", || {
            if let Some(changes) = &canvas.buffer_changes {
                self.lines.upload_ranges(
                    device,
                    queue,
                    &mut self.range_scatter,
                    "tileink wgpu canvas lines",
                    &canvas.lines,
                    &changes.lines,
                ) + self.path_records.upload_ranges(
                    device,
                    queue,
                    &mut self.range_scatter,
                    "tileink wgpu canvas path records",
                    &canvas.path_records,
                    &changes.paths,
                )
            } else {
                self.lines
                    .upload_cached(device, queue, "tileink wgpu canvas lines", &canvas.lines)
                    + self.path_records.upload_cached(
                        device,
                        queue,
                        "tileink wgpu canvas path records",
                        &canvas.path_records,
                    )
            }
        });
        let paint = profile_cpu("prepare.upload_scene.upload_paint_blob", || {
            let has_image_resources =
                image_resources.is_some_and(|resources| !resources.is_empty());
            let patches_resources =
                has_image_resources && staging.update_resource_brush_draws(canvas);
            if !has_image_resources {
                // Draw mutations while no image table exists are intentionally not tracked. The
                // first later resource insertion rebuilds membership once, then resumes journal
                // updates without charging image-free scenes for resource bookkeeping.
                staging.resource_brush_draws_initialized = false;
            }
            let resource_generation = image_resources.map(GpuImageResourceUpload::generation);
            let repatch_all_resources =
                patches_resources && staging.image_resource_generation != resource_generation;
            let scene_brush_blob = if patches_resources {
                let full_patch = repatch_all_resources
                    || canvas.buffer_changes.is_none()
                    || staging.scene_brush_blob.len() != canvas.brush_blob.len();
                if full_patch {
                    staging.scene_brush_blob.clear();
                    staging
                        .scene_brush_blob
                        .extend_from_slice(&canvas.brush_blob);
                    GpuBrushUpload::patch_scene_brush_blob(
                        &mut staging.scene_brush_blob,
                        &canvas.draw_records,
                        image_resources,
                    );
                } else {
                    let changes = canvas.buffer_changes.as_ref().unwrap();
                    patch_u32_ranges(
                        &mut staging.scene_brush_blob,
                        0,
                        &canvas.brush_blob,
                        &changes.brushes,
                    );
                    GpuBrushUpload::patch_scene_brush_blob_draw_ranges(
                        &mut staging.scene_brush_blob,
                        &canvas.draw_records,
                        &changes.draws,
                        image_resources,
                    );
                }
                staging.image_resource_generation = resource_generation;
                &staging.scene_brush_blob
            } else {
                staging.image_resource_generation = resource_generation;
                &canvas.brush_blob
            };

            if canvas.buffer_changes.is_none() {
                self.paint_sdf_shadow_base = canvas.sdf_blob.len() as u32;
                self.paint_brush_base =
                    (canvas.sdf_blob.len() + canvas.sdf_shadow_blob.len()) as u32;
                // Immediate canvases have no dirty-allocation journal and are prepared as
                // one-shot contiguous buffers. Writing the three existing slices directly is
                // substantially cheaper than concatenating them, diffing a byte cache, and then
                // discarding that work on the next full frame.
                let words =
                    canvas.sdf_blob.len() + canvas.sdf_shadow_blob.len() + scene_brush_blob.len();
                self.paint_blob.resize_uninit::<u32>(
                    device,
                    "tileink wgpu canvas paint blob",
                    words,
                );
                self.paint_blob.write_at(queue, 0, &canvas.sdf_blob);
                self.paint_blob.write_at(
                    queue,
                    word_offset(self.paint_sdf_shadow_base as usize),
                    &canvas.sdf_shadow_blob,
                );
                self.paint_blob.write_at(
                    queue,
                    word_offset(self.paint_brush_base as usize),
                    scene_brush_blob,
                );
                staging.paint_blob.clear();
                staging.paint_layout = (0, 0, 0);
                return words * std::mem::size_of::<u32>();
            }
            // SDF, SDF-shadow, and scene brushes share one storage buffer so coarse, fine,
            // and filter bind the same paint data. Keeping one staging vector
            // also lets retained uploads transmit only the changed range.
            let required = (
                canvas.sdf_blob.len(),
                canvas.sdf_shadow_blob.len(),
                scene_brush_blob.len(),
            );
            let layout = grow_paint_layout(staging.paint_layout, required);
            let shadow_base = layout.0;
            let brush_base = layout.0 + layout.1;
            self.paint_sdf_shadow_base = shadow_base as u32;
            self.paint_brush_base = brush_base as u32;
            let relayout = staging.paint_layout != layout
                || staging.paint_blob.len() != layout.0 + layout.1 + layout.2;
            let ranges = if relayout {
                staging.paint_blob.clear();
                staging.paint_blob.resize(layout.0 + layout.1 + layout.2, 0);
                staging.paint_blob[..required.0].copy_from_slice(&canvas.sdf_blob);
                staging.paint_blob[shadow_base..shadow_base + required.1]
                    .copy_from_slice(&canvas.sdf_shadow_blob);
                staging.paint_blob[brush_base..brush_base + required.2]
                    .copy_from_slice(scene_brush_blob);
                staging.paint_layout = layout;
                std::iter::once(0..staging.paint_blob.len()).collect()
            } else if patches_resources {
                let changes = canvas.buffer_changes.as_ref().unwrap();
                patch_u32_ranges(&mut staging.paint_blob, 0, &canvas.sdf_blob, &changes.sdfs);
                patch_u32_ranges(
                    &mut staging.paint_blob,
                    shadow_base,
                    &canvas.sdf_shadow_blob,
                    &changes.shadows,
                );
                if repatch_all_resources {
                    staging.paint_blob[brush_base..brush_base + required.2]
                        .copy_from_slice(scene_brush_blob);
                } else {
                    patch_u32_ranges(
                        &mut staging.paint_blob,
                        brush_base,
                        scene_brush_blob,
                        &changes.brushes,
                    );
                }
                let mut ranges = changes
                    .sdfs
                    .iter()
                    .cloned()
                    .chain(
                        changes
                            .shadows
                            .iter()
                            .map(|range| range.start + shadow_base..range.end + shadow_base),
                    )
                    .collect::<Vec<_>>();
                if repatch_all_resources {
                    ranges.push(brush_base..brush_base + required.2);
                } else {
                    ranges.extend(
                        changes
                            .brushes
                            .iter()
                            .map(|range| range.start + brush_base..range.end + brush_base),
                    );
                }
                ranges
            } else {
                let changes = canvas.buffer_changes.as_ref().unwrap();
                patch_u32_ranges(&mut staging.paint_blob, 0, &canvas.sdf_blob, &changes.sdfs);
                patch_u32_ranges(
                    &mut staging.paint_blob,
                    shadow_base,
                    &canvas.sdf_shadow_blob,
                    &changes.shadows,
                );
                patch_u32_ranges(
                    &mut staging.paint_blob,
                    brush_base,
                    scene_brush_blob,
                    &changes.brushes,
                );
                changes
                    .sdfs
                    .iter()
                    .cloned()
                    .chain(
                        changes
                            .shadows
                            .iter()
                            .map(|range| range.start + shadow_base..range.end + shadow_base),
                    )
                    .chain(
                        changes
                            .brushes
                            .iter()
                            .map(|range| range.start + brush_base..range.end + brush_base),
                    )
                    .collect::<Vec<_>>()
            };
            self.paint_blob.upload_ranges(
                device,
                queue,
                &mut self.range_scatter,
                "tileink wgpu canvas paint blob",
                &staging.paint_blob,
                &ranges,
            )
        });
        geometry + paint
    }

    fn upload_draw_records(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        draw_records: &[DrawRecord],
        ranges: Option<&[std::ops::Range<usize>]>,
    ) -> usize {
        if let Some(ranges) = ranges {
            self.draw_records.upload_ranges(
                device,
                queue,
                &mut self.range_scatter,
                "tileink wgpu canvas draw records",
                draw_records,
                ranges,
            )
        } else {
            self.draw_records.upload_cached(
                device,
                queue,
                "tileink wgpu canvas draw records",
                draw_records,
            )
        }
    }

    fn upload_plan_layer_stack(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        layer_stack: &[LayerStackEntry],
        staging: &mut WgpuSceneUploadStaging,
    ) -> usize {
        staging.layer_stack.clear();
        staging.layer_stack.reserve(layer_stack.len());
        staging
            .layer_stack
            .extend(layer_stack.iter().copied().map(layer_stack_record));
        self.plan_layer_stack.upload_cached(
            device,
            queue,
            "tileink wgpu canvas plan layer stack",
            &staging.layer_stack,
        )
    }

    fn upload_plan_layer_stack_ranges(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        layer_stack: &[LayerStackEntry],
        ranges: &[std::ops::Range<usize>],
        staging: &mut WgpuSceneUploadStaging,
    ) -> usize {
        if staging.layer_stack.len() != layer_stack.len() {
            return self.upload_plan_layer_stack(device, queue, layer_stack, staging);
        }
        for range in ranges {
            for index in range.clone() {
                staging.layer_stack[index] = layer_stack_record(layer_stack[index]);
            }
        }
        self.plan_layer_stack.upload_ranges(
            device,
            queue,
            &mut self.range_scatter,
            "tileink wgpu canvas plan layer stack",
            &staging.layer_stack,
            ranges,
        )
    }

    fn upload_text(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        canvas: &Canvas,
        text: Option<&PreparedTextData>,
        staging: &mut WgpuSceneUploadStaging,
        flat_text_changes: Option<&PreparedTextChanges>,
    ) -> usize {
        profile_cpu("prepare.upload_scene.text.refill", || {
            staging.text.refill(
                canvas,
                text,
                self.glyph_atlas_signature,
                canvas.buffer_changes.as_ref(),
                flat_text_changes,
            );
        });
        let uploaded = profile_cpu("prepare.upload_scene.text.runs", || {
            let mut uploaded = self.text_runs.upload_ranges(
                device,
                queue,
                &mut self.range_scatter,
                "tileink wgpu canvas text runs",
                &staging.text.runs,
                &staging.text.dirty_runs,
            );
            uploaded += upload_coarse_text_blob(
                device,
                queue,
                &mut self.range_scatter,
                &mut self.coarse_text_blob,
                &staging.text,
            );
            let (image_base, image_data_base, fine_uploaded) = upload_fine_text_blob(
                device,
                queue,
                &mut self.range_scatter,
                &mut self.fine_text_blob,
                &staging.text,
            );
            uploaded += fine_uploaded;
            self.fine_text_image_base = image_base;
            self.fine_text_image_data_base = image_data_base;
            uploaded
        });
        if text.is_none() {
            self.glyph_atlas_signature = AtlasSignature::default();
            return uploaded;
        }
        if !staging.text.atlas_dirty {
            return uploaded;
        }

        self.glyph_atlas_signature = staging.text.atlas_signature;
        uploaded
    }

    fn upload_scan_plan(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        plans: &PersistentPathPlans,
        dirty: &crate::shared::gpu_plan::GpuPathPlanDirty,
    ) -> usize {
        // Scan kernels consume the CPU-built AoS plan directly, avoiding per-field packing in prepare.
        self.scan_chunks.upload_ranges(
            device,
            queue,
            &mut self.range_scatter,
            "tileink wgpu canvas scan chunks",
            plans.scan_chunks(),
            &dirty.scan_chunks,
        ) + self.scan_chunk_ranges.upload_ranges(
            device,
            queue,
            &mut self.range_scatter,
            "tileink wgpu canvas scan chunk ranges",
            plans.scan_ranges(),
            &dirty.scan_ranges,
        )
    }

    pub(crate) fn upload_cumsum_plan(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        plan: &GpuCumsumPlan,
        dirty: &crate::shared::gpu_plan::GpuPathPlanDirty,
    ) -> usize {
        self.cumsum_chunk_backdrop_offsets.upload_ranges(
            device,
            queue,
            &mut self.range_scatter,
            "tileink wgpu canvas cumsum chunk backdrop offsets",
            &plan.chunk_backdrop_offsets,
            &dirty.cumsum_chunks,
        ) + self.cumsum_chunk_lens.upload_ranges(
            device,
            queue,
            &mut self.range_scatter,
            "tileink wgpu canvas cumsum chunk lens",
            &plan.chunk_lens,
            &dirty.cumsum_chunks,
        ) + self.cumsum_row_chunk_starts.upload_ranges(
            device,
            queue,
            &mut self.range_scatter,
            "tileink wgpu canvas cumsum row chunk starts",
            &plan.row_chunk_starts,
            &dirty.cumsum_rows,
        ) + self.cumsum_row_chunk_ends.upload_ranges(
            device,
            queue,
            &mut self.range_scatter,
            "tileink wgpu canvas cumsum row chunk ends",
            &plan.row_chunk_ends,
            &dirty.cumsum_rows,
        )
    }
}

fn grow_image_resource_atlas_capacity(
    current: (u32, u32, u32),
    required: (u32, u32, u32),
    max_dimension: u32,
    max_layers: u32,
) -> (u32, u32, u32) {
    (
        grow_image_resource_atlas_axis(current.0, required.0, max_dimension),
        grow_image_resource_atlas_axis(current.1, required.1, max_dimension),
        grow_image_resource_atlas_axis(current.2, required.2, max_layers),
    )
}

fn grow_image_resource_atlas_axis(current: u32, required: u32, max_dimension: u32) -> u32 {
    let required = required.max(1);
    let max_dimension = max_dimension.max(1);
    let mut capacity = current.max(1).min(max_dimension);
    while capacity < required {
        let doubled = capacity.saturating_mul(2).min(max_dimension);
        if doubled <= capacity {
            capacity = required.min(max_dimension);
            break;
        }
        capacity = doubled;
    }
    capacity
}

impl WgpuCoarseBuffers {
    pub(crate) fn upload_tile_draw_bins(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        lengths: crate::shared::gpu_plan::GpuBufferLengths,
        staging: &mut WgpuSceneUploadStaging,
    ) -> (usize, u64) {
        let record_word_offset = coarse_work_tile_draw_record_word_offset(
            lengths.tile_count,
            lengths.coarse_ptcl_capacity,
            lengths.coarse_glyph_capacity,
        );
        let index_word_offset = coarse_work_tile_draw_index_word_offset(
            lengths.tile_count,
            lengths.coarse_ptcl_capacity,
            lengths.coarse_glyph_capacity,
        );
        let (bins_full, dirty_records, dirty_pages) = profile_cpu(
            "prepare.coarse_buffers.upload_tile_draw_bins.take_dirty",
            || staging.tile_draw_bins.take_dirty(),
        );
        let bins = &staging.tile_draw_bins;
        let layout = (
            self.work.generation(),
            record_word_offset,
            index_word_offset,
            lengths.tile_draw_index_count,
        );
        let full = bins_full || self.tile_bin_layout != Some(layout);
        self.tile_bin_staging_words.clear();
        self.pending_tile_bin_copies.clear();
        if full {
            profile_cpu("prepare.coarse_buffers.upload_tile_draw_bins.full", || {
                self.work.write_at(
                    queue,
                    word_offset(record_word_offset),
                    bins.upload_records(),
                );
                self.work
                    .write_at(queue, word_offset(index_word_offset), bins.upload_indices());
            });
        } else {
            profile_cpu(
                "prepare.coarse_buffers.upload_tile_draw_bins.records",
                || {
                    for tiles in contiguous_index_runs(dirty_records.iter().copied()) {
                        let words: &[u32] = bytemuck::cast_slice(&bins.records[tiles.clone()]);
                        let source = self.tile_bin_staging_words.len();
                        self.tile_bin_staging_words.extend_from_slice(words);
                        self.pending_tile_bin_copies.push((
                            word_offset(source),
                            word_offset(record_word_offset)
                                + (tiles.start
                                    * std::mem::size_of::<
                                        crate::shared::gpu_coarse::TileDrawRecord,
                                    >()) as u64,
                            word_offset(words.len()),
                        ));
                    }
                },
            );
            profile_cpu("prepare.coarse_buffers.upload_tile_draw_bins.pages", || {
                for pages in contiguous_index_runs(dirty_pages.iter().map(|page| *page as usize)) {
                    let words =
                        pages.start * TILE_DRAW_PAGE_WORDS..pages.end * TILE_DRAW_PAGE_WORDS;
                    let source = self.tile_bin_staging_words.len();
                    self.tile_bin_staging_words
                        .extend_from_slice(&bins.draw_indices[words.clone()]);
                    self.pending_tile_bin_copies.push((
                        word_offset(source),
                        word_offset(index_word_offset + words.start),
                        word_offset(words.len()),
                    ));
                }
            });
            profile_cpu(
                "prepare.coarse_buffers.upload_tile_draw_bins.staging_upload",
                || {
                    if !self.tile_bin_staging_words.is_empty() {
                        self.tile_bin_staging.upload(
                            device,
                            queue,
                            "tileink wgpu tile bin staging",
                            &self.tile_bin_staging_words,
                        );
                    }
                },
            );
        }
        self.tile_bin_layout = Some(layout);
        let rewritten = if full {
            bins.active_page_count()
        } else {
            dirty_pages.len()
        };
        let compactions = bins.compactions();
        staging
            .tile_draw_bins
            .recycle_dirty(dirty_records, dirty_pages);
        (rewritten, compactions)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GlyphCapacityCache, TextUpload, WORK_CAPACITY_SHRINK_DELAY, changed_ranges,
        contiguous_index_runs, grow_image_resource_atlas_capacity, grow_paint_layout,
        indices_from_ranges, merge_sorted_dirty_ranges, stable_work_capacity,
    };
    use crate::{
        TextContext,
        canvas::{Canvas, SceneBufferChanges},
        shared::{
            affine::GpuAffine,
            bounds::PixelBounds,
            draw_record::{DrawRecord, DrawTag, FillRuleWord},
        },
        text::{AtlasSignature, CanvasGlyph, PreparedTextChanges, PreparedTextData, TextRun},
    };
    use cosmic_text::{CacheKey, CacheKeyFlags, FontSystem};

    fn glyph_draw(run: u32) -> DrawRecord {
        DrawRecord {
            path_id: DrawRecord::NONE,
            glyph_run_id: run,
            sdf_offset: DrawRecord::NONE,
            sdf_len: 0,
            sdf_shadow_offset: DrawRecord::NONE,
            sdf_shadow_len: 0,
            brush_offset: DrawRecord::NONE,
            brush_len: 0,
            tag: DrawTag::Brush.into(),
            fill_rule: FillRuleWord::default(),
            pixel_bounds: PixelBounds {
                x0: 0,
                y0: 0,
                x1: 16,
                y1: 16,
            },
            local_pixel_bounds: PixelBounds {
                x0: 0,
                y0: 0,
                x1: 16,
                y1: 16,
            },
            solid_rect: 0,
            transform: GpuAffine::IDENTITY,
            inverse_transform: GpuAffine::IDENTITY,
        }
    }

    fn glyph_capacity_fixture() -> (GlyphCapacityCache, Canvas, PreparedTextData) {
        let mut canvas = Canvas::new(64, 64, 1.0);
        canvas.text_runs = vec![
            TextRun {
                glyph_start: 0,
                glyph_count: 2,
            },
            TextRun {
                glyph_start: 2,
                glyph_count: 2,
            },
        ];
        let (cache_key, x, y) = CacheKey::new(
            fontdb::ID::dummy(),
            0,
            16.0,
            (0.0, 0.0),
            fontdb::Weight::NORMAL,
            CacheKeyFlags::empty(),
        );
        canvas.text_glyphs = vec![CanvasGlyph { cache_key, x, y }; 4];
        canvas.draw_records = vec![glyph_draw(0), glyph_draw(0), glyph_draw(1)];
        let mut font_system = FontSystem::new();
        let mut context = TextContext::new();
        let text = PreparedTextData::new(&[], &[], &mut font_system, &mut context);
        let mut cache = GlyphCapacityCache::default();
        cache.rebuild(&canvas, &text);
        (cache, canvas, text)
    }

    #[test]
    fn flat_text_changes_patch_only_changed_gpu_glyph_records() {
        let (_, mut canvas, text) = glyph_capacity_fixture();
        let mut upload = TextUpload::default();
        upload.refill(&canvas, Some(&text), AtlasSignature::default(), None, None);
        canvas.text_glyphs[1].x += 5;
        let changes = PreparedTextChanges::from_ranges(std::iter::once(1..2).collect(), Vec::new());

        upload.refill(
            &canvas,
            Some(&text),
            text.atlas_signature(),
            None,
            Some(&changes),
        );

        assert!(upload.dirty_runs.is_empty());
        assert!(!upload.dirty_coarse.is_empty());
        assert!(
            upload
                .dirty_coarse
                .iter()
                .all(|range| range.len() < upload.coarse_blob.len())
        );
        assert!(
            upload
                .dirty_fine
                .iter()
                .all(|range| range.len() < upload.fine_blob.len())
        );
    }

    #[test]
    fn flat_text_changes_update_only_dependent_glyph_capacity_draws() {
        let (mut cache, canvas, text) = glyph_capacity_fixture();
        let changes = PreparedTextChanges::from_ranges(std::iter::once(0..1).collect(), Vec::new());

        cache.update(&canvas, Some(&text), None, Some(&changes));

        assert_eq!(cache.affected_draws.len(), 2);
        assert!(cache.affected_draws.contains(0));
        assert!(cache.affected_draws.contains(1));
        assert!(!cache.affected_draws.contains(2));
    }

    #[test]
    fn flat_draw_diff_coalesces_changed_tail_with_appended_draws() {
        let (cache, mut canvas, _) = glyph_capacity_fixture();
        canvas.draw_records[2].pixel_bounds.x1 += 1;
        canvas.draw_records.push(glyph_draw(1));

        let changes = cache.flat_draw_changes(&canvas);

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0], 2..4);
    }

    #[test]
    fn changed_glyphs_collect_each_dependent_draw_once() {
        let (mut cache, canvas, text) = glyph_capacity_fixture();
        cache.update_incremental(
            &canvas,
            &text,
            &SceneBufferChanges {
                glyphs: std::iter::once(0..2).collect(),
                ..Default::default()
            },
        );

        assert_eq!(cache.affected_draws.len(), 2);
        assert!(cache.affected_draws.contains(0));
        assert!(cache.affected_draws.contains(1));
    }

    #[test]
    fn changed_draw_rebinds_future_glyph_damage_to_its_new_run() {
        let (mut cache, mut canvas, text) = glyph_capacity_fixture();
        canvas.draw_records[0].glyph_run_id = 1;
        cache.update_incremental(
            &canvas,
            &text,
            &SceneBufferChanges {
                draws: std::iter::once(0..1).collect(),
                ..Default::default()
            },
        );
        cache.update_incremental(
            &canvas,
            &text,
            &SceneBufferChanges {
                glyphs: std::iter::once(0..2).collect(),
                ..Default::default()
            },
        );

        assert_eq!(cache.affected_draws.len(), 1);
        assert!(!cache.affected_draws.contains(0));
        assert!(cache.affected_draws.contains(1));
    }

    #[test]
    fn shrinking_text_state_removes_stale_run_and_draw_membership() {
        let (mut cache, mut canvas, text) = glyph_capacity_fixture();
        canvas.text_glyphs.truncate(2);
        canvas.text_runs.truncate(1);
        canvas.draw_records.truncate(2);

        cache.update_incremental(&canvas, &text, &SceneBufferChanges::default());

        assert_eq!(cache.glyph_runs.len(), 2);
        assert_eq!(cache.run_draws.len(), 1);
        assert_eq!(cache.draw_runs.len(), 2);
        assert!(cache.run_draws[0].iter().all(|&draw| draw < 2));
    }

    #[test]
    fn sorted_change_ranges_are_traversed_without_materializing_indices() {
        assert_eq!(
            indices_from_ranges(&[1..3, 5..10], 7).collect::<Vec<_>>(),
            [1, 2, 5, 6]
        );
    }

    #[test]
    fn contiguous_dirty_indices_are_coalesced_without_bridging_gaps() {
        assert_eq!(
            contiguous_index_runs([2, 3, 4, 8, 10, 11]),
            [2..5, 8..9, 10..12]
        );
        assert!(contiguous_index_runs([]).is_empty());
    }

    #[test]
    fn sorted_draw_and_painter_ranges_merge_linearly() {
        assert_eq!(
            merge_sorted_dirty_ranges(&[0..4, 12..16], &[3..8, 20..24]),
            [0..8, 12..16, 20..24]
        );
    }

    #[test]
    fn growing_text_ranges_merge_the_dirty_tail_upstream() {
        let dirty = std::iter::once(2..6).collect::<Vec<_>>();
        let expected = std::iter::once(2..8).collect::<Vec<_>>();
        assert_eq!(changed_ranges(Some(&dirty), 4, 8), expected);
    }

    #[test]
    fn work_capacity_does_not_thrash_under_alternating_layer_depth() {
        let mut capacity = 1_000;
        let mut underused = 0;
        for _ in 0..WORK_CAPACITY_SHRINK_DELAY * 2 {
            assert_eq!(
                stable_work_capacity(&mut capacity, &mut underused, 400),
                1_000
            );
            assert_eq!(
                stable_work_capacity(&mut capacity, &mut underused, 900),
                1_000
            );
            assert_eq!(underused, 0);
        }
    }

    #[test]
    fn work_capacity_releases_sustained_excess_capacity() {
        let mut capacity = 1_000;
        let mut underused = 0;
        for _ in 1..WORK_CAPACITY_SHRINK_DELAY {
            assert_eq!(
                stable_work_capacity(&mut capacity, &mut underused, 400),
                1_000
            );
        }
        assert_eq!(
            stable_work_capacity(&mut capacity, &mut underused, 400),
            600
        );
        assert_eq!(underused, 0);

        assert_eq!(
            stable_work_capacity(&mut capacity, &mut underused, 800),
            1_200
        );
        assert_eq!(underused, 0);
    }

    #[test]
    fn paint_layout_keeps_segment_bases_stable_until_capacity_is_exhausted() {
        let initial = grow_paint_layout((0, 0, 0), (100, 20, 200));
        assert_eq!(initial, (256, 256, 256));
        assert_eq!(grow_paint_layout(initial, (120, 10, 250)), initial);
        assert_eq!(grow_paint_layout(initial, (257, 10, 250)), (512, 256, 256));
        assert_eq!(grow_paint_layout((0, 0, 0), (256, 0, 0)).0, 512);
        assert_eq!(
            grow_paint_layout((1024, 512, 256), (100, 0, 200)),
            (256, 0, 256)
        );
    }

    #[test]
    fn image_resource_atlas_capacity_reuses_existing_texture_when_it_fits() {
        assert_eq!(
            grow_image_resource_atlas_capacity((256, 128, 4), (128, 64, 2), 4096, 256),
            (256, 128, 4)
        );
        assert_eq!(
            grow_image_resource_atlas_capacity((256, 128, 4), (256, 128, 4), 4096, 256),
            (256, 128, 4)
        );
    }

    #[test]
    fn image_resource_atlas_capacity_grows_by_doubling_until_required_size_fits() {
        assert_eq!(
            grow_image_resource_atlas_capacity((256, 128, 4), (257, 129, 5), 4096, 256),
            (512, 256, 8)
        );
        assert_eq!(
            grow_image_resource_atlas_capacity((256, 128, 4), (900, 129, 9), 4096, 256),
            (1024, 256, 16)
        );
    }

    #[test]
    fn image_resource_atlas_capacity_respects_device_limit() {
        assert_eq!(
            grow_image_resource_atlas_capacity((4096, 4096, 128), (5000, 5000, 300), 6000, 256),
            (6000, 6000, 256)
        );
        assert_eq!(
            grow_image_resource_atlas_capacity((1, 1, 1), (0, 0, 0), 4096, 256),
            (1, 1, 1)
        );
    }
}
