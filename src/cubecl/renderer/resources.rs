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
    },
    text::{AtlasSignature, PreparedGlyphContent, PreparedTextData, TextCompositeMode},
};
use ::cubecl::prelude::Runtime;

use crate::cubecl::{
    buffer::CubeBuffer,
    pipelines::common::{
        DRAW_FLAG_FILL_RULE_EVEN_ODD, DRAW_FLAG_HAS_GLYPH, DRAW_FLAG_HAS_SDF,
        DRAW_FLAG_SOLID_COLOR_FAST_PATH, DRAW_FLAG_SOLID_RECT,
    },
    sdf::{encode_sdf, encode_sdf_shadow},
    types::{
        CUBE_DRAW_BLEND, CUBE_DRAW_BRUSH, CUBE_DRAW_CLIP, CUBE_DRAW_ISOLATE, CUBE_DRAW_OPACITY,
        CUBE_DRAW_PATH_GLYPH, CUBE_GLYPH_COLOR, CUBE_GLYPH_LINEAR_COLOR, CUBE_GLYPH_LINEAR_MASK,
        CUBE_GLYPH_LINEAR_SUBPIXEL_MASK, CUBE_GLYPH_MASK, CUBE_GLYPH_SUBPIXEL_MASK,
        CUBE_LAYER_BLEND, CUBE_LAYER_CLIP, CUBE_LAYER_OPACITY, CubeBufferLengths, CubeCumsumPlan,
        CubeScanChunk, CubeScanChunkRange, build_cumsum_plan_into, build_scan_chunks_into,
    },
};
#[cfg(feature = "profile")]
use crate::shared::memory::MemoryUsage;

use super::executor::encode_layer_payload;

const INVALID_SDF_REF: u32 = u32::MAX;

/// CPU staging for GPU SDF draws.
///
/// Draw records are dense, but SDF records are usually sparse in SVG-heavy
/// scenes. `refs` keeps one draw-to-SDF index per draw while the encoded SDF
/// columns only store real SDF payloads. This keeps draw indexing stable for
/// kernels and removes the old cost of uploading 17 empty SDF columns for every
/// non-SDF draw.
#[derive(Default)]
struct DrawSdfUpload {
    refs: Vec<u32>,
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
    shadow_offset_x: Vec<f32>,
    shadow_offset_y: Vec<f32>,
    shadow_expand: Vec<f32>,
    shadow_intensity: Vec<f32>,
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

impl DrawSdfUpload {
    fn refill(&mut self, draws: &[DrawRecord]) {
        let sdf_count = draws
            .iter()
            .filter(|draw| draw.sdf.is_some() || draw.sdf_shadow.is_some())
            .count();
        self.clear_and_reserve(draws.len(), sdf_count);
        for draw in draws {
            let sdf = match (draw.sdf, draw.sdf_shadow) {
                (Some(sdf), None) => Some(encode_sdf(sdf)),
                (None, Some(sdf_shadow)) => Some(encode_sdf_shadow(sdf_shadow)),
                (None, None) => None,
                (Some(_), Some(_)) => unreachable!("draw cannot store both SDF and SDF shadow"),
            };
            if let Some(sdf) = sdf {
                self.refs.push(self.kinds.len() as u32);
                self.push(sdf.kind, sdf.coords, sdf.radii, sdf.stroke, sdf.shadow);
            } else {
                self.refs.push(INVALID_SDF_REF);
            }
        }
    }

    fn clear_and_reserve(&mut self, draw_count: usize, sdf_count: usize) {
        self.refs.clear();
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
        self.shadow_offset_x.clear();
        self.shadow_offset_y.clear();
        self.shadow_expand.clear();
        self.shadow_intensity.clear();
        self.refs.reserve(draw_count);
        self.kinds.reserve(sdf_count);
        self.x0.reserve(sdf_count);
        self.y0.reserve(sdf_count);
        self.x1.reserve(sdf_count);
        self.y1.reserve(sdf_count);
        self.r0.reserve(sdf_count);
        self.r1.reserve(sdf_count);
        self.r2.reserve(sdf_count);
        self.r3.reserve(sdf_count);
        self.stroke_top.reserve(sdf_count);
        self.stroke_right.reserve(sdf_count);
        self.stroke_bottom.reserve(sdf_count);
        self.stroke_left.reserve(sdf_count);
        self.shadow_offset_x.reserve(sdf_count);
        self.shadow_offset_y.reserve(sdf_count);
        self.shadow_expand.reserve(sdf_count);
        self.shadow_intensity.reserve(sdf_count);
    }

    fn push(
        &mut self,
        kind: u32,
        xy: [f32; 4],
        radii: [f32; 4],
        stroke_widths: [f32; 4],
        shadow: [f32; 4],
    ) {
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
        self.shadow_offset_x.push(shadow[0]);
        self.shadow_offset_y.push(shadow[1]);
        self.shadow_expand.push(shadow[2]);
        self.shadow_intensity.push(shadow[3]);
    }

    #[cfg(feature = "profile")]
    fn memory_usage(&self) -> MemoryUsage {
        MemoryUsage::sum([
            MemoryUsage::vec(&self.refs),
            MemoryUsage::vec(&self.kinds),
            MemoryUsage::vec(&self.x0),
            MemoryUsage::vec(&self.y0),
            MemoryUsage::vec(&self.x1),
            MemoryUsage::vec(&self.y1),
            MemoryUsage::vec(&self.r0),
            MemoryUsage::vec(&self.r1),
            MemoryUsage::vec(&self.r2),
            MemoryUsage::vec(&self.r3),
            MemoryUsage::vec(&self.stroke_top),
            MemoryUsage::vec(&self.stroke_right),
            MemoryUsage::vec(&self.stroke_bottom),
            MemoryUsage::vec(&self.stroke_left),
            MemoryUsage::vec(&self.shadow_offset_x),
            MemoryUsage::vec(&self.shadow_offset_y),
            MemoryUsage::vec(&self.shadow_expand),
            MemoryUsage::vec(&self.shadow_intensity),
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
    i32s: Vec<i32>,
    f32s: Vec<f32>,
    sdf: DrawSdfUpload,
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
            MemoryUsage::vec(&self.i32s),
            MemoryUsage::vec(&self.f32s),
            self.sdf.memory_usage(),
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

fn packed_u8_len(len: usize) -> usize {
    len.div_ceil(4)
}

fn pack_mapped_u8s<T>(scratch: &mut Vec<u32>, items: &[T], mut map: impl FnMut(&T) -> u32) {
    scratch.clear();
    scratch.resize(packed_u8_len(items.len()), 0);
    for (i, item) in items.iter().enumerate() {
        let tag = map(item);
        debug_assert!(tag <= u8::MAX as u32);
        scratch[i / 4] |= (tag & 255) << ((i as u32 & 3) * 8);
    }
}

fn upload_packed_u8<R: Runtime, T>(
    client: &::cubecl::client::ComputeClient<R>,
    buffer: &mut CubeBuffer<u32>,
    scratch: &mut Vec<u32>,
    items: &[T],
    map: impl FnMut(&T) -> u32,
) {
    pack_mapped_u8s(scratch, items, map);
    buffer.replace(client, scratch);
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

fn draw_tag_byte(draw: &DrawRecord) -> u32 {
    match draw.tag {
        DrawTag::Brush => CUBE_DRAW_BRUSH,
        DrawTag::PathGlyph => CUBE_DRAW_PATH_GLYPH,
        DrawTag::Clip => CUBE_DRAW_CLIP,
        DrawTag::Isolate => CUBE_DRAW_ISOLATE,
        DrawTag::Opacity => CUBE_DRAW_OPACITY,
        DrawTag::Blend => CUBE_DRAW_BLEND,
    }
}

fn draw_flags_byte(draw: &DrawRecord, text_enabled: bool) -> u32 {
    let mut flags = draw_tag_byte(draw);
    if draw.fill_rule == FillRule::EvenOdd {
        flags |= DRAW_FLAG_FILL_RULE_EVEN_ODD;
    }
    if draw.solid_rect {
        flags |= DRAW_FLAG_SOLID_RECT;
        if draw.brush.solid_color().is_some() {
            flags |= DRAW_FLAG_SOLID_COLOR_FAST_PATH;
        }
    }
    if draw.sdf.is_some() || draw.sdf_shadow.is_some() {
        flags |= DRAW_FLAG_HAS_SDF;
    }
    if text_enabled && draw.glyph_run_id.is_some() {
        flags |= DRAW_FLAG_HAS_GLYPH;
    }
    flags
}

pub(crate) struct SceneBuffers {
    pub(crate) line_path_ids: CubeBuffer<u32>,
    pub(crate) line_p0x: CubeBuffer<f32>,
    pub(crate) line_p0y: CubeBuffer<f32>,
    pub(crate) line_p1x: CubeBuffer<f32>,
    pub(crate) line_p1y: CubeBuffer<f32>,
    pub(crate) draw_path_ids: CubeBuffer<u32>,
    pub(crate) draw_glyph_run_ids: CubeBuffer<u32>,
    /// Packed per-draw byte:
    ///
    /// - bits 0..=2: draw tag
    /// - bit 3: even-odd fill rule
    /// - bit 4: solid rectangle
    /// - bit 5: solid color full-tile fast path
    /// - bit 6: draw has an SDF payload
    /// - bit 7: draw has a glyph run
    ///
    /// CubeCL's wgpu backend does not expose `u8` storage, so four bytes are
    /// packed into one `u32` word. Kernels must read this through the helpers in
    /// `pipelines::common` instead of indexing the buffer directly.
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
        upload_packed_u8(
            client,
            &mut self.draw_flags,
            &mut staging.u32s,
            draws,
            |draw| draw_flags_byte(draw, text_enabled),
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
        self.draw_sdf_refs.replace(client, &staging.sdf.refs);
        self.sdf_kinds.replace(client, &staging.sdf.kinds);
        self.sdf_x0.replace(client, &staging.sdf.x0);
        self.sdf_y0.replace(client, &staging.sdf.y0);
        self.sdf_x1.replace(client, &staging.sdf.x1);
        self.sdf_y1.replace(client, &staging.sdf.y1);
        self.sdf_r0.replace(client, &staging.sdf.r0);
        self.sdf_r1.replace(client, &staging.sdf.r1);
        self.sdf_r2.replace(client, &staging.sdf.r2);
        self.sdf_r3.replace(client, &staging.sdf.r3);
        self.sdf_stroke_top.replace(client, &staging.sdf.stroke_top);
        self.sdf_stroke_right
            .replace(client, &staging.sdf.stroke_right);
        self.sdf_stroke_bottom
            .replace(client, &staging.sdf.stroke_bottom);
        self.sdf_stroke_left
            .replace(client, &staging.sdf.stroke_left);
        self.sdf_shadow_offset_x
            .replace(client, &staging.sdf.shadow_offset_x);
        self.sdf_shadow_offset_y
            .replace(client, &staging.sdf.shadow_offset_y);
        self.sdf_shadow_expand
            .replace(client, &staging.sdf.shadow_expand);
        self.sdf_shadow_intensity
            .replace(client, &staging.sdf.shadow_intensity);
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

    use super::{AtlasSignature, TextUpload, draw_flags_byte, pack_mapped_u8s};
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
        },
        text::PreparedTextData,
    };

    #[test]
    fn pack_mapped_u8s_stores_four_tags_per_word() {
        let tags = [1, 2, 3, 4, 5];
        let mut scratch = Vec::new();
        pack_mapped_u8s(&mut scratch, &tags, |tag| *tag);
        assert_eq!(scratch, vec![0x0403_0201, 0x0000_0005]);
    }

    #[test]
    fn draw_flags_byte_packs_draw_tag_and_boolean_fields() {
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
        let flags = draw_flags_byte(&draw, true);

        assert_eq!(flags & DRAW_FLAG_TAG_MASK, CUBE_DRAW_BRUSH);
        assert_ne!(flags & DRAW_FLAG_FILL_RULE_EVEN_ODD, 0);
        assert_ne!(flags & DRAW_FLAG_SOLID_RECT, 0);
        assert_ne!(flags & DRAW_FLAG_SOLID_COLOR_FAST_PATH, 0);
        assert_ne!(flags & DRAW_FLAG_HAS_GLYPH, 0);
        assert_eq!(flags & DRAW_FLAG_HAS_SDF, 0);
        assert_eq!(
            draw_flags_byte(&draw, false) & DRAW_FLAG_HAS_GLYPH,
            0,
            "glyph payload is only valid when text upload is enabled"
        );

        let mut scene = Scene::new(16, 16);
        scene.push_rect(
            peniko::kurbo::Rect::new(0.0, 0.0, 16.0, 16.0),
            Radius::ZERO,
            Color::WHITE,
            FillRule::NonZero,
        );
        let flags = draw_flags_byte(&scene.draw_records[0], false);
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
