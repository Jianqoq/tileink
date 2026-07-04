use crate::{
    canvas::Canvas,
    shared::{
        draw_record::DrawRecord,
        execution::{ExecPlan, LayerStackEntry},
        gpu_brush::GpuBrushUpload,
        gpu_plan::{
            GpuCumsumPlan, GpuScanChunk, GpuScanChunkRange, TileDrawBins, build_cumsum_plan_into,
            build_scan_chunks_into, build_tile_draw_bins_into,
        },
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
use super::WgpuSceneBuffers;

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
}

#[derive(Default)]
struct TextUpload {
    run_starts: Vec<u32>,
    run_counts: Vec<u32>,
    glyph_image_ids: Vec<u32>,
    glyph_x: Vec<i32>,
    glyph_y: Vec<i32>,
    image_left: Vec<i32>,
    image_top: Vec<i32>,
    image_width: Vec<u32>,
    image_height: Vec<u32>,
    image_content: Vec<u32>,
    image_data_offsets: Vec<u32>,
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

        self.run_starts
            .extend(canvas.text_runs.iter().map(|run| run.glyph_start));
        self.run_counts
            .extend(canvas.text_runs.iter().map(|run| run.glyph_count));
        self.glyph_image_ids.reserve(canvas.text_glyphs.len());
        self.glyph_x
            .extend(canvas.text_glyphs.iter().map(|glyph| glyph.x));
        self.glyph_y
            .extend(canvas.text_glyphs.iter().map(|glyph| glyph.y));
        for glyph in &canvas.text_glyphs {
            self.glyph_image_ids.push(
                text.image_id_for_cache_key(glyph.cache_key)
                    .unwrap_or(u32::MAX),
            );
        }

        let atlas_signature = text.atlas_signature();
        if atlas_signature == current_atlas_signature {
            return;
        }

        self.atlas_dirty = true;
        self.atlas_signature = atlas_signature;
        for image in text.images() {
            self.image_left.push(image.left);
            self.image_top.push(image.top);
            self.image_width.push(image.width);
            self.image_height.push(image.height);
            self.image_data_offsets.push(self.image_data.len() as u32);
            match image.content {
                PreparedGlyphContent::Mask => {
                    self.image_content.push(match image.composite_mode {
                        TextCompositeMode::Srgb => GPU_GLYPH_MASK,
                        TextCompositeMode::Linear => GPU_GLYPH_LINEAR_MASK,
                    });
                    self.image_data
                        .extend(image.data.iter().map(|&alpha| alpha as u32));
                }
                PreparedGlyphContent::Color => {
                    self.image_content.push(match image.composite_mode {
                        TextCompositeMode::Srgb => GPU_GLYPH_COLOR,
                        TextCompositeMode::Linear => GPU_GLYPH_LINEAR_COLOR,
                    });
                    for pixel in image.data.chunks_exact(4) {
                        let a = pixel[3];
                        self.image_data.push(rgba8_pack([
                            mul_div255(pixel[0], a),
                            mul_div255(pixel[1], a),
                            mul_div255(pixel[2], a),
                            a,
                        ]));
                    }
                }
                PreparedGlyphContent::SubpixelMask => {
                    self.image_content.push(match image.composite_mode {
                        TextCompositeMode::Srgb => GPU_GLYPH_SUBPIXEL_MASK,
                        TextCompositeMode::Linear => GPU_GLYPH_LINEAR_SUBPIXEL_MASK,
                    });
                    for pixel in image.data.chunks_exact(3) {
                        self.image_data
                            .push(rgba8_pack([pixel[0], pixel[1], pixel[2], 0]));
                    }
                }
            }
        }
    }

    fn clear(&mut self) {
        self.run_starts.clear();
        self.run_counts.clear();
        self.glyph_image_ids.clear();
        self.glyph_x.clear();
        self.glyph_y.clear();
        self.image_left.clear();
        self.image_top.clear();
        self.image_width.clear();
        self.image_height.clear();
        self.image_content.clear();
        self.image_data_offsets.clear();
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
            self.upload_scene_records(device, queue, canvas, image_resources);
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
        profile_cpu("prepare.upload_scene.upload_tile_draw_bins", || {
            self.upload_tile_draw_bins(device, queue, staging);
        });
    }

    fn upload_scene_records(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        canvas: &Canvas,
        image_resources: Option<&GpuImageResourceUpload>,
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
            let brush_upload = GpuBrushUpload::from_scene_brush_blob(
                &canvas.draw_records,
                &canvas.brush_blob,
                image_resources,
            );
            self.brush_blob.upload(
                device,
                queue,
                "tileink wgpu canvas brush blob",
                &brush_upload.blob,
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
        upload_mapped_u32(
            device,
            queue,
            &mut self.plan_layer_stack_tags,
            "tileink wgpu canvas plan layer stack tags",
            &mut staging.u32s,
            layer_stack,
            |entry| match entry {
                LayerStackEntry::Clip { .. } => GPU_LAYER_CLIP,
                LayerStackEntry::Opacity { .. } => GPU_LAYER_OPACITY,
                LayerStackEntry::Blend { .. } => GPU_LAYER_BLEND,
            },
        );
        upload_mapped_u32(
            device,
            queue,
            &mut self.plan_layer_stack_draws,
            "tileink wgpu canvas plan layer stack draws",
            &mut staging.u32s,
            layer_stack,
            |entry| match *entry {
                LayerStackEntry::Clip { draw }
                | LayerStackEntry::Opacity { draw, .. }
                | LayerStackEntry::Blend { draw, .. } => draw,
            },
        );
        upload_mapped_u32(
            device,
            queue,
            &mut self.plan_layer_stack_payloads,
            "tileink wgpu canvas plan layer stack payloads",
            &mut staging.u32s,
            layer_stack,
            |entry| encode_layer_payload(*entry),
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
            self.text_run_starts.upload(
                device,
                queue,
                "tileink wgpu canvas text run starts",
                &staging.text.run_starts,
            );
            self.text_run_counts.upload(
                device,
                queue,
                "tileink wgpu canvas text run counts",
                &staging.text.run_counts,
            );
            self.glyph_image_ids.upload(
                device,
                queue,
                "tileink wgpu canvas glyph image ids",
                &staging.text.glyph_image_ids,
            );
            self.glyph_x.upload(
                device,
                queue,
                "tileink wgpu canvas glyph x",
                &staging.text.glyph_x,
            );
            self.glyph_y.upload(
                device,
                queue,
                "tileink wgpu canvas glyph y",
                &staging.text.glyph_y,
            );
        });
        if !staging.text.atlas_dirty {
            return;
        }

        self.glyph_atlas_signature = staging.text.atlas_signature;
        profile_cpu("prepare.upload_scene.text.atlas", || {
            self.glyph_image_left.upload(
                device,
                queue,
                "tileink wgpu canvas glyph image left",
                &staging.text.image_left,
            );
            self.glyph_image_top.upload(
                device,
                queue,
                "tileink wgpu canvas glyph image top",
                &staging.text.image_top,
            );
            self.glyph_image_width.upload(
                device,
                queue,
                "tileink wgpu canvas glyph image width",
                &staging.text.image_width,
            );
            self.glyph_image_height.upload(
                device,
                queue,
                "tileink wgpu canvas glyph image height",
                &staging.text.image_height,
            );
            self.glyph_image_content.upload(
                device,
                queue,
                "tileink wgpu canvas glyph image content",
                &staging.text.image_content,
            );
            self.glyph_image_data_offsets.upload(
                device,
                queue,
                "tileink wgpu canvas glyph image data offsets",
                &staging.text.image_data_offsets,
            );
            self.glyph_image_data.upload(
                device,
                queue,
                "tileink wgpu canvas glyph image data",
                &staging.text.image_data,
            );
        });
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

    fn upload_tile_draw_bins(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        staging: &WgpuSceneUploadStaging,
    ) {
        let bins = &staging.tile_draw_bins;
        self.tile_draw_range_starts.upload(
            device,
            queue,
            "tileink wgpu canvas tile draw range starts",
            &bins.range_starts,
        );
        self.tile_draw_range_ends.upload(
            device,
            queue,
            "tileink wgpu canvas tile draw range ends",
            &bins.range_ends,
        );
        self.tile_draw_indices.upload(
            device,
            queue,
            "tileink wgpu canvas tile draw indices",
            &bins.draw_indices,
        );
    }
}
