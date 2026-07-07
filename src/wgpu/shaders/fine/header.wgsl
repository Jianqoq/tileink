const INVALID_REF: u32 = 4294967295u;

const GPU_SDF_RECT: u32 = 1u;
const GPU_SDF_CIRCLE: u32 = 2u;
const GPU_SDF_RECT_STROKE: u32 = 3u;
const GPU_SDF_CIRCLE_STROKE: u32 = 4u;
const GPU_SDF_CANDLESTICK: u32 = 5u;
const GPU_SDF_LINE: u32 = 6u;
const GPU_SDF_RECT_SHADOW: u32 = 7u;
const GPU_SDF_ARC: u32 = 8u;
const GPU_SDF_ARC_SHADOW: u32 = 9u;
const GPU_SDF_CIRCLE_SHADOW: u32 = 10u;
const GPU_SDF_LINE_SHADOW: u32 = 11u;
const GPU_SDF_DASH_LINE: u32 = 12u;
const GPU_PTCL_END: u32 = 0u;
const GPU_PTCL_FILL: u32 = 1u;
const GPU_PTCL_COLOR: u32 = 2u;
const GPU_PTCL_BEGIN_CLIP: u32 = 3u;
const GPU_PTCL_END_CLIP: u32 = 4u;
const GPU_PTCL_BEGIN_OPACITY: u32 = 5u;
const GPU_PTCL_END_OPACITY: u32 = 6u;
const GPU_PTCL_BEGIN_BLEND: u32 = 7u;
const GPU_PTCL_END_BLEND: u32 = 8u;
const GPU_PTCL_SDF: u32 = 9u;
const GPU_PTCL_GLYPH: u32 = 10u;
const GPU_PTCL_PATH_GLYPH: u32 = 11u;
const GPU_PTCL_BEGIN_SDF_CLIP: u32 = 12u;
const GPU_GLYPH_MASK: u32 = 0u;
const GPU_GLYPH_COLOR: u32 = 1u;
const GPU_GLYPH_SUBPIXEL_MASK: u32 = 2u;
const GPU_GLYPH_LINEAR_MASK: u32 = 3u;
const GPU_GLYPH_LINEAR_COLOR: u32 = 4u;
const GPU_GLYPH_LINEAR_SUBPIXEL_MASK: u32 = 5u;
const FINE_LOCAL_CLIP_DEPTH: u32 = 4u;
const FINE_LOCAL_GROUP_DEPTH: u32 = 2u;
const FINE_GROUP_SPILL_FIELDS: u32 = 5u;
const FINE_WORKGROUP_SIZE: u32 = 256u;
const FINE_AREA_EPSILON: f32 = 1.0e-6;

const GPU_BRUSH_U32_STRIDE: u32 = 9u;
const GPU_BRUSH_PARAM_STRIDE: u32 = 12u;
const GPU_BRUSH_SOLID: u32 = 1u;
const GPU_BRUSH_LINEAR: u32 = 2u;
const GPU_BRUSH_RADIAL: u32 = 3u;
const GPU_BRUSH_SWEEP: u32 = 4u;
const GPU_BRUSH_FOUR_CORNER: u32 = 5u;
const GPU_BRUSH_PATTERN: u32 = 6u;
const GPU_BRUSH_PATTERN_RESOURCE: u32 = 7u;
const GPU_PATTERN_BILINEAR: u32 = 1u;
const GPU_EXTEND_REPEAT: u32 = 1u;
const GPU_EXTEND_REFLECT: u32 = 2u;

const TEXT_DARK_ON_LIGHT_COVERAGE_STRENGTH: f32 = 0.95;
const TEXT_DARK_ON_LIGHT_LUMA_BASE: f32 = 1.5728465;
const TEXT_DARK_ON_LIGHT_LUMA_TAPER: f32 = 1.15;
const TEXT_DARK_ON_LIGHT_CHROMA_BOOST: f32 = 0.3656558;
const TEXT_SOURCE_CHROMA_COVERAGE_BOOST: f32 = 0.0;
const TEXT_SOURCE_CHROMA_COVERAGE_CONTRAST_LIMIT: f32 = 0.23100804;
const TEXT_LIGHT_ON_DARK_COVERAGE_REDUCTION: f32 = 0.20662805;
const TEXT_LIGHT_ON_DARK_BLACK_LUMA_LIMIT: f32 = 0.02875403;
const TEXT_LIGHT_ON_DARK_CHROMA_REDUCTION: f32 = 0.11479953;
const TEXT_LIGHT_ON_DARK_HIGH_LUMA_CHROMA_REDUCTION: f32 = 0.4492354;
const TEXT_LIGHT_ON_DARK_HIGH_LUMA_THRESHOLD: f32 = 0.26129702;
const TEXT_LIGHT_ON_COLORED_DARK_CHROMA_REDUCTION: f32 = 0.48728964;
const TEXT_LIGHT_ON_COLORED_DARK_LUMA_LIMIT: f32 = 0.11519971;
const TEXT_ALPHA_MASK_CHROMA_SCALE: f32 = 1.3566802;
const TEXT_SUBPIXEL_MASK_CHROMA_SCALE: f32 = 0.9483659;
const TEXT_ALPHA_MASK_APPARENT_AXIS_STRENGTH: f32 = 1.036124;
const TEXT_ALPHA_MASK_APPARENT_AXIS_LUMA_LIMIT: f32 = 0.6887328;
const TEXT_SUBPIXEL_MASK_APPARENT_AXIS_STRENGTH: f32 = 1.447765;
const TEXT_SUBPIXEL_MASK_APPARENT_AXIS_LUMA_LIMIT: f32 = 0.1682842;
const TEXT_ALPHA_MASK_LOW_LUMA_CHROMA_REDUCTION: f32 = 0.41302064;
const TEXT_ALPHA_MASK_LOW_LUMA_CONTRAST_LIMIT: f32 = 0.10123872;
const TEXT_SUBPIXEL_MASK_LOW_LUMA_CHROMA_REDUCTION: f32 = 0.0;
const TEXT_SUBPIXEL_MASK_LOW_LUMA_CONTRAST_LIMIT: f32 = 0.06385561;

struct FineConfig {
    width: u32,
    height: u32,
    clear_color: u32,
    tile_count: u32,
    tiles_width: u32,
    tiles_height: u32,
    load_target: u32,
    clip_spill_depth: u32,
    group_spill_depth: u32,
    ptcl_capacity: u32,
    paint_sdf_shadow_base: u32,
    paint_brush_base: u32,
    text_image_base: u32,
    text_image_data_base: u32,
    group_spill_base: u32,
    fine_tile_kind_base: u32,
};

@group(0) @binding(0) var<uniform> config: FineConfig;
struct DrawRecord {
    path_id: u32,
    glyph_run_id: u32,
    sdf_offset: u32,
    sdf_len: u32,
    sdf_shadow_offset: u32,
    sdf_shadow_len: u32,
    brush_offset: u32,
    brush_len: u32,
    tag: u32,
    fill_rule: u32,
    pixel_x0: i32,
    pixel_y0: i32,
    pixel_x1: i32,
    pixel_y1: i32,
    solid_rect: u32,
};
struct LineSegment {
    p0x: f32,
    p0y: f32,
    p1x: f32,
    p1y: f32,
    y_edge: f32,
};
struct GlyphRecord {
    image_id: u32,
    x: i32,
    y: i32,
};
struct GlyphImageRecord {
    left: i32,
    top: i32,
    width: u32,
    height: u32,
    content: u32,
    data_offset: u32,
};
struct TileCoarseRecord {
    ptcl_count: u32,
    ptcl_start: u32,
    ptcl_end: u32,
    glyph_count: u32,
    glyph_start: u32,
    glyph_end: u32,
};
struct PtclRecord {
    tag: u32,
    backdrop: i32,
    fill_rule: u32,
    segment_start: u32,
    segment_end: u32,
    color: u32,
};
@group(0) @binding(2) var<storage, read> draw_records: array<DrawRecord>;
@group(0) @binding(3) var<storage, read> paint_blob: array<u32>;
@group(0) @binding(4) var<storage, read_write> coarse_work: array<u32>;
@group(0) @binding(5) var<storage, read> segments: array<LineSegment>;
@group(0) @binding(6) var<storage, read> text_blob: array<u32>;
@group(0) @binding(7) var<storage, read_write> spills: array<u32>;
@group(0) @binding(8) var image_resource_atlas: texture_2d<f32>;
@group(0) @binding(9) var image_resource_sampler: sampler;

const TILE_COARSE_RECORD_WORDS: u32 = 6u;
const PTCL_RECORD_WORDS: u32 = 6u;
const GLYPH_RECORD_WORDS: u32 = 3u;
const GLYPH_IMAGE_RECORD_WORDS: u32 = 6u;
const FINE_TILE_KIND_FULL_INTERPRETER: u32 = 0u;
const FINE_TILE_KIND_EMPTY_OR_CLEAR: u32 = 1u;
const FINE_TILE_KIND_COLOR_ONLY_NO_STACK: u32 = 2u;
const FINE_TILE_KIND_PURE_SDF_SOLID_NO_STACK: u32 = 3u;
const FINE_TILE_KIND_MIXED_ANALYTIC_SOLID_NO_STACK: u32 = 4u;
const FINE_TILE_KIND_ANALYTIC_WITH_STACK: u32 = 5u;
const FINE_TILE_LIST_SDF: u32 = 0u;
const FINE_TILE_LIST_MIXED: u32 = 1u;
const FINE_TILE_LIST_FULL: u32 = 2u;

fn coarse_tile_base(tile_ix: u32) -> u32 {
    return tile_ix * TILE_COARSE_RECORD_WORDS;
}

fn coarse_ptcl_base(ptcl_ix: u32) -> u32 {
    return config.tile_count * TILE_COARSE_RECORD_WORDS + ptcl_ix * PTCL_RECORD_WORDS;
}

fn coarse_glyph_base(glyph_ix: u32) -> u32 {
    return config.tile_count * TILE_COARSE_RECORD_WORDS +
        config.ptcl_capacity * PTCL_RECORD_WORDS +
        glyph_ix;
}

fn coarse_load_tile(tile_ix: u32) -> TileCoarseRecord {
    let base = coarse_tile_base(tile_ix);
    return TileCoarseRecord(
        coarse_work[base],
        coarse_work[base + 1u],
        coarse_work[base + 2u],
        coarse_work[base + 3u],
        coarse_work[base + 4u],
        coarse_work[base + 5u],
    );
}

fn coarse_load_ptcl(ptcl_ix: u32) -> PtclRecord {
    let base = coarse_ptcl_base(ptcl_ix);
    return PtclRecord(
        coarse_work[base],
        bitcast<i32>(coarse_work[base + 1u]),
        coarse_work[base + 2u],
        coarse_work[base + 3u],
        coarse_work[base + 4u],
        coarse_work[base + 5u],
    );
}

fn coarse_load_glyph(glyph_ix: u32) -> u32 {
    return coarse_work[coarse_glyph_base(glyph_ix)];
}

fn fine_tile_kind_at(tile_ix: u32) -> u32 {
    return coarse_work[config.fine_tile_kind_base + tile_ix];
}

fn fine_tile_list_base(list_ix: u32) -> u32 {
    return config.fine_tile_kind_base + config.tile_count + list_ix * config.tile_count;
}

fn fine_tile_list_at(list_ix: u32, tile_list_ix: u32) -> u32 {
    return coarse_work[fine_tile_list_base(list_ix) + tile_list_ix];
}

fn glyph_at(glyph_ix: u32) -> GlyphRecord {
    let base = glyph_ix * GLYPH_RECORD_WORDS;
    return GlyphRecord(
        text_blob[base],
        bitcast<i32>(text_blob[base + 1u]),
        bitcast<i32>(text_blob[base + 2u]),
    );
}

fn glyph_image_at(image_ix: u32) -> GlyphImageRecord {
    let base = config.text_image_base + image_ix * GLYPH_IMAGE_RECORD_WORDS;
    return GlyphImageRecord(
        bitcast<i32>(text_blob[base]),
        bitcast<i32>(text_blob[base + 1u]),
        text_blob[base + 2u],
        text_blob[base + 3u],
        text_blob[base + 4u],
        text_blob[base + 5u],
    );
}

fn glyph_image_data_at(data_ix: u32) -> u32 {
    return text_blob[config.text_image_data_base + data_ix];
}

fn brush_word(index: u32) -> u32 {
    return paint_blob[config.paint_brush_base + index];
}

fn sdf_storage_word(index: u32, shadow_blob: bool) -> u32 {
    if (shadow_blob) {
        return paint_blob[config.paint_sdf_shadow_base + index];
    }
    return paint_blob[index];
}
