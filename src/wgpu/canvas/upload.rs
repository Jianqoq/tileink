use std::collections::HashSet;

use crate::{
    canvas::{Canvas, SceneBufferChanges},
    shared::{
        draw_record::DrawRecord,
        execution::{ExecPlan, LayerStackEntry},
        gpu_brush::GpuBrushUpload,
        gpu_coarse::LayerStackRecord,
        gpu_coarse::{
            coarse_work_tile_draw_index_word_offset, coarse_work_tile_draw_record_word_offset,
        },
        gpu_plan::{
            GpuBufferLengths, GpuCumsumPlan, GpuLengthOverrides, PersistentPathPlans,
            TILE_DRAW_PAGE_WORDS, TileDrawBins, coarse_glyph_capacity_for_draw,
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
    text::{AtlasSignature, PreparedGlyphContent, PreparedTextData, TextCompositeMode},
};

use super::super::buffer::WgpuBuffer;
use super::super::profile::profile_cpu;
use super::{
    WgpuCoarseBuffers, WgpuSceneBuffers, create_image_resource_atlas_texture,
    create_image_resource_atlas_view, create_image_resource_texture,
};

#[derive(Default)]
pub(crate) struct WgpuSceneUploadStaging {
    text: TextUpload,
    path_plans: PersistentPathPlans,
    glyph_capacity: GlyphCapacityCache,
    coarse_ptcl_capacity: usize,
    coarse_glyph_capacity: usize,
    tile_draw_bins: TileDrawBins,
    tile_draw_cursors: Vec<u32>,
    layer_stack: Vec<LayerStackRecord>,
    scene_brush_blob: Vec<u32>,
    paint_blob: Vec<u32>,
    paint_layout: (usize, usize, usize),
}

impl WgpuSceneUploadStaging {
    pub(crate) fn active_batch_ids(&self, tiles: &[u32], draw_batch_ids: &[u32]) -> HashSet<u32> {
        self.tile_draw_bins.active_batch_ids(tiles, draw_batch_ids)
    }

    pub(crate) fn build_lengths(
        &mut self,
        canvas: &Canvas,
        text: Option<&PreparedTextData>,
        plan: &ExecPlan,
        reused_plan: bool,
        cached_stack_depths: Option<(usize, usize)>,
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
        let glyph_capacity =
            self.glyph_capacity
                .update(canvas, text, canvas.buffer_changes.as_ref());
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
            lengths.coarse_ptcl_capacity =
                stable_work_capacity(&mut self.coarse_ptcl_capacity, lengths.coarse_ptcl_capacity);
            lengths.coarse_glyph_capacity = stable_work_capacity(
                &mut self.coarse_glyph_capacity,
                lengths.coarse_glyph_capacity,
            );
        } else {
            self.coarse_ptcl_capacity = lengths.coarse_ptcl_capacity;
            self.coarse_glyph_capacity = lengths.coarse_glyph_capacity;
        }
        lengths
    }
}

fn stable_work_capacity(capacity: &mut usize, live: usize) -> usize {
    if live > *capacity {
        *capacity = live.saturating_add(live / 2).max(live);
    } else if live == 0 {
        *capacity = 0;
    } else if capacity.saturating_mul(10) > live.saturating_mul(18) {
        // A persistent shrink past the 1.8x bound is an explicit work-arena compaction. Small
        // shape oscillations retain their offsets, while mass deletion releases excess memory.
        *capacity = live.saturating_add(live / 2).max(live);
    }
    *capacity
}

#[derive(Default)]
struct GlyphCapacityCache {
    capacities: Vec<usize>,
    draw_runs: Vec<Option<u32>>,
    run_draws: Vec<HashSet<usize>>,
    run_glyph_ranges: Vec<std::ops::Range<usize>>,
    glyph_runs: Vec<u32>,
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
    ) -> usize {
        let Some(text) = text else {
            *self = Self::default();
            return 0;
        };
        let tiles_size = (canvas.width_in_tiles(), canvas.height_in_tiles());
        let resource_only_change = text.atlas_signature() != self.atlas_signature
            && changes.is_some_and(|changes| changes.glyphs.is_empty());
        if !self.initialized
            || changes.is_none()
            || self.tiles_size != tiles_size
            || resource_only_change
        {
            self.rebuild(canvas, text);
            return self.total;
        }
        self.update_incremental(canvas, text, changes.unwrap());
        self.atlas_signature = text.atlas_signature();
        self.total
    }

    fn rebuild(&mut self, canvas: &Canvas, text: &PreparedTextData) {
        self.capacities.clear();
        self.capacities.resize(canvas.draw_records.len(), 0);
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

    fn update_incremental(
        &mut self,
        canvas: &Canvas,
        text: &PreparedTextData,
        changes: &SceneBufferChanges,
    ) {
        let mut affected = HashSet::new();

        if canvas.text_glyphs.len() < self.glyph_runs.len() {
            for &run in &self.glyph_runs[canvas.text_glyphs.len()..] {
                self.add_run_draws(run, &mut affected);
            }
        }
        self.glyph_runs.resize(canvas.text_glyphs.len(), u32::MAX);

        if canvas.text_runs.len() < self.run_glyph_ranges.len() {
            for run in canvas.text_runs.len()..self.run_glyph_ranges.len() {
                affected.extend(self.run_draws[run].iter().copied());
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

        let changed_runs = indices_from_ranges(&changes.text_runs, canvas.text_runs.len());
        for run in changed_runs {
            affected.extend(self.run_draws[run].iter().copied());
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
        let old_draw_len = self.draw_runs.len();
        self.draw_runs.resize(canvas.draw_records.len(), None);
        self.capacities.resize(canvas.draw_records.len(), 0);
        let mut changed_draws = indices_from_ranges(&changes.draws, canvas.draw_records.len());
        changed_draws.extend(old_draw_len..canvas.draw_records.len());
        changed_draws.sort_unstable();
        changed_draws.dedup();
        for draw in changed_draws {
            affected.insert(draw);
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

        for glyph in indices_from_ranges(&changes.glyphs, canvas.text_glyphs.len()) {
            self.add_run_draws(self.glyph_runs[glyph], &mut affected);
        }
        for draw in affected {
            if draw >= canvas.draw_records.len() {
                continue;
            }
            self.total -= self.capacities[draw];
            self.capacities[draw] = coarse_glyph_capacity_for_draw(canvas, Some(text), draw);
            self.total += self.capacities[draw];
        }
    }

    fn add_run_draws(&self, run: u32, draws: &mut HashSet<usize>) {
        if let Some(run_draws) = self.run_draws.get(run as usize) {
            draws.extend(run_draws.iter().copied());
        }
    }
}

fn run_glyph_range(run: crate::text::TextRun, glyph_len: usize) -> std::ops::Range<usize> {
    let start = (run.glyph_start as usize).min(glyph_len);
    let end = (run.glyph_start.saturating_add(run.glyph_count) as usize).min(glyph_len);
    start..end
}

fn indices_from_ranges(ranges: &[std::ops::Range<usize>], len: usize) -> Vec<usize> {
    let mut indices = ranges
        .iter()
        .flat_map(|range| range.start.min(len)..range.end.min(len))
        .collect::<Vec<_>>();
    indices.sort_unstable();
    indices.dedup();
    indices
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
        let incremental = changes.is_some();
        self.runs
            .resize(canvas.text_runs.len(), GlyphRunRecord::default());
        self.glyphs
            .resize(canvas.text_glyphs.len(), GlyphRecord::default());
        let run_ranges = changed_ranges(
            changes.map(|changes| changes.text_runs.as_slice()),
            old_run_len,
            self.runs.len(),
        );
        let glyph_ranges = changed_ranges(
            changes.map(|changes| changes.glyphs.as_slice()),
            old_glyph_len,
            self.glyphs.len(),
        );
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
    let mut result = ranges
        .iter()
        .map(|range| range.start.min(new_len)..range.end.min(new_len))
        .filter(|range| !range.is_empty())
        .collect::<Vec<_>>();
    if new_len > old_len {
        result.push(old_len..new_len);
    }
    result.sort_unstable_by_key(|range| range.start);
    result
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

fn upload_coarse_text_blob(
    device: &::wgpu::Device,
    queue: &::wgpu::Queue,
    buffer: &mut WgpuBuffer,
    text: &TextUpload,
) -> usize {
    buffer.upload_ranges(
        device,
        queue,
        "tileink wgpu canvas coarse text blob",
        &text.coarse_blob,
        &text.dirty_coarse,
    )
}

fn upload_fine_text_blob(
    device: &::wgpu::Device,
    queue: &::wgpu::Queue,
    buffer: &mut WgpuBuffer,
    text: &TextUpload,
) -> (u32, u32, usize) {
    let uploaded = buffer.upload_ranges(
        device,
        queue,
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
                }
                self.image_resource_textures[index] = create_image_resource_texture(
                    device,
                    "tileink wgpu image resource texture",
                    texture.width,
                    texture.height,
                );
                self.image_resource_texture_views[index] = self.image_resource_textures[index]
                    .create_view(&::wgpu::TextureViewDescriptor::default());
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
                        changes
                            .draws
                            .iter()
                            .cloned()
                            .chain(changes.painter.iter().cloned())
                            .collect()
                    });
                bytes += self.draw_batch_ids.upload_ranges(
                    device,
                    queue,
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
            self.upload_text(device, queue, canvas, text, staging)
        });
        let path_dirty = staging.path_plans.take_dirty();
        uploaded += profile_cpu("prepare.upload_scene.upload_scan_plan", || {
            self.upload_scan_plan(device, queue, &staging.path_plans, &path_dirty)
        });
        uploaded += profile_cpu("prepare.upload_scene.upload_cumsum_plan", || {
            self.upload_cumsum_plan(device, queue, staging.path_plans.cumsum_plan(), &path_dirty)
        });
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
                    "tileink wgpu canvas lines",
                    &canvas.lines,
                    &changes.lines,
                ) + self.path_records.upload_ranges(
                    device,
                    queue,
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
            let patches_resources = image_resources.is_some_and(|resources| !resources.is_empty())
                && GpuBrushUpload::scene_brushes_need_resource_patch(
                    &canvas.draw_records,
                    &canvas.brush_blob,
                );
            let scene_brush_blob = if patches_resources {
                staging.scene_brush_blob.clear();
                staging
                    .scene_brush_blob
                    .extend_from_slice(&canvas.brush_blob);
                GpuBrushUpload::patch_scene_brush_blob(
                    &mut staging.scene_brush_blob,
                    &canvas.draw_records,
                    image_resources,
                );
                &staging.scene_brush_blob
            } else {
                &canvas.brush_blob
            };

            self.paint_sdf_shadow_base = canvas.sdf_blob.len() as u32;
            self.paint_brush_base = (canvas.sdf_blob.len() + canvas.sdf_shadow_blob.len()) as u32;
            // SDF, SDF-shadow, and scene brushes share one storage buffer so coarse, fine,
            // and filter bind the same paint data. Keeping one staging vector
            // also lets retained uploads transmit only the changed range.
            let layout = (
                canvas.sdf_blob.len(),
                canvas.sdf_shadow_blob.len(),
                scene_brush_blob.len(),
            );
            let shadow_base = layout.0;
            let brush_base = layout.0 + layout.1;
            let incremental = !patches_resources
                && staging.paint_layout == layout
                && canvas.buffer_changes.is_some();
            let ranges = if incremental {
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
            } else {
                staging.paint_blob.clear();
                staging.paint_blob.extend_from_slice(&canvas.sdf_blob);
                staging
                    .paint_blob
                    .extend_from_slice(&canvas.sdf_shadow_blob);
                staging.paint_blob.extend_from_slice(scene_brush_blob);
                staging.paint_layout = layout;
                std::iter::once(0..staging.paint_blob.len()).collect()
            };
            self.paint_blob.upload_ranges(
                device,
                queue,
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
    ) -> usize {
        profile_cpu("prepare.upload_scene.text.refill", || {
            staging.text.refill(
                canvas,
                text,
                self.glyph_atlas_signature,
                canvas.buffer_changes.as_ref(),
            );
        });
        let uploaded = profile_cpu("prepare.upload_scene.text.runs", || {
            let mut uploaded = self.text_runs.upload_ranges(
                device,
                queue,
                "tileink wgpu canvas text runs",
                &staging.text.runs,
                &staging.text.dirty_runs,
            );
            uploaded +=
                upload_coarse_text_blob(device, queue, &mut self.coarse_text_blob, &staging.text);
            let (image_base, image_data_base, fine_uploaded) =
                upload_fine_text_blob(device, queue, &mut self.fine_text_blob, &staging.text);
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
            "tileink wgpu canvas scan chunks",
            plans.scan_chunks(),
            &dirty.scan_chunks,
        ) + self.scan_chunk_ranges.upload_ranges(
            device,
            queue,
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
            "tileink wgpu canvas cumsum chunk backdrop offsets",
            &plan.chunk_backdrop_offsets,
            &dirty.cumsum_chunks,
        ) + self.cumsum_chunk_lens.upload_ranges(
            device,
            queue,
            "tileink wgpu canvas cumsum chunk lens",
            &plan.chunk_lens,
            &dirty.cumsum_chunks,
        ) + self.cumsum_row_chunk_starts.upload_ranges(
            device,
            queue,
            "tileink wgpu canvas cumsum row chunk starts",
            &plan.row_chunk_starts,
            &dirty.cumsum_rows,
        ) + self.cumsum_row_chunk_ends.upload_ranges(
            device,
            queue,
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
        let (bins_full, dirty_records, dirty_pages) = staging.tile_draw_bins.take_dirty();
        let bins = &staging.tile_draw_bins;
        let layout = (
            self.work.generation(),
            record_word_offset,
            index_word_offset,
            lengths.tile_draw_index_count,
        );
        let full = bins_full || self.tile_bin_layout != Some(layout);
        if full {
            self.work
                .write_at(queue, word_offset(record_word_offset), &bins.records);
            self.work
                .write_at(queue, word_offset(index_word_offset), &bins.draw_indices);
        } else {
            for tile in dirty_records {
                self.work.write_at(
                    queue,
                    word_offset(record_word_offset)
                        + (tile * std::mem::size_of::<crate::shared::gpu_coarse::TileDrawRecord>())
                            as u64,
                    &bins.records[tile..tile + 1],
                );
            }
            for page in &dirty_pages {
                let start = *page as usize * TILE_DRAW_PAGE_WORDS;
                self.work.write_at(
                    queue,
                    word_offset(index_word_offset + start),
                    &bins.draw_indices[start..start + TILE_DRAW_PAGE_WORDS],
                );
            }
        }
        self.tile_bin_layout = Some(layout);
        let rewritten = if full {
            bins.active_page_count()
        } else {
            dirty_pages.len()
        };
        (rewritten, bins.compactions())
    }
}

#[cfg(test)]
mod tests {
    use super::grow_image_resource_atlas_capacity;

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
