use crate::{
    scene::Scene,
    shared::{
        bd_record::BackdropRecord,
        draw_record::{DrawRecord, DrawTag},
        execution::{ExecPlan, LayerStackEntry},
        fill::FillRule,
        image::rgba8_pack,
        line::Line,
        pixel::{mul_div255, premul_f32_to_u32},
        sdf::Sdf,
    },
    text::{AtlasSignature, PreparedGlyphContent, PreparedTextData, TextCompositeMode},
};
use ::cubecl::prelude::Runtime;

use crate::cubecl::{
    buffer::CubeBuffer,
    types::{
        CUBE_DRAW_BLEND, CUBE_DRAW_BRUSH, CUBE_DRAW_CLIP, CUBE_DRAW_ISOLATE, CUBE_DRAW_OPACITY,
        CUBE_DRAW_PATH_GLYPH, CUBE_GLYPH_COLOR, CUBE_GLYPH_LINEAR_COLOR, CUBE_GLYPH_LINEAR_MASK,
        CUBE_GLYPH_LINEAR_SUBPIXEL_MASK, CUBE_GLYPH_MASK, CUBE_GLYPH_SUBPIXEL_MASK,
        CUBE_LAYER_BLEND, CUBE_LAYER_CLIP, CUBE_LAYER_OPACITY, CUBE_SDF_CANDLESTICK,
        CUBE_SDF_CIRCLE, CUBE_SDF_CIRCLE_STROKE, CUBE_SDF_NONE, CUBE_SDF_RECT,
        CUBE_SDF_RECT_STROKE, CubeBufferLengths, CubeCumsumPlan, CubeScanChunk, CubeScanChunkRange,
        build_cumsum_plan_into, build_scan_chunks_into,
    },
};

use super::executor::encode_layer_payload;
#[derive(Default)]
struct DrawSdfUpload {
    kinds: Vec<u32>,
    x0: Vec<f32>,
    y0: Vec<f32>,
    x1: Vec<f32>,
    y1: Vec<f32>,
    r0: Vec<f32>,
    r1: Vec<f32>,
    r2: Vec<f32>,
    r3: Vec<f32>,
    stroke_top: Vec<f32>,
    stroke_right: Vec<f32>,
    stroke_bottom: Vec<f32>,
    stroke_left: Vec<f32>,
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
            .extend(scene.text_runs.iter().map(|run| run.glyph_start));
        self.run_counts
            .extend(scene.text_runs.iter().map(|run| run.glyph_count));
        self.glyph_image_ids.reserve(scene.text_glyphs.len());
        self.glyph_x.reserve(scene.text_glyphs.len());
        self.glyph_y.reserve(scene.text_glyphs.len());
        for glyph in &scene.text_glyphs {
            self.glyph_image_ids.push(
                text.image_id_for_cache_key(glyph.cache_key)
                    .unwrap_or(u32::MAX),
            );
            self.glyph_x.push(glyph.x);
            self.glyph_y.push(glyph.y);
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
}

impl DrawSdfUpload {
    fn refill(&mut self, draws: &[DrawRecord]) {
        self.clear_and_reserve(draws.len());
        for draw in draws {
            match draw.sdf {
                Some(Sdf::Rect(rect)) => {
                    let (x0, y0, x1, y1) = rect.axis_bounds();
                    self.push(
                        CUBE_SDF_RECT,
                        [x0 as f32, y0 as f32, x1 as f32, y1 as f32],
                        [
                            rect.radius.top_left,
                            rect.radius.top_right,
                            rect.radius.bottom_left,
                            rect.radius.bottom_right,
                        ],
                        [0.0; 4],
                    );
                }
                Some(Sdf::RectStroke(stroke)) => {
                    let (x0, y0, x1, y1) = stroke.rect.axis_bounds();
                    let half = stroke.widths.half();
                    self.push(
                        CUBE_SDF_RECT_STROKE,
                        [x0 as f32, y0 as f32, x1 as f32, y1 as f32],
                        [
                            stroke.rect.radius.top_left,
                            stroke.rect.radius.top_right,
                            stroke.rect.radius.bottom_left,
                            stroke.rect.radius.bottom_right,
                        ],
                        [half.top, half.right, half.bottom, half.left],
                    );
                }
                Some(Sdf::Circle(circle)) => {
                    self.push(
                        CUBE_SDF_CIRCLE,
                        [
                            circle.center.x as f32,
                            circle.center.y as f32,
                            circle.radius,
                            0.0,
                        ],
                        [0.0; 4],
                        [0.0; 4],
                    );
                }
                Some(Sdf::CircleStroke(stroke)) => {
                    self.push(
                        CUBE_SDF_CIRCLE_STROKE,
                        [
                            stroke.circle.center.x as f32,
                            stroke.circle.center.y as f32,
                            stroke.circle.radius,
                            0.0,
                        ],
                        [0.0; 4],
                        [stroke.half_width; 4],
                    );
                }
                Some(Sdf::CandleStick(candle)) => {
                    self.push(
                        CUBE_SDF_CANDLESTICK,
                        [
                            candle.center_x,
                            candle.high_y,
                            candle.low_y,
                            candle.body_top_y,
                        ],
                        [candle.body_bottom_y, candle.body_width as f32, 0.0, 0.0],
                        [0.0; 4],
                    );
                }
                None => {
                    self.push(CUBE_SDF_NONE, [0.0; 4], [0.0; 4], [0.0; 4]);
                }
            }
        }
    }

    fn clear_and_reserve(&mut self, len: usize) {
        self.kinds.clear();
        self.x0.clear();
        self.y0.clear();
        self.x1.clear();
        self.y1.clear();
        self.r0.clear();
        self.r1.clear();
        self.r2.clear();
        self.r3.clear();
        self.stroke_top.clear();
        self.stroke_right.clear();
        self.stroke_bottom.clear();
        self.stroke_left.clear();
        self.kinds.reserve(len);
        self.x0.reserve(len);
        self.y0.reserve(len);
        self.x1.reserve(len);
        self.y1.reserve(len);
        self.r0.reserve(len);
        self.r1.reserve(len);
        self.r2.reserve(len);
        self.r3.reserve(len);
        self.stroke_top.reserve(len);
        self.stroke_right.reserve(len);
        self.stroke_bottom.reserve(len);
        self.stroke_left.reserve(len);
    }

    fn push(&mut self, kind: u32, xy: [f32; 4], radii: [f32; 4], stroke_widths: [f32; 4]) {
        self.kinds.push(kind);
        self.x0.push(xy[0]);
        self.y0.push(xy[1]);
        self.x1.push(xy[2]);
        self.y1.push(xy[3]);
        self.r0.push(radii[0]);
        self.r1.push(radii[1]);
        self.r2.push(radii[2]);
        self.r3.push(radii[3]);
        self.stroke_top.push(stroke_widths[0]);
        self.stroke_right.push(stroke_widths[1]);
        self.stroke_bottom.push(stroke_widths[2]);
        self.stroke_left.push(stroke_widths[3]);
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
    i32s: Vec<i32>,
    f32s: Vec<f32>,
    sdf: DrawSdfUpload,
    text: TextUpload,
    scan_chunks: Vec<CubeScanChunk>,
    scan_chunk_ranges: Vec<CubeScanChunkRange>,
    cumsum_plan: CubeCumsumPlan,
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

fn upload_mapped_i32<R: Runtime, T>(
    client: &::cubecl::client::ComputeClient<R>,
    buffer: &mut CubeBuffer<i32>,
    scratch: &mut Vec<i32>,
    items: &[T],
    map: impl FnMut(&T) -> i32,
) {
    scratch.clear();
    scratch.reserve(items.len());
    scratch.extend(items.iter().map(map));
    buffer.replace(client, scratch);
}

fn upload_mapped_f32<R: Runtime, T>(
    client: &::cubecl::client::ComputeClient<R>,
    buffer: &mut CubeBuffer<f32>,
    scratch: &mut Vec<f32>,
    items: &[T],
    map: impl FnMut(&T) -> f32,
) {
    scratch.clear();
    scratch.reserve(items.len());
    scratch.extend(items.iter().map(map));
    buffer.replace(client, scratch);
}

pub(crate) struct SceneBuffers {
    pub(crate) line_path_ids: CubeBuffer<u32>,
    pub(crate) line_p0x: CubeBuffer<f32>,
    pub(crate) line_p0y: CubeBuffer<f32>,
    pub(crate) line_p1x: CubeBuffer<f32>,
    pub(crate) line_p1y: CubeBuffer<f32>,
    pub(crate) draw_path_ids: CubeBuffer<u32>,
    pub(crate) draw_glyph_run_ids: CubeBuffer<u32>,
    pub(crate) draw_tags: CubeBuffer<u32>,
    pub(crate) draw_fill_rules: CubeBuffer<u32>,
    pub(crate) draw_solid_rects: CubeBuffer<u32>,
    pub(crate) draw_solid_color_fast_paths: CubeBuffer<u32>,
    pub(crate) draw_brush_colors: CubeBuffer<u32>,
    pub(crate) draw_pixel_x0: CubeBuffer<i32>,
    pub(crate) draw_pixel_y0: CubeBuffer<i32>,
    pub(crate) draw_pixel_x1: CubeBuffer<i32>,
    pub(crate) draw_pixel_y1: CubeBuffer<i32>,
    pub(crate) draw_sdf_kinds: CubeBuffer<u32>,
    pub(crate) draw_sdf_x0: CubeBuffer<f32>,
    pub(crate) draw_sdf_y0: CubeBuffer<f32>,
    pub(crate) draw_sdf_x1: CubeBuffer<f32>,
    pub(crate) draw_sdf_y1: CubeBuffer<f32>,
    pub(crate) draw_sdf_r0: CubeBuffer<f32>,
    pub(crate) draw_sdf_r1: CubeBuffer<f32>,
    pub(crate) draw_sdf_r2: CubeBuffer<f32>,
    pub(crate) draw_sdf_r3: CubeBuffer<f32>,
    pub(crate) draw_sdf_stroke_top: CubeBuffer<f32>,
    pub(crate) draw_sdf_stroke_right: CubeBuffer<f32>,
    pub(crate) draw_sdf_stroke_bottom: CubeBuffer<f32>,
    pub(crate) draw_sdf_stroke_left: CubeBuffer<f32>,
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
            draw_path_ids: CubeBuffer::new(client, 0),
            draw_glyph_run_ids: CubeBuffer::new(client, 0),
            draw_tags: CubeBuffer::new(client, 0),
            draw_fill_rules: CubeBuffer::new(client, 0),
            draw_solid_rects: CubeBuffer::new(client, 0),
            draw_solid_color_fast_paths: CubeBuffer::new(client, 0),
            draw_brush_colors: CubeBuffer::new(client, 0),
            draw_pixel_x0: CubeBuffer::new(client, 0),
            draw_pixel_y0: CubeBuffer::new(client, 0),
            draw_pixel_x1: CubeBuffer::new(client, 0),
            draw_pixel_y1: CubeBuffer::new(client, 0),
            draw_sdf_kinds: CubeBuffer::new(client, 0),
            draw_sdf_x0: CubeBuffer::new(client, 0),
            draw_sdf_y0: CubeBuffer::new(client, 0),
            draw_sdf_x1: CubeBuffer::new(client, 0),
            draw_sdf_y1: CubeBuffer::new(client, 0),
            draw_sdf_r0: CubeBuffer::new(client, 0),
            draw_sdf_r1: CubeBuffer::new(client, 0),
            draw_sdf_r2: CubeBuffer::new(client, 0),
            draw_sdf_r3: CubeBuffer::new(client, 0),
            draw_sdf_stroke_top: CubeBuffer::new(client, 0),
            draw_sdf_stroke_right: CubeBuffer::new(client, 0),
            draw_sdf_stroke_bottom: CubeBuffer::new(client, 0),
            draw_sdf_stroke_left: CubeBuffer::new(client, 0),
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
        self.upload_lines(client, &scene.lines, staging);
        self.upload_draws(client, &scene.draw_records, text.is_some(), staging);
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

    fn upload_lines<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        lines: &[Line],
        staging: &mut SceneUploadStaging,
    ) {
        upload_mapped_u32(
            client,
            &mut self.line_path_ids,
            &mut staging.u32s,
            lines,
            |line| line.path_id,
        );
        upload_mapped_f32(
            client,
            &mut self.line_p0x,
            &mut staging.f32s,
            lines,
            |line| line.p0[0],
        );
        upload_mapped_f32(
            client,
            &mut self.line_p0y,
            &mut staging.f32s,
            lines,
            |line| line.p0[1],
        );
        upload_mapped_f32(
            client,
            &mut self.line_p1x,
            &mut staging.f32s,
            lines,
            |line| line.p1[0],
        );
        upload_mapped_f32(
            client,
            &mut self.line_p1y,
            &mut staging.f32s,
            lines,
            |line| line.p1[1],
        );
    }

    fn upload_draws<R: Runtime>(
        &mut self,
        client: &::cubecl::client::ComputeClient<R>,
        draws: &[DrawRecord],
        text_enabled: bool,
        staging: &mut SceneUploadStaging,
    ) {
        upload_mapped_u32(
            client,
            &mut self.draw_path_ids,
            &mut staging.u32s,
            draws,
            |draw| draw.path_id.unwrap_or(u32::MAX),
        );
        upload_mapped_u32(
            client,
            &mut self.draw_glyph_run_ids,
            &mut staging.u32s,
            draws,
            |draw| {
                if text_enabled {
                    draw.glyph_run_id.unwrap_or(u32::MAX)
                } else {
                    u32::MAX
                }
            },
        );
        upload_mapped_u32(
            client,
            &mut self.draw_tags,
            &mut staging.u32s,
            draws,
            |draw| match draw.tag {
                DrawTag::Brush => CUBE_DRAW_BRUSH,
                DrawTag::PathGlyph => CUBE_DRAW_PATH_GLYPH,
                DrawTag::Clip => CUBE_DRAW_CLIP,
                DrawTag::Isolate => CUBE_DRAW_ISOLATE,
                DrawTag::Opacity => CUBE_DRAW_OPACITY,
                DrawTag::Blend => CUBE_DRAW_BLEND,
            },
        );
        upload_mapped_u32(
            client,
            &mut self.draw_fill_rules,
            &mut staging.u32s,
            draws,
            |draw| match draw.fill_rule {
                FillRule::NonZero => 0,
                FillRule::EvenOdd => 1,
            },
        );
        upload_mapped_u32(
            client,
            &mut self.draw_solid_rects,
            &mut staging.u32s,
            draws,
            |draw| u32::from(draw.solid_rect),
        );
        upload_mapped_u32(
            client,
            &mut self.draw_solid_color_fast_paths,
            &mut staging.u32s,
            draws,
            |draw| u32::from(draw.solid_rect && draw.brush.solid_color().is_some()),
        );
        upload_mapped_u32(
            client,
            &mut self.draw_brush_colors,
            &mut staging.u32s,
            draws,
            |draw| {
                draw.brush
                    .solid_color()
                    .map(|color| premul_f32_to_u32(color.premultiply().components))
                    .unwrap_or(0)
            },
        );
        upload_mapped_i32(
            client,
            &mut self.draw_pixel_x0,
            &mut staging.i32s,
            draws,
            |draw| draw.pixel_bounds.x0,
        );
        upload_mapped_i32(
            client,
            &mut self.draw_pixel_y0,
            &mut staging.i32s,
            draws,
            |draw| draw.pixel_bounds.y0,
        );
        upload_mapped_i32(
            client,
            &mut self.draw_pixel_x1,
            &mut staging.i32s,
            draws,
            |draw| draw.pixel_bounds.x1,
        );
        upload_mapped_i32(
            client,
            &mut self.draw_pixel_y1,
            &mut staging.i32s,
            draws,
            |draw| draw.pixel_bounds.y1,
        );
        staging.sdf.refill(draws);
        self.draw_sdf_kinds.replace(client, &staging.sdf.kinds);
        self.draw_sdf_x0.replace(client, &staging.sdf.x0);
        self.draw_sdf_y0.replace(client, &staging.sdf.y0);
        self.draw_sdf_x1.replace(client, &staging.sdf.x1);
        self.draw_sdf_y1.replace(client, &staging.sdf.y1);
        self.draw_sdf_r0.replace(client, &staging.sdf.r0);
        self.draw_sdf_r1.replace(client, &staging.sdf.r1);
        self.draw_sdf_r2.replace(client, &staging.sdf.r2);
        self.draw_sdf_r3.replace(client, &staging.sdf.r3);
        self.draw_sdf_stroke_top
            .replace(client, &staging.sdf.stroke_top);
        self.draw_sdf_stroke_right
            .replace(client, &staging.sdf.stroke_right);
        self.draw_sdf_stroke_bottom
            .replace(client, &staging.sdf.stroke_bottom);
        self.draw_sdf_stroke_left
            .replace(client, &staging.sdf.stroke_left);
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
        self.ptcl_tags
            .resize_uninit(client, lengths.coarse_ptcl_capacity);
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
    use crate::{Scene, TextContext, TextLayoutOptions, text::PreparedTextData};

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
