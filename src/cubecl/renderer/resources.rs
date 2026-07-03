use crate::{
    scene::Scene,
    shared::{
        bd_record::BackdropRecord,
        execution::{ExecPlan, LayerStackEntry},
        image::rgba8_pack,
        pixel::mul_div255,
    },
    text::{AtlasSignature, PreparedGlyphContent, PreparedTextData, TextCompositeMode},
};
use ::cubecl::prelude::Runtime;

use crate::cubecl::{
    buffer::CubeBuffer,
    types::{
        CUBE_GLYPH_COLOR, CUBE_GLYPH_LINEAR_COLOR, CUBE_GLYPH_LINEAR_MASK,
        CUBE_GLYPH_LINEAR_SUBPIXEL_MASK, CUBE_GLYPH_MASK, CUBE_GLYPH_SUBPIXEL_MASK,
        CUBE_LAYER_BLEND, CUBE_LAYER_CLIP, CUBE_LAYER_OPACITY, CubeBufferLengths, CubeCumsumPlan,
        CubeScanChunk, CubeScanChunkRange, build_cumsum_plan_into, build_scan_chunks_into,
    },
};
#[cfg(feature = "profile")]
use crate::shared::memory::MemoryUsage;

use super::executor::encode_layer_payload;

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
                        TextCompositeMode::Srgb => CUBE_GLYPH_MASK,
                        TextCompositeMode::Linear => CUBE_GLYPH_LINEAR_MASK,
                    });
                    self.image_data
                        .extend(image.data.iter().map(|&alpha| alpha as u32));
                }
                PreparedGlyphContent::Color => {
                    self.image_content.push(match image.composite_mode {
                        TextCompositeMode::Srgb => CUBE_GLYPH_COLOR,
                        TextCompositeMode::Linear => CUBE_GLYPH_LINEAR_COLOR,
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
                        TextCompositeMode::Srgb => CUBE_GLYPH_SUBPIXEL_MASK,
                        TextCompositeMode::Linear => CUBE_GLYPH_LINEAR_SUBPIXEL_MASK,
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

    #[cfg(feature = "profile")]
    fn memory_usage(&self) -> MemoryUsage {
        MemoryUsage::sum([
            MemoryUsage::vec(&self.run_starts),
            MemoryUsage::vec(&self.run_counts),
            MemoryUsage::vec(&self.glyph_image_ids),
            MemoryUsage::vec(&self.glyph_x),
            MemoryUsage::vec(&self.glyph_y),
            MemoryUsage::vec(&self.image_left),
            MemoryUsage::vec(&self.image_top),
            MemoryUsage::vec(&self.image_width),
            MemoryUsage::vec(&self.image_height),
            MemoryUsage::vec(&self.image_content),
            MemoryUsage::vec(&self.image_data_offsets),
            MemoryUsage::vec(&self.image_data),
        ])
    }
}

/// Reusable CPU-side staging for columnar scene uploads.
///
/// CubeCL 0.10 uploads immutable inputs by creating handles from slices, so
/// input GPU handles are still replaced per scene. This staging owner removes
/// renderer-side per-column `Vec` allocation while keeping the upload layout explicit.
#[derive(Default)]
pub(super) struct SceneUploadStaging {
    u32s: Vec<u32>,
    text: TextUpload,
    scan_chunks: Vec<CubeScanChunk>,
    scan_chunk_ranges: Vec<CubeScanChunkRange>,
    cumsum_plan: CubeCumsumPlan,
}

impl SceneUploadStaging {
    #[cfg(feature = "profile")]
    pub(super) fn memory_usage(&self) -> MemoryUsage {
        MemoryUsage::sum([
            MemoryUsage::vec(&self.u32s),
            self.text.memory_usage(),
            MemoryUsage::vec(&self.scan_chunks),
            MemoryUsage::vec(&self.scan_chunk_ranges),
            MemoryUsage::vec(&self.cumsum_plan.chunk_backdrop_offsets),
            MemoryUsage::vec(&self.cumsum_plan.chunk_lens),
            MemoryUsage::vec(&self.cumsum_plan.row_chunk_starts),
            MemoryUsage::vec(&self.cumsum_plan.row_chunk_ends),
        ])
    }
}

fn upload_mapped_u32<R: Runtime, T>(
    client: &::cubecl::client::ComputeClient<R>,
    buffer: &mut CubeBuffer<u32>,
    scratch: &mut Vec<u32>,
    items: &[T],
    map: impl FnMut(&T) -> u32,
) {
    scratch.clear();
    scratch.reserve(items.len());
    scratch.extend(items.iter().map(map));
    buffer.replace(client, scratch);
}

fn packed_u8_len(len: usize) -> usize {
    len.div_ceil(4)
}

pub(crate) struct SceneBuffers {
    pub(crate) line_path_ids: CubeBuffer<u32>,
    pub(crate) line_p0x: CubeBuffer<f32>,
    pub(crate) line_p0y: CubeBuffer<f32>,
    pub(crate) line_p1x: CubeBuffer<f32>,
    pub(crate) line_p1y: CubeBuffer<f32>,
    /// Per-path scan flags, indexed by `path_id`.
    pub(crate) path_flags: CubeBuffer<u32>,
    pub(crate) draw_path_ids: CubeBuffer<u32>,
    pub(crate) draw_glyph_run_ids: CubeBuffer<u32>,
    /// Per-draw flags, indexed by `draw_id`.
    ///
    /// - bits 0..=2: draw tag
    /// - bit 3: even-odd fill rule
    /// - bit 4: solid rectangle
    /// - bit 5: solid color full-tile fast path
    /// - bit 6: draw has an SDF payload
    /// - bit 7: draw has a glyph run
    pub(crate) draw_flags: CubeBuffer<u32>,
    pub(crate) draw_brush_colors: CubeBuffer<u32>,
    pub(crate) draw_pixel_x0: CubeBuffer<i32>,
    pub(crate) draw_pixel_y0: CubeBuffer<i32>,
    pub(crate) draw_pixel_x1: CubeBuffer<i32>,
    pub(crate) draw_pixel_y1: CubeBuffer<i32>,
    /// Draw-to-SDF indirection. `u32::MAX` means the draw has no SDF payload.
    ///
    /// Kernels still receive draw indices from particles and layer stacks, so
    /// this compact reference buffer preserves those contracts while the large
    /// SDF parameter columns are sized by `sdf_count` instead of `draw_count`.
    pub(crate) draw_sdf_refs: CubeBuffer<u32>,
    pub(crate) sdf_kinds: CubeBuffer<u32>,
    pub(crate) sdf_x0: CubeBuffer<f32>,
    pub(crate) sdf_y0: CubeBuffer<f32>,
    pub(crate) sdf_x1: CubeBuffer<f32>,
    pub(crate) sdf_y1: CubeBuffer<f32>,
    pub(crate) sdf_r0: CubeBuffer<f32>,
    pub(crate) sdf_r1: CubeBuffer<f32>,
    pub(crate) sdf_r2: CubeBuffer<f32>,
    pub(crate) sdf_r3: CubeBuffer<f32>,
    pub(crate) sdf_stroke_top: CubeBuffer<f32>,
    pub(crate) sdf_stroke_right: CubeBuffer<f32>,
    pub(crate) sdf_stroke_bottom: CubeBuffer<f32>,
    pub(crate) sdf_stroke_left: CubeBuffer<f32>,
    pub(crate) sdf_shadow_offset_x: CubeBuffer<f32>,
    pub(crate) sdf_shadow_offset_y: CubeBuffer<f32>,
    pub(crate) sdf_shadow_expand: CubeBuffer<f32>,
    pub(crate) sdf_shadow_intensity: CubeBuffer<f32>,
    pub(crate) backdrop_data_offsets: CubeBuffer<u32>,
    pub(crate) backdrop_data_lens: CubeBuffer<u32>,
    pub(crate) backdrop_tile_x0: CubeBuffer<u32>,
    pub(crate) backdrop_tile_y0: CubeBuffer<u32>,
    pub(crate) backdrop_tile_x1: CubeBuffer<u32>,
    pub(crate) backdrop_tile_y1: CubeBuffer<u32>,
    pub(crate) backdrop_segment_starts: CubeBuffer<u32>,
    pub(crate) backdrop_segment_capacities: CubeBuffer<u32>,
    pub(crate) scan_chunk_path_ids: CubeBuffer<u32>,
    pub(crate) scan_chunk_backdrop_offsets: CubeBuffer<u32>,
    pub(crate) scan_chunk_segment_starts: CubeBuffer<u32>,
    pub(crate) scan_chunk_lens: CubeBuffer<u32>,
    pub(crate) scan_chunk_range_starts: CubeBuffer<u32>,
    pub(crate) scan_chunk_range_ends: CubeBuffer<u32>,
    pub(crate) cumsum_chunk_backdrop_offsets: CubeBuffer<u32>,
    pub(crate) cumsum_chunk_lens: CubeBuffer<u32>,
    pub(crate) cumsum_row_chunk_starts: CubeBuffer<u32>,
    pub(crate) cumsum_row_chunk_ends: CubeBuffer<u32>,
    pub(crate) plan_layer_stack_tags: CubeBuffer<u32>,
    pub(crate) plan_layer_stack_draws: CubeBuffer<u32>,
    pub(crate) plan_layer_stack_payloads: CubeBuffer<u32>,
    pub(crate) glyph_run_starts: CubeBuffer<u32>,
    pub(crate) glyph_run_counts: CubeBuffer<u32>,
    pub(crate) glyph_image_ids: CubeBuffer<u32>,
    pub(crate) glyph_x: CubeBuffer<i32>,
    pub(crate) glyph_y: CubeBuffer<i32>,
    pub(crate) glyph_image_left: CubeBuffer<i32>,
    pub(crate) glyph_image_top: CubeBuffer<i32>,
    pub(crate) glyph_image_width: CubeBuffer<u32>,
    pub(crate) glyph_image_height: CubeBuffer<u32>,
    pub(crate) glyph_image_content: CubeBuffer<u32>,
    pub(crate) glyph_image_data_offsets: CubeBuffer<u32>,
    pub(crate) glyph_image_data: CubeBuffer<u32>,
    glyph_atlas_signature: AtlasSignature,
}

impl SceneBuffers {
    pub(super) fn new<R: Runtime>(client: &::cubecl::client::ComputeClient<R>) -> Self {
        Self {
            line_path_ids: CubeBuffer::new(client, 0),
            line_p0x: CubeBuffer::new(client, 0),
            line_p0y: CubeBuffer::new(client, 0),
            line_p1x: CubeBuffer::new(client, 0),
            line_p1y: CubeBuffer::new(client, 0),
            path_flags: CubeBuffer::new(client, 0),
            draw_path_ids: CubeBuffer::new(client, 0),
            draw_glyph_run_ids: CubeBuffer::new(client, 0),
            draw_flags: CubeBuffer::new(client, 0),
            draw_brush_colors: CubeBuffer::new(client, 0),
            draw_pixel_x0: CubeBuffer::new(client, 0),
            draw_pixel_y0: CubeBuffer::new(client, 0),
            draw_pixel_x1: CubeBuffer::new(client, 0),
            draw_pixel_y1: CubeBuffer::new(client, 0),
            draw_sdf_refs: CubeBuffer::new(client, 0),
            sdf_kinds: CubeBuffer::new(client, 0),
            sdf_x0: CubeBuffer::new(client, 0),
            sdf_y0: CubeBuffer::new(client, 0),
            sdf_x1: CubeBuffer::new(client, 0),
            sdf_y1: CubeBuffer::new(client, 0),
            sdf_r0: CubeBuffer::new(client, 0),
            sdf_r1: CubeBuffer::new(client, 0),
            sdf_r2: CubeBuffer::new(client, 0),
            sdf_r3: CubeBuffer::new(client, 0),
            sdf_stroke_top: CubeBuffer::new(client, 0),
            sdf_stroke_right: CubeBuffer::new(client, 0),
            sdf_stroke_bottom: CubeBuffer::new(client, 0),
            sdf_stroke_left: CubeBuffer::new(client, 0),
            sdf_shadow_offset_x: CubeBuffer::new(client, 0),
            sdf_shadow_offset_y: CubeBuffer::new(client, 0),
            sdf_shadow_expand: CubeBuffer::new(client, 0),
            sdf_shadow_intensity: CubeBuffer::new(client, 0),
            backdrop_data_offsets: CubeBuffer::new(client, 0),
            backdrop_data_lens: CubeBuffer::new(client, 0),
            backdrop_tile_x0: CubeBuffer::new(client, 0),
            backdrop_tile_y0: CubeBuffer::new(client, 0),
            backdrop_tile_x1: CubeBuffer::new(client, 0),
            backdrop_tile_y1: CubeBuffer::new(client, 0),
            backdrop_segment_starts: CubeBuffer::new(client, 0),
            backdrop_segment_capacities: CubeBuffer::new(client, 0),
            scan_chunk_path_ids: CubeBuffer::new(client, 0),
            scan_chunk_backdrop_offsets: CubeBuffer::new(client, 0),
            scan_chunk_segment_starts: CubeBuffer::new(client, 0),
            scan_chunk_lens: CubeBuffer::new(client, 0),
            scan_chunk_range_starts: CubeBuffer::new(client, 0),
            scan_chunk_range_ends: CubeBuffer::new(client, 0),
            cumsum_chunk_backdrop_offsets: CubeBuffer::new(client, 0),
            cumsum_chunk_lens: CubeBuffer::new(client, 0),
            cumsum_row_chunk_starts: CubeBuffer::new(client, 0),
            cumsum_row_chunk_ends: CubeBuffer::new(client, 0),
            plan_layer_stack_tags: CubeBuffer::new(client, 0),
            plan_layer_stack_draws: CubeBuffer::new(client, 0),
            plan_layer_stack_payloads: CubeBuffer::new(client, 0),
            glyph_run_starts: CubeBuffer::new(client, 0),
            glyph_run_counts: CubeBuffer::new(client, 0),
            glyph_image_ids: CubeBuffer::new(client, 0),
            glyph_x: CubeBuffer::new(client, 0),
            glyph_y: CubeBuffer::new(client, 0),
            glyph_image_left: CubeBuffer::new(client, 0),
            glyph_image_top: CubeBuffer::new(client, 0),
            glyph_image_width: CubeBuffer::new(client, 0),
            glyph_image_height: CubeBuffer::new(client, 0),
            glyph_image_content: CubeBuffer::new(client, 0),
            glyph_image_data_offsets: CubeBuffer::new(client, 0),
            glyph_image_data: CubeBuffer::new(client, 0),
            glyph_atlas_signature: AtlasSignature::default(),
        }
    }

    #[cfg(feature = "profile")]
    pub(super) fn memory_usage(&self) -> MemoryUsage {
        MemoryUsage::sum([
            self.line_path_ids.memory_usage(),
            self.line_p0x.memory_usage(),
            self.line_p0y.memory_usage(),
            self.line_p1x.memory_usage(),
            self.line_p1y.memory_usage(),
            self.path_flags.memory_usage(),
            self.draw_path_ids.memory_usage(),
            self.draw_glyph_run_ids.memory_usage(),
            self.draw_flags.memory_usage(),
            self.draw_brush_colors.memory_usage(),
            self.draw_pixel_x0.memory_usage(),
            self.draw_pixel_y0.memory_usage(),
            self.draw_pixel_x1.memory_usage(),
            self.draw_pixel_y1.memory_usage(),
            self.draw_sdf_refs.memory_usage(),
            self.sdf_kinds.memory_usage(),
            self.sdf_x0.memory_usage(),
            self.sdf_y0.memory_usage(),
            self.sdf_x1.memory_usage(),
            self.sdf_y1.memory_usage(),
            self.sdf_r0.memory_usage(),
            self.sdf_r1.memory_usage(),
            self.sdf_r2.memory_usage(),
            self.sdf_r3.memory_usage(),
            self.sdf_stroke_top.memory_usage(),
            self.sdf_stroke_right.memory_usage(),
            self.sdf_stroke_bottom.memory_usage(),
            self.sdf_stroke_left.memory_usage(),
            self.sdf_shadow_offset_x.memory_usage(),
            self.sdf_shadow_offset_y.memory_usage(),
            self.sdf_shadow_expand.memory_usage(),
            self.sdf_shadow_intensity.memory_usage(),
            self.backdrop_data_offsets.memory_usage(),
            self.backdrop_data_lens.memory_usage(),
            self.backdrop_tile_x0.memory_usage(),
            self.backdrop_tile_y0.memory_usage(),
            self.backdrop_tile_x1.memory_usage(),
            self.backdrop_tile_y1.memory_usage(),
            self.backdrop_segment_starts.memory_usage(),
            self.backdrop_segment_capacities.memory_usage(),
            self.scan_chunk_path_ids.memory_usage(),
            self.scan_chunk_backdrop_offsets.memory_usage(),
            self.scan_chunk_segment_starts.memory_usage(),
            self.scan_chunk_lens.memory_usage(),
            self.scan_chunk_range_starts.memory_usage(),
            self.scan_chunk_range_ends.memory_usage(),
            self.cumsum_chunk_backdrop_offsets.memory_usage(),
            self.cumsum_chunk_lens.memory_usage(),
            self.cumsum_row_chunk_starts.memory_usage(),
            self.cumsum_row_chunk_ends.memory_usage(),
            self.plan_layer_stack_tags.memory_usage(),
            self.plan_layer_stack_draws.memory_usage(),
            self.plan_layer_stack_payloads.memory_usage(),
            self.glyph_run_starts.memory_usage(),
            self.glyph_run_counts.memory_usage(),
            self.glyph_image_ids.memory_usage(),
            self.glyph_x.memory_usage(),
            self.glyph_y.memory_usage(),
            self.glyph_image_left.memory_usage(),
            self.glyph_image_top.memory_usage(),
            self.glyph_image_width.memory_usage(),
            self.glyph_image_height.memory_usage(),
            self.glyph_image_content.memory_usage(),
            self.glyph_image_data_offsets.memory_usage(),
            self.glyph_image_data.memory_usage(),
        ])
    }

    pub(super) fn upload<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        scene: &Scene,
        plan: &ExecPlan,
        text: Option<&PreparedTextData>,
        staging: &mut SceneUploadStaging,
    ) {
        build_scan_chunks_into(
            scene,
            &mut staging.scan_chunks,
            &mut staging.scan_chunk_ranges,
        );
        build_cumsum_plan_into(scene, &mut staging.cumsum_plan);
        self.upload_path_geometry(client, scene);
        self.upload_draws(client, scene, text.is_some());
        self.upload_backdrops(client, &scene.bd_records, staging);
        self.upload_plan_layer_stack(client, &plan.layer_stack_data, staging);
        self.upload_text(client, scene, text, staging);

        upload_mapped_u32(
            client,
            &mut self.scan_chunk_path_ids,
            &mut staging.u32s,
            &staging.scan_chunks,
            |chunk| chunk.path_id,
        );
        upload_mapped_u32(
            client,
            &mut self.scan_chunk_backdrop_offsets,
            &mut staging.u32s,
            &staging.scan_chunks,
            |chunk| chunk.backdrop_offset,
        );
        upload_mapped_u32(
            client,
            &mut self.scan_chunk_segment_starts,
            &mut staging.u32s,
            &staging.scan_chunks,
            |chunk| chunk.segment_start,
        );
        upload_mapped_u32(
            client,
            &mut self.scan_chunk_lens,
            &mut staging.u32s,
            &staging.scan_chunks,
            |chunk| chunk.len,
        );
        upload_mapped_u32(
            client,
            &mut self.scan_chunk_range_starts,
            &mut staging.u32s,
            &staging.scan_chunk_ranges,
            |range| range.start,
        );
        upload_mapped_u32(
            client,
            &mut self.scan_chunk_range_ends,
            &mut staging.u32s,
            &staging.scan_chunk_ranges,
            |range| range.end,
        );
        self.cumsum_chunk_backdrop_offsets
            .replace(client, &staging.cumsum_plan.chunk_backdrop_offsets);
        self.cumsum_chunk_lens
            .replace(client, &staging.cumsum_plan.chunk_lens);
        self.cumsum_row_chunk_starts
            .replace(client, &staging.cumsum_plan.row_chunk_starts);
        self.cumsum_row_chunk_ends
            .replace(client, &staging.cumsum_plan.row_chunk_ends);
    }

    fn upload_plan_layer_stack<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        layer_stack: &[LayerStackEntry],
        staging: &mut SceneUploadStaging,
    ) {
        upload_mapped_u32(
            client,
            &mut self.plan_layer_stack_tags,
            &mut staging.u32s,
            layer_stack,
            |entry| match entry {
                LayerStackEntry::Clip { .. } => CUBE_LAYER_CLIP,
                LayerStackEntry::Opacity { .. } => CUBE_LAYER_OPACITY,
                LayerStackEntry::Blend { .. } => CUBE_LAYER_BLEND,
            },
        );
        upload_mapped_u32(
            client,
            &mut self.plan_layer_stack_draws,
            &mut staging.u32s,
            layer_stack,
            |entry| match *entry {
                LayerStackEntry::Clip { draw }
                | LayerStackEntry::Opacity { draw, .. }
                | LayerStackEntry::Blend { draw, .. } => draw,
            },
        );
        upload_mapped_u32(
            client,
            &mut self.plan_layer_stack_payloads,
            &mut staging.u32s,
            layer_stack,
            |entry| encode_layer_payload(*entry),
        );
    }

    fn upload_path_geometry<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        scene: &Scene,
    ) {
        let columns = &scene.columns;
        self.line_path_ids.replace(client, &columns.line_path_ids);
        self.line_p0x.replace(client, &columns.line_p0x);
        self.line_p0y.replace(client, &columns.line_p0y);
        self.line_p1x.replace(client, &columns.line_p1x);
        self.line_p1y.replace(client, &columns.line_p1y);
        self.path_flags.replace(client, &columns.path_flags);
    }

    fn upload_draws<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        scene: &Scene,
        text_enabled: bool,
    ) {
        let columns = &scene.columns;
        self.draw_path_ids.replace(client, &columns.draw_path_ids);
        self.draw_glyph_run_ids.replace(
            client,
            if text_enabled {
                &columns.draw_glyph_run_ids
            } else {
                &columns.draw_glyph_run_ids_without_text
            },
        );
        self.draw_flags.replace(
            client,
            if text_enabled {
                &columns.draw_flags
            } else {
                &columns.draw_flags_without_text
            },
        );
        self.draw_brush_colors
            .replace(client, &columns.draw_brush_colors);
        self.draw_pixel_x0.replace(client, &columns.draw_pixel_x0);
        self.draw_pixel_y0.replace(client, &columns.draw_pixel_y0);
        self.draw_pixel_x1.replace(client, &columns.draw_pixel_x1);
        self.draw_pixel_y1.replace(client, &columns.draw_pixel_y1);
        self.draw_sdf_refs.replace(client, &columns.sdf.refs);
        self.sdf_kinds.replace(client, &columns.sdf.kinds);
        self.sdf_x0.replace(client, &columns.sdf.x0);
        self.sdf_y0.replace(client, &columns.sdf.y0);
        self.sdf_x1.replace(client, &columns.sdf.x1);
        self.sdf_y1.replace(client, &columns.sdf.y1);
        self.sdf_r0.replace(client, &columns.sdf.r0);
        self.sdf_r1.replace(client, &columns.sdf.r1);
        self.sdf_r2.replace(client, &columns.sdf.r2);
        self.sdf_r3.replace(client, &columns.sdf.r3);
        self.sdf_stroke_top.replace(client, &columns.sdf.stroke_top);
        self.sdf_stroke_right
            .replace(client, &columns.sdf.stroke_right);
        self.sdf_stroke_bottom
            .replace(client, &columns.sdf.stroke_bottom);
        self.sdf_stroke_left
            .replace(client, &columns.sdf.stroke_left);
        self.sdf_shadow_offset_x
            .replace(client, &columns.sdf.shadow_offset_x);
        self.sdf_shadow_offset_y
            .replace(client, &columns.sdf.shadow_offset_y);
        self.sdf_shadow_expand
            .replace(client, &columns.sdf.shadow_expand);
        self.sdf_shadow_intensity
            .replace(client, &columns.sdf.shadow_intensity);
    }

    fn upload_text<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        scene: &Scene,
        text: Option<&PreparedTextData>,
        staging: &mut SceneUploadStaging,
    ) {
        staging.text.refill(scene, text, self.glyph_atlas_signature);
        self.glyph_run_starts
            .replace(client, &staging.text.run_starts);
        self.glyph_run_counts
            .replace(client, &staging.text.run_counts);
        self.glyph_image_ids
            .replace(client, &staging.text.glyph_image_ids);
        self.glyph_x.replace(client, &staging.text.glyph_x);
        self.glyph_y.replace(client, &staging.text.glyph_y);
        if !staging.text.atlas_dirty {
            return;
        }

        self.glyph_atlas_signature = staging.text.atlas_signature;
        self.glyph_image_left
            .replace_growing(client, &staging.text.image_left);
        self.glyph_image_top
            .replace_growing(client, &staging.text.image_top);
        self.glyph_image_width
            .replace_growing(client, &staging.text.image_width);
        self.glyph_image_height
            .replace_growing(client, &staging.text.image_height);
        self.glyph_image_content
            .replace_growing(client, &staging.text.image_content);
        self.glyph_image_data_offsets
            .replace_growing(client, &staging.text.image_data_offsets);
        self.glyph_image_data
            .replace_growing(client, &staging.text.image_data);
    }

    fn upload_backdrops<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        records: &[BackdropRecord],
        staging: &mut SceneUploadStaging,
    ) {
        upload_mapped_u32(
            client,
            &mut self.backdrop_data_offsets,
            &mut staging.u32s,
            records,
            |record| record.data_offset,
        );
        upload_mapped_u32(
            client,
            &mut self.backdrop_data_lens,
            &mut staging.u32s,
            records,
            |record| record.data_len,
        );
        upload_mapped_u32(
            client,
            &mut self.backdrop_tile_x0,
            &mut staging.u32s,
            records,
            |record| record.tile_x0,
        );
        upload_mapped_u32(
            client,
            &mut self.backdrop_tile_y0,
            &mut staging.u32s,
            records,
            |record| record.tile_y0,
        );
        upload_mapped_u32(
            client,
            &mut self.backdrop_tile_x1,
            &mut staging.u32s,
            records,
            |record| record.tile_x1,
        );
        upload_mapped_u32(
            client,
            &mut self.backdrop_tile_y1,
            &mut staging.u32s,
            records,
            |record| record.tile_y1,
        );
        upload_mapped_u32(
            client,
            &mut self.backdrop_segment_starts,
            &mut staging.u32s,
            records,
            |record| record.segment_start,
        );
        upload_mapped_u32(
            client,
            &mut self.backdrop_segment_capacities,
            &mut staging.u32s,
            records,
            |record| record.segment_capacity,
        );
    }
}

pub(crate) struct ScanBuffers {
    pub(crate) backdrops: CubeBuffer<i32>,
    pub(crate) tile_segment_range_starts: CubeBuffer<u32>,
    pub(crate) tile_segment_range_ends: CubeBuffer<u32>,
    pub(crate) segment_p0x: CubeBuffer<f32>,
    pub(crate) segment_p0y: CubeBuffer<f32>,
    pub(crate) segment_p1x: CubeBuffer<f32>,
    pub(crate) segment_p1y: CubeBuffer<f32>,
    pub(crate) segment_y_edge: CubeBuffer<f32>,
    pub(crate) segment_tile_counts: CubeBuffer<u32>,
    pub(crate) segment_tile_cursors: CubeBuffer<u32>,
    pub(crate) segment_bumps: CubeBuffer<u32>,
    pub(crate) chunk_totals: CubeBuffer<u32>,
    pub(crate) chunk_offsets: CubeBuffer<u32>,
    pub(crate) cumsum_chunk_totals: CubeBuffer<i32>,
    pub(crate) cumsum_chunk_offsets: CubeBuffer<i32>,
}

impl ScanBuffers {
    pub(super) fn new<R: Runtime>(client: &::cubecl::client::ComputeClient<R>) -> Self {
        Self {
            backdrops: CubeBuffer::new(client, 0),
            tile_segment_range_starts: CubeBuffer::new(client, 0),
            tile_segment_range_ends: CubeBuffer::new(client, 0),
            segment_p0x: CubeBuffer::new(client, 0),
            segment_p0y: CubeBuffer::new(client, 0),
            segment_p1x: CubeBuffer::new(client, 0),
            segment_p1y: CubeBuffer::new(client, 0),
            segment_y_edge: CubeBuffer::new(client, 0),
            segment_tile_counts: CubeBuffer::new(client, 0),
            segment_tile_cursors: CubeBuffer::new(client, 0),
            segment_bumps: CubeBuffer::new(client, 0),
            chunk_totals: CubeBuffer::new(client, 0),
            chunk_offsets: CubeBuffer::new(client, 0),
            cumsum_chunk_totals: CubeBuffer::new(client, 0),
            cumsum_chunk_offsets: CubeBuffer::new(client, 0),
        }
    }

    #[cfg(feature = "profile")]
    pub(super) fn memory_usage(&self) -> MemoryUsage {
        MemoryUsage::sum([
            self.backdrops.memory_usage(),
            self.tile_segment_range_starts.memory_usage(),
            self.tile_segment_range_ends.memory_usage(),
            self.segment_p0x.memory_usage(),
            self.segment_p0y.memory_usage(),
            self.segment_p1x.memory_usage(),
            self.segment_p1y.memory_usage(),
            self.segment_y_edge.memory_usage(),
            self.segment_tile_counts.memory_usage(),
            self.segment_tile_cursors.memory_usage(),
            self.segment_bumps.memory_usage(),
            self.chunk_totals.memory_usage(),
            self.chunk_offsets.memory_usage(),
            self.cumsum_chunk_totals.memory_usage(),
            self.cumsum_chunk_offsets.memory_usage(),
        ])
    }

    pub(super) fn prepare_outputs<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        lengths: CubeBufferLengths,
    ) {
        self.backdrops.resize_uninit(client, lengths.backdrop_len);
        self.tile_segment_range_starts
            .resize_uninit(client, lengths.backdrop_len);
        self.tile_segment_range_ends
            .resize_uninit(client, lengths.backdrop_len);
        self.segment_p0x
            .resize_uninit(client, lengths.segment_capacity);
        self.segment_p0y
            .resize_uninit(client, lengths.segment_capacity);
        self.segment_p1x
            .resize_uninit(client, lengths.segment_capacity);
        self.segment_p1y
            .resize_uninit(client, lengths.segment_capacity);
        self.segment_y_edge
            .resize_uninit(client, lengths.segment_capacity);
        self.segment_tile_counts
            .resize_uninit(client, lengths.backdrop_len);
        self.segment_tile_cursors
            .resize_uninit(client, lengths.backdrop_len);
        self.segment_bumps.resize_uninit(client, lengths.path_count);
        self.chunk_totals
            .resize_uninit(client, lengths.scan_chunk_count);
        self.chunk_offsets
            .resize_uninit(client, lengths.scan_chunk_count);
        self.cumsum_chunk_totals
            .resize_uninit(client, lengths.cumsum_chunk_count);
        self.cumsum_chunk_offsets
            .resize_uninit(client, lengths.cumsum_chunk_count);
    }
}

pub(crate) struct CoarseBuffers {
    pub(crate) tile_ptcl_range_starts: CubeBuffer<u32>,
    pub(crate) tile_ptcl_range_ends: CubeBuffer<u32>,
    pub(crate) tile_ptcl_counts: CubeBuffer<u32>,
    pub(crate) tile_glyph_range_starts: CubeBuffer<u32>,
    pub(crate) tile_glyph_range_ends: CubeBuffer<u32>,
    pub(crate) tile_glyph_counts: CubeBuffer<u32>,
    pub(crate) chunk_totals: CubeBuffer<u32>,
    pub(crate) chunk_offsets: CubeBuffer<u32>,
    pub(crate) glyph_chunk_totals: CubeBuffer<u32>,
    pub(crate) glyph_chunk_offsets: CubeBuffer<u32>,
    pub(crate) ptcl_tags: CubeBuffer<u32>,
    pub(crate) ptcl_backdrops: CubeBuffer<i32>,
    pub(crate) ptcl_fill_rules: CubeBuffer<u32>,
    pub(crate) ptcl_segment_starts: CubeBuffer<u32>,
    pub(crate) ptcl_segment_ends: CubeBuffer<u32>,
    pub(crate) ptcl_colors: CubeBuffer<u32>,
    // Coarse stores per-tile glyph ids here; glyph particles point at ranges
    // in this buffer so fine does not scan whole text runs per pixel.
    pub(crate) glyph_indices: CubeBuffer<u32>,
}

impl CoarseBuffers {
    pub(super) fn new<R: Runtime>(client: &::cubecl::client::ComputeClient<R>) -> Self {
        Self {
            tile_ptcl_range_starts: CubeBuffer::new(client, 0),
            tile_ptcl_range_ends: CubeBuffer::new(client, 0),
            tile_ptcl_counts: CubeBuffer::new(client, 0),
            tile_glyph_range_starts: CubeBuffer::new(client, 0),
            tile_glyph_range_ends: CubeBuffer::new(client, 0),
            tile_glyph_counts: CubeBuffer::new(client, 0),
            chunk_totals: CubeBuffer::new(client, 0),
            chunk_offsets: CubeBuffer::new(client, 0),
            glyph_chunk_totals: CubeBuffer::new(client, 0),
            glyph_chunk_offsets: CubeBuffer::new(client, 0),
            ptcl_tags: CubeBuffer::new(client, 0),
            ptcl_backdrops: CubeBuffer::new(client, 0),
            ptcl_fill_rules: CubeBuffer::new(client, 0),
            ptcl_segment_starts: CubeBuffer::new(client, 0),
            ptcl_segment_ends: CubeBuffer::new(client, 0),
            ptcl_colors: CubeBuffer::new(client, 0),
            glyph_indices: CubeBuffer::new(client, 0),
        }
    }

    #[cfg(feature = "profile")]
    pub(super) fn memory_usage(&self) -> MemoryUsage {
        MemoryUsage::sum([
            self.tile_ptcl_range_starts.memory_usage(),
            self.tile_ptcl_range_ends.memory_usage(),
            self.tile_ptcl_counts.memory_usage(),
            self.tile_glyph_range_starts.memory_usage(),
            self.tile_glyph_range_ends.memory_usage(),
            self.tile_glyph_counts.memory_usage(),
            self.chunk_totals.memory_usage(),
            self.chunk_offsets.memory_usage(),
            self.glyph_chunk_totals.memory_usage(),
            self.glyph_chunk_offsets.memory_usage(),
            self.ptcl_tags.memory_usage(),
            self.ptcl_backdrops.memory_usage(),
            self.ptcl_fill_rules.memory_usage(),
            self.ptcl_segment_starts.memory_usage(),
            self.ptcl_segment_ends.memory_usage(),
            self.ptcl_colors.memory_usage(),
            self.glyph_indices.memory_usage(),
        ])
    }

    pub(super) fn prepare_outputs<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        lengths: CubeBufferLengths,
    ) {
        self.tile_ptcl_range_starts
            .resize_uninit(client, lengths.tile_count);
        self.tile_ptcl_range_ends
            .resize_uninit(client, lengths.tile_count);
        self.tile_ptcl_counts
            .resize_uninit(client, lengths.tile_count);
        self.tile_glyph_range_starts
            .resize_uninit(client, lengths.tile_count);
        self.tile_glyph_range_ends
            .resize_uninit(client, lengths.tile_count);
        self.tile_glyph_counts
            .resize_uninit(client, lengths.tile_count);
        self.chunk_totals
            .resize_uninit(client, lengths.coarse_chunk_count);
        self.chunk_offsets
            .resize_uninit(client, lengths.coarse_chunk_count);
        self.glyph_chunk_totals
            .resize_uninit(client, lengths.coarse_chunk_count);
        self.glyph_chunk_offsets
            .resize_uninit(client, lengths.coarse_chunk_count);
        let tag_words = packed_u8_len(lengths.coarse_ptcl_capacity);
        self.ptcl_tags.resize_uninit(client, tag_words);
        self.ptcl_backdrops
            .resize_uninit(client, lengths.coarse_ptcl_capacity);
        self.ptcl_fill_rules
            .resize_uninit(client, lengths.coarse_ptcl_capacity);
        self.ptcl_segment_starts
            .resize_uninit(client, lengths.coarse_ptcl_capacity);
        self.ptcl_segment_ends
            .resize_uninit(client, lengths.coarse_ptcl_capacity);
        self.ptcl_colors
            .resize_uninit(client, lengths.coarse_ptcl_capacity);
        self.glyph_indices
            .resize_uninit(client, lengths.coarse_glyph_capacity);
    }
}

#[cfg(test)]
mod tests {
    use peniko::{Color, kurbo::Point};

    use super::{AtlasSignature, TextUpload};
    use crate::{
        FillRule, Radius, Scene, TextContext, TextLayoutOptions,
        cubecl::{
            pipelines::common::{
                DRAW_FLAG_FILL_RULE_EVEN_ODD, DRAW_FLAG_HAS_GLYPH, DRAW_FLAG_HAS_SDF,
                DRAW_FLAG_SOLID_COLOR_FAST_PATH, DRAW_FLAG_SOLID_RECT, DRAW_FLAG_TAG_MASK,
            },
            types::CUBE_DRAW_BRUSH,
        },
        shared::{
            bounds::PixelBounds,
            brush::Brush,
            draw_record::{DrawRecord, DrawTag},
            scene_columns::draw_flags_word,
        },
        text::PreparedTextData,
    };

    #[test]
    fn draw_flags_word_stores_draw_tag_and_boolean_fields() {
        let draw = DrawRecord {
            path_id: Some(0),
            glyph_run_id: Some(7),
            sdf: None,
            sdf_shadow: None,
            tag: DrawTag::Brush,
            brush: Brush::Solid(Color::BLACK),
            fill_rule: FillRule::EvenOdd,
            pixel_bounds: PixelBounds {
                x0: 0,
                y0: 0,
                x1: 16,
                y1: 16,
            },
            solid_rect: true,
        };
        let flags = draw_flags_word(&draw, true);

        assert_eq!(flags & DRAW_FLAG_TAG_MASK, CUBE_DRAW_BRUSH);
        assert_ne!(flags & DRAW_FLAG_FILL_RULE_EVEN_ODD, 0);
        assert_ne!(flags & DRAW_FLAG_SOLID_RECT, 0);
        assert_ne!(flags & DRAW_FLAG_SOLID_COLOR_FAST_PATH, 0);
        assert_ne!(flags & DRAW_FLAG_HAS_GLYPH, 0);
        assert_eq!(flags & DRAW_FLAG_HAS_SDF, 0);
        assert_eq!(
            draw_flags_word(&draw, false) & DRAW_FLAG_HAS_GLYPH,
            0,
            "glyph payload is only valid when text upload is enabled"
        );

        let mut scene = Scene::new(16, 16);
        scene.push_rect(
            peniko::kurbo::Rect::new(0.0, 0.0, 16.0, 16.0),
            Radius::ZERO,
            Color::WHITE,
        );
        let flags = draw_flags_word(&scene.draw_records[0], false);
        assert_ne!(flags & DRAW_FLAG_HAS_SDF, 0);
        assert_eq!(flags & DRAW_FLAG_SOLID_RECT, 0);
    }

    #[test]
    fn text_upload_marks_atlas_dirty_only_when_signature_changes() {
        let mut context = TextContext::new();
        let layout = context.layout(TextLayoutOptions::new("Cache", 20.0));
        if layout.is_empty() {
            return;
        }

        let mut scene = Scene::new(160, 64);
        scene.push_text_layout(&layout, Point::new(8.0, 36.0), Color::BLACK);
        let text = PreparedTextData::new(&scene.text_glyphs, &scene.text_runs, &mut context);

        let mut upload = TextUpload::default();
        upload.refill(&scene, Some(&text), AtlasSignature::default());
        assert!(upload.atlas_dirty);

        let signature = upload.atlas_signature;
        upload.refill(&scene, Some(&text), signature);
        assert!(!upload.atlas_dirty);
        assert!(upload.image_data.is_empty());
    }
}
