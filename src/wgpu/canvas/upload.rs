use crate::{
    canvas::Canvas,
    shared::{
        draw_record::DrawRecord,
        execution::{ExecPlan, LayerStackEntry},
        gpu_brush::GpuBrushUpload,
        gpu_coarse::LayerStackRecord,
        gpu_coarse::coarse_work_tile_draw_record_word_offset,
        gpu_plan::{
            GpuCumsumPlan, GpuScanChunk, GpuScanChunkRange, TileDrawBins, build_cumsum_plan_into,
            build_scan_chunks_into, build_tile_draw_bins_into,
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
use super::{WgpuCoarseBuffers, WgpuSceneBuffers};

#[derive(Default)]
pub(crate) struct WgpuSceneUploadStaging {
    u32s: Vec<u32>,
    draw_records: Vec<DrawRecord>,
    text: TextUpload,
    scan_chunks: Vec<GpuScanChunk>,
    scan_chunk_ranges: Vec<GpuScanChunkRange>,
    cumsum_plan: GpuCumsumPlan,
    tile_draw_bins: TileDrawBins,
    tile_draw_cursors: Vec<u32>,
    tile_draw_data: Vec<u32>,
    layer_stack: Vec<LayerStackRecord>,
    scene_brush_blob: Vec<u32>,
    fine_text_blob: Vec<u32>,
}

#[derive(Default)]
struct TextUpload {
    runs: Vec<GlyphRunRecord>,
    glyphs: Vec<GlyphRecord>,
    images: Vec<GlyphImageRecord>,
    image_data: Vec<u32>,
    atlas_signature: AtlasSignature,
    atlas_dirty: bool,
}

impl TextUpload {
    fn refill(
        &mut self,
        canvas: &Canvas,
        text: Option<&PreparedTextData>,
        current_atlas_signature: AtlasSignature,
    ) {
        self.clear();
        let Some(text) = text else {
            return;
        };

        self.runs
            .extend(canvas.text_runs.iter().map(|run| GlyphRunRecord {
                glyph_start: run.glyph_start,
                glyph_count: run.glyph_count,
            }));
        self.glyphs.reserve(canvas.text_glyphs.len());
        for glyph in &canvas.text_glyphs {
            self.glyphs.push(GlyphRecord {
                image_id: text
                    .image_id_for_cache_key(glyph.cache_key)
                    .unwrap_or(u32::MAX),
                x: glyph.x,
                y: glyph.y,
            });
        }

        let atlas_signature = text.atlas_signature();
        self.atlas_dirty = atlas_signature != current_atlas_signature;
        self.atlas_signature = atlas_signature;
        for image in text.images() {
            let data_offset = self.image_data.len() as u32;
            let content = match image.content {
                PreparedGlyphContent::Mask => {
                    let content = match image.composite_mode {
                        TextCompositeMode::Srgb => GPU_GLYPH_MASK,
                        TextCompositeMode::Linear => GPU_GLYPH_LINEAR_MASK,
                    };
                    self.image_data
                        .extend(image.data.iter().map(|&alpha| alpha as u32));
                    content
                }
                PreparedGlyphContent::Color => {
                    let content = match image.composite_mode {
                        TextCompositeMode::Srgb => GPU_GLYPH_COLOR,
                        TextCompositeMode::Linear => GPU_GLYPH_LINEAR_COLOR,
                    };
                    for pixel in image.data.chunks_exact(4) {
                        let a = pixel[3];
                        self.image_data.push(rgba8_pack([
                            mul_div255(pixel[0], a),
                            mul_div255(pixel[1], a),
                            mul_div255(pixel[2], a),
                            a,
                        ]));
                    }
                    content
                }
                PreparedGlyphContent::SubpixelMask => {
                    let content = match image.composite_mode {
                        TextCompositeMode::Srgb => GPU_GLYPH_SUBPIXEL_MASK,
                        TextCompositeMode::Linear => GPU_GLYPH_LINEAR_SUBPIXEL_MASK,
                    };
                    for pixel in image.data.chunks_exact(3) {
                        self.image_data
                            .push(rgba8_pack([pixel[0], pixel[1], pixel[2], 0]));
                    }
                    content
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

    fn clear(&mut self) {
        self.runs.clear();
        self.glyphs.clear();
        self.images.clear();
        self.image_data.clear();
        self.atlas_signature = AtlasSignature::default();
        self.atlas_dirty = false;
    }
}

fn upload_mapped_u32<T>(
    device: &::wgpu::Device,
    queue: &::wgpu::Queue,
    buffer: &mut WgpuBuffer,
    label: &'static str,
    scratch: &mut Vec<u32>,
    items: &[T],
    map: impl FnMut(&T) -> u32,
) {
    scratch.clear();
    scratch.reserve(items.len());
    scratch.extend(items.iter().map(map));
    buffer.upload(device, queue, label, scratch);
}

fn upload_coarse_text_blob(
    device: &::wgpu::Device,
    queue: &::wgpu::Queue,
    buffer: &mut WgpuBuffer,
    scratch: &mut Vec<u32>,
    text: &TextUpload,
) {
    scratch.clear();
    scratch.reserve(text_blob_word_len(
        text.runs.len(),
        text.glyphs.len(),
        text.images.len(),
    ));
    scratch.extend_from_slice(bytemuck::cast_slice(&text.runs));
    scratch.extend_from_slice(bytemuck::cast_slice(&text.glyphs));
    scratch.extend_from_slice(bytemuck::cast_slice(&text.images));
    buffer.upload(
        device,
        queue,
        "tileink wgpu canvas coarse text blob",
        scratch,
    );
}

fn upload_fine_text_blob(
    device: &::wgpu::Device,
    queue: &::wgpu::Queue,
    buffer: &mut WgpuBuffer,
    blob: &mut Vec<u32>,
    text: &TextUpload,
) -> (u32, u32) {
    let image_base = text.glyphs.len() * std::mem::size_of::<GlyphRecord>() / 4;
    let image_data_base =
        image_base + text.images.len() * std::mem::size_of::<GlyphImageRecord>() / 4;
    blob.clear();
    blob.reserve(image_data_base + text.image_data.len());
    blob.extend_from_slice(bytemuck::cast_slice(&text.glyphs));
    blob.extend_from_slice(bytemuck::cast_slice(&text.images));
    blob.extend_from_slice(&text.image_data);
    buffer.upload(device, queue, "tileink wgpu canvas fine text blob", blob);
    (image_base as u32, image_data_base as u32)
}

fn encode_layer_payload(entry: LayerStackEntry) -> u32 {
    match entry {
        LayerStackEntry::Clip { .. } => 0,
        LayerStackEntry::Opacity { opacity, .. } => opacity_f32_to_u8(opacity) as u32,
        LayerStackEntry::Blend { mode, .. } => mode.mix as u32 | ((mode.compose as u32) << 8),
    }
}

impl WgpuSceneBuffers {
    pub(crate) fn upload_image_resources(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        upload: &GpuImageResourceUpload,
    ) {
        self.image_resource_metadata.upload(
            device,
            queue,
            "tileink wgpu image resource metadata",
            &upload.metadata,
        );
        self.image_resource_pixels.upload(
            device,
            queue,
            "tileink wgpu image resource pixels",
            &upload.pixels,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn upload(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        canvas: &Canvas,
        plan: &ExecPlan,
        text: Option<&PreparedTextData>,
        image_resources: Option<&GpuImageResourceUpload>,
        staging: &mut WgpuSceneUploadStaging,
    ) {
        // Keep canvas upload profiling split between CPU-side plan construction and queue uploads.
        profile_cpu("prepare.upload_scene.build_scan_chunks", || {
            build_scan_chunks_into(
                canvas,
                &mut staging.scan_chunks,
                &mut staging.scan_chunk_ranges,
            );
        });
        profile_cpu("prepare.upload_scene.build_cumsum_plan", || {
            build_cumsum_plan_into(canvas, &mut staging.cumsum_plan);
        });
        profile_cpu("prepare.upload_scene.build_tile_draw_bins", || {
            build_tile_draw_bins_into(
                canvas,
                &mut staging.tile_draw_bins,
                &mut staging.tile_draw_cursors,
            );
        });
        profile_cpu("prepare.upload_scene.upload_draw_records", || {
            self.upload_draw_records(device, queue, &canvas.draw_records, text.is_some(), staging);
        });
        profile_cpu("prepare.upload_scene.upload_scene_records", || {
            self.upload_scene_records(device, queue, canvas, image_resources, staging);
        });
        profile_cpu("prepare.upload_scene.upload_layer_stack", || {
            self.upload_plan_layer_stack(device, queue, &plan.layer_stack_data, staging);
        });
        profile_cpu("prepare.upload_scene.upload_text", || {
            self.upload_text(device, queue, canvas, text, staging);
        });
        profile_cpu("prepare.upload_scene.upload_scan_plan", || {
            self.upload_scan_plan(device, queue, staging);
        });
        profile_cpu("prepare.upload_scene.upload_cumsum_plan", || {
            self.upload_cumsum_plan(device, queue, staging);
        });
    }

    fn upload_scene_records(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        canvas: &Canvas,
        image_resources: Option<&GpuImageResourceUpload>,
        staging: &mut WgpuSceneUploadStaging,
    ) {
        profile_cpu("prepare.upload_scene.records.geometry", || {
            self.lines
                .upload(device, queue, "tileink wgpu canvas lines", &canvas.lines);
            self.path_records.upload(
                device,
                queue,
                "tileink wgpu canvas path records",
                &canvas.path_records,
            );
        });
        profile_cpu("prepare.upload_scene.upload_sdf_blobs", || {
            self.sdf_blob.upload(
                device,
                queue,
                "tileink wgpu canvas sdf blob",
                &canvas.sdf_blob,
            );
            self.sdf_shadow_blob.upload(
                device,
                queue,
                "tileink wgpu canvas sdf shadow blob",
                &canvas.sdf_shadow_blob,
            );
        });
        profile_cpu("prepare.upload_scene.blobs.brushes", || {
            let scene_brush_blob = if GpuBrushUpload::scene_brushes_need_resource_patch(
                &canvas.draw_records,
                &canvas.brush_blob,
            ) {
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
            self.brush_blob.upload(
                device,
                queue,
                "tileink wgpu canvas brush blob",
                scene_brush_blob,
            );
        });
    }

    fn upload_draw_records(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        draw_records: &[DrawRecord],
        text_enabled: bool,
        staging: &mut WgpuSceneUploadStaging,
    ) {
        if text_enabled {
            self.draw_records.upload(
                device,
                queue,
                "tileink wgpu canvas draw records",
                draw_records,
            );
            return;
        }

        staging.draw_records.clear();
        staging.draw_records.extend_from_slice(draw_records);
        for draw in &mut staging.draw_records {
            draw.glyph_run_id = DrawRecord::NONE;
        }
        self.draw_records.upload(
            device,
            queue,
            "tileink wgpu canvas draw records",
            &staging.draw_records,
        );
    }

    fn upload_plan_layer_stack(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        layer_stack: &[LayerStackEntry],
        staging: &mut WgpuSceneUploadStaging,
    ) {
        staging.layer_stack.clear();
        staging.layer_stack.reserve(layer_stack.len());
        staging
            .layer_stack
            .extend(layer_stack.iter().map(|entry| LayerStackRecord {
                tag: match entry {
                    LayerStackEntry::Clip { .. } => GPU_LAYER_CLIP,
                    LayerStackEntry::Opacity { .. } => GPU_LAYER_OPACITY,
                    LayerStackEntry::Blend { .. } => GPU_LAYER_BLEND,
                },
                draw: match *entry {
                    LayerStackEntry::Clip { draw }
                    | LayerStackEntry::Opacity { draw, .. }
                    | LayerStackEntry::Blend { draw, .. } => draw,
                },
                payload: encode_layer_payload(*entry),
            }));
        self.plan_layer_stack.upload(
            device,
            queue,
            "tileink wgpu canvas plan layer stack",
            &staging.layer_stack,
        );
    }

    fn upload_text(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        canvas: &Canvas,
        text: Option<&PreparedTextData>,
        staging: &mut WgpuSceneUploadStaging,
    ) {
        profile_cpu("prepare.upload_scene.text.refill", || {
            staging
                .text
                .refill(canvas, text, self.glyph_atlas_signature);
        });
        profile_cpu("prepare.upload_scene.text.runs", || {
            self.text_runs.upload(
                device,
                queue,
                "tileink wgpu canvas text runs",
                &staging.text.runs,
            );
            upload_coarse_text_blob(
                device,
                queue,
                &mut self.coarse_text_blob,
                &mut staging.u32s,
                &staging.text,
            );
            let (image_base, image_data_base) = upload_fine_text_blob(
                device,
                queue,
                &mut self.fine_text_blob,
                &mut staging.fine_text_blob,
                &staging.text,
            );
            self.fine_text_image_base = image_base;
            self.fine_text_image_data_base = image_data_base;
        });
        if text.is_none() {
            self.glyph_atlas_signature = AtlasSignature::default();
            return;
        }
        if !staging.text.atlas_dirty {
            return;
        }

        self.glyph_atlas_signature = staging.text.atlas_signature;
    }

    fn upload_scan_plan(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        staging: &mut WgpuSceneUploadStaging,
    ) {
        upload_mapped_u32(
            device,
            queue,
            &mut self.scan_chunk_path_ids,
            "tileink wgpu canvas scan chunk path ids",
            &mut staging.u32s,
            &staging.scan_chunks,
            |chunk| chunk.path_id,
        );
        upload_mapped_u32(
            device,
            queue,
            &mut self.scan_chunk_backdrop_offsets,
            "tileink wgpu canvas scan chunk backdrop offsets",
            &mut staging.u32s,
            &staging.scan_chunks,
            |chunk| chunk.backdrop_offset,
        );
        upload_mapped_u32(
            device,
            queue,
            &mut self.scan_chunk_segment_starts,
            "tileink wgpu canvas scan chunk segment starts",
            &mut staging.u32s,
            &staging.scan_chunks,
            |chunk| chunk.segment_start,
        );
        upload_mapped_u32(
            device,
            queue,
            &mut self.scan_chunk_lens,
            "tileink wgpu canvas scan chunk lens",
            &mut staging.u32s,
            &staging.scan_chunks,
            |chunk| chunk.len,
        );
        upload_mapped_u32(
            device,
            queue,
            &mut self.scan_chunk_range_starts,
            "tileink wgpu canvas scan chunk range starts",
            &mut staging.u32s,
            &staging.scan_chunk_ranges,
            |range| range.start,
        );
        upload_mapped_u32(
            device,
            queue,
            &mut self.scan_chunk_range_ends,
            "tileink wgpu canvas scan chunk range ends",
            &mut staging.u32s,
            &staging.scan_chunk_ranges,
            |range| range.end,
        );
    }

    fn upload_cumsum_plan(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        staging: &WgpuSceneUploadStaging,
    ) {
        self.cumsum_chunk_backdrop_offsets.upload(
            device,
            queue,
            "tileink wgpu canvas cumsum chunk backdrop offsets",
            &staging.cumsum_plan.chunk_backdrop_offsets,
        );
        self.cumsum_chunk_lens.upload(
            device,
            queue,
            "tileink wgpu canvas cumsum chunk lens",
            &staging.cumsum_plan.chunk_lens,
        );
        self.cumsum_row_chunk_starts.upload(
            device,
            queue,
            "tileink wgpu canvas cumsum row chunk starts",
            &staging.cumsum_plan.row_chunk_starts,
        );
        self.cumsum_row_chunk_ends.upload(
            device,
            queue,
            "tileink wgpu canvas cumsum row chunk ends",
            &staging.cumsum_plan.row_chunk_ends,
        );
    }
}

impl WgpuCoarseBuffers {
    pub(crate) fn upload_tile_draw_bins(
        &self,
        queue: &::wgpu::Queue,
        lengths: crate::shared::gpu_plan::GpuBufferLengths,
        staging: &mut WgpuSceneUploadStaging,
    ) {
        let bins = &staging.tile_draw_bins;
        staging.tile_draw_data.clear();
        staging
            .tile_draw_data
            .extend_from_slice(bytemuck::cast_slice(&bins.records));
        staging.tile_draw_data.extend_from_slice(&bins.draw_indices);
        let word_offset = coarse_work_tile_draw_record_word_offset(
            lengths.tile_count,
            lengths.coarse_ptcl_capacity,
            lengths.coarse_glyph_capacity,
        );
        self.work.write_at(
            queue,
            (word_offset * std::mem::size_of::<u32>()) as ::wgpu::BufferAddress,
            &staging.tile_draw_data,
        );
    }
}
