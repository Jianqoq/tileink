use crate::render::upload::paint::{PaintData, PaintUploadState};
use crate::render::upload::{
    glyph_capacity::GlyphCapacityCache,
    ranges::{contiguous_index_runs, merge_sorted_dirty_ranges},
    text::TextUpload,
};

use crate::{
    canvas::Canvas,
    shared::{
        draw_record::DrawRecord,
        execution::{ExecPlan, LayerStackEntry},
        gpu_coarse::LayerStackRecord,
        gpu_coarse::{
            coarse_work_tile_draw_index_word_offset, coarse_work_tile_draw_record_word_offset,
        },
        gpu_plan::{
            CoarseBinningStats, GpuBufferLengths, GpuCumsumPlan, GpuLengthOverrides,
            GpuScanChunkRange, PersistentPathPlans, TILE_DRAW_PAGE_WORDS, TileDrawBins,
        },
        gpu_types::{GPU_LAYER_BLEND, GPU_LAYER_CLIP, GPU_LAYER_OPACITY},
        image_resource::GpuImageResourceUpload,
        pixel::opacity_f32_to_u8,
    },
    text::{AtlasSignature, PreparedTextChanges, PreparedTextData},
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
    coarse_ptcl_underused_frames: u16,
    coarse_glyph_capacity: usize,
    coarse_glyph_underused_frames: u16,
    tile_draw_bins: TileDrawBins,
    tile_draw_cursors: Vec<u32>,
    layer_stack: Vec<LayerStackRecord>,
    paint: PaintUploadState,
}

impl WgpuSceneUploadStaging {
    pub(crate) fn coarse_binning_stats(&self, tiles: &[u32]) -> CoarseBinningStats {
        self.tile_draw_bins.coarse_binning_stats(tiles)
    }

    pub(crate) fn scan_ranges(&self) -> &[GpuScanChunkRange] {
        self.path_plans.scan_ranges()
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

fn word_offset(words: usize) -> ::wgpu::BufferAddress {
    words as ::wgpu::BufferAddress * std::mem::size_of::<u32>() as ::wgpu::BufferAddress
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
        let atlas_recreated = self.image_resource_atlas_size != atlas_capacity;
        // Recreating the array loses every old page, including clean CPU pages
        // and GPU-generated vector images. Independent raster textures keep
        // their own dirty/recreation rules; atlas growth must not rewrite them.
        if atlas_recreated {
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
            if !force_all && !atlas_recreated && !page.dirty {
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
        self.prepare_vector_image_upload(upload, force_all || atlas_recreated);
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
            if !texture.pixels.is_empty() && (force_all || texture.dirty || recreate) {
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
            let upload = staging.paint.prepare(canvas, image_resources);
            self.paint_sdf_shadow_base = upload.shadow_base;
            self.paint_brush_base = upload.brush_base;
            match upload.data {
                PaintData::Immediate {
                    sdfs,
                    shadows,
                    brushes,
                } => {
                    let words = sdfs.len() + shadows.len() + brushes.len();
                    self.paint_blob.resize_uninit::<u32>(
                        device,
                        "tileink wgpu canvas paint blob",
                        words,
                    );
                    self.paint_blob.write_at(queue, 0, sdfs);
                    self.paint_blob.write_at(
                        queue,
                        word_offset(upload.shadow_base as usize),
                        shadows,
                    );
                    self.paint_blob.write_at(
                        queue,
                        word_offset(upload.brush_base as usize),
                        brushes,
                    );
                    words * std::mem::size_of::<u32>()
                }
                PaintData::Retained { words, ranges } => self.paint_blob.upload_ranges(
                    device,
                    queue,
                    &mut self.range_scatter,
                    "tileink wgpu canvas paint blob",
                    words,
                    &ranges,
                ),
            }
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
        WORK_CAPACITY_SHRINK_DELAY, grow_image_resource_atlas_capacity, stable_work_capacity,
    };

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
