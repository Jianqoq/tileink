const INVALID: u32 = 0xffffffffu;

const GPU_DRAW_BRUSH: u32 = 0u;
const GPU_DRAW_CLIP: u32 = 1u;
const GPU_DRAW_OPACITY: u32 = 2u;
const GPU_DRAW_BLEND: u32 = 3u;
const GPU_DRAW_ISOLATE: u32 = 4u;
const GPU_DRAW_PATH_GLYPH: u32 = 5u;

const GPU_LAYER_CLIP: u32 = 0u;
const GPU_LAYER_OPACITY: u32 = 1u;
const GPU_LAYER_BLEND: u32 = 2u;

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
const GPU_SDF_TRIANGLE: u32 = 13u;
const GPU_SDF_CHECKERBOARD: u32 = 14u;

const FILTER_BRIGHTNESS: u32 = 1u;
const FILTER_CONTRAST: u32 = 2u;
const FILTER_GRAYSCALE: u32 = 3u;
const FILTER_HUE_ROTATE: u32 = 4u;
const FILTER_INVERT: u32 = 5u;
const FILTER_OPACITY: u32 = 6u;
const FILTER_SATURATE: u32 = 7u;
const FILTER_SEPIA: u32 = 8u;
const SVG_MASK_LUMINANCE: u32 = 1u;
const FILTER_GROUP_STACK_CAPACITY: u32 = 64u;
const COMPONENT_TRANSFER_TABLE_SIZE: u32 = 256u;
const COMPONENT_TRANSFER_TABLE_LEN: u32 = 1024u;
const GPU_BRUSH_U32_STRIDE: u32 = 9u;
const GPU_BRUSH_PARAM_STRIDE: u32 = 12u;
const GPU_BRUSH_LINEAR: u32 = 2u;
const GPU_BRUSH_RADIAL: u32 = 3u;
const GPU_BRUSH_SWEEP: u32 = 4u;
const GPU_BRUSH_FOUR_CORNER: u32 = 5u;
const GPU_BRUSH_PATTERN: u32 = 6u;
const GPU_BRUSH_PATTERN_RESOURCE: u32 = 7u;
const GPU_RESOURCE_TEXTURE_PLACEMENT_BIT: u32 = 2147483648u;
const GPU_RESOURCE_TEXTURE_INDEX_MASK: u32 = 2147483647u;
const GPU_PATTERN_BILINEAR: u32 = 1u;
const GPU_EXTEND_REPEAT: u32 = 1u;
const GPU_EXTEND_REFLECT: u32 = 2u;
const TURBULENCE_TABLE_LEN: u32 = 514u;
const TURBULENCE_GRADIENT_LEN: u32 = 4112u;
const LIQUID_GLASS_CHROMATIC_R: f32 = 0.98;
const LIQUID_GLASS_CHROMATIC_G: f32 = 1.0;
const LIQUID_GLASS_CHROMATIC_B: f32 = 1.02;
const LIQUID_GLASS_PI: f32 = 3.1415927;
const LIQUID_GLASS_REFRACTION_PIXEL_SCALE: f32 = 70.71068;
const LIQUID_GLASS_NORMAL_LENGTH_SCALE: f32 = 1414.2136;
const LIQUID_GLASS_ACTIVE_DISTANCE_NORM: f32 = 0.005;
const LIQUID_GLASS_EDGE_BLEND_START: f32 = -0.001;
const LIQUID_GLASS_EDGE_BLEND_END: f32 = 0.001;
const LIQUID_GLASS_TINT_MIX: f32 = 0.8;
const LIQUID_GLASS_TINT_BASE_MIX: f32 = 0.5;
const LIQUID_GLASS_FRESNEL_LIGHTNESS_GAIN: f32 = 20.0;
const LIQUID_GLASS_FRESNEL_MIX_SCALE: f32 = 0.7;
const LIQUID_GLASS_GLARE_LIGHTNESS_GAIN: f32 = 150.0;
const LIQUID_GLASS_GLARE_CHROMA_GAIN: f32 = 30.0;
const LIQUID_GLASS_GLARE_SIDE_SCALE: f32 = 1.2;
const LIQUID_GLASS_GLARE_POWER_BASE: f32 = 0.1;
const LIQUID_GLASS_GLARE_POWER_SCALE: f32 = 2.0;
const LIQUID_GLASS_GEOMETRY_DISTANCE_SCALE: f32 = 1500.0;
const LIQUID_GLASS_GEOMETRY_RANGE_SCALE: f32 = 500.0;
const LIQUID_GLASS_EPSILON: f32 = 0.000001;
const LIQUID_GLASS_D65_X: f32 = 0.9504559;
const LIQUID_GLASS_D65_Y: f32 = 1.0;
const LIQUID_GLASS_D65_Z: f32 = 1.0890578;

struct FilterConfig {
    width: u32,
    height: u32,
    tiles_width: u32,
    tiles_height: u32,
    region_x0: u32,
    region_y0: u32,
    region_width: u32,
    region_height: u32,
    pixel_count: u32,
    active_tile_count: u32,
    compact_tiles: u32,
    active_tile_pad0: u32,
    active_tile_pad1: u32,
    downsample: u32,
    downsample_filter: u32,
    upsample_filter: u32,
    downsample_pad: u32,
    source_x0: u32,
    source_y0: u32,
    source_x1: u32,
    source_y1: u32,
    layer_stack_start: u32,
    layer_stack_end: u32,
    draw_ix: u32,
    mask_enabled: u32,
    blend_mode: u32,
    mask_kind: u32,
    clear_color: u32,
    filter_kind: u32,
    table_index: u32,
    brush_offset: u32,
    paint_sdf_shadow_base: u32,
    offset_x: i32,
    offset_y: i32,
    morphology_radius: u32,
    morphology_operator: u32,
    morphology_axis: u32,
    blur_axis: u32,
    kernel_offset: u32,
    kernel_columns: u32,
    kernel_rows: u32,
    kernel_target_x: u32,
    kernel_target_y: u32,
    kernel_edge_mode: u32,
    kernel_preserve_alpha: u32,
    lighting_output_kind: u32,
    surface_origin_x: i32,
    surface_origin_y: i32,
    light_kind: u32,
    amount: f32,
    rect_x0: f32,
    rect_y0: f32,
    rect_x1: f32,
    rect_y1: f32,
    radius_top_left: f32,
    radius_top_right: f32,
    radius_bottom_left: f32,
    radius_bottom_right: f32,
    surface_scale: f32,
    light_constant: f32,
    specular_exponent: f32,
    light_r: f32,
    light_g: f32,
    light_b: f32,
    light_p0: f32,
    light_p1: f32,
    light_p2: f32,
    light_p3: f32,
    light_p4: f32,
    light_p5: f32,
    light_p6: f32,
    light_p7: f32,
    light_p8: f32,
    turbulence_base_frequency_x: f32,
    turbulence_base_frequency_y: f32,
    turbulence_num_octaves: u32,
    turbulence_stitch_tiles: u32,
    turbulence_kind: u32,
    turbulence_linear_rgb: u32,
    turbulence_pad0: u32,
    turbulence_pad1: u32,
    turbulence_transform_x: f32,
    turbulence_transform_y: f32,
    turbulence_scale_x: f32,
    turbulence_scale_y: f32,
    turbulence_tile_x: f32,
    turbulence_tile_y: f32,
    turbulence_tile_width: f32,
    turbulence_tile_height: f32,
    liquid_tint_r: f32,
    liquid_tint_g: f32,
    liquid_tint_b: f32,
    liquid_tint_a: f32,
    liquid_refraction_thickness: f32,
    liquid_refraction_factor: f32,
    liquid_refraction_dispersion: f32,
    liquid_fresnel_range: f32,
    liquid_fresnel_hardness: f32,
    liquid_fresnel_factor: f32,
    liquid_glare_range: f32,
    liquid_glare_hardness: f32,
    liquid_glare_convergence: f32,
    liquid_glare_opposite_factor: f32,
    liquid_glare_factor: f32,
    liquid_glare_angle: f32,
    matrix_pad0: u32,
    matrix_pad1: u32,
    matrix_pad2: u32,
    matrix_r: vec4<f32>,
    matrix_g: vec4<f32>,
    matrix_b: vec4<f32>,
    matrix_a: vec4<f32>,
    matrix_bias: vec4<f32>,
};

@group(0) @binding(0) var<uniform> config: FilterConfig;
struct AffineRecord {
    a: f32, b: f32, c: f32, d: f32, e: f32, f: f32,
};
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
    local_pixel_x0: i32,
    local_pixel_y0: i32,
    local_pixel_x1: i32,
    local_pixel_y1: i32,
    solid_rect: u32,
    transform: AffineRecord,
    inverse_transform: AffineRecord,
};
struct PathRecord {
    path_id: u32,
    line_count: u32,
    line_start: u32,
    flags: u32,
    data_offset: u32,
    data_len: u32,
    tile_x0: u32,
    tile_y0: u32,
    tile_x1: u32,
    tile_y1: u32,
    segment_start: u32,
    segment_capacity: u32,
    segment_count: u32,
    transform: AffineRecord,
};
struct LineSegment {
    p0x: f32,
    p0y: f32,
    p1x: f32,
    p1y: f32,
    y_edge: f32,
};
struct TileSegmentRange {
    start: u32,
    end: u32,
};
struct LayerStackRecord {
    tag: u32,
    draw: u32,
    payload: u32,
};
@group(0) @binding(4) var<storage, read> draw_records: array<DrawRecord>;
@group(0) @binding(10) var<storage, read> paint_blob: array<u32>;
@group(0) @binding(28) var<storage, read> path_records: array<PathRecord>;
@group(0) @binding(29) var<storage, read_write> backdrops: array<atomic<i32>>;
@group(0) @binding(30) var<storage, read> segment_ranges: array<TileSegmentRange>;
@group(0) @binding(32) var<storage, read> segments: array<LineSegment>;
@group(0) @binding(33) var<storage, read> layer_stack: array<LayerStackRecord>;
@group(0) @binding(36) var<storage, read> transfer_tables: array<u32>;
@group(0) @binding(37) var<storage, read> brush_blob: array<u32>;
@group(0) @binding(40) var<storage, read> convolve_kernels: array<f32>;
@group(0) @binding(41) var<storage, read> turbulence_selectors: array<u32>;
@group(0) @binding(42) var<storage, read> turbulence_gradients: array<f32>;
@group(0) @binding(43) var<storage, read> path_range_starts: array<u32>;
@group(0) @binding(44) var<storage, read> path_range_ends: array<u32>;
@group(0) @binding(45) var<storage, read> path_p0x: array<i32>;
@group(0) @binding(46) var<storage, read> path_p0y: array<i32>;
@group(0) @binding(47) var<storage, read> path_p1x: array<i32>;
@group(0) @binding(48) var<storage, read> path_p1y: array<i32>;
@group(0) @binding(52) var<storage, read> active_tiles: array<u32>;
@group(0) @binding(49) var filter_source_sample_texture: texture_2d<f32>;
@group(0) @binding(50) var filter_aux_sample_texture: texture_2d<f32>;
@group(0) @binding(51) var filter_linear_sampler: sampler;
@group(1) @binding(0) var image_resource_atlas: texture_2d_array<f32>;
@group(1) @binding(1) var image_resource_sampler: sampler;
// TILEINK_IMAGE_RESOURCE_TEXTURE_TABLE_BINDING

fn brush_word(index: u32) -> u32 {
    return brush_blob[index];
}

fn sdf_storage_word(index: u32, shadow_blob: bool) -> u32 {
    if (shadow_blob) {
        return paint_blob[config.paint_sdf_shadow_base + index];
    }
    return paint_blob[index];
}
