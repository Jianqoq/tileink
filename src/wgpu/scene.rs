use crate::{
    scene::Scene,
    shared::{
        bd_record::BackdropRecord,
        execution::{ExecPlan, LayerStackEntry},
        gpu_plan::{
            GpuBufferLengths, GpuCumsumPlan, GpuScanChunk, GpuScanChunkRange,
            build_cumsum_plan_into, build_scan_chunks_into,
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

use super::buffer::WgpuBuffer;

#[derive(Default)]
pub(crate) struct WgpuSceneUploadStaging {
    u32s: Vec<u32>,
    text: TextUpload,
    scan_chunks: Vec<GpuScanChunk>,
    scan_chunk_ranges: Vec<GpuScanChunkRange>,
    cumsum_plan: GpuCumsumPlan,
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

fn packed_u8_len(len: usize) -> usize {
    len.div_ceil(4)
}

pub(crate) struct WgpuSceneBuffers {
    line_path_ids: WgpuBuffer,
    line_p0x: WgpuBuffer,
    line_p0y: WgpuBuffer,
    line_p1x: WgpuBuffer,
    line_p1y: WgpuBuffer,
    path_flags: WgpuBuffer,
    draw_path_ids: WgpuBuffer,
    draw_glyph_run_ids: WgpuBuffer,
    draw_glyph_run_ids_without_text: WgpuBuffer,
    draw_flags: WgpuBuffer,
    draw_flags_without_text: WgpuBuffer,
    draw_brush_colors: WgpuBuffer,
    draw_pixel_x0: WgpuBuffer,
    draw_pixel_y0: WgpuBuffer,
    draw_pixel_x1: WgpuBuffer,
    draw_pixel_y1: WgpuBuffer,
    sdf_refs: WgpuBuffer,
    sdf_kinds: WgpuBuffer,
    sdf_x0: WgpuBuffer,
    sdf_y0: WgpuBuffer,
    sdf_x1: WgpuBuffer,
    sdf_y1: WgpuBuffer,
    sdf_r0: WgpuBuffer,
    sdf_r1: WgpuBuffer,
    sdf_r2: WgpuBuffer,
    sdf_r3: WgpuBuffer,
    sdf_stroke_top: WgpuBuffer,
    sdf_stroke_right: WgpuBuffer,
    sdf_stroke_bottom: WgpuBuffer,
    sdf_stroke_left: WgpuBuffer,
    sdf_shadow_offset_x: WgpuBuffer,
    sdf_shadow_offset_y: WgpuBuffer,
    sdf_shadow_expand: WgpuBuffer,
    sdf_shadow_intensity: WgpuBuffer,
    backdrop_data_offsets: WgpuBuffer,
    backdrop_data_lens: WgpuBuffer,
    backdrop_tile_x0: WgpuBuffer,
    backdrop_tile_y0: WgpuBuffer,
    backdrop_tile_x1: WgpuBuffer,
    backdrop_tile_y1: WgpuBuffer,
    backdrop_segment_starts: WgpuBuffer,
    backdrop_segment_capacities: WgpuBuffer,
    scan_chunk_path_ids: WgpuBuffer,
    scan_chunk_backdrop_offsets: WgpuBuffer,
    scan_chunk_segment_starts: WgpuBuffer,
    scan_chunk_lens: WgpuBuffer,
    scan_chunk_range_starts: WgpuBuffer,
    scan_chunk_range_ends: WgpuBuffer,
    cumsum_chunk_backdrop_offsets: WgpuBuffer,
    cumsum_chunk_lens: WgpuBuffer,
    cumsum_row_chunk_starts: WgpuBuffer,
    cumsum_row_chunk_ends: WgpuBuffer,
    plan_layer_stack_tags: WgpuBuffer,
    plan_layer_stack_draws: WgpuBuffer,
    plan_layer_stack_payloads: WgpuBuffer,
    draw_brush_data: WgpuBuffer,
    draw_brush_params: WgpuBuffer,
    draw_brush_payloads: WgpuBuffer,
    text_run_starts: WgpuBuffer,
    text_run_counts: WgpuBuffer,
    glyph_image_ids: WgpuBuffer,
    glyph_x: WgpuBuffer,
    glyph_y: WgpuBuffer,
    glyph_image_left: WgpuBuffer,
    glyph_image_top: WgpuBuffer,
    glyph_image_width: WgpuBuffer,
    glyph_image_height: WgpuBuffer,
    glyph_image_content: WgpuBuffer,
    glyph_image_data_offsets: WgpuBuffer,
    glyph_image_data: WgpuBuffer,
    glyph_atlas_signature: AtlasSignature,
}

impl WgpuSceneBuffers {
    pub(crate) fn new(device: &::wgpu::Device) -> Self {
        Self {
            line_path_ids: WgpuBuffer::new(device, "tileink wgpu scene line path ids"),
            line_p0x: WgpuBuffer::new(device, "tileink wgpu scene line p0x"),
            line_p0y: WgpuBuffer::new(device, "tileink wgpu scene line p0y"),
            line_p1x: WgpuBuffer::new(device, "tileink wgpu scene line p1x"),
            line_p1y: WgpuBuffer::new(device, "tileink wgpu scene line p1y"),
            path_flags: WgpuBuffer::new(device, "tileink wgpu scene path flags"),
            draw_path_ids: WgpuBuffer::new(device, "tileink wgpu scene draw path ids"),
            draw_glyph_run_ids: WgpuBuffer::new(device, "tileink wgpu scene draw glyph run ids"),
            draw_glyph_run_ids_without_text: WgpuBuffer::new(
                device,
                "tileink wgpu scene draw glyph run ids without text",
            ),
            draw_flags: WgpuBuffer::new(device, "tileink wgpu scene draw flags"),
            draw_flags_without_text: WgpuBuffer::new(
                device,
                "tileink wgpu scene draw flags without text",
            ),
            draw_brush_colors: WgpuBuffer::new(device, "tileink wgpu scene draw brush colors"),
            draw_pixel_x0: WgpuBuffer::new(device, "tileink wgpu scene draw pixel x0"),
            draw_pixel_y0: WgpuBuffer::new(device, "tileink wgpu scene draw pixel y0"),
            draw_pixel_x1: WgpuBuffer::new(device, "tileink wgpu scene draw pixel x1"),
            draw_pixel_y1: WgpuBuffer::new(device, "tileink wgpu scene draw pixel y1"),
            sdf_refs: WgpuBuffer::new(device, "tileink wgpu scene sdf refs"),
            sdf_kinds: WgpuBuffer::new(device, "tileink wgpu scene sdf kinds"),
            sdf_x0: WgpuBuffer::new(device, "tileink wgpu scene sdf x0"),
            sdf_y0: WgpuBuffer::new(device, "tileink wgpu scene sdf y0"),
            sdf_x1: WgpuBuffer::new(device, "tileink wgpu scene sdf x1"),
            sdf_y1: WgpuBuffer::new(device, "tileink wgpu scene sdf y1"),
            sdf_r0: WgpuBuffer::new(device, "tileink wgpu scene sdf r0"),
            sdf_r1: WgpuBuffer::new(device, "tileink wgpu scene sdf r1"),
            sdf_r2: WgpuBuffer::new(device, "tileink wgpu scene sdf r2"),
            sdf_r3: WgpuBuffer::new(device, "tileink wgpu scene sdf r3"),
            sdf_stroke_top: WgpuBuffer::new(device, "tileink wgpu scene sdf stroke top"),
            sdf_stroke_right: WgpuBuffer::new(device, "tileink wgpu scene sdf stroke right"),
            sdf_stroke_bottom: WgpuBuffer::new(device, "tileink wgpu scene sdf stroke bottom"),
            sdf_stroke_left: WgpuBuffer::new(device, "tileink wgpu scene sdf stroke left"),
            sdf_shadow_offset_x: WgpuBuffer::new(device, "tileink wgpu scene sdf shadow offset x"),
            sdf_shadow_offset_y: WgpuBuffer::new(device, "tileink wgpu scene sdf shadow offset y"),
            sdf_shadow_expand: WgpuBuffer::new(device, "tileink wgpu scene sdf shadow expand"),
            sdf_shadow_intensity: WgpuBuffer::new(
                device,
                "tileink wgpu scene sdf shadow intensity",
            ),
            backdrop_data_offsets: WgpuBuffer::new(
                device,
                "tileink wgpu scene backdrop data offsets",
            ),
            backdrop_data_lens: WgpuBuffer::new(device, "tileink wgpu scene backdrop data lens"),
            backdrop_tile_x0: WgpuBuffer::new(device, "tileink wgpu scene backdrop tile x0"),
            backdrop_tile_y0: WgpuBuffer::new(device, "tileink wgpu scene backdrop tile y0"),
            backdrop_tile_x1: WgpuBuffer::new(device, "tileink wgpu scene backdrop tile x1"),
            backdrop_tile_y1: WgpuBuffer::new(device, "tileink wgpu scene backdrop tile y1"),
            backdrop_segment_starts: WgpuBuffer::new(
                device,
                "tileink wgpu scene backdrop segment starts",
            ),
            backdrop_segment_capacities: WgpuBuffer::new(
                device,
                "tileink wgpu scene backdrop segment capacities",
            ),
            scan_chunk_path_ids: WgpuBuffer::new(device, "tileink wgpu scene scan chunk path ids"),
            scan_chunk_backdrop_offsets: WgpuBuffer::new(
                device,
                "tileink wgpu scene scan chunk backdrop offsets",
            ),
            scan_chunk_segment_starts: WgpuBuffer::new(
                device,
                "tileink wgpu scene scan chunk segment starts",
            ),
            scan_chunk_lens: WgpuBuffer::new(device, "tileink wgpu scene scan chunk lens"),
            scan_chunk_range_starts: WgpuBuffer::new(
                device,
                "tileink wgpu scene scan chunk range starts",
            ),
            scan_chunk_range_ends: WgpuBuffer::new(
                device,
                "tileink wgpu scene scan chunk range ends",
            ),
            cumsum_chunk_backdrop_offsets: WgpuBuffer::new(
                device,
                "tileink wgpu scene cumsum chunk backdrop offsets",
            ),
            cumsum_chunk_lens: WgpuBuffer::new(device, "tileink wgpu scene cumsum chunk lens"),
            cumsum_row_chunk_starts: WgpuBuffer::new(
                device,
                "tileink wgpu scene cumsum row chunk starts",
            ),
            cumsum_row_chunk_ends: WgpuBuffer::new(
                device,
                "tileink wgpu scene cumsum row chunk ends",
            ),
            plan_layer_stack_tags: WgpuBuffer::new(
                device,
                "tileink wgpu scene plan layer stack tags",
            ),
            plan_layer_stack_draws: WgpuBuffer::new(
                device,
                "tileink wgpu scene plan layer stack draws",
            ),
            plan_layer_stack_payloads: WgpuBuffer::new(
                device,
                "tileink wgpu scene plan layer stack payloads",
            ),
            draw_brush_data: WgpuBuffer::new(device, "tileink wgpu scene draw brush data"),
            draw_brush_params: WgpuBuffer::new(device, "tileink wgpu scene draw brush params"),
            draw_brush_payloads: WgpuBuffer::new(device, "tileink wgpu scene draw brush payloads"),
            text_run_starts: WgpuBuffer::new(device, "tileink wgpu scene text run starts"),
            text_run_counts: WgpuBuffer::new(device, "tileink wgpu scene text run counts"),
            glyph_image_ids: WgpuBuffer::new(device, "tileink wgpu scene glyph image ids"),
            glyph_x: WgpuBuffer::new(device, "tileink wgpu scene glyph x"),
            glyph_y: WgpuBuffer::new(device, "tileink wgpu scene glyph y"),
            glyph_image_left: WgpuBuffer::new(device, "tileink wgpu scene glyph image left"),
            glyph_image_top: WgpuBuffer::new(device, "tileink wgpu scene glyph image top"),
            glyph_image_width: WgpuBuffer::new(device, "tileink wgpu scene glyph image width"),
            glyph_image_height: WgpuBuffer::new(device, "tileink wgpu scene glyph image height"),
            glyph_image_content: WgpuBuffer::new(device, "tileink wgpu scene glyph image content"),
            glyph_image_data_offsets: WgpuBuffer::new(
                device,
                "tileink wgpu scene glyph image data offsets",
            ),
            glyph_image_data: WgpuBuffer::new(device, "tileink wgpu scene glyph image data"),
            glyph_atlas_signature: AtlasSignature::default(),
        }
    }

    pub(crate) fn upload(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        scene: &Scene,
        plan: &ExecPlan,
        text: Option<&PreparedTextData>,
        staging: &mut WgpuSceneUploadStaging,
    ) {
        build_scan_chunks_into(
            scene,
            &mut staging.scan_chunks,
            &mut staging.scan_chunk_ranges,
        );
        build_cumsum_plan_into(scene, &mut staging.cumsum_plan);
        self.upload_columns(device, queue, &scene.columns, text.is_some());
        self.upload_backdrops(device, queue, &scene.bd_records, staging);
        self.upload_plan_layer_stack(device, queue, &plan.layer_stack_data, staging);
        self.upload_text(device, queue, scene, text, staging);
        self.upload_scan_plan(device, queue, staging);
        self.upload_cumsum_plan(device, queue, staging);
    }

    fn upload_columns(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        columns: &SceneColumns,
        text_enabled: bool,
    ) {
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
        staging.text.refill(scene, text, self.glyph_atlas_signature);
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
        if !staging.text.atlas_dirty {
            return;
        }

        self.glyph_atlas_signature = staging.text.atlas_signature;
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

    #[cfg(test)]
    pub(crate) fn draw_flags_capacity(&self) -> ::wgpu::BufferAddress {
        self.draw_flags.capacity()
    }

    pub(crate) fn fine_bindings(&self) -> WgpuFineSceneBindings<'_> {
        WgpuFineSceneBindings {
            draw_flags: self.draw_flags.buffer(),
            draw_brush_colors: self.draw_brush_colors.buffer(),
            draw_pixel_x0: self.draw_pixel_x0.buffer(),
            draw_pixel_y0: self.draw_pixel_y0.buffer(),
            draw_pixel_x1: self.draw_pixel_x1.buffer(),
            draw_pixel_y1: self.draw_pixel_y1.buffer(),
            sdf_refs: self.sdf_refs.buffer(),
            sdf_kinds: self.sdf_kinds.buffer(),
            sdf_x0: self.sdf_x0.buffer(),
            sdf_y0: self.sdf_y0.buffer(),
            sdf_x1: self.sdf_x1.buffer(),
            sdf_y1: self.sdf_y1.buffer(),
            sdf_r0: self.sdf_r0.buffer(),
            sdf_r1: self.sdf_r1.buffer(),
            sdf_r2: self.sdf_r2.buffer(),
            sdf_r3: self.sdf_r3.buffer(),
            sdf_stroke_top: self.sdf_stroke_top.buffer(),
            sdf_stroke_right: self.sdf_stroke_right.buffer(),
            sdf_stroke_bottom: self.sdf_stroke_bottom.buffer(),
            sdf_stroke_left: self.sdf_stroke_left.buffer(),
            sdf_shadow_offset_x: self.sdf_shadow_offset_x.buffer(),
            sdf_shadow_offset_y: self.sdf_shadow_offset_y.buffer(),
            sdf_shadow_expand: self.sdf_shadow_expand.buffer(),
            sdf_shadow_intensity: self.sdf_shadow_intensity.buffer(),
            brush_data: self.draw_brush_data.buffer(),
            brush_params: self.draw_brush_params.buffer(),
            brush_payloads: self.draw_brush_payloads.buffer(),
        }
    }

    pub(crate) fn tile_fine_bindings<'a>(
        &'a self,
        scan: &'a WgpuScanBuffers,
        coarse: &'a WgpuCoarseBuffers,
        clip_spills: &'a WgpuBuffer,
        group_spills: &'a WgpuBuffer,
    ) -> WgpuTileFineBindings<'a> {
        WgpuTileFineBindings {
            fine: self.fine_bindings(),
            tile_range_starts: coarse.tile_ptcl_range_starts.buffer(),
            tile_range_ends: coarse.tile_ptcl_range_ends.buffer(),
            ptcl_tags: coarse.ptcl_tags.buffer(),
            ptcl_backdrops: coarse.ptcl_backdrops.buffer(),
            ptcl_fill_rules: coarse.ptcl_fill_rules.buffer(),
            ptcl_segment_starts: coarse.ptcl_segment_starts.buffer(),
            ptcl_segment_ends: coarse.ptcl_segment_ends.buffer(),
            ptcl_colors: coarse.ptcl_colors.buffer(),
            segment_p0x: scan.segment_p0x.buffer(),
            segment_p0y: scan.segment_p0y.buffer(),
            segment_p1x: scan.segment_p1x.buffer(),
            segment_p1y: scan.segment_p1y.buffer(),
            segment_y_edge: scan.segment_y_edge.buffer(),
            glyph_indices: coarse.glyph_indices.buffer(),
            glyph_image_ids: self.glyph_image_ids.buffer(),
            glyph_x: self.glyph_x.buffer(),
            glyph_y: self.glyph_y.buffer(),
            glyph_image_left: self.glyph_image_left.buffer(),
            glyph_image_top: self.glyph_image_top.buffer(),
            glyph_image_width: self.glyph_image_width.buffer(),
            glyph_image_height: self.glyph_image_height.buffer(),
            glyph_image_content: self.glyph_image_content.buffer(),
            glyph_image_data_offsets: self.glyph_image_data_offsets.buffer(),
            glyph_image_data: self.glyph_image_data.buffer(),
            clip_spills: clip_spills.buffer(),
            group_spills: group_spills.buffer(),
        }
    }

    pub(crate) fn cumsum_bindings<'a>(
        &'a self,
        scan: &'a WgpuScanBuffers,
    ) -> WgpuCumsumBindings<'a> {
        WgpuCumsumBindings {
            chunk_backdrop_offsets: self.cumsum_chunk_backdrop_offsets.buffer(),
            chunk_lens: self.cumsum_chunk_lens.buffer(),
            row_chunk_starts: self.cumsum_row_chunk_starts.buffer(),
            row_chunk_ends: self.cumsum_row_chunk_ends.buffer(),
            backdrops: scan.backdrops.buffer(),
            chunk_totals: scan.cumsum_chunk_totals.buffer(),
            chunk_offsets: scan.cumsum_chunk_offsets.buffer(),
        }
    }

    pub(crate) fn filter_bindings<'a>(
        &'a self,
        scan: &'a WgpuScanBuffers,
    ) -> WgpuFilterBindings<'a> {
        WgpuFilterBindings {
            draw_path_ids: self.draw_path_ids.buffer(),
            draw_flags: self.draw_flags.buffer(),
            draw_pixel_x0: self.draw_pixel_x0.buffer(),
            draw_pixel_y0: self.draw_pixel_y0.buffer(),
            draw_pixel_x1: self.draw_pixel_x1.buffer(),
            draw_pixel_y1: self.draw_pixel_y1.buffer(),
            draw_sdf_refs: self.sdf_refs.buffer(),
            sdf_kinds: self.sdf_kinds.buffer(),
            sdf_x0: self.sdf_x0.buffer(),
            sdf_y0: self.sdf_y0.buffer(),
            sdf_x1: self.sdf_x1.buffer(),
            sdf_y1: self.sdf_y1.buffer(),
            sdf_r0: self.sdf_r0.buffer(),
            sdf_r1: self.sdf_r1.buffer(),
            sdf_r2: self.sdf_r2.buffer(),
            sdf_r3: self.sdf_r3.buffer(),
            sdf_stroke_top: self.sdf_stroke_top.buffer(),
            sdf_stroke_right: self.sdf_stroke_right.buffer(),
            sdf_stroke_bottom: self.sdf_stroke_bottom.buffer(),
            sdf_stroke_left: self.sdf_stroke_left.buffer(),
            sdf_shadow_offset_x: self.sdf_shadow_offset_x.buffer(),
            sdf_shadow_offset_y: self.sdf_shadow_offset_y.buffer(),
            sdf_shadow_expand: self.sdf_shadow_expand.buffer(),
            sdf_shadow_intensity: self.sdf_shadow_intensity.buffer(),
            backdrop_data_offsets: self.backdrop_data_offsets.buffer(),
            backdrop_tile_x0: self.backdrop_tile_x0.buffer(),
            backdrop_tile_y0: self.backdrop_tile_y0.buffer(),
            backdrop_tile_x1: self.backdrop_tile_x1.buffer(),
            backdrop_tile_y1: self.backdrop_tile_y1.buffer(),
            backdrops: scan.backdrops.buffer(),
            segment_starts: scan.tile_segment_range_starts.buffer(),
            segment_ends: scan.tile_segment_range_ends.buffer(),
            segment_p0x: scan.segment_p0x.buffer(),
            segment_p0y: scan.segment_p0y.buffer(),
            segment_p1x: scan.segment_p1x.buffer(),
            segment_p1y: scan.segment_p1y.buffer(),
            segment_y_edge: scan.segment_y_edge.buffer(),
            layer_stack_tags: self.plan_layer_stack_tags.buffer(),
            layer_stack_draws: self.plan_layer_stack_draws.buffer(),
            layer_stack_payloads: self.plan_layer_stack_payloads.buffer(),
        }
    }

    pub(crate) fn scan_bindings<'a>(&'a self, scan: &'a WgpuScanBuffers) -> WgpuScanBindings<'a> {
        WgpuScanBindings {
            line_path_ids: self.line_path_ids.buffer(),
            line_p0x: self.line_p0x.buffer(),
            line_p0y: self.line_p0y.buffer(),
            line_p1x: self.line_p1x.buffer(),
            line_p1y: self.line_p1y.buffer(),
            path_flags: self.path_flags.buffer(),
            backdrop_data_offsets: self.backdrop_data_offsets.buffer(),
            backdrop_tile_x0: self.backdrop_tile_x0.buffer(),
            backdrop_tile_y0: self.backdrop_tile_y0.buffer(),
            backdrop_tile_x1: self.backdrop_tile_x1.buffer(),
            backdrop_tile_y1: self.backdrop_tile_y1.buffer(),
            scan_chunk_backdrop_offsets: self.scan_chunk_backdrop_offsets.buffer(),
            scan_chunk_lens: self.scan_chunk_lens.buffer(),
            scan_chunk_range_starts: self.scan_chunk_range_starts.buffer(),
            scan_chunk_range_ends: self.scan_chunk_range_ends.buffer(),
            backdrop_segment_starts: self.backdrop_segment_starts.buffer(),
            backdrops: scan.backdrops.buffer(),
            tile_segment_range_starts: scan.tile_segment_range_starts.buffer(),
            tile_segment_range_ends: scan.tile_segment_range_ends.buffer(),
            segment_tile_counts: scan.segment_tile_counts.buffer(),
            segment_tile_cursors: scan.segment_tile_cursors.buffer(),
            segment_bumps: scan.segment_bumps.buffer(),
            chunk_totals: scan.chunk_totals.buffer(),
            chunk_offsets: scan.chunk_offsets.buffer(),
            segment_p0x: scan.segment_p0x.buffer(),
            segment_p0y: scan.segment_p0y.buffer(),
            segment_p1x: scan.segment_p1x.buffer(),
            segment_p1y: scan.segment_p1y.buffer(),
            segment_y_edge: scan.segment_y_edge.buffer(),
        }
    }

    pub(crate) fn coarse_bindings<'a>(
        &'a self,
        scan: &'a WgpuScanBuffers,
        coarse: &'a WgpuCoarseBuffers,
    ) -> WgpuCoarseBindings<'a> {
        WgpuCoarseBindings {
            draw_path_ids: self.draw_path_ids.buffer(),
            draw_glyph_run_ids: self.draw_glyph_run_ids.buffer(),
            glyph_run_starts: self.text_run_starts.buffer(),
            glyph_run_counts: self.text_run_counts.buffer(),
            glyph_image_ids: self.glyph_image_ids.buffer(),
            glyph_x: self.glyph_x.buffer(),
            glyph_y: self.glyph_y.buffer(),
            glyph_image_left: self.glyph_image_left.buffer(),
            glyph_image_top: self.glyph_image_top.buffer(),
            glyph_image_width: self.glyph_image_width.buffer(),
            glyph_image_height: self.glyph_image_height.buffer(),
            draw_flags: self.draw_flags.buffer(),
            draw_brush_colors: self.draw_brush_colors.buffer(),
            draw_pixel_x0: self.draw_pixel_x0.buffer(),
            draw_pixel_y0: self.draw_pixel_y0.buffer(),
            draw_pixel_x1: self.draw_pixel_x1.buffer(),
            draw_pixel_y1: self.draw_pixel_y1.buffer(),
            backdrop_data_offsets: self.backdrop_data_offsets.buffer(),
            backdrop_tile_x0: self.backdrop_tile_x0.buffer(),
            backdrop_tile_y0: self.backdrop_tile_y0.buffer(),
            backdrop_tile_x1: self.backdrop_tile_x1.buffer(),
            backdrop_tile_y1: self.backdrop_tile_y1.buffer(),
            backdrops: scan.backdrops.buffer(),
            segment_starts: scan.tile_segment_range_starts.buffer(),
            segment_ends: scan.tile_segment_range_ends.buffer(),
            layer_stack_tags: self.plan_layer_stack_tags.buffer(),
            layer_stack_draws: self.plan_layer_stack_draws.buffer(),
            layer_stack_payloads: self.plan_layer_stack_payloads.buffer(),
            tile_ptcl_counts: coarse.tile_ptcl_counts.buffer(),
            tile_ptcl_range_starts: coarse.tile_ptcl_range_starts.buffer(),
            tile_ptcl_range_ends: coarse.tile_ptcl_range_ends.buffer(),
            tile_glyph_counts: coarse.tile_glyph_counts.buffer(),
            tile_glyph_range_starts: coarse.tile_glyph_range_starts.buffer(),
            tile_glyph_range_ends: coarse.tile_glyph_range_ends.buffer(),
            chunk_totals: coarse.chunk_totals.buffer(),
            chunk_offsets: coarse.chunk_offsets.buffer(),
            glyph_chunk_totals: coarse.glyph_chunk_totals.buffer(),
            glyph_chunk_offsets: coarse.glyph_chunk_offsets.buffer(),
            ptcl_tags: coarse.ptcl_tags.buffer(),
            ptcl_backdrops: coarse.ptcl_backdrops.buffer(),
            ptcl_fill_rules: coarse.ptcl_fill_rules.buffer(),
            ptcl_segment_starts: coarse.ptcl_segment_starts.buffer(),
            ptcl_segment_ends: coarse.ptcl_segment_ends.buffer(),
            ptcl_colors: coarse.ptcl_colors.buffer(),
            glyph_indices: coarse.glyph_indices.buffer(),
        }
    }
}

pub(crate) struct WgpuFineSceneBindings<'a> {
    pub(crate) draw_flags: &'a ::wgpu::Buffer,
    pub(crate) draw_brush_colors: &'a ::wgpu::Buffer,
    pub(crate) draw_pixel_x0: &'a ::wgpu::Buffer,
    pub(crate) draw_pixel_y0: &'a ::wgpu::Buffer,
    pub(crate) draw_pixel_x1: &'a ::wgpu::Buffer,
    pub(crate) draw_pixel_y1: &'a ::wgpu::Buffer,
    pub(crate) sdf_refs: &'a ::wgpu::Buffer,
    pub(crate) sdf_kinds: &'a ::wgpu::Buffer,
    pub(crate) sdf_x0: &'a ::wgpu::Buffer,
    pub(crate) sdf_y0: &'a ::wgpu::Buffer,
    pub(crate) sdf_x1: &'a ::wgpu::Buffer,
    pub(crate) sdf_y1: &'a ::wgpu::Buffer,
    pub(crate) sdf_r0: &'a ::wgpu::Buffer,
    pub(crate) sdf_r1: &'a ::wgpu::Buffer,
    pub(crate) sdf_r2: &'a ::wgpu::Buffer,
    pub(crate) sdf_r3: &'a ::wgpu::Buffer,
    pub(crate) sdf_stroke_top: &'a ::wgpu::Buffer,
    pub(crate) sdf_stroke_right: &'a ::wgpu::Buffer,
    pub(crate) sdf_stroke_bottom: &'a ::wgpu::Buffer,
    pub(crate) sdf_stroke_left: &'a ::wgpu::Buffer,
    pub(crate) sdf_shadow_offset_x: &'a ::wgpu::Buffer,
    pub(crate) sdf_shadow_offset_y: &'a ::wgpu::Buffer,
    pub(crate) sdf_shadow_expand: &'a ::wgpu::Buffer,
    pub(crate) sdf_shadow_intensity: &'a ::wgpu::Buffer,
    pub(crate) brush_data: &'a ::wgpu::Buffer,
    pub(crate) brush_params: &'a ::wgpu::Buffer,
    pub(crate) brush_payloads: &'a ::wgpu::Buffer,
}

pub(crate) struct WgpuTileFineBindings<'a> {
    pub(crate) fine: WgpuFineSceneBindings<'a>,
    pub(crate) tile_range_starts: &'a ::wgpu::Buffer,
    pub(crate) tile_range_ends: &'a ::wgpu::Buffer,
    pub(crate) ptcl_tags: &'a ::wgpu::Buffer,
    pub(crate) ptcl_backdrops: &'a ::wgpu::Buffer,
    pub(crate) ptcl_fill_rules: &'a ::wgpu::Buffer,
    pub(crate) ptcl_segment_starts: &'a ::wgpu::Buffer,
    pub(crate) ptcl_segment_ends: &'a ::wgpu::Buffer,
    pub(crate) ptcl_colors: &'a ::wgpu::Buffer,
    pub(crate) segment_p0x: &'a ::wgpu::Buffer,
    pub(crate) segment_p0y: &'a ::wgpu::Buffer,
    pub(crate) segment_p1x: &'a ::wgpu::Buffer,
    pub(crate) segment_p1y: &'a ::wgpu::Buffer,
    pub(crate) segment_y_edge: &'a ::wgpu::Buffer,
    pub(crate) glyph_indices: &'a ::wgpu::Buffer,
    pub(crate) glyph_image_ids: &'a ::wgpu::Buffer,
    pub(crate) glyph_x: &'a ::wgpu::Buffer,
    pub(crate) glyph_y: &'a ::wgpu::Buffer,
    pub(crate) glyph_image_left: &'a ::wgpu::Buffer,
    pub(crate) glyph_image_top: &'a ::wgpu::Buffer,
    pub(crate) glyph_image_width: &'a ::wgpu::Buffer,
    pub(crate) glyph_image_height: &'a ::wgpu::Buffer,
    pub(crate) glyph_image_content: &'a ::wgpu::Buffer,
    pub(crate) glyph_image_data_offsets: &'a ::wgpu::Buffer,
    pub(crate) glyph_image_data: &'a ::wgpu::Buffer,
    pub(crate) clip_spills: &'a ::wgpu::Buffer,
    pub(crate) group_spills: &'a ::wgpu::Buffer,
}

pub(crate) struct WgpuFilterBindings<'a> {
    pub(crate) draw_path_ids: &'a ::wgpu::Buffer,
    pub(crate) draw_flags: &'a ::wgpu::Buffer,
    pub(crate) draw_pixel_x0: &'a ::wgpu::Buffer,
    pub(crate) draw_pixel_y0: &'a ::wgpu::Buffer,
    pub(crate) draw_pixel_x1: &'a ::wgpu::Buffer,
    pub(crate) draw_pixel_y1: &'a ::wgpu::Buffer,
    pub(crate) draw_sdf_refs: &'a ::wgpu::Buffer,
    pub(crate) sdf_kinds: &'a ::wgpu::Buffer,
    pub(crate) sdf_x0: &'a ::wgpu::Buffer,
    pub(crate) sdf_y0: &'a ::wgpu::Buffer,
    pub(crate) sdf_x1: &'a ::wgpu::Buffer,
    pub(crate) sdf_y1: &'a ::wgpu::Buffer,
    pub(crate) sdf_r0: &'a ::wgpu::Buffer,
    pub(crate) sdf_r1: &'a ::wgpu::Buffer,
    pub(crate) sdf_r2: &'a ::wgpu::Buffer,
    pub(crate) sdf_r3: &'a ::wgpu::Buffer,
    pub(crate) sdf_stroke_top: &'a ::wgpu::Buffer,
    pub(crate) sdf_stroke_right: &'a ::wgpu::Buffer,
    pub(crate) sdf_stroke_bottom: &'a ::wgpu::Buffer,
    pub(crate) sdf_stroke_left: &'a ::wgpu::Buffer,
    pub(crate) sdf_shadow_offset_x: &'a ::wgpu::Buffer,
    pub(crate) sdf_shadow_offset_y: &'a ::wgpu::Buffer,
    pub(crate) sdf_shadow_expand: &'a ::wgpu::Buffer,
    pub(crate) sdf_shadow_intensity: &'a ::wgpu::Buffer,
    pub(crate) backdrop_data_offsets: &'a ::wgpu::Buffer,
    pub(crate) backdrop_tile_x0: &'a ::wgpu::Buffer,
    pub(crate) backdrop_tile_y0: &'a ::wgpu::Buffer,
    pub(crate) backdrop_tile_x1: &'a ::wgpu::Buffer,
    pub(crate) backdrop_tile_y1: &'a ::wgpu::Buffer,
    pub(crate) backdrops: &'a ::wgpu::Buffer,
    pub(crate) segment_starts: &'a ::wgpu::Buffer,
    pub(crate) segment_ends: &'a ::wgpu::Buffer,
    pub(crate) segment_p0x: &'a ::wgpu::Buffer,
    pub(crate) segment_p0y: &'a ::wgpu::Buffer,
    pub(crate) segment_p1x: &'a ::wgpu::Buffer,
    pub(crate) segment_p1y: &'a ::wgpu::Buffer,
    pub(crate) segment_y_edge: &'a ::wgpu::Buffer,
    pub(crate) layer_stack_tags: &'a ::wgpu::Buffer,
    pub(crate) layer_stack_draws: &'a ::wgpu::Buffer,
    pub(crate) layer_stack_payloads: &'a ::wgpu::Buffer,
}

pub(crate) struct WgpuCumsumBindings<'a> {
    pub(crate) chunk_backdrop_offsets: &'a ::wgpu::Buffer,
    pub(crate) chunk_lens: &'a ::wgpu::Buffer,
    pub(crate) row_chunk_starts: &'a ::wgpu::Buffer,
    pub(crate) row_chunk_ends: &'a ::wgpu::Buffer,
    pub(crate) backdrops: &'a ::wgpu::Buffer,
    pub(crate) chunk_totals: &'a ::wgpu::Buffer,
    pub(crate) chunk_offsets: &'a ::wgpu::Buffer,
}

pub(crate) struct WgpuScanBindings<'a> {
    pub(crate) line_path_ids: &'a ::wgpu::Buffer,
    pub(crate) line_p0x: &'a ::wgpu::Buffer,
    pub(crate) line_p0y: &'a ::wgpu::Buffer,
    pub(crate) line_p1x: &'a ::wgpu::Buffer,
    pub(crate) line_p1y: &'a ::wgpu::Buffer,
    pub(crate) path_flags: &'a ::wgpu::Buffer,
    pub(crate) backdrop_data_offsets: &'a ::wgpu::Buffer,
    pub(crate) backdrop_tile_x0: &'a ::wgpu::Buffer,
    pub(crate) backdrop_tile_y0: &'a ::wgpu::Buffer,
    pub(crate) backdrop_tile_x1: &'a ::wgpu::Buffer,
    pub(crate) backdrop_tile_y1: &'a ::wgpu::Buffer,
    pub(crate) scan_chunk_backdrop_offsets: &'a ::wgpu::Buffer,
    pub(crate) scan_chunk_lens: &'a ::wgpu::Buffer,
    pub(crate) scan_chunk_range_starts: &'a ::wgpu::Buffer,
    pub(crate) scan_chunk_range_ends: &'a ::wgpu::Buffer,
    pub(crate) backdrop_segment_starts: &'a ::wgpu::Buffer,
    pub(crate) backdrops: &'a ::wgpu::Buffer,
    pub(crate) tile_segment_range_starts: &'a ::wgpu::Buffer,
    pub(crate) tile_segment_range_ends: &'a ::wgpu::Buffer,
    pub(crate) segment_tile_counts: &'a ::wgpu::Buffer,
    pub(crate) segment_tile_cursors: &'a ::wgpu::Buffer,
    pub(crate) segment_bumps: &'a ::wgpu::Buffer,
    pub(crate) chunk_totals: &'a ::wgpu::Buffer,
    pub(crate) chunk_offsets: &'a ::wgpu::Buffer,
    pub(crate) segment_p0x: &'a ::wgpu::Buffer,
    pub(crate) segment_p0y: &'a ::wgpu::Buffer,
    pub(crate) segment_p1x: &'a ::wgpu::Buffer,
    pub(crate) segment_p1y: &'a ::wgpu::Buffer,
    pub(crate) segment_y_edge: &'a ::wgpu::Buffer,
}

pub(crate) struct WgpuCoarseBindings<'a> {
    pub(crate) draw_path_ids: &'a ::wgpu::Buffer,
    pub(crate) draw_glyph_run_ids: &'a ::wgpu::Buffer,
    pub(crate) glyph_run_starts: &'a ::wgpu::Buffer,
    pub(crate) glyph_run_counts: &'a ::wgpu::Buffer,
    pub(crate) glyph_image_ids: &'a ::wgpu::Buffer,
    pub(crate) glyph_x: &'a ::wgpu::Buffer,
    pub(crate) glyph_y: &'a ::wgpu::Buffer,
    pub(crate) glyph_image_left: &'a ::wgpu::Buffer,
    pub(crate) glyph_image_top: &'a ::wgpu::Buffer,
    pub(crate) glyph_image_width: &'a ::wgpu::Buffer,
    pub(crate) glyph_image_height: &'a ::wgpu::Buffer,
    pub(crate) draw_flags: &'a ::wgpu::Buffer,
    pub(crate) draw_brush_colors: &'a ::wgpu::Buffer,
    pub(crate) draw_pixel_x0: &'a ::wgpu::Buffer,
    pub(crate) draw_pixel_y0: &'a ::wgpu::Buffer,
    pub(crate) draw_pixel_x1: &'a ::wgpu::Buffer,
    pub(crate) draw_pixel_y1: &'a ::wgpu::Buffer,
    pub(crate) backdrop_data_offsets: &'a ::wgpu::Buffer,
    pub(crate) backdrop_tile_x0: &'a ::wgpu::Buffer,
    pub(crate) backdrop_tile_y0: &'a ::wgpu::Buffer,
    pub(crate) backdrop_tile_x1: &'a ::wgpu::Buffer,
    pub(crate) backdrop_tile_y1: &'a ::wgpu::Buffer,
    pub(crate) backdrops: &'a ::wgpu::Buffer,
    pub(crate) segment_starts: &'a ::wgpu::Buffer,
    pub(crate) segment_ends: &'a ::wgpu::Buffer,
    pub(crate) layer_stack_tags: &'a ::wgpu::Buffer,
    pub(crate) layer_stack_draws: &'a ::wgpu::Buffer,
    pub(crate) layer_stack_payloads: &'a ::wgpu::Buffer,
    pub(crate) tile_ptcl_counts: &'a ::wgpu::Buffer,
    pub(crate) tile_ptcl_range_starts: &'a ::wgpu::Buffer,
    pub(crate) tile_ptcl_range_ends: &'a ::wgpu::Buffer,
    pub(crate) tile_glyph_counts: &'a ::wgpu::Buffer,
    pub(crate) tile_glyph_range_starts: &'a ::wgpu::Buffer,
    pub(crate) tile_glyph_range_ends: &'a ::wgpu::Buffer,
    pub(crate) chunk_totals: &'a ::wgpu::Buffer,
    pub(crate) chunk_offsets: &'a ::wgpu::Buffer,
    pub(crate) glyph_chunk_totals: &'a ::wgpu::Buffer,
    pub(crate) glyph_chunk_offsets: &'a ::wgpu::Buffer,
    pub(crate) ptcl_tags: &'a ::wgpu::Buffer,
    pub(crate) ptcl_backdrops: &'a ::wgpu::Buffer,
    pub(crate) ptcl_fill_rules: &'a ::wgpu::Buffer,
    pub(crate) ptcl_segment_starts: &'a ::wgpu::Buffer,
    pub(crate) ptcl_segment_ends: &'a ::wgpu::Buffer,
    pub(crate) ptcl_colors: &'a ::wgpu::Buffer,
    pub(crate) glyph_indices: &'a ::wgpu::Buffer,
}

pub(crate) struct WgpuScanBuffers {
    pub(crate) backdrops: WgpuBuffer,
    pub(crate) tile_segment_range_starts: WgpuBuffer,
    pub(crate) tile_segment_range_ends: WgpuBuffer,
    pub(crate) segment_p0x: WgpuBuffer,
    pub(crate) segment_p0y: WgpuBuffer,
    pub(crate) segment_p1x: WgpuBuffer,
    pub(crate) segment_p1y: WgpuBuffer,
    pub(crate) segment_y_edge: WgpuBuffer,
    pub(crate) segment_tile_counts: WgpuBuffer,
    pub(crate) segment_tile_cursors: WgpuBuffer,
    pub(crate) segment_bumps: WgpuBuffer,
    pub(crate) chunk_totals: WgpuBuffer,
    pub(crate) chunk_offsets: WgpuBuffer,
    pub(crate) cumsum_chunk_totals: WgpuBuffer,
    pub(crate) cumsum_chunk_offsets: WgpuBuffer,
}

impl WgpuScanBuffers {
    pub(crate) fn new(device: &::wgpu::Device) -> Self {
        Self {
            backdrops: WgpuBuffer::new(device, "tileink wgpu scan backdrops"),
            tile_segment_range_starts: WgpuBuffer::new(
                device,
                "tileink wgpu scan tile segment range starts",
            ),
            tile_segment_range_ends: WgpuBuffer::new(
                device,
                "tileink wgpu scan tile segment range ends",
            ),
            segment_p0x: WgpuBuffer::new(device, "tileink wgpu scan segment p0x"),
            segment_p0y: WgpuBuffer::new(device, "tileink wgpu scan segment p0y"),
            segment_p1x: WgpuBuffer::new(device, "tileink wgpu scan segment p1x"),
            segment_p1y: WgpuBuffer::new(device, "tileink wgpu scan segment p1y"),
            segment_y_edge: WgpuBuffer::new(device, "tileink wgpu scan segment y edge"),
            segment_tile_counts: WgpuBuffer::new(device, "tileink wgpu scan segment tile counts"),
            segment_tile_cursors: WgpuBuffer::new(device, "tileink wgpu scan segment tile cursors"),
            segment_bumps: WgpuBuffer::new(device, "tileink wgpu scan segment bumps"),
            chunk_totals: WgpuBuffer::new(device, "tileink wgpu scan chunk totals"),
            chunk_offsets: WgpuBuffer::new(device, "tileink wgpu scan chunk offsets"),
            cumsum_chunk_totals: WgpuBuffer::new(device, "tileink wgpu scan cumsum chunk totals"),
            cumsum_chunk_offsets: WgpuBuffer::new(device, "tileink wgpu scan cumsum chunk offsets"),
        }
    }

    pub(crate) fn prepare_outputs(&mut self, device: &::wgpu::Device, lengths: GpuBufferLengths) {
        self.backdrops.resize_uninit::<i32>(
            device,
            "tileink wgpu scan backdrops",
            lengths.backdrop_len,
        );
        self.tile_segment_range_starts.resize_uninit::<u32>(
            device,
            "tileink wgpu scan tile segment range starts",
            lengths.backdrop_len,
        );
        self.tile_segment_range_ends.resize_uninit::<u32>(
            device,
            "tileink wgpu scan tile segment range ends",
            lengths.backdrop_len,
        );
        self.segment_p0x.resize_uninit::<f32>(
            device,
            "tileink wgpu scan segment p0x",
            lengths.segment_capacity,
        );
        self.segment_p0y.resize_uninit::<f32>(
            device,
            "tileink wgpu scan segment p0y",
            lengths.segment_capacity,
        );
        self.segment_p1x.resize_uninit::<f32>(
            device,
            "tileink wgpu scan segment p1x",
            lengths.segment_capacity,
        );
        self.segment_p1y.resize_uninit::<f32>(
            device,
            "tileink wgpu scan segment p1y",
            lengths.segment_capacity,
        );
        self.segment_y_edge.resize_uninit::<f32>(
            device,
            "tileink wgpu scan segment y edge",
            lengths.segment_capacity,
        );
        self.segment_tile_counts.resize_uninit::<u32>(
            device,
            "tileink wgpu scan segment tile counts",
            lengths.backdrop_len,
        );
        self.segment_tile_cursors.resize_uninit::<u32>(
            device,
            "tileink wgpu scan segment tile cursors",
            lengths.backdrop_len,
        );
        self.segment_bumps.resize_uninit::<u32>(
            device,
            "tileink wgpu scan segment bumps",
            lengths.path_count,
        );
        self.chunk_totals.resize_uninit::<u32>(
            device,
            "tileink wgpu scan chunk totals",
            lengths.scan_chunk_count,
        );
        self.chunk_offsets.resize_uninit::<u32>(
            device,
            "tileink wgpu scan chunk offsets",
            lengths.scan_chunk_count,
        );
        self.cumsum_chunk_totals.resize_uninit::<i32>(
            device,
            "tileink wgpu scan cumsum chunk totals",
            lengths.cumsum_chunk_count,
        );
        self.cumsum_chunk_offsets.resize_uninit::<i32>(
            device,
            "tileink wgpu scan cumsum chunk offsets",
            lengths.cumsum_chunk_count,
        );
    }
}

pub(crate) struct WgpuCoarseBuffers {
    pub(crate) tile_ptcl_range_starts: WgpuBuffer,
    pub(crate) tile_ptcl_range_ends: WgpuBuffer,
    pub(crate) tile_ptcl_counts: WgpuBuffer,
    pub(crate) tile_glyph_range_starts: WgpuBuffer,
    pub(crate) tile_glyph_range_ends: WgpuBuffer,
    pub(crate) tile_glyph_counts: WgpuBuffer,
    pub(crate) chunk_totals: WgpuBuffer,
    pub(crate) chunk_offsets: WgpuBuffer,
    pub(crate) glyph_chunk_totals: WgpuBuffer,
    pub(crate) glyph_chunk_offsets: WgpuBuffer,
    pub(crate) ptcl_tags: WgpuBuffer,
    pub(crate) ptcl_backdrops: WgpuBuffer,
    pub(crate) ptcl_fill_rules: WgpuBuffer,
    pub(crate) ptcl_segment_starts: WgpuBuffer,
    pub(crate) ptcl_segment_ends: WgpuBuffer,
    pub(crate) ptcl_colors: WgpuBuffer,
    pub(crate) glyph_indices: WgpuBuffer,
}

impl WgpuCoarseBuffers {
    pub(crate) fn new(device: &::wgpu::Device) -> Self {
        Self {
            tile_ptcl_range_starts: WgpuBuffer::new(
                device,
                "tileink wgpu coarse tile ptcl range starts",
            ),
            tile_ptcl_range_ends: WgpuBuffer::new(
                device,
                "tileink wgpu coarse tile ptcl range ends",
            ),
            tile_ptcl_counts: WgpuBuffer::new(device, "tileink wgpu coarse tile ptcl counts"),
            tile_glyph_range_starts: WgpuBuffer::new(
                device,
                "tileink wgpu coarse tile glyph range starts",
            ),
            tile_glyph_range_ends: WgpuBuffer::new(
                device,
                "tileink wgpu coarse tile glyph range ends",
            ),
            tile_glyph_counts: WgpuBuffer::new(device, "tileink wgpu coarse tile glyph counts"),
            chunk_totals: WgpuBuffer::new(device, "tileink wgpu coarse chunk totals"),
            chunk_offsets: WgpuBuffer::new(device, "tileink wgpu coarse chunk offsets"),
            glyph_chunk_totals: WgpuBuffer::new(device, "tileink wgpu coarse glyph chunk totals"),
            glyph_chunk_offsets: WgpuBuffer::new(device, "tileink wgpu coarse glyph chunk offsets"),
            ptcl_tags: WgpuBuffer::new(device, "tileink wgpu coarse ptcl tags"),
            ptcl_backdrops: WgpuBuffer::new(device, "tileink wgpu coarse ptcl backdrops"),
            ptcl_fill_rules: WgpuBuffer::new(device, "tileink wgpu coarse ptcl fill rules"),
            ptcl_segment_starts: WgpuBuffer::new(device, "tileink wgpu coarse ptcl segment starts"),
            ptcl_segment_ends: WgpuBuffer::new(device, "tileink wgpu coarse ptcl segment ends"),
            ptcl_colors: WgpuBuffer::new(device, "tileink wgpu coarse ptcl colors"),
            glyph_indices: WgpuBuffer::new(device, "tileink wgpu coarse glyph indices"),
        }
    }

    pub(crate) fn prepare_outputs(&mut self, device: &::wgpu::Device, lengths: GpuBufferLengths) {
        self.tile_ptcl_range_starts.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse tile ptcl range starts",
            lengths.tile_count,
        );
        self.tile_ptcl_range_ends.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse tile ptcl range ends",
            lengths.tile_count,
        );
        self.tile_ptcl_counts.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse tile ptcl counts",
            lengths.tile_count,
        );
        self.tile_glyph_range_starts.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse tile glyph range starts",
            lengths.tile_count,
        );
        self.tile_glyph_range_ends.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse tile glyph range ends",
            lengths.tile_count,
        );
        self.tile_glyph_counts.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse tile glyph counts",
            lengths.tile_count,
        );
        self.chunk_totals.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse chunk totals",
            lengths.coarse_chunk_count,
        );
        self.chunk_offsets.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse chunk offsets",
            lengths.coarse_chunk_count,
        );
        self.glyph_chunk_totals.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse glyph chunk totals",
            lengths.coarse_chunk_count,
        );
        self.glyph_chunk_offsets.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse glyph chunk offsets",
            lengths.coarse_chunk_count,
        );
        self.ptcl_tags.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse ptcl tags",
            packed_u8_len(lengths.coarse_ptcl_capacity),
        );
        self.ptcl_backdrops.resize_uninit::<i32>(
            device,
            "tileink wgpu coarse ptcl backdrops",
            lengths.coarse_ptcl_capacity,
        );
        self.ptcl_fill_rules.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse ptcl fill rules",
            lengths.coarse_ptcl_capacity,
        );
        self.ptcl_segment_starts.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse ptcl segment starts",
            lengths.coarse_ptcl_capacity,
        );
        self.ptcl_segment_ends.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse ptcl segment ends",
            lengths.coarse_ptcl_capacity,
        );
        self.ptcl_colors.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse ptcl colors",
            lengths.coarse_ptcl_capacity,
        );
        self.glyph_indices.resize_uninit::<u32>(
            device,
            "tileink wgpu coarse glyph indices",
            lengths.coarse_glyph_capacity,
        );
    }
}
