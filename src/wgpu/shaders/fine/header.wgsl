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

const GPU_BRUSH_U32_STRIDE: u32 = 9u;
const GPU_BRUSH_PARAM_STRIDE: u32 = 12u;
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
};

@group(0) @binding(0) var<uniform> config: FineConfig;
@group(0) @binding(2) var<storage, read> draw_flags: array<u32>;
@group(0) @binding(3) var<storage, read> draw_brush_colors: array<u32>;
@group(0) @binding(4) var<storage, read> draw_pixel_x0: array<i32>;
@group(0) @binding(5) var<storage, read> draw_pixel_y0: array<i32>;
@group(0) @binding(6) var<storage, read> draw_pixel_x1: array<i32>;
@group(0) @binding(7) var<storage, read> draw_pixel_y1: array<i32>;
@group(0) @binding(8) var<storage, read> draw_sdf_refs: array<u32>;
@group(0) @binding(9) var<storage, read> sdf_kinds: array<u32>;
@group(0) @binding(10) var<storage, read> sdf_x0: array<f32>;
@group(0) @binding(11) var<storage, read> sdf_y0: array<f32>;
@group(0) @binding(12) var<storage, read> sdf_x1: array<f32>;
@group(0) @binding(13) var<storage, read> sdf_y1: array<f32>;
@group(0) @binding(14) var<storage, read> sdf_r0: array<f32>;
@group(0) @binding(15) var<storage, read> sdf_r1: array<f32>;
@group(0) @binding(16) var<storage, read> sdf_r2: array<f32>;
@group(0) @binding(17) var<storage, read> sdf_r3: array<f32>;
@group(0) @binding(18) var<storage, read> sdf_stroke_top: array<f32>;
@group(0) @binding(19) var<storage, read> sdf_stroke_right: array<f32>;
@group(0) @binding(20) var<storage, read> sdf_stroke_bottom: array<f32>;
@group(0) @binding(21) var<storage, read> sdf_stroke_left: array<f32>;
@group(0) @binding(22) var<storage, read> sdf_shadow_offset_x: array<f32>;
@group(0) @binding(23) var<storage, read> sdf_shadow_offset_y: array<f32>;
@group(0) @binding(24) var<storage, read> sdf_shadow_expand: array<f32>;
@group(0) @binding(25) var<storage, read> sdf_shadow_intensity: array<f32>;
@group(0) @binding(26) var<storage, read> brush_data: array<u32>;
@group(0) @binding(27) var<storage, read> brush_params: array<f32>;
@group(0) @binding(28) var<storage, read> brush_payloads: array<u32>;
@group(0) @binding(29) var<storage, read> tile_range_starts: array<u32>;
@group(0) @binding(30) var<storage, read> tile_range_ends: array<u32>;
@group(0) @binding(31) var<storage, read> ptcl_tags: array<u32>;
@group(0) @binding(32) var<storage, read> ptcl_backdrops: array<i32>;
@group(0) @binding(33) var<storage, read> ptcl_fill_rules: array<u32>;
@group(0) @binding(34) var<storage, read> ptcl_segment_starts: array<u32>;
@group(0) @binding(35) var<storage, read> ptcl_segment_ends: array<u32>;
@group(0) @binding(36) var<storage, read> ptcl_colors: array<u32>;
@group(0) @binding(37) var<storage, read> segment_p0x: array<f32>;
@group(0) @binding(38) var<storage, read> segment_p0y: array<f32>;
@group(0) @binding(39) var<storage, read> segment_p1x: array<f32>;
@group(0) @binding(40) var<storage, read> segment_p1y: array<f32>;
@group(0) @binding(41) var<storage, read> segment_y_edge: array<f32>;
@group(0) @binding(42) var<storage, read> glyph_indices: array<u32>;
@group(0) @binding(43) var<storage, read> glyph_image_ids: array<u32>;
@group(0) @binding(44) var<storage, read> glyph_x: array<i32>;
@group(0) @binding(45) var<storage, read> glyph_y: array<i32>;
@group(0) @binding(46) var<storage, read> glyph_image_left: array<i32>;
@group(0) @binding(47) var<storage, read> glyph_image_top: array<i32>;
@group(0) @binding(48) var<storage, read> glyph_image_width: array<u32>;
@group(0) @binding(49) var<storage, read> glyph_image_height: array<u32>;
@group(0) @binding(50) var<storage, read> glyph_image_content: array<u32>;
@group(0) @binding(51) var<storage, read> glyph_image_data_offsets: array<u32>;
@group(0) @binding(52) var<storage, read> glyph_image_data: array<u32>;
@group(0) @binding(53) var<storage, read_write> clip_spills: array<u32>;
@group(0) @binding(54) var<storage, read_write> group_spills: array<u32>;
@group(0) @binding(55) var<storage, read> image_resource_metadata: array<u32>;
@group(0) @binding(56) var<storage, read> image_resource_pixels: array<u32>;
