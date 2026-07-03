use crate::{
    scene::Scene,
    shared::{
        bd_record::BackdropRecord,
        execution::{ExecPlan, LayerStackEntry},
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
        pixel::{mul_div255, opacity_f32_to_u8},
        scene_columns::SceneColumns,
    },
    text::{AtlasSignature, PreparedGlyphContent, PreparedTextData, TextCompositeMode},
};

use super::super::buffer::WgpuBuffer;
use super::super::profile::profile_cpu;
use super::WgpuSceneBuffers;

#[derive(Default)]
pub(crate) struct WgpuSceneUploadStaging {
    u32s: Vec<u32>,
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
        scene: &Scene,
        text: Option<&PreparedTextData>,
        current_atlas_signature: AtlasSignature,
    ) {
        self.clear();
        let Some(text) = text else {
            return;
        };

        self.run_starts
            .extend_from_slice(&scene.columns.text_run_starts);
        self.run_counts
            .extend_from_slice(&scene.columns.text_run_counts);
        self.glyph_image_ids.reserve(scene.text_glyphs.len());
        self.glyph_x.extend_from_slice(&scene.columns.glyph_x);
        self.glyph_y.extend_from_slice(&scene.columns.glyph_y);
        for glyph in &scene.text_glyphs {
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
    pub(crate) fn upload(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        scene: &Scene,
        plan: &ExecPlan,
        text: Option<&PreparedTextData>,
        staging: &mut WgpuSceneUploadStaging,
    ) {
        // Keep scene upload profiling split between CPU-side plan construction and queue uploads.
        profile_cpu("prepare.upload_scene.build_scan_chunks", || {
            build_scan_chunks_into(
                scene,
                &mut staging.scan_chunks,
                &mut staging.scan_chunk_ranges,
            );
        });
        profile_cpu("prepare.upload_scene.build_cumsum_plan", || {
            build_cumsum_plan_into(scene, &mut staging.cumsum_plan);
        });
        profile_cpu("prepare.upload_scene.build_tile_draw_bins", || {
            build_tile_draw_bins_into(
                scene,
                &mut staging.tile_draw_bins,
                &mut staging.tile_draw_cursors,
            );
        });
        profile_cpu("prepare.upload_scene.upload_columns", || {
            self.upload_columns(device, queue, &scene.columns, text.is_some());
        });
        profile_cpu("prepare.upload_scene.upload_backdrops", || {
            self.upload_backdrops(device, queue, &scene.bd_records, staging);
        });
        profile_cpu("prepare.upload_scene.upload_layer_stack", || {
            self.upload_plan_layer_stack(device, queue, &plan.layer_stack_data, staging);
        });
        profile_cpu("prepare.upload_scene.upload_text", || {
            self.upload_text(device, queue, scene, text, staging);
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

    fn upload_columns(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        columns: &SceneColumns,
        text_enabled: bool,
    ) {
        profile_cpu("prepare.upload_scene.columns.lines", || {
            self.line_path_ids.upload(
                device,
                queue,
                "tileink wgpu scene line path ids",
                &columns.line_path_ids,
            );
            self.line_p0x.upload(
                device,
                queue,
                "tileink wgpu scene line p0x",
                &columns.line_p0x,
            );
            self.line_p0y.upload(
                device,
                queue,
                "tileink wgpu scene line p0y",
                &columns.line_p0y,
            );
            self.line_p1x.upload(
                device,
                queue,
                "tileink wgpu scene line p1x",
                &columns.line_p1x,
            );
            self.line_p1y.upload(
                device,
                queue,
                "tileink wgpu scene line p1y",
                &columns.line_p1y,
            );
            self.path_flags.upload(
                device,
                queue,
                "tileink wgpu scene path flags",
                &columns.path_flags,
            );
        });
        profile_cpu("prepare.upload_scene.columns.draws", || {
            self.draw_path_ids.upload(
                device,
                queue,
                "tileink wgpu scene draw path ids",
                &columns.draw_path_ids,
            );
            self.draw_glyph_run_ids.upload(
                device,
                queue,
                "tileink wgpu scene draw glyph run ids",
                if text_enabled {
                    &columns.draw_glyph_run_ids
                } else {
                    &columns.draw_glyph_run_ids_without_text
                },
            );
            self.draw_glyph_run_ids_without_text.upload(
                device,
                queue,
                "tileink wgpu scene draw glyph run ids without text",
                &columns.draw_glyph_run_ids_without_text,
            );
            self.draw_flags.upload(
                device,
                queue,
                "tileink wgpu scene draw flags",
                if text_enabled {
                    &columns.draw_flags
                } else {
                    &columns.draw_flags_without_text
                },
            );
            self.draw_flags_without_text.upload(
                device,
                queue,
                "tileink wgpu scene draw flags without text",
                &columns.draw_flags_without_text,
            );
            self.draw_brush_colors.upload(
                device,
                queue,
                "tileink wgpu scene draw brush colors",
                &columns.draw_brush_colors,
            );
            self.draw_pixel_x0.upload(
                device,
                queue,
                "tileink wgpu scene draw pixel x0",
                &columns.draw_pixel_x0,
            );
            self.draw_pixel_y0.upload(
                device,
                queue,
                "tileink wgpu scene draw pixel y0",
                &columns.draw_pixel_y0,
            );
            self.draw_pixel_x1.upload(
                device,
                queue,
                "tileink wgpu scene draw pixel x1",
                &columns.draw_pixel_x1,
            );
            self.draw_pixel_y1.upload(
                device,
                queue,
                "tileink wgpu scene draw pixel y1",
                &columns.draw_pixel_y1,
            );
        });
        profile_cpu("prepare.upload_scene.columns.sdf", || {
            let sdf = &columns.sdf;
            self.sdf_refs
                .upload(device, queue, "tileink wgpu scene sdf refs", &sdf.refs);
            self.sdf_kinds
                .upload(device, queue, "tileink wgpu scene sdf kinds", &sdf.kinds);
            self.sdf_x0
                .upload(device, queue, "tileink wgpu scene sdf x0", &sdf.x0);
            self.sdf_y0
                .upload(device, queue, "tileink wgpu scene sdf y0", &sdf.y0);
            self.sdf_x1
                .upload(device, queue, "tileink wgpu scene sdf x1", &sdf.x1);
            self.sdf_y1
                .upload(device, queue, "tileink wgpu scene sdf y1", &sdf.y1);
            self.sdf_r0
                .upload(device, queue, "tileink wgpu scene sdf r0", &sdf.r0);
            self.sdf_r1
                .upload(device, queue, "tileink wgpu scene sdf r1", &sdf.r1);
            self.sdf_r2
                .upload(device, queue, "tileink wgpu scene sdf r2", &sdf.r2);
            self.sdf_r3
                .upload(device, queue, "tileink wgpu scene sdf r3", &sdf.r3);
            self.sdf_stroke_top.upload(
                device,
                queue,
                "tileink wgpu scene sdf stroke top",
                &sdf.stroke_top,
            );
            self.sdf_stroke_right.upload(
                device,
                queue,
                "tileink wgpu scene sdf stroke right",
                &sdf.stroke_right,
            );
            self.sdf_stroke_bottom.upload(
                device,
                queue,
                "tileink wgpu scene sdf stroke bottom",
                &sdf.stroke_bottom,
            );
            self.sdf_stroke_left.upload(
                device,
                queue,
                "tileink wgpu scene sdf stroke left",
                &sdf.stroke_left,
            );
            self.sdf_shadow_offset_x.upload(
                device,
                queue,
                "tileink wgpu scene sdf shadow offset x",
                &sdf.shadow_offset_x,
            );
            self.sdf_shadow_offset_y.upload(
                device,
                queue,
                "tileink wgpu scene sdf shadow offset y",
                &sdf.shadow_offset_y,
            );
            self.sdf_shadow_expand.upload(
                device,
                queue,
                "tileink wgpu scene sdf shadow expand",
                &sdf.shadow_expand,
            );
            self.sdf_shadow_intensity.upload(
                device,
                queue,
                "tileink wgpu scene sdf shadow intensity",
                &sdf.shadow_intensity,
            );
        });
        profile_cpu("prepare.upload_scene.columns.brushes", || {
            let brushes = &columns.draw_brushes;
            self.draw_brush_data.upload(
                device,
                queue,
                "tileink wgpu scene draw brush data",
                &brushes.data,
            );
            self.draw_brush_params.upload(
                device,
                queue,
                "tileink wgpu scene draw brush params",
                &brushes.params,
            );
            self.draw_brush_payloads.upload(
                device,
                queue,
                "tileink wgpu scene draw brush payloads",
                &brushes.payloads,
            );
        });
        profile_cpu("prepare.upload_scene.columns.text", || {
            self.text_run_starts.upload(
                device,
                queue,
                "tileink wgpu scene text run starts",
                &columns.text_run_starts,
            );
            self.text_run_counts.upload(
                device,
                queue,
                "tileink wgpu scene text run counts",
                &columns.text_run_counts,
            );
            self.glyph_x.upload(
                device,
                queue,
                "tileink wgpu scene glyph x",
                &columns.glyph_x,
            );
            self.glyph_y.upload(
                device,
                queue,
                "tileink wgpu scene glyph y",
                &columns.glyph_y,
            );
        });
    }

    fn upload_backdrops(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        records: &[BackdropRecord],
        staging: &mut WgpuSceneUploadStaging,
    ) {
        upload_mapped_u32(
            device,
            queue,
            &mut self.backdrop_data_offsets,
            "tileink wgpu scene backdrop data offsets",
            &mut staging.u32s,
            records,
            |record| record.data_offset,
        );
        upload_mapped_u32(
            device,
            queue,
            &mut self.backdrop_data_lens,
            "tileink wgpu scene backdrop data lens",
            &mut staging.u32s,
            records,
            |record| record.data_len,
        );
        upload_mapped_u32(
            device,
            queue,
            &mut self.backdrop_tile_x0,
            "tileink wgpu scene backdrop tile x0",
            &mut staging.u32s,
            records,
            |record| record.tile_x0,
        );
        upload_mapped_u32(
            device,
            queue,
            &mut self.backdrop_tile_y0,
            "tileink wgpu scene backdrop tile y0",
            &mut staging.u32s,
            records,
            |record| record.tile_y0,
        );
        upload_mapped_u32(
            device,
            queue,
            &mut self.backdrop_tile_x1,
            "tileink wgpu scene backdrop tile x1",
            &mut staging.u32s,
            records,
            |record| record.tile_x1,
        );
        upload_mapped_u32(
            device,
            queue,
            &mut self.backdrop_tile_y1,
            "tileink wgpu scene backdrop tile y1",
            &mut staging.u32s,
            records,
            |record| record.tile_y1,
        );
        upload_mapped_u32(
            device,
            queue,
            &mut self.backdrop_segment_starts,
            "tileink wgpu scene backdrop segment starts",
            &mut staging.u32s,
            records,
            |record| record.segment_start,
        );
        upload_mapped_u32(
            device,
            queue,
            &mut self.backdrop_segment_capacities,
            "tileink wgpu scene backdrop segment capacities",
            &mut staging.u32s,
            records,
            |record| record.segment_capacity,
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
            "tileink wgpu scene plan layer stack tags",
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
            "tileink wgpu scene plan layer stack draws",
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
            "tileink wgpu scene plan layer stack payloads",
            &mut staging.u32s,
            layer_stack,
            |entry| encode_layer_payload(*entry),
        );
    }

    fn upload_text(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        scene: &Scene,
        text: Option<&PreparedTextData>,
        staging: &mut WgpuSceneUploadStaging,
    ) {
        profile_cpu("prepare.upload_scene.text.refill", || {
            staging.text.refill(scene, text, self.glyph_atlas_signature);
        });
        profile_cpu("prepare.upload_scene.text.runs", || {
            self.text_run_starts.upload(
                device,
                queue,
                "tileink wgpu scene text run starts",
                &staging.text.run_starts,
            );
            self.text_run_counts.upload(
                device,
                queue,
                "tileink wgpu scene text run counts",
                &staging.text.run_counts,
            );
            self.glyph_image_ids.upload(
                device,
                queue,
                "tileink wgpu scene glyph image ids",
                &staging.text.glyph_image_ids,
            );
            self.glyph_x.upload(
                device,
                queue,
                "tileink wgpu scene glyph x",
                &staging.text.glyph_x,
            );
            self.glyph_y.upload(
                device,
                queue,
                "tileink wgpu scene glyph y",
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
                "tileink wgpu scene glyph image left",
                &staging.text.image_left,
            );
            self.glyph_image_top.upload(
                device,
                queue,
                "tileink wgpu scene glyph image top",
                &staging.text.image_top,
            );
            self.glyph_image_width.upload(
                device,
                queue,
                "tileink wgpu scene glyph image width",
                &staging.text.image_width,
            );
            self.glyph_image_height.upload(
                device,
                queue,
                "tileink wgpu scene glyph image height",
                &staging.text.image_height,
            );
            self.glyph_image_content.upload(
                device,
                queue,
                "tileink wgpu scene glyph image content",
                &staging.text.image_content,
            );
            self.glyph_image_data_offsets.upload(
                device,
                queue,
                "tileink wgpu scene glyph image data offsets",
                &staging.text.image_data_offsets,
            );
            self.glyph_image_data.upload(
                device,
                queue,
                "tileink wgpu scene glyph image data",
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
            "tileink wgpu scene scan chunk path ids",
            &mut staging.u32s,
            &staging.scan_chunks,
            |chunk| chunk.path_id,
        );
        upload_mapped_u32(
            device,
            queue,
            &mut self.scan_chunk_backdrop_offsets,
            "tileink wgpu scene scan chunk backdrop offsets",
            &mut staging.u32s,
            &staging.scan_chunks,
            |chunk| chunk.backdrop_offset,
        );
        upload_mapped_u32(
            device,
            queue,
            &mut self.scan_chunk_segment_starts,
            "tileink wgpu scene scan chunk segment starts",
            &mut staging.u32s,
            &staging.scan_chunks,
            |chunk| chunk.segment_start,
        );
        upload_mapped_u32(
            device,
            queue,
            &mut self.scan_chunk_lens,
            "tileink wgpu scene scan chunk lens",
            &mut staging.u32s,
            &staging.scan_chunks,
            |chunk| chunk.len,
        );
        upload_mapped_u32(
            device,
            queue,
            &mut self.scan_chunk_range_starts,
            "tileink wgpu scene scan chunk range starts",
            &mut staging.u32s,
            &staging.scan_chunk_ranges,
            |range| range.start,
        );
        upload_mapped_u32(
            device,
            queue,
            &mut self.scan_chunk_range_ends,
            "tileink wgpu scene scan chunk range ends",
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
            "tileink wgpu scene cumsum chunk backdrop offsets",
            &staging.cumsum_plan.chunk_backdrop_offsets,
        );
        self.cumsum_chunk_lens.upload(
            device,
            queue,
            "tileink wgpu scene cumsum chunk lens",
            &staging.cumsum_plan.chunk_lens,
        );
        self.cumsum_row_chunk_starts.upload(
            device,
            queue,
            "tileink wgpu scene cumsum row chunk starts",
            &staging.cumsum_plan.row_chunk_starts,
        );
        self.cumsum_row_chunk_ends.upload(
            device,
            queue,
            "tileink wgpu scene cumsum row chunk ends",
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
            "tileink wgpu scene tile draw range starts",
            &bins.range_starts,
        );
        self.tile_draw_range_ends.upload(
            device,
            queue,
            "tileink wgpu scene tile draw range ends",
            &bins.range_ends,
        );
        self.tile_draw_indices.upload(
            device,
            queue,
            "tileink wgpu scene tile draw indices",
            &bins.draw_indices,
        );
    }
}
