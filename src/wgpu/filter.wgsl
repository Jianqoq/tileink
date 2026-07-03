const INVALID: u32 = 0xffffffffu;
const DRAW_FLAG_TAG_MASK: u32 = 7u;
const DRAW_FLAG_FILL_RULE_EVEN_ODD: u32 = 8u;
const DRAW_FLAG_HAS_SDF: u32 = 64u;

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
    layer_stack_start: u32,
    layer_stack_end: u32,
    draw_ix: u32,
    mask_enabled: u32,
    blend_mode: u32,
    mask_kind: u32,
    clear_color: u32,
    filter_kind: u32,
    table_index: u32,
    brush_index: u32,
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
    matrix_r: vec4<f32>,
    matrix_g: vec4<f32>,
    matrix_b: vec4<f32>,
    matrix_a: vec4<f32>,
    matrix_bias: vec4<f32>,
};

@group(0) @binding(0) var<uniform> config: FilterConfig;
@group(0) @binding(1) var source_texture: texture_storage_2d<rgba8unorm, read>;
@group(0) @binding(2) var aux_texture: texture_storage_2d<rgba8unorm, read>;
@group(0) @binding(3) var target_texture: texture_storage_2d<rgba8unorm, read_write>;
@group(0) @binding(4) var<storage, read> draw_path_ids: array<u32>;
@group(0) @binding(5) var<storage, read> draw_flags: array<u32>;
@group(0) @binding(6) var<storage, read> draw_pixel_x0: array<i32>;
@group(0) @binding(7) var<storage, read> draw_pixel_y0: array<i32>;
@group(0) @binding(8) var<storage, read> draw_pixel_x1: array<i32>;
@group(0) @binding(9) var<storage, read> draw_pixel_y1: array<i32>;
@group(0) @binding(10) var<storage, read> draw_sdf_refs: array<u32>;
@group(0) @binding(11) var<storage, read> sdf_kinds: array<u32>;
@group(0) @binding(12) var<storage, read> sdf_x0: array<f32>;
@group(0) @binding(13) var<storage, read> sdf_y0: array<f32>;
@group(0) @binding(14) var<storage, read> sdf_x1: array<f32>;
@group(0) @binding(15) var<storage, read> sdf_y1: array<f32>;
@group(0) @binding(16) var<storage, read> sdf_r0: array<f32>;
@group(0) @binding(17) var<storage, read> sdf_r1: array<f32>;
@group(0) @binding(18) var<storage, read> sdf_r2: array<f32>;
@group(0) @binding(19) var<storage, read> sdf_r3: array<f32>;
@group(0) @binding(20) var<storage, read> sdf_stroke_top: array<f32>;
@group(0) @binding(21) var<storage, read> sdf_stroke_right: array<f32>;
@group(0) @binding(22) var<storage, read> sdf_stroke_bottom: array<f32>;
@group(0) @binding(23) var<storage, read> sdf_stroke_left: array<f32>;
@group(0) @binding(24) var<storage, read> sdf_shadow_offset_x: array<f32>;
@group(0) @binding(25) var<storage, read> sdf_shadow_offset_y: array<f32>;
@group(0) @binding(26) var<storage, read> sdf_shadow_expand: array<f32>;
@group(0) @binding(27) var<storage, read> sdf_shadow_intensity: array<f32>;
@group(0) @binding(28) var<storage, read> backdrop_data_offsets: array<u32>;
@group(0) @binding(29) var<storage, read> backdrop_tile_x0: array<u32>;
@group(0) @binding(30) var<storage, read> backdrop_tile_y0: array<u32>;
@group(0) @binding(31) var<storage, read> backdrop_tile_x1: array<u32>;
@group(0) @binding(32) var<storage, read> backdrop_tile_y1: array<u32>;
@group(0) @binding(33) var<storage, read_write> backdrops: array<atomic<i32>>;
@group(0) @binding(34) var<storage, read> segment_starts: array<u32>;
@group(0) @binding(35) var<storage, read> segment_ends: array<u32>;
@group(0) @binding(36) var<storage, read> segment_p0x: array<f32>;
@group(0) @binding(37) var<storage, read> segment_p0y: array<f32>;
@group(0) @binding(38) var<storage, read> segment_p1x: array<f32>;
@group(0) @binding(39) var<storage, read> segment_p1y: array<f32>;
@group(0) @binding(40) var<storage, read> segment_y_edge: array<f32>;
@group(0) @binding(41) var<storage, read> layer_stack_tags: array<u32>;
@group(0) @binding(42) var<storage, read> layer_stack_draws: array<u32>;
@group(0) @binding(43) var<storage, read> layer_stack_payloads: array<u32>;
@group(0) @binding(44) var<storage, read> transfer_tables: array<u32>;
@group(0) @binding(45) var<storage, read> brush_data: array<u32>;
@group(0) @binding(46) var<storage, read> brush_params: array<f32>;
@group(0) @binding(47) var<storage, read> brush_payloads: array<u32>;
@group(0) @binding(48) var<storage, read> convolve_kernels: array<f32>;
@group(0) @binding(49) var<storage, read> turbulence_selectors: array<u32>;
@group(0) @binding(50) var<storage, read> turbulence_gradients: array<f32>;
@group(0) @binding(51) var<storage, read> path_range_starts: array<u32>;
@group(0) @binding(52) var<storage, read> path_range_ends: array<u32>;
@group(0) @binding(53) var<storage, read> path_p0x: array<i32>;
@group(0) @binding(54) var<storage, read> path_p0y: array<i32>;
@group(0) @binding(55) var<storage, read> path_p1x: array<i32>;
@group(0) @binding(56) var<storage, read> path_p1y: array<i32>;

@compute @workgroup_size(256)
fn filter_clear_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    target_store_ix(target_ix_for_region_ix(region_ix), config.clear_color);
}

@compute @workgroup_size(256)
fn filter_copy_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    target_store_ix(ix, source_pixel_ix(ix));
}

@compute @workgroup_size(256)
fn filter_source_alpha_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    target_store_ix(ix, source_pixel_ix(ix) & 0xff000000u);
}

@compute @workgroup_size(256)
fn filter_source_over_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    target_store_ix(ix, blend_premul_u8(target_load_ix(ix), source_pixel_ix(ix), 3u << 8u));
}

@compute @workgroup_size(256)
fn filter_tile_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let source_x0 = u32(config.rect_x0);
    let source_y0 = u32(config.rect_y0);
    let source_width = u32(config.rect_x1) - source_x0;
    let source_height = u32(config.rect_y1) - source_y0;
    if (source_width == 0u || source_height == 0u) {
        return;
    }
    let sx = source_x0 + (xy.x + source_width - (source_x0 % source_width)) % source_width;
    let sy = source_y0 + (xy.y + source_height - (source_y0 % source_height)) % source_height;
    target_store_at(xy.x, xy.y, source_pixel_at(sx, sy));
}

@compute @workgroup_size(256)
fn filter_offset_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let sx = i32(xy.x) - config.offset_x;
    let sy = i32(xy.y) - config.offset_y;
    let region_x1 = i32(config.region_x0 + config.region_width);
    let region_y1 = i32(config.region_y0 + config.region_height);
    var pixel = 0u;
    if (
        sx >= i32(config.region_x0) &&
        sx < region_x1 &&
        sy >= i32(config.region_y0) &&
        sy < region_y1
    ) {
        pixel = source_pixel_at(u32(sx), u32(sy));
    }
    target_store_at(xy.x, xy.y, pixel);
}

@compute @workgroup_size(256)
fn filter_turbulence_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    target_store_at(xy.x, xy.y, filter_turbulence_pixel(f32(xy.x), f32(xy.y)));
}

@compute @workgroup_size(256)
fn filter_flood_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    target_store_at(xy.x, xy.y, sample_brush(config.brush_index, f32(xy.x) + 0.5, f32(xy.y) + 0.5));
}

@compute @workgroup_size(256)
fn filter_drop_shadow_mask_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }

    let xy = xy_for_region_ix(region_ix);
    let alpha = source_pixel_at(xy.x, xy.y) >> 24u;
    if (alpha == 0u) {
        return;
    }

    let tx = i32(xy.x) + config.offset_x;
    let ty = i32(xy.y) + config.offset_y;
    let region_x1 = i32(config.region_x0 + config.region_width);
    let region_y1 = i32(config.region_y0 + config.region_height);
    if (
        tx >= i32(config.region_x0) &&
        tx < region_x1 &&
        ty >= i32(config.region_y0) &&
        ty < region_y1
    ) {
        target_store_at(u32(tx), u32(ty), gray_alpha(alpha));
    }
}

@compute @workgroup_size(256)
fn filter_morphology_axis_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let ix = target_ix_at(xy.x, xy.y);
    let radius = config.morphology_radius;
    let morph_operator = config.morphology_operator;
    let axis = config.morphology_axis;
    let pos = select(xy.y, xy.x, axis == 0u);
    let line_len = select(config.height, config.width, axis == 0u);

    if (morph_operator == 0u && (pos < radius || pos + radius >= line_len)) {
        target_store_ix(ix, 0u);
        return;
    }

    var out_r = 1.0;
    var out_g = 1.0;
    var out_b = 1.0;
    var out_a = 1.0;
    if (morph_operator == 1u) {
        out_r = 0.0;
        out_g = 0.0;
        out_b = 0.0;
        out_a = 0.0;
    }

    var start = 0u;
    if (pos > radius) {
        start = pos - radius;
    }
    var end = line_len - 1u;
    if (pos + radius < end) {
        end = pos + radius;
    }

    var sample_pos = start;
    loop {
        if (sample_pos > end) {
            break;
        }
        let sx = select(xy.x, sample_pos, axis == 0u);
        let sy = select(sample_pos, xy.y, axis == 0u);
        let sample = source_pixel_at(sx, sy);
        let alpha = (sample >> 24u) & 255u;
        let sample_r = straight_channel(sample & 255u, alpha);
        let sample_g = straight_channel((sample >> 8u) & 255u, alpha);
        let sample_b = straight_channel((sample >> 16u) & 255u, alpha);
        let sample_a = f32(alpha) / 255.0;

        if (morph_operator == 1u) {
            out_r = max(out_r, sample_r);
            out_g = max(out_g, sample_g);
            out_b = max(out_b, sample_b);
            out_a = max(out_a, sample_a);
        } else {
            out_r = min(out_r, sample_r);
            out_g = min(out_g, sample_g);
            out_b = min(out_b, sample_b);
            out_a = min(out_a, sample_a);
        }
        sample_pos += 1u;
    }

    target_store_ix(ix, pack_premul_rgba8(out_r * out_a, out_g * out_a, out_b * out_a, out_a));
}

@compute @workgroup_size(256)
fn filter_blur_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }

    let std_dev = max(config.amount, 0.0);
    let xy = xy_for_region_ix(region_ix);
    let dst_ix = target_ix_at(xy.x, xy.y);
    if (std_dev <= 0.0) {
        target_store_ix(dst_ix, source_pixel_ix(dst_ix));
        return;
    }

    let half_width = i32(max(ceil(std_dev * 3.0), 1.0));
    let sigma = max(std_dev, 0.0001);
    let two_sigma_sq = 2.0 * sigma * sigma;
    let region_x1 = i32(config.region_x0 + config.region_width);
    let region_y1 = i32(config.region_y0 + config.region_height);
    let base_x = i32(xy.x);
    let base_y = i32(xy.y);
    var sum = 0.0;
    var r = 0.0;
    var g = 0.0;
    var b = 0.0;
    var a = 0.0;
    var d = -half_width;
    loop {
        if (d > half_width) {
            break;
        }
        let df = f32(d);
        let weight = exp(-(df * df) / two_sigma_sq);
        sum += weight;
        var sample_x = base_x;
        var sample_y = base_y;
        if (config.blur_axis == 0u) {
            sample_x += d;
        } else {
            sample_y += d;
        }
        if (
            sample_x >= i32(config.region_x0) &&
            sample_x < region_x1 &&
            sample_y >= i32(config.region_y0) &&
            sample_y < region_y1
        ) {
            let px = source_pixel_at(u32(sample_x), u32(sample_y));
            r += f32(px & 255u) * weight;
            g += f32((px >> 8u) & 255u) * weight;
            b += f32((px >> 16u) & 255u) * weight;
            a += f32((px >> 24u) & 255u) * weight;
        }
        d += 1i;
    }

    var scale = 0.0;
    if (sum > 0.0) {
        scale = 1.0 / (255.0 * sum);
    }
    target_store_ix(dst_ix, pack_premul_rgba8(r * scale, g * scale, b * scale, a * scale));
}

@compute @workgroup_size(256)
fn filter_svg_mask_coverage_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    let px = source_pixel_ix(ix);
    let a = px >> 24u;
    var mask_alpha = a;
    if (config.mask_kind == SVG_MASK_LUMINANCE) {
        var safe_a = a;
        if (safe_a == 0u) {
            safe_a = 1u;
        }
        let r = px & 255u;
        let g = (px >> 8u) & 255u;
        let b = (px >> 16u) & 255u;
        let straight_r = (r * 255u + safe_a / 2u) / safe_a;
        let straight_g = (g * 255u + safe_a / 2u) / safe_a;
        let straight_b = (b * 255u + safe_a / 2u) / safe_a;
        mask_alpha = ((2126u * straight_r + 7152u * straight_g + 722u * straight_b) * a + 1275000u) / 2550000u;
    }
    target_store_ix(ix, gray_alpha(mask_alpha));
}

@compute @workgroup_size(256)
fn filter_apply_region_mask(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    let alpha = combine_alpha(target_load_ix(ix) >> 24u, aux_pixel_ix(ix) >> 24u);
    target_store_ix(ix, gray_alpha(alpha));
}

@compute @workgroup_size(256)
fn filter_color_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    target_store_ix(ix, apply_color_filter_pixel(target_load_ix(ix), config.filter_kind, config.amount));
}

@compute @workgroup_size(256)
fn filter_color_matrix_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    target_store_ix(ix, apply_color_matrix_pixel(target_load_ix(ix)));
}

@compute @workgroup_size(256)
fn filter_component_transfer_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    target_store_ix(ix, apply_component_transfer_pixel(target_load_ix(ix), config.table_index));
}

@compute @workgroup_size(256)
fn filter_blend_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    target_store_ix(ix, blend_premul_u8(aux_pixel_ix(ix), source_pixel_ix(ix), config.blend_mode));
}

@compute @workgroup_size(256)
fn filter_composite_inputs_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    target_store_ix(ix, composite_inputs_pixel(
        source_pixel_ix(ix),
        aux_pixel_ix(ix),
        config.filter_kind,
        config.matrix_bias.x,
        config.matrix_bias.y,
        config.matrix_bias.z,
        config.matrix_bias.w,
    ));
}

@compute @workgroup_size(256)
fn filter_displacement_map_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let ix = target_ix_at(xy.x, xy.y);
    let map = aux_pixel_ix(ix);
    let dx = filter_displacement_channel(map, config.kernel_edge_mode, config.lighting_output_kind) - 0.5;
    let dy = filter_displacement_channel(map, config.kernel_preserve_alpha, config.lighting_output_kind) - 0.5;
    let sx = i32(round(f32(xy.x) + dx * config.amount));
    let sy = i32(round(f32(xy.y) + dy * config.rect_x0));
    var out = 0u;
    if (sx >= 0 && sx < i32(config.width) && sy >= 0 && sy < i32(config.height)) {
        out = source_pixel_at(u32(sx), u32(sy));
    }
    target_store_ix(ix, out);
}

@compute @workgroup_size(256)
fn filter_convolve_matrix_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let dst_ix = target_ix_at(xy.x, xy.y);
    let divisor = config.amount;
    if (config.kernel_columns == 0u || config.kernel_rows == 0u || divisor == 0.0) {
        target_store_ix(dst_ix, source_pixel_ix(dst_ix));
        return;
    }

    let region_x1 = i32(config.region_x0 + config.region_width);
    let region_y1 = i32(config.region_y0 + config.region_height);
    var out_r = 0.0;
    var out_g = 0.0;
    var out_b = 0.0;
    var out_a = 0.0;
    var ky = 0u;
    loop {
        if (ky >= config.kernel_rows) {
            break;
        }
        var kx = 0u;
        loop {
            if (kx >= config.kernel_columns) {
                break;
            }
            let kernel_ix = config.kernel_offset +
                (config.kernel_rows - 1u - ky) * config.kernel_columns +
                (config.kernel_columns - 1u - kx);
            let weight = convolve_kernels[kernel_ix];
            var sx = i32(xy.x) + i32(kx) - i32(config.kernel_target_x);
            var sy = i32(xy.y) + i32(ky) - i32(config.kernel_target_y);
            var sample = 0u;
            if (config.kernel_edge_mode == 1u) {
                sx = clamp(sx, i32(config.region_x0), region_x1 - 1);
                sy = clamp(sy, i32(config.region_y0), region_y1 - 1);
                sample = source_pixel_at(u32(sx), u32(sy));
            } else if (config.kernel_edge_mode == 2u) {
                while (sx < i32(config.region_x0)) {
                    sx = sx + i32(config.region_width);
                }
                while (sx >= region_x1) {
                    sx = sx - i32(config.region_width);
                }
                while (sy < i32(config.region_y0)) {
                    sy = sy + i32(config.region_height);
                }
                while (sy >= region_y1) {
                    sy = sy - i32(config.region_height);
                }
                sample = source_pixel_at(u32(sx), u32(sy));
            } else if (
                sx >= i32(config.region_x0) &&
                sx < region_x1 &&
                sy >= i32(config.region_y0) &&
                sy < region_y1
            ) {
                sample = source_pixel_at(u32(sx), u32(sy));
            }

            let alpha = (sample >> 24u) & 255u;
            out_r += straight_channel(sample & 255u, alpha) * weight;
            out_g += straight_channel((sample >> 8u) & 255u, alpha) * weight;
            out_b += straight_channel((sample >> 16u) & 255u, alpha) * weight;
            out_a += (f32(alpha) / 255.0) * weight;
            kx += 1u;
        }
        ky += 1u;
    }

    let base_alpha = f32((source_pixel_ix(dst_ix) >> 24u) & 255u) / 255.0;
    var alpha = clamp(out_a / divisor + config.rect_x0, 0.0, 1.0);
    if (config.kernel_preserve_alpha == 1u) {
        alpha = base_alpha;
    }
    let r = clamp(out_r / divisor + config.rect_x0, 0.0, 1.0);
    let g = clamp(out_g / divisor + config.rect_x0, 0.0, 1.0);
    let b = clamp(out_b / divisor + config.rect_x0, 0.0, 1.0);
    target_store_ix(dst_ix, pack_premul_rgba8(r * alpha, g * alpha, b * alpha, alpha));
}

@compute @workgroup_size(256)
fn filter_lighting_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let dst_ix = target_ix_at(xy.x, xy.y);
    var no_light = 0u;
    if (config.lighting_output_kind == 0u) {
        no_light = 0xff000000u;
    }

    let alpha = source_alpha_at(xy.x, xy.y);
    let z = alpha * config.surface_scale;
    let dx = alpha_gradient_x(xy.x, xy.y) * config.surface_scale;
    let dy = alpha_gradient_y(xy.x, xy.y) * config.surface_scale;
    let normal_len = sqrt(dx * dx + dy * dy + 1.0);
    let nx = -dx / normal_len;
    let ny = -dy / normal_len;
    let nz = 1.0 / normal_len;

    let world_x = f32(config.surface_origin_x) + f32(xy.x) + 0.5;
    let world_y = f32(config.surface_origin_y) + f32(xy.y) + 0.5;
    var lx = config.light_p0 - world_x;
    var ly = config.light_p1 - world_y;
    var lz = config.light_p2 - z;
    var attenuation = 1.0;
    let eps = 0.000001;

    if (config.light_kind == 0u) {
        let azimuth = config.light_p0 * 0.017453292;
        let elevation = config.light_p1 * 0.017453292;
        lx = cos(azimuth) * cos(elevation);
        ly = sin(azimuth) * cos(elevation);
        lz = sin(elevation);
    } else {
        let len = sqrt(lx * lx + ly * ly + lz * lz);
        if (len <= eps) {
            target_store_ix(dst_ix, no_light);
            return;
        }
        lx = lx / len;
        ly = ly / len;
        lz = lz / len;

        if (config.light_kind == 2u) {
            var sx = config.light_p3 - config.light_p0;
            var sy = config.light_p4 - config.light_p1;
            var sz = config.light_p5 - config.light_p2;
            let slen = sqrt(sx * sx + sy * sy + sz * sz);
            if (slen <= eps) {
                target_store_ix(dst_ix, no_light);
                return;
            }
            sx = sx / slen;
            sy = sy / slen;
            sz = sz / slen;
            let focus = max(-(lx * sx + ly * sy + lz * sz), 0.0);
            if (config.light_p7 >= 0.0 && focus < cos(config.light_p7 * 0.017453292)) {
                target_store_ix(dst_ix, no_light);
                return;
            }
            attenuation = pow(focus, max(config.light_p6, 0.0));
        }
    }

    if (config.lighting_output_kind == 0u) {
        let amount = config.light_constant * attenuation * max(nx * lx + ny * ly + nz * lz, 0.0);
        target_store_ix(dst_ix, pack_premul_rgba8(
            clamp(config.light_r * amount, 0.0, 1.0),
            clamp(config.light_g * amount, 0.0, 1.0),
            clamp(config.light_b * amount, 0.0, 1.0),
            1.0,
        ));
    } else {
        var hx = lx;
        var hy = ly;
        var hz = lz + 1.0;
        let hlen = sqrt(hx * hx + hy * hy + hz * hz);
        if (hlen <= eps) {
            target_store_ix(dst_ix, no_light);
            return;
        }
        hx = hx / hlen;
        hy = hy / hlen;
        hz = hz / hlen;

        let normal_dot_half = max(nx * hx + ny * hy + nz * hz, 0.0);
        let amount = config.light_constant * attenuation * pow(normal_dot_half, max(config.specular_exponent, 0.0));
        let r = clamp(config.light_r * amount, 0.0, 1.0);
        let g = clamp(config.light_g * amount, 0.0, 1.0);
        let b = clamp(config.light_b * amount, 0.0, 1.0);
        target_store_ix(dst_ix, pack_premul_rgba8(r, g, b, max(max(r, g), b)));
    }
}

@compute @workgroup_size(256)
fn filter_liquid_glass_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let ix = target_ix_at(xy.x, xy.y);
    let world_x = f32(xy.x) + 0.5;
    let world_y = f32(xy.y) + 0.5;

    let distance = liquid_glass_round_rect_distance(
        world_x,
        world_y,
        config.rect_x0,
        config.rect_y0,
        config.rect_x1,
        config.rect_y1,
        config.radius_top_left,
        config.radius_top_right,
        config.radius_bottom_left,
        config.radius_bottom_right,
    );
    let base = source_pixel_ix(ix);
    let surface_height = f32(max(config.height, 1u));
    let distance_norm = distance / surface_height;
    if (distance_norm >= LIQUID_GLASS_ACTIVE_DISTANCE_NORM) {
        target_store_ix(ix, base);
        return;
    }

    target_store_ix(ix, liquid_glass_pixel(
        base,
        world_x,
        world_y,
        f32(xy.x),
        f32(xy.y),
        distance,
        distance_norm,
        surface_height,
    ));
}

@compute @workgroup_size(256)
fn filter_composite_drop_shadow_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }

    let xy = xy_for_region_ix(region_ix);
    let ix = target_ix_at(xy.x, xy.y);
    let alpha = aux_pixel_ix(ix) >> 24u;
    let shadow_color = sample_brush(config.brush_index, f32(xy.x) + 0.5, f32(xy.y) + 0.5);
    let shadow = scale_premul_u8(shadow_color, alpha);
    target_store_ix(ix, src_over_premul_u8(shadow, target_load_ix(ix)));
}

@compute @workgroup_size(256)
fn filter_layer_mask_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let alpha = layer_stack_alpha_at(config.draw_ix, xy.x, xy.y);
    target_store_at(xy.x, xy.y, gray_alpha(alpha));
}

@compute @workgroup_size(256)
fn filter_rect_mask_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let dist = rect_sdf_distance(
        f32(xy.x) + 0.5,
        f32(xy.y) + 0.5,
        config.rect_x0,
        config.rect_y0,
        config.rect_x1,
        config.rect_y1,
        config.radius_top_left,
        config.radius_top_right,
        config.radius_bottom_left,
        config.radius_bottom_right,
    );
    target_store_at(xy.x, xy.y, gray_alpha(coverage_to_u8(sdf_coverage_from_dist(dist))));
}

@compute @workgroup_size(256)
fn filter_path_mask_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let px = f32(xy.x) + 0.5;
    let py = f32(xy.y) + 0.5;
    let path_index = config.table_index;
    var winding = 0i;
    if (path_index < arrayLength(&path_range_starts)) {
        var line_ix = path_range_starts[path_index];
        let line_end = path_range_ends[path_index];
        loop {
            if (line_ix >= line_end) {
                break;
            }
            let inv_scale = 0.00390625;
            let y0 = f32(path_p0y[line_ix]) * inv_scale;
            let y1 = f32(path_p1y[line_ix]) * inv_scale;
            var winding_delta = 0i;
            if (y0 <= py) {
                if (y1 > py) {
                    winding_delta = 1i;
                }
            }
            if (y1 <= py) {
                if (y0 > py) {
                    winding_delta = -1i;
                }
            }
            if (winding_delta != 0i) {
                let x0 = f32(path_p0x[line_ix]) * inv_scale;
                let x1 = f32(path_p1x[line_ix]) * inv_scale;
                let t = (py - y0) / (y1 - y0);
                let x_cross = x0 + (x1 - x0) * t;
                if (x_cross > px) {
                    winding += winding_delta;
                }
            }
            line_ix += 1u;
        }
    }

    var alpha = 0u;
    if (winding != 0i) {
        alpha = 255u;
    }
    target_store_at(xy.x, xy.y, gray_alpha(alpha));
}

@compute @workgroup_size(256)
fn filter_composite_stack_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let ix = target_ix_at(xy.x, xy.y);
    target_store_ix(ix, composite_with_stack(target_load_ix(ix), source_pixel_ix(ix), aux_pixel_ix(ix), xy.x, xy.y, false));
}

@compute @workgroup_size(256)
fn filter_composite_blend_stack_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let ix = target_ix_at(xy.x, xy.y);
    target_store_ix(ix, composite_with_stack(target_load_ix(ix), source_pixel_ix(ix), aux_pixel_ix(ix), xy.x, xy.y, true));
}

@compute @workgroup_size(256)
fn filter_composite_surface_stack_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let sx = i32(xy.x) - config.offset_x;
    let sy = i32(xy.y) - config.offset_y;
    if (
        sx < 0 ||
        sy < 0 ||
        sx >= i32(config.kernel_columns) ||
        sy >= i32(config.kernel_rows)
    ) {
        return;
    }

    let ix = target_ix_at(xy.x, xy.y);
    target_store_ix(ix, composite_surface_with_stack(target_load_ix(ix), source_pixel_at(u32(sx), u32(sy)), xy.x, xy.y));
}

fn composite_with_stack(dst: u32, source: u32, mask: u32, x: u32, y: u32, force_blend: bool) -> u32 {
    var pixel = dst;
    var clip_mask = 255u;
    var group_depth = 0u;
    var group_kinds: array<u32, 64>;
    var group_parent_pixels: array<u32, 64>;
    var group_parent_clips: array<u32, 64>;
    var group_layer_alphas: array<u32, 64>;
    var group_payloads: array<u32, 64>;

    var stack_ix = config.layer_stack_start;
    loop {
        if (stack_ix >= config.layer_stack_end) {
            break;
        }
        let tag = layer_stack_tags[stack_ix];
        let alpha = layer_stack_alpha_at(layer_stack_draws[stack_ix], x, y);
        if (tag == GPU_LAYER_CLIP) {
            clip_mask = combine_alpha(clip_mask, alpha);
        } else if (tag == GPU_LAYER_OPACITY || tag == GPU_LAYER_BLEND) {
            if (group_depth < FILTER_GROUP_STACK_CAPACITY) {
                group_kinds[group_depth] = tag;
                group_parent_pixels[group_depth] = pixel;
                group_parent_clips[group_depth] = clip_mask;
                group_layer_alphas[group_depth] = alpha;
                group_payloads[group_depth] = layer_stack_payloads[stack_ix];
                group_depth += 1u;
                pixel = 0u;
            }
        }
        stack_ix += 1u;
    }

    var source_alpha = clip_mask;
    if (config.mask_enabled != 0u || force_blend) {
        source_alpha = combine_alpha(source_alpha, mask >> 24u);
    }
    let scaled_source = scale_premul_u8(source, source_alpha);
    if (force_blend) {
        if ((scaled_source >> 24u) != 0u) {
            pixel = blend_premul_u8(pixel, scaled_source, config.blend_mode);
        }
    } else {
        pixel = src_over_premul_u8(pixel, scaled_source);
    }

    loop {
        if (group_depth == 0u) {
            break;
        }
        group_depth -= 1u;
        let parent = group_parent_pixels[group_depth];
        let parent_clip = group_parent_clips[group_depth];
        let layer_alpha = group_layer_alphas[group_depth];
        let payload = group_payloads[group_depth];
        let group_kind = group_kinds[group_depth];
        var alpha = combine_alpha(layer_alpha, parent_clip);
        if (group_kind == GPU_LAYER_OPACITY) {
            alpha = combine_alpha(alpha, payload);
            pixel = src_over_premul_u8(parent, scale_premul_u8(pixel, alpha));
        } else if (group_kind == GPU_LAYER_BLEND) {
            let src = scale_premul_u8(pixel, alpha);
            if ((src >> 24u) == 0u) {
                pixel = parent;
            } else {
                pixel = blend_premul_u8(parent, src, payload);
            }
        }
    }

    return pixel;
}

fn composite_surface_with_stack(dst: u32, source: u32, x: u32, y: u32) -> u32 {
    var pixel = dst;
    var clip_mask = 255u;
    var group_depth = 0u;
    var group_kinds: array<u32, 64>;
    var group_parent_pixels: array<u32, 64>;
    var group_parent_clips: array<u32, 64>;
    var group_layer_alphas: array<u32, 64>;
    var group_payloads: array<u32, 64>;

    var stack_ix = config.layer_stack_start;
    loop {
        if (stack_ix >= config.layer_stack_end) {
            break;
        }
        let tag = layer_stack_tags[stack_ix];
        let alpha = layer_stack_alpha_at(layer_stack_draws[stack_ix], x, y);
        if (tag == GPU_LAYER_CLIP) {
            clip_mask = combine_alpha(clip_mask, alpha);
        } else if (tag == GPU_LAYER_OPACITY || tag == GPU_LAYER_BLEND) {
            if (group_depth < FILTER_GROUP_STACK_CAPACITY) {
                group_kinds[group_depth] = tag;
                group_parent_pixels[group_depth] = pixel;
                group_parent_clips[group_depth] = clip_mask;
                group_layer_alphas[group_depth] = alpha;
                group_payloads[group_depth] = layer_stack_payloads[stack_ix];
                group_depth += 1u;
                pixel = 0u;
            }
        }
        stack_ix += 1u;
    }

    pixel = src_over_premul_u8(pixel, scale_premul_u8(source, clip_mask));

    loop {
        if (group_depth == 0u) {
            break;
        }
        group_depth -= 1u;
        let parent = group_parent_pixels[group_depth];
        let parent_clip = group_parent_clips[group_depth];
        let layer_alpha = group_layer_alphas[group_depth];
        let payload = group_payloads[group_depth];
        let group_kind = group_kinds[group_depth];
        var alpha = combine_alpha(layer_alpha, parent_clip);
        if (group_kind == GPU_LAYER_OPACITY) {
            alpha = combine_alpha(alpha, payload);
            pixel = src_over_premul_u8(parent, scale_premul_u8(pixel, alpha));
        } else if (group_kind == GPU_LAYER_BLEND) {
            let src = scale_premul_u8(pixel, alpha);
            if ((src >> 24u) == 0u) {
                pixel = parent;
            } else {
                pixel = blend_premul_u8(parent, src, payload);
            }
        }
    }

    return pixel;
}

fn xy_for_region_ix(region_ix: u32) -> vec2<u32> {
    return vec2<u32>(
        config.region_x0 + region_ix % config.region_width,
        config.region_y0 + region_ix / config.region_width,
    );
}

fn target_ix_for_region_ix(region_ix: u32) -> u32 {
    let xy = xy_for_region_ix(region_ix);
    return target_ix_at(xy.x, xy.y);
}

fn target_ix_at(x: u32, y: u32) -> u32 {
    return y * config.width + x;
}

fn xy_for_target_ix(ix: u32) -> vec2<u32> {
    return vec2<u32>(ix % config.width, ix / config.width);
}

fn source_pixel_at(x: u32, y: u32) -> u32 {
    return unorm_to_rgba8(textureLoad(source_texture, vec2<i32>(i32(x), i32(y))));
}

fn source_pixel_ix(ix: u32) -> u32 {
    let xy = xy_for_target_ix(ix);
    return source_pixel_at(xy.x, xy.y);
}

fn aux_pixel_at(x: u32, y: u32) -> u32 {
    return unorm_to_rgba8(textureLoad(aux_texture, vec2<i32>(i32(x), i32(y))));
}

fn aux_pixel_ix(ix: u32) -> u32 {
    let xy = xy_for_target_ix(ix);
    return aux_pixel_at(xy.x, xy.y);
}

fn target_load_at(x: u32, y: u32) -> u32 {
    return unorm_to_rgba8(textureLoad(target_texture, vec2<i32>(i32(x), i32(y))));
}

fn target_load_ix(ix: u32) -> u32 {
    let xy = xy_for_target_ix(ix);
    return target_load_at(xy.x, xy.y);
}

fn target_store_at(x: u32, y: u32, pixel: u32) {
    textureStore(target_texture, vec2<i32>(i32(x), i32(y)), rgba8_to_unorm(pixel));
}

fn target_store_ix(ix: u32, pixel: u32) {
    let xy = xy_for_target_ix(ix);
    target_store_at(xy.x, xy.y, pixel);
}

fn gray_alpha(alpha: u32) -> u32 {
    return alpha | (alpha << 8u) | (alpha << 16u) | (alpha << 24u);
}

fn layer_stack_alpha_at(draw_ix: u32, x: u32, y: u32) -> u32 {
    let draw_i = draw_ix;
    var alpha = 0u;
    if (draw_i >= arrayLength(&draw_flags)) {
        return alpha;
    }
    let global_x = i32(x);
    let global_y = i32(y);
    let sdf_ix = draw_sdf_refs[draw_i];
    if (sdf_ix != INVALID) {
        if (
            global_x >= draw_pixel_x0[draw_i] &&
            global_x < draw_pixel_x1[draw_i] &&
            global_y >= draw_pixel_y0[draw_i] &&
            global_y < draw_pixel_y1[draw_i]
        ) {
            alpha = coverage_to_u8(sdf_coverage_from_encoded(sdf_ix, f32(x) + 0.5, f32(y) + 0.5));
        }
    } else {
        let tile_x = x / 16u;
        let tile_y = y / 16u;
        let local_x = x - tile_x * 16u;
        let local_y = y - tile_y * 16u;
        let backdrop_ix = draw_backdrop_ix(draw_ix, tile_x, tile_y);
        if (backdrop_ix != INVALID) {
            alpha = fill_alpha_at(
                atomicLoad(&backdrops[backdrop_ix]),
                draw_fill_rule_at(draw_ix),
                segment_starts[backdrop_ix],
                segment_ends[backdrop_ix],
                local_x,
                local_y,
            );
        }
    }
    return alpha;
}

fn draw_backdrop_ix(draw_ix: u32, tile_x: u32, tile_y: u32) -> u32 {
    let path_id = draw_path_ids[draw_ix];
    let draw_tag = draw_tag_at(draw_ix);
    var result = INVALID;
    if (
        path_id != INVALID &&
        (draw_tag == GPU_DRAW_BRUSH ||
         draw_tag == GPU_DRAW_PATH_GLYPH ||
         draw_tag == GPU_DRAW_CLIP ||
         draw_tag == GPU_DRAW_OPACITY ||
         draw_tag == GPU_DRAW_BLEND ||
         draw_tag == GPU_DRAW_ISOLATE)
    ) {
        let draw_x0 = pixel_tile_min(draw_pixel_x0[draw_ix], config.tiles_width);
        let draw_y0 = pixel_tile_min(draw_pixel_y0[draw_ix], config.tiles_height);
        let draw_x1 = pixel_tile_max(draw_pixel_x1[draw_ix], config.tiles_width);
        let draw_y1 = pixel_tile_max(draw_pixel_y1[draw_ix], config.tiles_height);
        if (
            tile_x >= draw_x0 &&
            tile_x < draw_x1 &&
            tile_y >= draw_y0 &&
            tile_y < draw_y1 &&
            path_id < arrayLength(&backdrop_data_offsets)
        ) {
            let bx0 = backdrop_tile_x0[path_id];
            let by0 = backdrop_tile_y0[path_id];
            let bx1 = backdrop_tile_x1[path_id];
            let by1 = backdrop_tile_y1[path_id];
            let stride = bx1 - bx0;
            if (stride > 0u && tile_x >= bx0 && tile_x < bx1 && tile_y >= by0 && tile_y < by1) {
                result = backdrop_data_offsets[path_id] + (tile_y - by0) * stride + tile_x - bx0;
            }
        }
    }
    return result;
}

fn pixel_tile_min(value: i32, limit: u32) -> u32 {
    var tile = 0u;
    if (value > 0i) {
        tile = min(u32(value) / 16u, limit);
    }
    return tile;
}

fn pixel_tile_max(value: i32, limit: u32) -> u32 {
    var tile = 0u;
    if (value > 0i) {
        tile = min((u32(value) + 15u) / 16u, limit);
    }
    return tile;
}

fn draw_tag_at(draw_ix: u32) -> u32 {
    return draw_flags[draw_ix] & DRAW_FLAG_TAG_MASK;
}

fn draw_fill_rule_at(draw_ix: u32) -> u32 {
    return (draw_flags[draw_ix] & DRAW_FLAG_FILL_RULE_EVEN_ODD) >> 3u;
}

fn fill_alpha_at(backdrop: i32, fill_rule: u32, segment_start: u32, segment_end: u32, x: u32, y: u32) -> u32 {
    var coverage = f32(backdrop);
    var segment_ix = segment_start;
    loop {
        if (segment_ix >= segment_end) {
            break;
        }
        coverage += segment_coverage_at(
            segment_p0x[segment_ix],
            segment_p0y[segment_ix],
            segment_p1x[segment_ix],
            segment_p1y[segment_ix],
            segment_y_edge[segment_ix],
            x,
            y,
        );
        segment_ix += 1u;
    }
    return coverage_to_alpha(coverage, fill_rule);
}

fn segment_coverage_at(p0x: f32, p0y: f32, p1x: f32, p1y: f32, y_edge: f32, x: u32, y: u32) -> f32 {
    let delta_x = p1x - p0x;
    let delta_y = p1y - p0y;
    let row_y = f32(y);
    let local_y = p0y - row_y;
    let y0 = clamp(local_y, 0.0, 1.0);
    let y1 = clamp(local_y + delta_y, 0.0, 1.0);
    let dy = y0 - y1;
    let x_sign = signum_f32(delta_x);
    var coverage = x_sign * clamp(row_y - y_edge + 1.0, 0.0, 1.0);

    if (dy != 0.0) {
        let recip = 1.0 / delta_y;
        let t0 = (y0 - local_y) * recip;
        let t1 = (y1 - local_y) * recip;
        let sx0 = p0x + t0 * delta_x;
        let sx1 = p0x + t1 * delta_x;
        let pixel_x = f32(x);
        let xmin = min(sx0, sx1) - pixel_x;
        let xmax = max(sx0, sx1) - pixel_x;
        var area = clamp(1.0 - xmin, 0.0, 1.0);
        if (xmax - xmin > 0.000001) {
            let a_min = min(xmin, 1.0) - 0.000001;
            let b = min(xmax, 1.0);
            let c = max(b, 0.0);
            let d = max(a_min, 0.0);
            area = (b + 0.5 * (d * d - c * c) - a_min) / (xmax - a_min);
        }
        coverage += area * dy;
    }

    return coverage;
}

fn signum_f32(value: f32) -> f32 {
    var out = 1.0;
    if (value < 0.0) {
        out = -1.0;
    }
    return out;
}

fn coverage_to_alpha(value: f32, fill_rule: u32) -> u32 {
    var alpha = min(abs(value), 1.0);
    if (fill_rule == 1u) {
        alpha = abs(value - 2.0 * round(0.5 * value));
    }
    return u32(clamp(alpha, 0.0, 1.0) * 255.0 + 0.5);
}

fn apply_color_filter_pixel(px: u32, filter_kind: u32, amount: f32) -> u32 {
    let inv_255 = 1.0 / 255.0;
    var r = f32(px & 255u) * inv_255;
    var g = f32((px >> 8u) & 255u) * inv_255;
    var b = f32((px >> 16u) & 255u) * inv_255;
    var a = f32((px >> 24u) & 255u) * inv_255;

    if (filter_kind == FILTER_OPACITY) {
        let opacity = clamp(amount, 0.0, 1.0);
        r *= opacity;
        g *= opacity;
        b *= opacity;
        a *= opacity;
    } else if (a > 0.0) {
        let alpha = a;
        var ur = r / alpha;
        var ug = g / alpha;
        var ub = b / alpha;

        if (filter_kind == FILTER_BRIGHTNESS) {
            ur *= amount;
            ug *= amount;
            ub *= amount;
        } else if (filter_kind == FILTER_CONTRAST) {
            ur = (ur - 0.5) * amount + 0.5;
            ug = (ug - 0.5) * amount + 0.5;
            ub = (ub - 0.5) * amount + 0.5;
        } else if (filter_kind == FILTER_GRAYSCALE) {
            let t = clamp(amount, 0.0, 1.0);
            let l = svg_lum3(ur, ug, ub);
            ur = lerp_f32(ur, l, t);
            ug = lerp_f32(ug, l, t);
            ub = lerp_f32(ub, l, t);
        } else if (filter_kind == FILTER_HUE_ROTATE) {
            let angle = amount * 0.017453292;
            let co = cos(angle);
            let si = sin(angle);
            let nr = (0.213 + co * 0.787 - si * 0.213) * ur +
                (0.715 - co * 0.715 - si * 0.715) * ug +
                (0.072 - co * 0.072 + si * 0.928) * ub;
            let ng = (0.213 - co * 0.213 + si * 0.143) * ur +
                (0.715 + co * 0.285 + si * 0.140) * ug +
                (0.072 - co * 0.072 - si * 0.283) * ub;
            let nb = (0.213 - co * 0.213 - si * 0.787) * ur +
                (0.715 - co * 0.715 + si * 0.715) * ug +
                (0.072 + co * 0.928 + si * 0.072) * ub;
            ur = nr;
            ug = ng;
            ub = nb;
        } else if (filter_kind == FILTER_INVERT) {
            let t = clamp(amount, 0.0, 1.0);
            ur = lerp_f32(ur, 1.0 - ur, t);
            ug = lerp_f32(ug, 1.0 - ug, t);
            ub = lerp_f32(ub, 1.0 - ub, t);
        } else if (filter_kind == FILTER_SATURATE) {
            let l = svg_lum3(ur, ug, ub);
            ur = l + (ur - l) * amount;
            ug = l + (ug - l) * amount;
            ub = l + (ub - l) * amount;
        } else if (filter_kind == FILTER_SEPIA) {
            let t = clamp(amount, 0.0, 1.0);
            let sr = ur * 0.393 + ug * 0.769 + ub * 0.189;
            let sg = ur * 0.349 + ug * 0.686 + ub * 0.168;
            let sb = ur * 0.272 + ug * 0.534 + ub * 0.131;
            ur = lerp_f32(ur, sr, t);
            ug = lerp_f32(ug, sg, t);
            ub = lerp_f32(ub, sb, t);
        }

        r = clamp(ur, 0.0, 1.0) * alpha;
        g = clamp(ug, 0.0, 1.0) * alpha;
        b = clamp(ub, 0.0, 1.0) * alpha;
    }

    return pack_premul_rgba8(r, g, b, a);
}

fn apply_color_matrix_pixel(px: u32) -> u32 {
    let inv_255 = 1.0 / 255.0;
    let premul_r = f32(px & 255u) * inv_255;
    let premul_g = f32((px >> 8u) & 255u) * inv_255;
    let premul_b = f32((px >> 16u) & 255u) * inv_255;
    let alpha = f32((px >> 24u) & 255u) * inv_255;

    var r = 0.0;
    var g = 0.0;
    var b = 0.0;
    if (alpha > 0.0) {
        r = premul_r / alpha;
        g = premul_g / alpha;
        b = premul_b / alpha;
    }

    let rgba = vec4<f32>(r, g, b, alpha);
    let out_r = dot(config.matrix_r, rgba) + config.matrix_bias.x;
    let out_g = dot(config.matrix_g, rgba) + config.matrix_bias.y;
    let out_b = dot(config.matrix_b, rgba) + config.matrix_bias.z;
    let out_a = clamp(dot(config.matrix_a, rgba) + config.matrix_bias.w, 0.0, 1.0);
    return pack_premul_rgba8(
        clamp(out_r, 0.0, 1.0) * out_a,
        clamp(out_g, 0.0, 1.0) * out_a,
        clamp(out_b, 0.0, 1.0) * out_a,
        out_a,
    );
}

fn apply_component_transfer_pixel(px: u32, table_index: u32) -> u32 {
    let alpha = (px >> 24u) & 255u;
    let base = table_index * COMPONENT_TRANSFER_TABLE_LEN;
    let r_index = straight_component_index(px & 255u, alpha);
    let g_index = straight_component_index((px >> 8u) & 255u, alpha);
    let b_index = straight_component_index((px >> 16u) & 255u, alpha);
    let inv_255 = 1.0 / 255.0;
    let r = f32(transfer_tables[base + r_index]) * inv_255;
    let g = f32(transfer_tables[base + COMPONENT_TRANSFER_TABLE_SIZE + g_index]) * inv_255;
    let b = f32(transfer_tables[base + 2u * COMPONENT_TRANSFER_TABLE_SIZE + b_index]) * inv_255;
    let a = f32(transfer_tables[base + 3u * COMPONENT_TRANSFER_TABLE_SIZE + alpha]) * inv_255;
    return pack_premul_rgba8(r * a, g * a, b * a, a);
}

fn straight_component_index(premul: u32, alpha: u32) -> u32 {
    if (alpha == 0u) {
        return 0u;
    }
    return min((premul * 255u + alpha / 2u) / alpha, 255u);
}

fn rgba8_pack(r: u32, g: u32, b: u32, a: u32) -> u32 {
    return r | (g << 8u) | (b << 16u) | (a << 24u);
}

fn unorm_to_rgba8(pixel: vec4<f32>) -> u32 {
    return rgba8_pack(
        u32(clamp(pixel.r * 255.0 + 0.5, 0.0, 255.0)),
        u32(clamp(pixel.g * 255.0 + 0.5, 0.0, 255.0)),
        u32(clamp(pixel.b * 255.0 + 0.5, 0.0, 255.0)),
        u32(clamp(pixel.a * 255.0 + 0.5, 0.0, 255.0)),
    );
}

fn rgba8_to_unorm(pixel: u32) -> vec4<f32> {
    return vec4<f32>(
        f32(pixel & 255u) * (1.0 / 255.0),
        f32((pixel >> 8u) & 255u) * (1.0 / 255.0),
        f32((pixel >> 16u) & 255u) * (1.0 / 255.0),
        f32((pixel >> 24u) & 255u) * (1.0 / 255.0),
    );
}

fn mul_div255(a: u32, b: u32) -> u32 {
    let t = a * b + 128u;
    return (t + (t >> 8u)) >> 8u;
}

fn combine_alpha(a: u32, b: u32) -> u32 {
    return mul_div255(a, b);
}

fn straight_channel(premul: u32, alpha: u32) -> f32 {
    if (alpha == 0u) {
        return 0.0;
    }
    return f32(premul) / f32(alpha);
}

fn source_alpha_at(x: u32, y: u32) -> f32 {
    return f32((source_pixel_at(x, y) >> 24u) & 255u) / 255.0;
}

fn filter_displacement_channel(px: u32, channel: u32, linear_rgb: u32) -> f32 {
    let alpha = (px >> 24u) & 255u;
    var value = f32(alpha) / 255.0;
    if (channel != 3u) {
        var premul = px & 255u;
        if (channel == 1u) {
            premul = (px >> 8u) & 255u;
        } else if (channel == 2u) {
            premul = (px >> 16u) & 255u;
        }
        value = straight_channel(premul, alpha);
        if (linear_rgb != 0u) {
            value = filter_srgb_to_linear(value);
        }
    }
    return value;
}

fn filter_srgb_to_linear(value: f32) -> f32 {
    var out = value / 12.92;
    if (value > 0.04045) {
        out = pow((value + 0.055) / 1.055, 2.4);
    }
    return out;
}

fn filter_linear_rgb_to_srgb(value: f32) -> f32 {
    var out = value * 12.92;
    if (value > 0.0031308) {
        out = 1.055 * pow(value, 1.0 / 2.4) - 0.055;
    }
    return out;
}

fn filter_turbulence_pixel(x: f32, y: f32) -> u32 {
    var result = 0u;
    if (
        abs(config.turbulence_scale_x) > 0.00000011920929 &&
        abs(config.turbulence_scale_y) > 0.00000011920929
    ) {
        let sample_base_x = (x - config.turbulence_transform_x) / config.turbulence_scale_x;
        let sample_base_y = (y - config.turbulence_transform_y) / config.turbulence_scale_y;
        let local_tile_x = x - config.turbulence_tile_x;
        let local_tile_y = y - config.turbulence_tile_y;
        var frequency_x = config.turbulence_base_frequency_x;
        var frequency_y = config.turbulence_base_frequency_y;
        var stitch_width = 0i;
        var stitch_height = 0i;
        var stitch_wrap_x = 0i;
        var stitch_wrap_y = 0i;
        if (config.turbulence_stitch_tiles == 1u) {
            let tw = max(config.turbulence_tile_width, 1.0);
            let th = max(config.turbulence_tile_height, 1.0);
            frequency_x = filter_stitch_frequency(frequency_x, tw);
            frequency_y = filter_stitch_frequency(frequency_y, th);
            stitch_width = i32(tw * frequency_x + 0.5);
            stitch_height = i32(th * frequency_y + 0.5);
            stitch_wrap_x = i32(local_tile_x * frequency_x + 4096.0 + f32(stitch_width));
            stitch_wrap_y = i32(local_tile_y * frequency_y + 4096.0 + f32(stitch_height));
        }

        let selector_offset = config.table_index * TURBULENCE_TABLE_LEN;
        let gradient_offset = config.table_index * TURBULENCE_GRADIENT_LEN;
        var ratio = 1.0;
        var out_r = 0.0;
        var out_g = 0.0;
        var out_b = 0.0;
        var out_a = 0.0;
        var octave = 0u;
        loop {
            if (octave >= config.turbulence_num_octaves) {
                break;
            }
            let sample_x = sample_base_x * frequency_x;
            let sample_y = sample_base_y * frequency_y;
            let r = filter_turbulence_noise2(0u, sample_x, sample_y, stitch_wrap_x, stitch_width, stitch_wrap_y, stitch_height, selector_offset, gradient_offset);
            let g = filter_turbulence_noise2(1u, sample_x, sample_y, stitch_wrap_x, stitch_width, stitch_wrap_y, stitch_height, selector_offset, gradient_offset);
            let b = filter_turbulence_noise2(2u, sample_x, sample_y, stitch_wrap_x, stitch_width, stitch_wrap_y, stitch_height, selector_offset, gradient_offset);
            let a = filter_turbulence_noise2(3u, sample_x, sample_y, stitch_wrap_x, stitch_width, stitch_wrap_y, stitch_height, selector_offset, gradient_offset);
            if (config.turbulence_kind == 0u) {
                out_r += abs(r) * ratio;
                out_g += abs(g) * ratio;
                out_b += abs(b) * ratio;
                out_a += abs(a) * ratio;
            } else {
                out_r += r * ratio;
                out_g += g * ratio;
                out_b += b * ratio;
                out_a += a * ratio;
            }
            frequency_x *= 2.0;
            frequency_y *= 2.0;
            ratio *= 0.5;
            if (config.turbulence_stitch_tiles == 1u) {
                stitch_width *= 2i;
                stitch_height *= 2i;
                stitch_wrap_x = 2i * stitch_wrap_x - 4096i;
                stitch_wrap_y = 2i * stitch_wrap_y - 4096i;
            }
            octave += 1u;
        }

        if (config.turbulence_kind == 1u) {
            out_r = out_r * 0.5 + 0.5;
            out_g = out_g * 0.5 + 0.5;
            out_b = out_b * 0.5 + 0.5;
            out_a = out_a * 0.5 + 0.5;
        }
        out_r = clamp(out_r, 0.0, 1.0);
        out_g = clamp(out_g, 0.0, 1.0);
        out_b = clamp(out_b, 0.0, 1.0);
        out_a = clamp(out_a, 0.0, 1.0);
        if (config.turbulence_linear_rgb == 1u) {
            out_r = filter_linear_rgb_to_srgb(out_r);
            out_g = filter_linear_rgb_to_srgb(out_g);
            out_b = filter_linear_rgb_to_srgb(out_b);
        }
        result = pack_premul_rgba8(out_r * out_a, out_g * out_a, out_b * out_a, out_a);
    }
    return result;
}

fn filter_stitch_frequency(frequency: f32, tile_size: f32) -> f32 {
    var out = 0.0;
    if (frequency > 0.0 && tile_size > 0.0) {
        let low = floor(tile_size * frequency) / tile_size;
        let high = ceil(tile_size * frequency) / tile_size;
        if (low != 0.0 && frequency / low < high / frequency) {
            out = low;
        } else {
            out = high;
        }
    }
    return out;
}

fn filter_turbulence_noise2(
    channel: u32,
    x: f32,
    y: f32,
    stitch_wrap_x: i32,
    stitch_width: i32,
    stitch_wrap_y: i32,
    stitch_height: i32,
    selector_offset: u32,
    gradient_offset: u32,
) -> f32 {
    let tx = x + 4096.0;
    let ty = y + 4096.0;
    var bx0 = i32(floor(tx));
    var bx1 = bx0 + 1i;
    var by0 = i32(floor(ty));
    var by1 = by0 + 1i;
    let rx0 = tx - f32(bx0);
    let rx1 = rx0 - 1.0;
    let ry0 = ty - f32(by0);
    let ry1 = ry0 - 1.0;
    if (config.turbulence_stitch_tiles == 1u) {
        if (bx0 >= stitch_wrap_x) {
            bx0 -= stitch_width;
        }
        if (bx1 >= stitch_wrap_x) {
            bx1 -= stitch_width;
        }
        if (by0 >= stitch_wrap_y) {
            by0 -= stitch_height;
        }
        if (by1 >= stitch_wrap_y) {
            by1 -= stitch_height;
        }
    }
    let ubx0 = u32(bx0 & 255i);
    let ubx1 = u32(bx1 & 255i);
    let uby0 = u32(by0 & 255i);
    let uby1 = u32(by1 & 255i);
    let ix = turbulence_selectors[selector_offset + ubx0];
    let jx = turbulence_selectors[selector_offset + ubx1];
    let b00 = turbulence_selectors[selector_offset + ix + uby0];
    let b10 = turbulence_selectors[selector_offset + jx + uby0];
    let b01 = turbulence_selectors[selector_offset + ix + uby1];
    let b11 = turbulence_selectors[selector_offset + jx + uby1];
    let sx = filter_turbulence_curve(rx0);
    let sy = filter_turbulence_curve(ry0);
    let a = lerp_f32(
        filter_turbulence_gradient_dot(gradient_offset, channel, b00, rx0, ry0),
        filter_turbulence_gradient_dot(gradient_offset, channel, b10, rx1, ry0),
        sx,
    );
    let b = lerp_f32(
        filter_turbulence_gradient_dot(gradient_offset, channel, b01, rx0, ry1),
        filter_turbulence_gradient_dot(gradient_offset, channel, b11, rx1, ry1),
        sx,
    );
    return lerp_f32(a, b, sy);
}

fn filter_turbulence_curve(t: f32) -> f32 {
    return t * t * (3.0 - 2.0 * t);
}

fn filter_turbulence_gradient_dot(
    gradient_offset: u32,
    channel: u32,
    selector: u32,
    x: f32,
    y: f32,
) -> f32 {
    let ix = gradient_offset + (channel * TURBULENCE_TABLE_LEN + selector) * 2u;
    return turbulence_gradients[ix] * x + turbulence_gradients[ix + 1u] * y;
}

fn liquid_glass_pixel(
    base: u32,
    world_x: f32,
    world_y: f32,
    pixel_x: f32,
    pixel_y: f32,
    distance: f32,
    distance_norm: f32,
    surface_height: f32,
) -> u32 {
    let nx = liquid_glass_normal_x(world_x, world_y);
    let ny = liquid_glass_normal_y(world_x, world_y);
    let inside_distance = -distance;
    let edge = liquid_glass_edge(
        inside_distance,
        config.liquid_refraction_thickness,
        config.liquid_refraction_factor,
    );
    var blur_mix = inside_distance / max(config.liquid_refraction_thickness, LIQUID_GLASS_EPSILON);
    if (config.mask_enabled == 1u) {
        blur_mix = 1.0;
    }
    blur_mix = clamp(blur_mix, 0.0, 1.0);

    let normal_len = LIQUID_GLASS_NORMAL_LENGTH_SCALE / surface_height;
    var r = liquid_glass_sample_straight_channel(1u, pixel_x, pixel_y, 0u);
    var g = liquid_glass_sample_straight_channel(1u, pixel_x, pixel_y, 1u);
    var b = liquid_glass_sample_straight_channel(1u, pixel_x, pixel_y, 2u);
    var a = liquid_glass_sample_straight_channel(1u, pixel_x, pixel_y, 3u);

    if (edge <= 0.0) {
        r = lerp_f32(r, config.liquid_tint_r, config.liquid_tint_a * LIQUID_GLASS_TINT_MIX);
        g = lerp_f32(g, config.liquid_tint_g, config.liquid_tint_a * LIQUID_GLASS_TINT_MIX);
        b = lerp_f32(b, config.liquid_tint_b, config.liquid_tint_a * LIQUID_GLASS_TINT_MIX);
        a = lerp_f32(a, 1.0, config.liquid_tint_a * LIQUID_GLASS_TINT_MIX);
    } else {
        let offset_x = -nx * edge * LIQUID_GLASS_REFRACTION_PIXEL_SCALE;
        let offset_y = -ny * edge * LIQUID_GLASS_REFRACTION_PIXEL_SCALE;
        r = liquid_glass_dispersion_channel(pixel_x, pixel_y, offset_x, offset_y, LIQUID_GLASS_CHROMATIC_R, 0u, blur_mix);
        g = liquid_glass_dispersion_channel(pixel_x, pixel_y, offset_x, offset_y, LIQUID_GLASS_CHROMATIC_G, 1u, blur_mix);
        b = liquid_glass_dispersion_channel(pixel_x, pixel_y, offset_x, offset_y, LIQUID_GLASS_CHROMATIC_B, 2u, blur_mix);
        a = liquid_glass_sample_alpha(pixel_x + offset_x, pixel_y + offset_y);
        let blurred_r = r;
        let blurred_g = g;
        let blurred_b = b;
        r = lerp_f32(r, config.liquid_tint_r, config.liquid_tint_a * LIQUID_GLASS_TINT_MIX);
        g = lerp_f32(g, config.liquid_tint_g, config.liquid_tint_a * LIQUID_GLASS_TINT_MIX);
        b = lerp_f32(b, config.liquid_tint_b, config.liquid_tint_a * LIQUID_GLASS_TINT_MIX);
        a = lerp_f32(a, 1.0, config.liquid_tint_a * LIQUID_GLASS_TINT_MIX);

        let fresnel = liquid_glass_fresnel(distance, config.liquid_fresnel_range, config.liquid_fresnel_hardness);
        let fresnel_base_r = lerp_f32(1.0, config.liquid_tint_r, config.liquid_tint_a * LIQUID_GLASS_TINT_BASE_MIX);
        let fresnel_base_g = lerp_f32(1.0, config.liquid_tint_g, config.liquid_tint_a * LIQUID_GLASS_TINT_BASE_MIX);
        let fresnel_base_b = lerp_f32(1.0, config.liquid_tint_b, config.liquid_tint_a * LIQUID_GLASS_TINT_BASE_MIX);
        var fresnel_l = liquid_glass_srgb_to_lch_l(fresnel_base_r, fresnel_base_g, fresnel_base_b);
        let fresnel_c = liquid_glass_srgb_to_lch_c(fresnel_base_r, fresnel_base_g, fresnel_base_b);
        let fresnel_h = liquid_glass_srgb_to_lch_h(fresnel_base_r, fresnel_base_g, fresnel_base_b);
        fresnel_l = clamp(fresnel_l + LIQUID_GLASS_FRESNEL_LIGHTNESS_GAIN * fresnel * config.liquid_fresnel_factor, 0.0, 100.0);
        let fresnel_mix = fresnel * config.liquid_fresnel_factor * LIQUID_GLASS_FRESNEL_MIX_SCALE * normal_len;
        r = lerp_f32(r, liquid_glass_lch_to_srgb_r(fresnel_l, fresnel_c, fresnel_h), fresnel_mix);
        g = lerp_f32(g, liquid_glass_lch_to_srgb_g(fresnel_l, fresnel_c, fresnel_h), fresnel_mix);
        b = lerp_f32(b, liquid_glass_lch_to_srgb_b(fresnel_l, fresnel_c, fresnel_h), fresnel_mix);
        a = lerp_f32(a, 1.0, fresnel_mix);

        let glare_geo = liquid_glass_glare_geometry(distance, config.liquid_glare_range, config.liquid_glare_hardness);
        let glare_angle_factor = liquid_glass_glare_angle(nx, ny);
        let glare_base_r = lerp_f32(blurred_r, config.liquid_tint_r, config.liquid_tint_a * LIQUID_GLASS_TINT_BASE_MIX);
        let glare_base_g = lerp_f32(blurred_g, config.liquid_tint_g, config.liquid_tint_a * LIQUID_GLASS_TINT_BASE_MIX);
        let glare_base_b = lerp_f32(blurred_b, config.liquid_tint_b, config.liquid_tint_a * LIQUID_GLASS_TINT_BASE_MIX);
        var glare_l = liquid_glass_srgb_to_lch_l(glare_base_r, glare_base_g, glare_base_b);
        var glare_c = liquid_glass_srgb_to_lch_c(glare_base_r, glare_base_g, glare_base_b);
        let glare_h = liquid_glass_srgb_to_lch_h(glare_base_r, glare_base_g, glare_base_b);
        glare_l = clamp(glare_l + LIQUID_GLASS_GLARE_LIGHTNESS_GAIN * glare_angle_factor * glare_geo, 0.0, 120.0);
        glare_c += LIQUID_GLASS_GLARE_CHROMA_GAIN * glare_angle_factor * glare_geo;
        let glare_mix = glare_angle_factor * glare_geo * normal_len;
        r = lerp_f32(r, liquid_glass_lch_to_srgb_r(glare_l, glare_c, glare_h), glare_mix);
        g = lerp_f32(g, liquid_glass_lch_to_srgb_g(glare_l, glare_c, glare_h), glare_mix);
        b = lerp_f32(b, liquid_glass_lch_to_srgb_b(glare_l, glare_c, glare_h), glare_mix);
        a = lerp_f32(a, 1.0, glare_mix);
    }

    let edge_mix = liquid_glass_smoothstep(LIQUID_GLASS_EDGE_BLEND_START, LIQUID_GLASS_EDGE_BLEND_END, distance_norm);
    r = lerp_f32(r, liquid_glass_pixel_straight_channel(base, 0u), edge_mix);
    g = lerp_f32(g, liquid_glass_pixel_straight_channel(base, 1u), edge_mix);
    b = lerp_f32(b, liquid_glass_pixel_straight_channel(base, 2u), edge_mix);
    a = lerp_f32(a, liquid_glass_pixel_straight_channel(base, 3u), edge_mix);
    return liquid_glass_pack_straight_rgba8(r, g, b, a);
}

fn liquid_glass_edge(inside_distance: f32, refraction_thickness: f32, refraction_factor: f32) -> f32 {
    let thickness = max(refraction_thickness, LIQUID_GLASS_EPSILON);
    var out = 0.0;
    if (inside_distance < thickness) {
        let ratio = 1.0 - inside_distance / thickness;
        let theta_i = asin(clamp(pow(ratio, 2.0), -1.0, 1.0));
        let theta_t = asin(clamp(sin(theta_i) / max(refraction_factor, 1.0), -1.0, 1.0));
        out = max(-tan(theta_t - theta_i), 0.0);
    }
    return out;
}

fn liquid_glass_fresnel(distance: f32, fresnel_range: f32, fresnel_hardness: f32) -> f32 {
    return clamp(
        pow(
            1.0 + distance / LIQUID_GLASS_GEOMETRY_DISTANCE_SCALE *
            pow(LIQUID_GLASS_GEOMETRY_RANGE_SCALE / max(fresnel_range, LIQUID_GLASS_EPSILON), 2.0) +
            fresnel_hardness,
            5.0,
        ),
        0.0,
        1.0,
    );
}

fn liquid_glass_glare_geometry(distance: f32, glare_range: f32, glare_hardness: f32) -> f32 {
    return clamp(
        pow(
            1.0 + distance / LIQUID_GLASS_GEOMETRY_DISTANCE_SCALE *
            pow(LIQUID_GLASS_GEOMETRY_RANGE_SCALE / max(glare_range, LIQUID_GLASS_EPSILON), 2.0) +
            glare_hardness,
            5.0,
        ),
        0.0,
        1.0,
    );
}

fn liquid_glass_glare_angle(nx: f32, ny: f32) -> f32 {
    let angle = (liquid_glass_vec2_angle(nx, ny) - LIQUID_GLASS_PI * 0.25 + config.liquid_glare_angle) * 2.0;
    var side = LIQUID_GLASS_GLARE_SIDE_SCALE;
    if ((angle > LIQUID_GLASS_PI * 1.5 && angle < LIQUID_GLASS_PI * 3.5) || angle < -LIQUID_GLASS_PI * 0.5) {
        side = LIQUID_GLASS_GLARE_SIDE_SCALE * config.liquid_glare_opposite_factor;
    }
    return clamp(
        pow(
            (0.5 + sin(angle) * 0.5) * side * config.liquid_glare_factor,
            LIQUID_GLASS_GLARE_POWER_BASE + config.liquid_glare_convergence * LIQUID_GLASS_GLARE_POWER_SCALE,
        ),
        0.0,
        1.0,
    );
}

fn liquid_glass_vec2_angle(x: f32, y: f32) -> f32 {
    let len = sqrt(x * x + y * y);
    var angle = 0.0;
    if (len >= 0.00000001) {
        angle = atan2(y, x);
        if (angle < 0.0) {
            angle += 2.0 * LIQUID_GLASS_PI;
        }
    }
    return angle;
}

fn liquid_glass_dispersion_channel(
    x: f32,
    y: f32,
    offset_x: f32,
    offset_y: f32,
    chromatic: f32,
    channel: u32,
    blur_mix: f32,
) -> f32 {
    let factor = 1.0 - (chromatic - 1.0) * config.liquid_refraction_dispersion;
    let sx = x + offset_x * factor;
    let sy = y + offset_y * factor;
    let src = liquid_glass_sample_straight_channel(0u, sx, sy, channel);
    let blur = liquid_glass_sample_straight_channel(1u, sx, sy, channel);
    return lerp_f32(src, blur, blur_mix);
}

fn liquid_glass_sample_alpha(x: f32, y: f32) -> f32 {
    return max(
        liquid_glass_sample_straight_channel(0u, x, y, 3u),
        liquid_glass_sample_straight_channel(1u, x, y, 3u),
    );
}

fn liquid_glass_sample_straight_channel(image_kind: u32, x: f32, y: f32, channel: u32) -> f32 {
    let sx = clamp(x, 0.0, f32(config.width - 1u));
    let sy = clamp(y, 0.0, f32(config.height - 1u));
    let x0 = u32(floor(sx));
    let y0 = u32(floor(sy));
    let x1 = min(x0 + 1u, config.width - 1u);
    let y1 = min(y0 + 1u, config.height - 1u);
    let tx = sx - f32(x0);
    let ty = sy - f32(y0);
    let tl = liquid_glass_pixel_straight_channel(liquid_glass_image_pixel(image_kind, x0, y0), channel);
    let tr = liquid_glass_pixel_straight_channel(liquid_glass_image_pixel(image_kind, x1, y0), channel);
    let bl = liquid_glass_pixel_straight_channel(liquid_glass_image_pixel(image_kind, x0, y1), channel);
    let br = liquid_glass_pixel_straight_channel(liquid_glass_image_pixel(image_kind, x1, y1), channel);
    return lerp_f32(lerp_f32(tl, tr, tx), lerp_f32(bl, br, tx), ty);
}

fn liquid_glass_image_pixel(image_kind: u32, x: u32, y: u32) -> u32 {
    if (image_kind == 1u) {
        return aux_pixel_at(x, y);
    }
    return source_pixel_at(x, y);
}

fn liquid_glass_pixel_straight_channel(px: u32, channel: u32) -> f32 {
    let a = f32((px >> 24u) & 255u) / 255.0;
    var value = px & 255u;
    if (channel == 1u) {
        value = (px >> 8u) & 255u;
    } else if (channel == 2u) {
        value = (px >> 16u) & 255u;
    } else if (channel == 3u) {
        value = (px >> 24u) & 255u;
    }
    var out = f32(value) / 255.0;
    if (channel != 3u && a > LIQUID_GLASS_EPSILON) {
        out = out / a;
    }
    return out;
}

fn liquid_glass_pack_straight_rgba8(r: f32, g: f32, b: f32, a: f32) -> u32 {
    let alpha = clamp(a, 0.0, 1.0);
    return pack_premul_rgba8(
        clamp(r, 0.0, 1.0) * alpha,
        clamp(g, 0.0, 1.0) * alpha,
        clamp(b, 0.0, 1.0) * alpha,
        alpha,
    );
}

fn liquid_glass_smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = clamp((x - edge0) / (edge1 - edge0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

fn liquid_glass_normal_x(x: f32, y: f32) -> f32 {
    let eps = 1.0;
    let dx = liquid_glass_round_rect_distance(x + eps, y, config.rect_x0, config.rect_y0, config.rect_x1, config.rect_y1, config.radius_top_left, config.radius_top_right, config.radius_bottom_left, config.radius_bottom_right) -
        liquid_glass_round_rect_distance(x - eps, y, config.rect_x0, config.rect_y0, config.rect_x1, config.rect_y1, config.radius_top_left, config.radius_top_right, config.radius_bottom_left, config.radius_bottom_right);
    let dy = liquid_glass_round_rect_distance(x, y + eps, config.rect_x0, config.rect_y0, config.rect_x1, config.rect_y1, config.radius_top_left, config.radius_top_right, config.radius_bottom_left, config.radius_bottom_right) -
        liquid_glass_round_rect_distance(x, y - eps, config.rect_x0, config.rect_y0, config.rect_x1, config.rect_y1, config.radius_top_left, config.radius_top_right, config.radius_bottom_left, config.radius_bottom_right);
    let len = sqrt(dx * dx + dy * dy);
    var out = 0.0;
    if (len > LIQUID_GLASS_EPSILON) {
        out = dx / len;
    }
    return out;
}

fn liquid_glass_normal_y(x: f32, y: f32) -> f32 {
    let eps = 1.0;
    let dx = liquid_glass_round_rect_distance(x + eps, y, config.rect_x0, config.rect_y0, config.rect_x1, config.rect_y1, config.radius_top_left, config.radius_top_right, config.radius_bottom_left, config.radius_bottom_right) -
        liquid_glass_round_rect_distance(x - eps, y, config.rect_x0, config.rect_y0, config.rect_x1, config.rect_y1, config.radius_top_left, config.radius_top_right, config.radius_bottom_left, config.radius_bottom_right);
    let dy = liquid_glass_round_rect_distance(x, y + eps, config.rect_x0, config.rect_y0, config.rect_x1, config.rect_y1, config.radius_top_left, config.radius_top_right, config.radius_bottom_left, config.radius_bottom_right) -
        liquid_glass_round_rect_distance(x, y - eps, config.rect_x0, config.rect_y0, config.rect_x1, config.rect_y1, config.radius_top_left, config.radius_top_right, config.radius_bottom_left, config.radius_bottom_right);
    let len = sqrt(dx * dx + dy * dy);
    var out = -1.0;
    if (len > LIQUID_GLASS_EPSILON) {
        out = dy / len;
    }
    return out;
}

fn liquid_glass_round_rect_distance(
    x: f32,
    y: f32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    radius_top_left: f32,
    radius_top_right: f32,
    radius_bottom_left: f32,
    radius_bottom_right: f32,
) -> f32 {
    let cx = (x0 + x1) * 0.5;
    let cy = (y0 + y1) * 0.5;
    let hx = max((x1 - x0) * 0.5, 0.0);
    let hy = max((y1 - y0) * 0.5, 0.0);
    let px = x - cx;
    let py = y - cy;
    let radius = max(min(min(liquid_glass_corner_radius(px, py, radius_top_left, radius_top_right, radius_bottom_left, radius_bottom_right), hx), hy), 0.0);
    let ax = abs(px);
    let ay = abs(py);
    let dx = ax - hx;
    let dy = ay - hy;
    var out = sqrt(max(dx, 0.0) * max(dx, 0.0) + max(dy, 0.0) * max(dy, 0.0)) + min(max(dx, dy), 0.0);
    if (radius > 0.0) {
        let qx = ax - hx + radius;
        let qy = ay - hy + radius;
        out = min(max(qx, qy), 0.0) + sqrt(max(qx, 0.0) * max(qx, 0.0) + max(qy, 0.0) * max(qy, 0.0)) - radius;
    }
    return out;
}

fn liquid_glass_corner_radius(
    px: f32,
    py: f32,
    radius_top_left: f32,
    radius_top_right: f32,
    radius_bottom_left: f32,
    radius_bottom_right: f32,
) -> f32 {
    var radius = radius_top_left;
    if (px >= 0.0) {
        if (py <= 0.0) {
            radius = radius_top_right;
        } else {
            radius = radius_bottom_right;
        }
    } else if (py > 0.0) {
        radius = radius_bottom_left;
    }
    return radius;
}

fn liquid_glass_srgb_to_lch_l(r: f32, g: f32, b: f32) -> f32 {
    let y = liquid_glass_xyz_to_lab_f(liquid_glass_srgb_to_xyz_y(r, g, b) / LIQUID_GLASS_D65_Y);
    return 116.0 * y - 16.0;
}

fn liquid_glass_srgb_to_lch_c(r: f32, g: f32, b: f32) -> f32 {
    let lab_a = liquid_glass_srgb_to_lab_a(r, g, b);
    let lab_b = liquid_glass_srgb_to_lab_b(r, g, b);
    return sqrt(lab_a * lab_a + lab_b * lab_b);
}

fn liquid_glass_srgb_to_lch_h(r: f32, g: f32, b: f32) -> f32 {
    return atan2(liquid_glass_srgb_to_lab_b(r, g, b), liquid_glass_srgb_to_lab_a(r, g, b)) * 57.29578;
}

fn liquid_glass_srgb_to_lab_a(r: f32, g: f32, b: f32) -> f32 {
    let x = liquid_glass_xyz_to_lab_f(liquid_glass_srgb_to_xyz_x(r, g, b) / LIQUID_GLASS_D65_X);
    let y = liquid_glass_xyz_to_lab_f(liquid_glass_srgb_to_xyz_y(r, g, b) / LIQUID_GLASS_D65_Y);
    return 500.0 * (x - y);
}

fn liquid_glass_srgb_to_lab_b(r: f32, g: f32, b: f32) -> f32 {
    let y = liquid_glass_xyz_to_lab_f(liquid_glass_srgb_to_xyz_y(r, g, b) / LIQUID_GLASS_D65_Y);
    let z = liquid_glass_xyz_to_lab_f(liquid_glass_srgb_to_xyz_z(r, g, b) / LIQUID_GLASS_D65_Z);
    return 200.0 * (y - z);
}

fn liquid_glass_srgb_to_xyz_x(r: f32, g: f32, b: f32) -> f32 {
    return liquid_glass_uncompand_srgb(r) * 0.4124 + liquid_glass_uncompand_srgb(g) * 0.3576 + liquid_glass_uncompand_srgb(b) * 0.1805;
}

fn liquid_glass_srgb_to_xyz_y(r: f32, g: f32, b: f32) -> f32 {
    return liquid_glass_uncompand_srgb(r) * 0.2126 + liquid_glass_uncompand_srgb(g) * 0.7152 + liquid_glass_uncompand_srgb(b) * 0.0722;
}

fn liquid_glass_srgb_to_xyz_z(r: f32, g: f32, b: f32) -> f32 {
    return liquid_glass_uncompand_srgb(r) * 0.0193 + liquid_glass_uncompand_srgb(g) * 0.1192 + liquid_glass_uncompand_srgb(b) * 0.9505;
}

fn liquid_glass_lch_to_srgb_r(l: f32, c: f32, h: f32) -> f32 {
    let x = liquid_glass_lch_to_xyz_x(l, c, h);
    let y = liquid_glass_lch_to_xyz_y(l);
    let z = liquid_glass_lch_to_xyz_z(l, c, h);
    return liquid_glass_compand_rgb(x * 3.2406255 + y * -1.537208 + z * -0.4986286);
}

fn liquid_glass_lch_to_srgb_g(l: f32, c: f32, h: f32) -> f32 {
    let x = liquid_glass_lch_to_xyz_x(l, c, h);
    let y = liquid_glass_lch_to_xyz_y(l);
    let z = liquid_glass_lch_to_xyz_z(l, c, h);
    return liquid_glass_compand_rgb(x * -0.9689307 + y * 1.8757561 + z * 0.0415175);
}

fn liquid_glass_lch_to_srgb_b(l: f32, c: f32, h: f32) -> f32 {
    let x = liquid_glass_lch_to_xyz_x(l, c, h);
    let y = liquid_glass_lch_to_xyz_y(l);
    let z = liquid_glass_lch_to_xyz_z(l, c, h);
    return liquid_glass_compand_rgb(x * 0.0557101 + y * -0.2040211 + z * 1.0569959);
}

fn liquid_glass_lch_to_xyz_x(l: f32, c: f32, h: f32) -> f32 {
    let hue = h * 0.017453292;
    let lab_a = c * cos(hue);
    let w = (l + 16.0) / 116.0;
    return LIQUID_GLASS_D65_X * liquid_glass_lab_to_xyz_f(w + lab_a / 500.0);
}

fn liquid_glass_lch_to_xyz_y(l: f32) -> f32 {
    let w = (l + 16.0) / 116.0;
    return LIQUID_GLASS_D65_Y * liquid_glass_lab_to_xyz_f(w);
}

fn liquid_glass_lch_to_xyz_z(l: f32, c: f32, h: f32) -> f32 {
    let hue = h * 0.017453292;
    let lab_b = c * sin(hue);
    let w = (l + 16.0) / 116.0;
    return LIQUID_GLASS_D65_Z * liquid_glass_lab_to_xyz_f(w - lab_b / 200.0);
}

fn liquid_glass_xyz_to_lab_f(x: f32) -> f32 {
    var out = 7.787037 * x + 0.13793103;
    if (x > 0.008856452) {
        out = pow(x, 0.33333334);
    }
    return out;
}

fn liquid_glass_lab_to_xyz_f(x: f32) -> f32 {
    var out = 0.12841855 * (x - 0.13793103);
    if (x > 0.206897) {
        out = x * x * x;
    }
    return out;
}

fn liquid_glass_uncompand_srgb(a: f32) -> f32 {
    var out = a / 12.92;
    if (a > 0.04045) {
        out = pow((a + 0.055) / 1.055, 2.4);
    }
    return out;
}

fn liquid_glass_compand_rgb(a: f32) -> f32 {
    var out = 12.92 * a;
    if (a > 0.0031308) {
        out = 1.055 * pow(a, 0.41666666) - 0.055;
    }
    return out;
}

fn alpha_gradient_x(x: u32, y: u32) -> f32 {
    var out = 0.0;
    if (config.region_width >= 2u) {
        let weighted_diff = alpha_gradient_x_sample(x, y, -1, 1.0) +
            alpha_gradient_x_sample(x, y, 0, 2.0) +
            alpha_gradient_x_sample(x, y, 1, 1.0);
        let weight_sum = gradient_sample_weight(y, config.region_y0, config.region_height, -1, 1.0) +
            gradient_sample_weight(y, config.region_y0, config.region_height, 0, 2.0) +
            gradient_sample_weight(y, config.region_y0, config.region_height, 1, 1.0);
        let one_sided = x == config.region_x0 || x == config.region_x0 + config.region_width - 1u;
        var edge_scale = 1.0;
        if (one_sided) {
            edge_scale = 2.0;
        }
        out = weighted_diff * edge_scale / max(weight_sum, 0.000001);
    }
    return out;
}

fn alpha_gradient_y(x: u32, y: u32) -> f32 {
    var out = 0.0;
    if (config.region_height >= 2u) {
        let weighted_diff = alpha_gradient_y_sample(x, y, -1, 1.0) +
            alpha_gradient_y_sample(x, y, 0, 2.0) +
            alpha_gradient_y_sample(x, y, 1, 1.0);
        let weight_sum = gradient_sample_weight(x, config.region_x0, config.region_width, -1, 1.0) +
            gradient_sample_weight(x, config.region_x0, config.region_width, 0, 2.0) +
            gradient_sample_weight(x, config.region_x0, config.region_width, 1, 1.0);
        let one_sided = y == config.region_y0 || y == config.region_y0 + config.region_height - 1u;
        var edge_scale = 1.0;
        if (one_sided) {
            edge_scale = 2.0;
        }
        out = weighted_diff * edge_scale / max(weight_sum, 0.000001);
    }
    return out;
}

fn gradient_sample_weight(pos: u32, start: u32, len: u32, offset: i32, weight: f32) -> f32 {
    let sample = i32(pos) + offset;
    let end = start + len;
    var out = 0.0;
    if (sample >= i32(start) && sample < i32(end)) {
        out = weight;
    }
    return out;
}

fn alpha_gradient_x_sample(x: u32, y: u32, offset: i32, weight: f32) -> f32 {
    let sy = i32(y) + offset;
    let region_x1 = config.region_x0 + config.region_width - 1u;
    let region_y1 = config.region_y0 + config.region_height;
    var out = 0.0;
    if (sy >= i32(config.region_y0) && sy < i32(region_y1)) {
        let syu = u32(sy);
        var left = x;
        if (x > config.region_x0) {
            left = x - 1u;
        }
        var right = x;
        if (x < region_x1) {
            right = x + 1u;
        }
        let center = source_alpha_at(x, syu);
        var diff = source_alpha_at(right, syu) - source_alpha_at(left, syu);
        if (x == config.region_x0) {
            diff = source_alpha_at(right, syu) - center;
        } else if (x == region_x1) {
            diff = center - source_alpha_at(left, syu);
        }
        out = weight * diff;
    }
    return out;
}

fn alpha_gradient_y_sample(x: u32, y: u32, offset: i32, weight: f32) -> f32 {
    let sx = i32(x) + offset;
    let region_x1 = config.region_x0 + config.region_width;
    let region_y1 = config.region_y0 + config.region_height - 1u;
    var out = 0.0;
    if (sx >= i32(config.region_x0) && sx < i32(region_x1)) {
        let sxu = u32(sx);
        var top = y;
        if (y > config.region_y0) {
            top = y - 1u;
        }
        var bottom = y;
        if (y < region_y1) {
            bottom = y + 1u;
        }
        let center = source_alpha_at(sxu, y);
        var diff = source_alpha_at(sxu, bottom) - source_alpha_at(sxu, top);
        if (y == config.region_y0) {
            diff = source_alpha_at(sxu, bottom) - center;
        } else if (y == region_y1) {
            diff = center - source_alpha_at(sxu, top);
        }
        out = weight * diff;
    }
    return out;
}

fn scale_premul_u8(src: u32, factor: u32) -> u32 {
    if (factor == 0u) {
        return 0u;
    }
    if (factor == 255u) {
        return src;
    }
    return rgba8_pack(
        mul_div255(src & 255u, factor),
        mul_div255((src >> 8u) & 255u, factor),
        mul_div255((src >> 16u) & 255u, factor),
        mul_div255((src >> 24u) & 255u, factor),
    );
}

fn src_over_premul_u8(dst: u32, src: u32) -> u32 {
    let sa = (src >> 24u) & 255u;
    if (sa == 0u) {
        return dst;
    }
    if (sa == 255u) {
        return src;
    }
    let inv = 255u - sa;
    return rgba8_pack(
        (src & 255u) + mul_div255(dst & 255u, inv),
        ((src >> 8u) & 255u) + mul_div255((dst >> 8u) & 255u, inv),
        ((src >> 16u) & 255u) + mul_div255((dst >> 16u) & 255u, inv),
        sa + mul_div255((dst >> 24u) & 255u, inv),
    );
}

fn sample_brush(brush_index: u32, x: f32, y: f32) -> u32 {
    let data_base = brush_index * GPU_BRUSH_U32_STRIDE;
    let kind = brush_data[data_base];
    let extend = brush_data[data_base + 1u];
    let payload_offset = brush_data[data_base + 2u];
    let payload_len = brush_data[data_base + 3u];
    let base = brush_index * GPU_BRUSH_PARAM_STRIDE;
    var color = brush_data[data_base + 4u];

    if (kind == GPU_BRUSH_LINEAR) {
        let tx = brush_params[base + 4u] * x + brush_params[base + 6u] * y + brush_params[base + 8u];
        let ty = brush_params[base + 5u] * x + brush_params[base + 7u] * y + brush_params[base + 9u];
        let sx = brush_params[base];
        let sy = brush_params[base + 1u];
        let ex = brush_params[base + 2u];
        let ey = brush_params[base + 3u];
        let dx = ex - sx;
        let dy = ey - sy;
        let denominator = dx * dx + dy * dy;
        var t = 0.0;
        if (denominator > 0.00000011920929) {
            t = ((tx - sx) * dx + (ty - sy) * dy) / denominator;
        }
        color = sample_ramp(payload_offset, payload_len, t, extend);
    } else if (kind == GPU_BRUSH_RADIAL) {
        color = sample_radial(x, y, base, extend, payload_offset, payload_len);
    } else if (kind == GPU_BRUSH_SWEEP) {
        let cx = brush_params[base];
        let cy = brush_params[base + 1u];
        let start_angle = brush_params[base + 2u];
        let end_angle = brush_params[base + 3u];
        let span = end_angle - start_angle;
        var t = 0.0;
        if (abs(span) > 0.00000011920929) {
            let tau = 6.2831855;
            var angle = atan2(y - cy, x - cx);
            if (span > 0.0) {
                while (angle < start_angle) {
                    angle = angle + tau;
                }
            } else {
                while (angle > start_angle) {
                    angle = angle - tau;
                }
            }
            t = (angle - start_angle) / span;
        }
        color = sample_ramp(payload_offset, payload_len, t, extend);
    } else if (kind == GPU_BRUSH_FOUR_CORNER) {
        color = sample_four_corner(x, y, base, payload_offset);
    } else if (kind == GPU_BRUSH_PATTERN) {
        color = sample_pattern(
            x,
            y,
            base,
            payload_offset,
            payload_len,
            brush_data[data_base + 5u],
            brush_data[data_base + 6u],
            brush_data[data_base + 7u],
            extend,
            brush_data[data_base + 8u],
        );
    }

    return color;
}

fn sample_radial(x: f32, y: f32, base: u32, extend: u32, payload_offset: u32, payload_len: u32) -> u32 {
    let tx = brush_params[base + 6u] * x + brush_params[base + 8u] * y + brush_params[base + 10u];
    let ty = brush_params[base + 7u] * x + brush_params[base + 9u] * y + brush_params[base + 11u];
    let sx = brush_params[base];
    let sy = brush_params[base + 1u];
    let ex = brush_params[base + 2u];
    let ey = brush_params[base + 3u];
    let start_radius = brush_params[base + 4u];
    let end_radius = brush_params[base + 5u];
    let qx = tx - sx;
    let qy = ty - sy;
    let dcx = ex - sx;
    let dcy = ey - sy;
    let dr = end_radius - start_radius;
    let a = dcx * dcx + dcy * dcy - dr * dr;
    let b = -2.0 * (qx * dcx + qy * dcy + start_radius * dr);
    let c = qx * qx + qy * qy - start_radius * start_radius;
    var has_t = false;
    var t = 0.0;

    if (abs(a) <= 0.000001) {
        if (abs(b) > 0.000001) {
            let candidate = -c / b;
            if (start_radius + candidate * dr >= 0.0) {
                has_t = true;
                t = candidate;
            }
        }
    } else {
        let discriminant = b * b - 4.0 * a * c;
        if (discriminant >= 0.0) {
            let root = sqrt(discriminant);
            let t0 = (-b - root) / (2.0 * a);
            let t1 = (-b + root) / (2.0 * a);
            let valid0 = start_radius + t0 * dr >= 0.0;
            let valid1 = start_radius + t1 * dr >= 0.0;
            if (valid0) {
                has_t = true;
                if (valid1) {
                    t = max(t0, t1);
                } else {
                    t = t0;
                }
            } else if (valid1) {
                has_t = true;
                t = t1;
            }
        }
    }

    var color = 0u;
    if (has_t) {
        color = sample_ramp(payload_offset, payload_len, t, extend);
    }
    return color;
}

fn sample_four_corner(x: f32, y: f32, base: u32, payload_offset: u32) -> u32 {
    let x0 = brush_params[base];
    let y0 = brush_params[base + 1u];
    let x1 = brush_params[base + 2u];
    let y1 = brush_params[base + 3u];
    let width = x1 - x0;
    let height = y1 - y0;
    var u = 0.0;
    var v = 0.0;
    if (abs(width) > 0.00000011920929) {
        u = clamp((x - x0) / width, 0.0, 1.0);
    }
    if (abs(height) > 0.00000011920929) {
        v = clamp((y - y0) / height, 0.0, 1.0);
    }
    let tl = brush_payloads[payload_offset];
    let tr = brush_payloads[payload_offset + 1u];
    let br = brush_payloads[payload_offset + 2u];
    let bl = brush_payloads[payload_offset + 3u];
    let top = lerp_premul_u8(tl, tr, u);
    let bottom = lerp_premul_u8(bl, br, u);
    return lerp_premul_u8(top, bottom, v);
}

fn sample_pattern(
    x: f32,
    y: f32,
    base: u32,
    payload_offset: u32,
    payload_len: u32,
    width: u32,
    height: u32,
    opacity: u32,
    extend: u32,
    sampling: u32,
) -> u32 {
    var color = 0u;
    if (payload_len > 0u && width > 0u && height > 0u) {
        let tx = brush_params[base] * x + brush_params[base + 2u] * y + brush_params[base + 4u];
        let ty = brush_params[base + 1u] * x + brush_params[base + 3u] * y + brush_params[base + 5u];
        if (sampling == GPU_PATTERN_BILINEAR) {
            let sx = tx - 0.5;
            let sy = ty - 0.5;
            let x0f = floor(sx);
            let y0f = floor(sy);
            let fx = sx - x0f;
            let fy = sy - y0f;
            let x0 = i32(x0f);
            let y0 = i32(y0f);
            let tl = pattern_pixel(payload_offset, payload_len, width, height, extend, x0, y0);
            let tr = pattern_pixel(payload_offset, payload_len, width, height, extend, x0 + 1, y0);
            let bl = pattern_pixel(payload_offset, payload_len, width, height, extend, x0, y0 + 1);
            let br = pattern_pixel(payload_offset, payload_len, width, height, extend, x0 + 1, y0 + 1);
            color = lerp_premul_u8(lerp_premul_u8(tl, tr, fx), lerp_premul_u8(bl, br, fx), fy);
        } else {
            color = pattern_pixel(payload_offset, payload_len, width, height, extend, i32(floor(tx)), i32(floor(ty)));
        }
        color = scale_premul_u8(color, opacity);
    }
    return color;
}

fn pattern_pixel(payload_offset: u32, payload_len: u32, width: u32, height: u32, extend: u32, x: i32, y: i32) -> u32 {
    let local_x = extend_coord_i32(x, width, extend);
    let local_y = extend_coord_i32(y, height, extend);
    let local_ix = min(local_y * width + local_x, payload_len - 1u);
    return brush_payloads[payload_offset + local_ix];
}

fn sample_ramp(payload_offset: u32, payload_len: u32, t: f32, extend: u32) -> u32 {
    var color = 0u;
    if (payload_len > 0u) {
        let last = payload_len - 1u;
        let position = apply_extend(t, extend) * f32(last);
        let left_ix = u32(floor(position));
        let right_ix = min(left_ix + 1u, last);
        let frac = position - f32(left_ix);
        let left = brush_payloads[payload_offset + left_ix];
        let right = brush_payloads[payload_offset + right_ix];
        if (frac <= 0.00000011920929 || left_ix == right_ix) {
            color = left;
        } else {
            color = lerp_premul_u8(left, right, frac);
        }
    }
    return color;
}

fn apply_extend(t: f32, extend: u32) -> f32 {
    var out = clamp(t, 0.0, 1.0);
    if (extend == GPU_EXTEND_REPEAT) {
        out = rem_euclid_f32(t, 1.0);
    } else if (extend == GPU_EXTEND_REFLECT) {
        let value = rem_euclid_f32(t, 2.0);
        if (value <= 1.0) {
            out = value;
        } else {
            out = 2.0 - value;
        }
    }
    return out;
}

fn repeat_coord_i32(value: i32, size: u32) -> u32 {
    let size_i = i32(size);
    var out = value % size_i;
    if (out < 0) {
        out = out + size_i;
    }
    return u32(out);
}

fn extend_coord_i32(value: i32, size: u32, extend: u32) -> u32 {
    let max_coord = i32(size) - 1;
    var clamped = value;
    if (clamped < 0) {
        clamped = 0;
    }
    if (clamped > max_coord) {
        clamped = max_coord;
    }
    var out = u32(clamped);
    if (extend == GPU_EXTEND_REPEAT) {
        out = repeat_coord_i32(value, size);
    } else if (extend == GPU_EXTEND_REFLECT) {
        out = reflect_coord_i32(value, size);
    }
    return out;
}

fn reflect_coord_i32(value: i32, size: u32) -> u32 {
    var out = 0u;
    if (size > 1u) {
        let size_i = i32(size);
        let period = size_i * 2;
        var coord = value % period;
        if (coord < 0) {
            coord = coord + period;
        }
        if (coord < size_i) {
            out = u32(coord);
        } else {
            out = u32(period - coord - 1);
        }
    }
    return out;
}

fn lerp_premul_u8(a: u32, b: u32, t: f32) -> u32 {
    let inv = 1.0 / 255.0;
    let ar = f32(a & 255u) * inv;
    let ag = f32((a >> 8u) & 255u) * inv;
    let ab = f32((a >> 16u) & 255u) * inv;
    let aa = f32((a >> 24u) & 255u) * inv;
    let br = f32(b & 255u) * inv;
    let bg = f32((b >> 8u) & 255u) * inv;
    let bb = f32((b >> 16u) & 255u) * inv;
    let ba = f32((b >> 24u) & 255u) * inv;
    return rgba8_pack(
        u32(clamp(ar + (br - ar) * t, 0.0, 1.0) * 255.0 + 0.5),
        u32(clamp(ag + (bg - ag) * t, 0.0, 1.0) * 255.0 + 0.5),
        u32(clamp(ab + (bb - ab) * t, 0.0, 1.0) * 255.0 + 0.5),
        u32(clamp(aa + (ba - aa) * t, 0.0, 1.0) * 255.0 + 0.5),
    );
}

fn blend_premul_u8(dst: u32, src: u32, mode: u32) -> u32 {
    let mix = mode & 255u;
    let compose = (mode >> 8u) & 255u;
    let inv = 1.0 / 255.0;
    let sr = f32(src & 255u) * inv;
    let sg = f32((src >> 8u) & 255u) * inv;
    let sb = f32((src >> 16u) & 255u) * inv;
    let sa = f32((src >> 24u) & 255u) * inv;
    let dr = f32(dst & 255u) * inv;
    let dg = f32((dst >> 8u) & 255u) * inv;
    let db = f32((dst >> 16u) & 255u) * inv;
    let da = f32((dst >> 24u) & 255u) * inv;

    var out_r = sr + dr * (1.0 - sa);
    var out_g = sg + dg * (1.0 - sa);
    var out_b = sb + db * (1.0 - sa);
    var out_a = sa + da * (1.0 - sa);

    if (mix == 0u && compose == 3u) {
    } else if (mix == 0u && compose == 2u) {
        out_r = dr;
        out_g = dg;
        out_b = db;
        out_a = da;
    } else if (mix == 0u && compose == 0u) {
        out_r = 0.0;
        out_g = 0.0;
        out_b = 0.0;
        out_a = 0.0;
    } else if (mix == 0u && compose == 1u) {
        out_r = sr;
        out_g = sg;
        out_b = sb;
        out_a = sa;
    } else if (mix == 0u) {
        let src_factor = compose_src_factor(compose, sa, da);
        let dst_factor = compose_dst_factor(compose, sa, da);
        out_r = sr * src_factor + dr * dst_factor;
        out_g = sg * src_factor + dg * dst_factor;
        out_b = sb * src_factor + db * dst_factor;
        out_a = sa * src_factor + da * dst_factor;
        if (compose == 13u) {
            out_r = min(out_r, 1.0);
            out_g = min(out_g, 1.0);
            out_b = min(out_b, 1.0);
            out_a = min(out_a, 1.0);
        }
    } else if (compose == 3u && mix == 6u) {
        out_r = color_dodge_premul(sr, dr, sa, da);
        out_g = color_dodge_premul(sg, dg, sa, da);
        out_b = color_dodge_premul(sb, db, sa, da);
        out_a = sa + da * (1.0 - sa);
    } else if (compose == 3u && mix == 7u) {
        out_r = color_burn_premul(sr, dr, sa, da);
        out_g = color_burn_premul(sg, dg, sa, da);
        out_b = color_burn_premul(sb, db, sa, da);
        out_a = sa + da * (1.0 - sa);
    } else {
        let src_alpha = clamp(sa, 0.0, 1.0);
        let dst_alpha = clamp(da, 0.0, 1.0);
        let src_r = unpremul_channel(sr, src_alpha);
        let src_g = unpremul_channel(sg, src_alpha);
        let src_b = unpremul_channel(sb, src_alpha);
        let dst_r = unpremul_channel(dr, dst_alpha);
        let dst_g = unpremul_channel(dg, dst_alpha);
        let dst_b = unpremul_channel(db, dst_alpha);
        let mixed_r = mix_rgb_channel(dst_r, dst_g, dst_b, src_r, src_g, src_b, mix, 0u);
        let mixed_g = mix_rgb_channel(dst_r, dst_g, dst_b, src_r, src_g, src_b, mix, 1u);
        let mixed_b = mix_rgb_channel(dst_r, dst_g, dst_b, src_r, src_g, src_b, mix, 2u);
        let effective_r = src_alpha * ((1.0 - dst_alpha) * src_r + dst_alpha * mixed_r);
        let effective_g = src_alpha * ((1.0 - dst_alpha) * src_g + dst_alpha * mixed_g);
        let effective_b = src_alpha * ((1.0 - dst_alpha) * src_b + dst_alpha * mixed_b);
        let src_factor = compose_src_factor(compose, src_alpha, dst_alpha);
        let dst_factor = compose_dst_factor(compose, src_alpha, dst_alpha);
        out_r = effective_r * src_factor + dr * dst_factor;
        out_g = effective_g * src_factor + dg * dst_factor;
        out_b = effective_b * src_factor + db * dst_factor;
        out_a = src_alpha * src_factor + da * dst_factor;
    }

    return pack_premul_rgba8(out_r, out_g, out_b, out_a);
}

fn composite_inputs_pixel(input1: u32, input2: u32, composite_operator: u32, k1: f32, k2: f32, k3: f32, k4: f32) -> u32 {
    var out = blend_premul_u8(input2, input1, 3u << 8u);
    if (composite_operator == 1u) {
        out = blend_premul_u8(input2, input1, 5u << 8u);
    } else if (composite_operator == 2u) {
        out = blend_premul_u8(input2, input1, 7u << 8u);
    } else if (composite_operator == 3u) {
        out = blend_premul_u8(input2, input1, 9u << 8u);
    } else if (composite_operator == 4u) {
        out = blend_premul_u8(input2, input1, 11u << 8u);
    } else if (composite_operator == 5u) {
        out = arithmetic_composite_pixel(input1, input2, k1, k2, k3, k4);
    }
    return out;
}

fn arithmetic_composite_pixel(input1: u32, input2: u32, k1: f32, k2: f32, k3: f32, k4: f32) -> u32 {
    let inv = 1.0 / 255.0;
    let a_r = f32(input1 & 255u) * inv;
    let a_g = f32((input1 >> 8u) & 255u) * inv;
    let a_b = f32((input1 >> 16u) & 255u) * inv;
    let a_a = f32((input1 >> 24u) & 255u) * inv;
    let b_r = f32(input2 & 255u) * inv;
    let b_g = f32((input2 >> 8u) & 255u) * inv;
    let b_b = f32((input2 >> 16u) & 255u) * inv;
    let b_a = f32((input2 >> 24u) & 255u) * inv;
    return pack_premul_rgba8(
        arithmetic_channel(a_r, b_r, k1, k2, k3, k4),
        arithmetic_channel(a_g, b_g, k1, k2, k3, k4),
        arithmetic_channel(a_b, b_b, k1, k2, k3, k4),
        arithmetic_channel(a_a, b_a, k1, k2, k3, k4),
    );
}

fn arithmetic_channel(a: f32, b: f32, k1: f32, k2: f32, k3: f32, k4: f32) -> f32 {
    return clamp(k1 * a * b + k2 * a + k3 * b + k4, 0.0, 1.0);
}

fn compose_src_factor(compose: u32, src_alpha: f32, dst_alpha: f32) -> f32 {
    _ = src_alpha;
    var factor = 1.0;
    if (compose == 0u || compose == 2u || compose == 6u || compose == 8u) {
        factor = 0.0;
    } else if (compose == 4u) {
        factor = 1.0 - dst_alpha;
    } else if (compose == 5u || compose == 9u) {
        factor = dst_alpha;
    } else if (compose == 7u || compose == 10u || compose == 11u) {
        factor = 1.0 - dst_alpha;
    }
    return factor;
}

fn compose_dst_factor(compose: u32, src_alpha: f32, dst_alpha: f32) -> f32 {
    _ = dst_alpha;
    var factor = 1.0 - src_alpha;
    if (compose == 0u || compose == 1u || compose == 5u || compose == 7u) {
        factor = 0.0;
    } else if (compose == 2u || compose == 4u) {
        factor = 1.0;
    } else if (compose == 6u || compose == 10u) {
        factor = src_alpha;
    } else if (compose == 8u || compose == 9u || compose == 11u) {
        factor = 1.0 - src_alpha;
    } else if (compose == 12u || compose == 13u) {
        factor = 1.0;
    }
    return factor;
}

fn unpremul_channel(value: f32, alpha: f32) -> f32 {
    var out = 0.0;
    if (alpha > 0.0) {
        out = value / alpha;
    }
    return out;
}

fn mix_rgb_channel(
    dst_r: f32,
    dst_g: f32,
    dst_b: f32,
    src_r: f32,
    src_g: f32,
    src_b: f32,
    mix: u32,
    channel: u32,
) -> f32 {
    var r = src_r;
    var g = src_g;
    var b = src_b;
    if (mix == 1u) {
        r = dst_r * src_r;
        g = dst_g * src_g;
        b = dst_b * src_b;
    } else if (mix == 2u) {
        r = dst_r + src_r - dst_r * src_r;
        g = dst_g + src_g - dst_g * src_g;
        b = dst_b + src_b - dst_b * src_b;
    } else if (mix == 3u) {
        r = overlay(dst_r, src_r);
        g = overlay(dst_g, src_g);
        b = overlay(dst_b, src_b);
    } else if (mix == 4u) {
        r = min(dst_r, src_r);
        g = min(dst_g, src_g);
        b = min(dst_b, src_b);
    } else if (mix == 5u) {
        r = max(dst_r, src_r);
        g = max(dst_g, src_g);
        b = max(dst_b, src_b);
    } else if (mix == 6u) {
        r = color_dodge(dst_r, src_r);
        g = color_dodge(dst_g, src_g);
        b = color_dodge(dst_b, src_b);
    } else if (mix == 7u) {
        r = color_burn(dst_r, src_r);
        g = color_burn(dst_g, src_g);
        b = color_burn(dst_b, src_b);
    } else if (mix == 8u) {
        r = overlay(src_r, dst_r);
        g = overlay(src_g, dst_g);
        b = overlay(src_b, dst_b);
    } else if (mix == 9u) {
        r = soft_light(dst_r, src_r);
        g = soft_light(dst_g, src_g);
        b = soft_light(dst_b, src_b);
    } else if (mix == 10u) {
        r = abs(dst_r - src_r);
        g = abs(dst_g - src_g);
        b = abs(dst_b - src_b);
    } else if (mix == 11u) {
        r = dst_r + src_r - 2.0 * dst_r * src_r;
        g = dst_g + src_g - 2.0 * dst_g * src_g;
        b = dst_b + src_b - 2.0 * dst_b * src_b;
    } else if (mix == 12u) {
        let sat_dst = sat3(dst_r, dst_g, dst_b);
        let lum_dst = lum3(dst_r, dst_g, dst_b);
        let sr = set_sat_channel(src_r, src_g, src_b, sat_dst, 0u);
        let sg = set_sat_channel(src_r, src_g, src_b, sat_dst, 1u);
        let sb = set_sat_channel(src_r, src_g, src_b, sat_dst, 2u);
        r = set_lum_channel(sr, sg, sb, lum_dst, 0u);
        g = set_lum_channel(sr, sg, sb, lum_dst, 1u);
        b = set_lum_channel(sr, sg, sb, lum_dst, 2u);
    } else if (mix == 13u) {
        let sat_src = sat3(src_r, src_g, src_b);
        let lum_dst = lum3(dst_r, dst_g, dst_b);
        let dr = set_sat_channel(dst_r, dst_g, dst_b, sat_src, 0u);
        let dg = set_sat_channel(dst_r, dst_g, dst_b, sat_src, 1u);
        let db = set_sat_channel(dst_r, dst_g, dst_b, sat_src, 2u);
        r = set_lum_channel(dr, dg, db, lum_dst, 0u);
        g = set_lum_channel(dr, dg, db, lum_dst, 1u);
        b = set_lum_channel(dr, dg, db, lum_dst, 2u);
    } else if (mix == 14u) {
        let lum_dst = lum3(dst_r, dst_g, dst_b);
        r = set_lum_channel(src_r, src_g, src_b, lum_dst, 0u);
        g = set_lum_channel(src_r, src_g, src_b, lum_dst, 1u);
        b = set_lum_channel(src_r, src_g, src_b, lum_dst, 2u);
    } else if (mix == 15u) {
        let lum_src = lum3(src_r, src_g, src_b);
        r = set_lum_channel(dst_r, dst_g, dst_b, lum_src, 0u);
        g = set_lum_channel(dst_r, dst_g, dst_b, lum_src, 1u);
        b = set_lum_channel(dst_r, dst_g, dst_b, lum_src, 2u);
    }

    if (channel == 0u) {
        return r;
    } else if (channel == 1u) {
        return g;
    }
    return b;
}

fn overlay(dst: f32, src: f32) -> f32 {
    if (dst <= 0.5) {
        return 2.0 * dst * src;
    }
    return 1.0 - 2.0 * (1.0 - dst) * (1.0 - src);
}

fn color_dodge(dst: f32, src: f32) -> f32 {
    var out = 1.0;
    if (src < 1.0) {
        out = min(dst / (1.0 - src), 1.0);
    }
    return out;
}

fn color_burn(dst: f32, src: f32) -> f32 {
    var out = 0.0;
    if (src > 0.0) {
        out = 1.0 - min((1.0 - dst) / src, 1.0);
    }
    return out;
}

fn color_dodge_premul(src: f32, dst: f32, src_alpha: f32, dst_alpha: f32) -> f32 {
    var out = src * (1.0 - dst_alpha);
    if (dst > 0.0) {
        if (src >= src_alpha) {
            out = src + dst * (1.0 - src_alpha);
        } else {
            out = src_alpha * min(dst_alpha, (dst * src_alpha) / (src_alpha - src)) +
                src * (1.0 - dst_alpha) +
                dst * (1.0 - src_alpha);
        }
    }
    return out;
}

fn color_burn_premul(src: f32, dst: f32, src_alpha: f32, dst_alpha: f32) -> f32 {
    var out = dst + src * (1.0 - dst_alpha);
    if (dst < dst_alpha) {
        if (src <= 0.0) {
            out = dst * (1.0 - src_alpha);
        } else {
            out = src_alpha * (dst_alpha - min(dst_alpha, ((dst_alpha - dst) * src_alpha) / src)) +
                src * (1.0 - dst_alpha) +
                dst * (1.0 - src_alpha);
        }
    }
    return out;
}

fn soft_light(dst: f32, src: f32) -> f32 {
    var out = dst - (1.0 - 2.0 * src) * dst * (1.0 - dst);
    if (src > 0.5) {
        var d = sqrt(dst);
        if (dst <= 0.25) {
            d = ((16.0 * dst - 12.0) * dst + 4.0) * dst;
        }
        out = dst + (2.0 * src - 1.0) * (d - dst);
    }
    return out;
}

fn lum3(r: f32, g: f32, b: f32) -> f32 {
    return 0.3 * r + 0.59 * g + 0.11 * b;
}

fn svg_lum3(r: f32, g: f32, b: f32) -> f32 {
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

fn sat3(r: f32, g: f32, b: f32) -> f32 {
    return max(max(r, g), b) - min(min(r, g), b);
}

fn set_lum_channel(r: f32, g: f32, b: f32, lum: f32, channel: u32) -> f32 {
    let d = lum - lum3(r, g, b);
    return clip_color_channel(r + d, g + d, b + d, channel);
}

fn clip_color_channel(r: f32, g: f32, b: f32, channel: u32) -> f32 {
    let lum = lum3(r, g, b);
    let min_c = min(min(r, g), b);
    let max_c = max(max(r, g), b);
    var out_r = r;
    var out_g = g;
    var out_b = b;
    if (min_c < 0.0) {
        out_r = lum + (out_r - lum) * lum / (lum - min_c);
        out_g = lum + (out_g - lum) * lum / (lum - min_c);
        out_b = lum + (out_b - lum) * lum / (lum - min_c);
    }
    if (max_c > 1.0) {
        out_r = lum + (out_r - lum) * (1.0 - lum) / (max_c - lum);
        out_g = lum + (out_g - lum) * (1.0 - lum) / (max_c - lum);
        out_b = lum + (out_b - lum) * (1.0 - lum) / (max_c - lum);
    }
    if (channel == 0u) {
        return out_r;
    } else if (channel == 1u) {
        return out_g;
    }
    return out_b;
}

fn set_sat_channel(r: f32, g: f32, b: f32, sat: f32, channel: u32) -> f32 {
    var min_ix = 0u;
    if (r <= g && r <= b) {
    } else if (g <= b) {
        min_ix = 1u;
    } else {
        min_ix = 2u;
    }
    var max_ix = 0u;
    if (r >= g && r >= b) {
    } else if (g >= b) {
        max_ix = 1u;
    } else {
        max_ix = 2u;
    }
    var out_r = 0.0;
    var out_g = 0.0;
    var out_b = 0.0;
    if (min_ix != max_ix) {
        let mid_ix = 3u - min_ix - max_ix;
        let min_v = channel_value(r, g, b, min_ix);
        let mid_v = channel_value(r, g, b, mid_ix);
        let max_v = channel_value(r, g, b, max_ix);
        var new_mid = 0.0;
        var new_max = 0.0;
        if (max_v > min_v) {
            new_mid = (mid_v - min_v) * sat / (max_v - min_v);
            new_max = sat;
        }
        out_r = set_channel_value(out_r, new_mid, mid_ix, 0u);
        out_g = set_channel_value(out_g, new_mid, mid_ix, 1u);
        out_b = set_channel_value(out_b, new_mid, mid_ix, 2u);
        out_r = set_channel_value(out_r, new_max, max_ix, 0u);
        out_g = set_channel_value(out_g, new_max, max_ix, 1u);
        out_b = set_channel_value(out_b, new_max, max_ix, 2u);
    }
    if (channel == 0u) {
        return out_r;
    } else if (channel == 1u) {
        return out_g;
    }
    return out_b;
}

fn channel_value(r: f32, g: f32, b: f32, channel: u32) -> f32 {
    if (channel == 0u) {
        return r;
    } else if (channel == 1u) {
        return g;
    }
    return b;
}

fn set_channel_value(current: f32, value: f32, src_channel: u32, dst_channel: u32) -> f32 {
    if (src_channel == dst_channel) {
        return value;
    }
    return current;
}

fn pack_premul_rgba8(r: f32, g: f32, b: f32, a: f32) -> u32 {
    return u32(clamp(r, 0.0, 1.0) * 255.0 + 0.5) |
        (u32(clamp(g, 0.0, 1.0) * 255.0 + 0.5) << 8u) |
        (u32(clamp(b, 0.0, 1.0) * 255.0 + 0.5) << 16u) |
        (u32(clamp(a, 0.0, 1.0) * 255.0 + 0.5) << 24u);
}

fn lerp_f32(a: f32, b: f32, t: f32) -> f32 {
    return a + (b - a) * t;
}

fn coverage_to_u8(coverage: f32) -> u32 {
    return u32(clamp(coverage, 0.0, 1.0) * 255.0 + 0.5);
}

fn sdf_coverage_from_encoded(sdf_ref: u32, x: f32, y: f32) -> f32 {
    let x0 = sdf_x0[sdf_ref];
    let y0 = sdf_y0[sdf_ref];
    let x1 = sdf_x1[sdf_ref];
    let y1 = sdf_y1[sdf_ref];
    let r0 = sdf_r0[sdf_ref];
    let r1 = sdf_r1[sdf_ref];
    let r2 = sdf_r2[sdf_ref];
    let r3 = sdf_r3[sdf_ref];
    let stroke_top = sdf_stroke_top[sdf_ref];
    let stroke_right = sdf_stroke_right[sdf_ref];
    let stroke_bottom = sdf_stroke_bottom[sdf_ref];
    let stroke_left = sdf_stroke_left[sdf_ref];
    let shadow_offset_x = sdf_shadow_offset_x[sdf_ref];
    let shadow_offset_y = sdf_shadow_offset_y[sdf_ref];
    let shadow_expand = sdf_shadow_expand[sdf_ref];
    let shadow_intensity = sdf_shadow_intensity[sdf_ref];
    let kind = sdf_kinds[sdf_ref];

    if (kind == GPU_SDF_RECT) {
        return sdf_coverage_from_dist(rect_sdf_distance(x, y, x0, y0, x1, y1, r0, r1, r2, r3));
    }
    if (kind == GPU_SDF_RECT_STROKE) {
        let half_top = max(stroke_top, 0.0);
        let half_right = max(stroke_right, 0.0);
        let half_bottom = max(stroke_bottom, 0.0);
        let half_left = max(stroke_left, 0.0);
        let rx0 = min(x0, x1);
        let ry0 = min(y0, y1);
        let rx1 = max(x0, x1);
        let ry1 = max(y0, y1);
        let outer = sdf_coverage_from_dist(rect_sdf_distance(
            x, y,
            rx0 - half_left,
            ry0 - half_top,
            rx1 + half_right,
            ry1 + half_bottom,
            r0 + max(half_top, half_left),
            r1 + max(half_top, half_right),
            r2 + max(half_bottom, half_left),
            r3 + max(half_bottom, half_right),
        ));
        let inner_x0 = rx0 + half_left;
        let inner_y0 = ry0 + half_top;
        let inner_x1 = rx1 - half_right;
        let inner_y1 = ry1 - half_bottom;
        var inner = 0.0;
        if (inner_x0 < inner_x1 && inner_y0 < inner_y1) {
            inner = sdf_coverage_from_dist(rect_sdf_distance(
                x, y,
                inner_x0,
                inner_y0,
                inner_x1,
                inner_y1,
                max(r0 - max(half_top, half_left), 0.0),
                max(r1 - max(half_top, half_right), 0.0),
                max(r2 - max(half_bottom, half_left), 0.0),
                max(r3 - max(half_bottom, half_right), 0.0),
            ));
        }
        return clamp(outer - inner, 0.0, 1.0);
    }
    if (kind == GPU_SDF_RECT_SHADOW) {
        return sdf_shadow_coverage_from_dist(
            rect_sdf_distance(
                x - shadow_offset_x,
                y - shadow_offset_y,
                x0, y0, x1, y1, r0, r1, r2, r3,
            ),
            shadow_expand,
            shadow_intensity,
        );
    }
    if (kind == GPU_SDF_CIRCLE) {
        return sdf_coverage_from_dist(circle_sdf_distance(x, y, x0, y0, x1));
    }
    if (kind == GPU_SDF_CIRCLE_STROKE) {
        let half = max(stroke_top, 0.0);
        let radius = max(x1, 0.0);
        let outer = sdf_coverage_from_dist(circle_sdf_distance(x, y, x0, y0, radius + half));
        var inner = 0.0;
        if (radius > half) {
            inner = sdf_coverage_from_dist(circle_sdf_distance(x, y, x0, y0, radius - half));
        }
        return clamp(outer - inner, 0.0, 1.0);
    }
    if (kind == GPU_SDF_CIRCLE_SHADOW) {
        return sdf_shadow_coverage_from_dist(
            circle_sdf_distance(x - shadow_offset_x, y - shadow_offset_y, x0, y0, x1),
            shadow_expand,
            shadow_intensity,
        );
    }
    if (kind == GPU_SDF_ARC) {
        return sdf_coverage_from_dist(arc_sdf_distance(x, y, x0, y0, x1, y1, r0, r1, r2));
    }
    if (kind == GPU_SDF_ARC_SHADOW) {
        return sdf_shadow_coverage_from_dist(
            arc_sdf_distance(x - shadow_offset_x, y - shadow_offset_y, x0, y0, x1, y1, r0, r1, r2),
            shadow_expand,
            shadow_intensity,
        );
    }
    if (kind == GPU_SDF_CANDLESTICK) {
        return candlestick_sdf_coverage(x, y, x0, y0, x1, y1, r0, r1, r2);
    }
    if (kind == GPU_SDF_LINE) {
        return sdf_coverage_from_dist(line_sdf_distance(x, y, x0, y0, x1, y1, r0, r1));
    }
    if (kind == GPU_SDF_DASH_LINE) {
        return sdf_coverage_from_dist(dash_line_sdf_distance(x, y, x0, y0, x1, y1, r0, r1, r2, r3, stroke_top));
    }
    if (kind == GPU_SDF_LINE_SHADOW) {
        return sdf_shadow_coverage_from_dist(
            line_sdf_distance(x - shadow_offset_x, y - shadow_offset_y, x0, y0, x1, y1, r0, r1),
            shadow_expand,
            shadow_intensity,
        );
    }
    return 0.0;
}

fn dash_line_sdf_distance(
    x: f32,
    y: f32,
    sx: f32,
    sy: f32,
    ex: f32,
    ey: f32,
    width: f32,
    cap: f32,
    dash_length_raw: f32,
    gap_length_raw: f32,
    dash_offset: f32,
) -> f32 {
    let dash_length = max(dash_length_raw, 0.0);
    let gap_length = max(gap_length_raw, 0.0);
    var dist = line_sdf_distance(x, y, sx, sy, ex, ey, width, cap);
    if (dash_length > 0.000001 && gap_length > 0.000001) {
        let half = max(width, 0.0) * 0.5;
        let dx = ex - sx;
        let dy = ey - sy;
        let len = sqrt(dx * dx + dy * dy);
        if (len > 0.000001) {
            let ux = dx / len;
            let uy = dy / len;
            let px = x - sx;
            let py = y - sy;
            let axis = px * ux + py * uy;
            let normal = -px * uy + py * ux;
            let cycle = dash_length + gap_length;
            let offset = rem_euclid_f32(dash_offset, cycle);
            let base = floor((axis + offset) / cycle);
            dist = min(
                min(
                    dash_line_segment_distance(axis, normal, len, half, cap, dash_length, cycle, offset, base - 1.0),
                    dash_line_segment_distance(axis, normal, len, half, cap, dash_length, cycle, offset, base),
                ),
                dash_line_segment_distance(axis, normal, len, half, cap, dash_length, cycle, offset, base + 1.0),
            );
        }
    }
    return dist;
}

fn dash_line_segment_distance(axis: f32, normal: f32, len: f32, half: f32, cap: f32, dash_length: f32, cycle: f32, offset: f32, dash_ix: f32) -> f32 {
    let dash_start = dash_ix * cycle - offset;
    let dash_end = dash_start + dash_length;
    var dist = 1000000.0;
    if (dash_end > 0.0 && dash_start < len) {
        let start = max(dash_start, 0.0);
        let end = min(dash_end, len);
        if (end > start) {
            dist = line_segment_sdf_distance(axis, normal, start, end, half, cap);
        }
    }
    return dist;
}

fn line_segment_sdf_distance(axis: f32, normal: f32, start: f32, end: f32, half: f32, cap: f32) -> f32 {
    let nearest = clamp(axis, start, end);
    var dist = sqrt((axis - nearest) * (axis - nearest) + normal * normal) - half;
    if (cap < 0.5) {
        dist = local_line_rect_distance(axis, normal, start, end, half);
    } else if (cap < 1.5) {
        dist = local_line_rect_distance(axis, normal, start - half, end + half, half);
    }
    return dist;
}

fn candlestick_sdf_coverage(
    x: f32,
    y: f32,
    center_x: f32,
    high_y: f32,
    low_y: f32,
    body_top_y: f32,
    body_bottom_y: f32,
    body_width: f32,
    wick_width: f32,
) -> f32 {
    let wick_half_width = max(wick_width, 1.0) * 0.5;
    let wick = sdf_coverage_from_dist(rect_sdf_distance(
        x, y,
        center_x - wick_half_width,
        min(high_y, low_y),
        center_x + wick_half_width,
        max(high_y, low_y),
        0.0, 0.0, 0.0, 0.0,
    ));
    let half_width = max(body_width, 1.0) * 0.5;
    var body_y0 = min(body_top_y, body_bottom_y);
    var body_y1 = max(body_top_y, body_bottom_y);
    if (body_y0 == body_y1) {
        body_y0 = body_y0 - 0.5;
        body_y1 = body_y1 + 0.5;
    }
    let body = sdf_coverage_from_dist(rect_sdf_distance(
        x, y,
        center_x - half_width,
        body_y0,
        center_x + half_width,
        body_y1,
        0.0, 0.0, 0.0, 0.0,
    ));
    return max(wick, body);
}

fn line_sdf_distance(x: f32, y: f32, sx: f32, sy: f32, ex: f32, ey: f32, width: f32, cap: f32) -> f32 {
    let half = max(width, 0.0) * 0.5;
    let dx = ex - sx;
    let dy = ey - sy;
    let len = sqrt(dx * dx + dy * dy);
    var dist = 1000000.0;
    if (len <= 0.000001) {
        if (cap >= 0.5) {
            if (cap > 1.5) {
                dist = sqrt((x - sx) * (x - sx) + (y - sy) * (y - sy)) - half;
            } else {
                dist = local_line_rect_distance(0.0, 0.0, -half, half, half);
            }
        }
    } else {
        let ux = dx / len;
        let uy = dy / len;
        let px = x - sx;
        let py = y - sy;
        let axis = px * ux + py * uy;
        let normal = -px * uy + py * ux;
        if (cap < 0.5) {
            dist = local_line_rect_distance(axis, normal, 0.0, len, half);
        } else if (cap < 1.5) {
            dist = local_line_rect_distance(axis, normal, -half, len + half, half);
        } else {
            let nearest = clamp(axis, 0.0, len);
            dist = sqrt((axis - nearest) * (axis - nearest) + normal * normal) - half;
        }
    }
    return dist;
}

fn arc_sdf_distance(x: f32, y: f32, cx: f32, cy: f32, radius_raw: f32, width_raw: f32, start_angle: f32, sweep_angle: f32, cap: f32) -> f32 {
    let radius = max(radius_raw, 0.0);
    let width = max(width_raw, 0.0);
    var dist = 1000000.0;
    if (radius > 0.000001 && width > 0.000001 && abs(sweep_angle) > 0.000001) {
        let vx = x - cx;
        let vy = y - cy;
        let len = sqrt(vx * vx + vy * vy);
        let half = width * 0.5;
        if (abs(sweep_angle) >= 6.2830853) {
            dist = abs(len - radius) - half;
        } else {
            dist = arc_butt_sdf_distance(vx, vy, len, radius, half, start_angle, sweep_angle);
            if (cap > 1.5) {
                dist = min(
                    min(dist, arc_endpoint_distance(vx, vy, radius, start_angle) - half),
                    arc_endpoint_distance(vx, vy, radius, start_angle + sweep_angle) - half,
                );
            } else if (cap >= 0.5) {
                dist = min(
                    min(dist, arc_square_cap_distance(vx, vy, radius, start_angle, sweep_angle, -half, 0.0, half)),
                    arc_square_cap_distance(vx, vy, radius, start_angle + sweep_angle, sweep_angle, 0.0, half, half),
                );
            }
        }
    }
    return dist;
}

fn arc_butt_sdf_distance(vx: f32, vy: f32, len: f32, radius: f32, half: f32, start_angle: f32, sweep_angle: f32) -> f32 {
    if (len <= 0.000001) {
        return min(
            arc_endpoint_distance(vx, vy, radius, start_angle),
            arc_endpoint_distance(vx, vy, radius, start_angle + sweep_angle),
        ) - half;
    }
    let angle = atan2(vy, vx);
    let radial = abs(len - radius) - half;
    if (arc_angle_in_sweep(angle, start_angle, sweep_angle)) {
        return radial;
    }
    return min(
        arc_cap_segment_distance(vx, vy, radius, start_angle, half),
        arc_cap_segment_distance(vx, vy, radius, start_angle + sweep_angle, half),
    );
}

fn arc_angle_in_sweep(angle: f32, start_angle: f32, sweep_angle: f32) -> bool {
    let tau = 6.2831855;
    let eps = 0.000001;
    if (sweep_angle >= 0.0) {
        return rem_euclid_f32(angle - start_angle, tau) <= sweep_angle + eps;
    }
    return rem_euclid_f32(start_angle - angle, tau) <= -sweep_angle + eps;
}

fn arc_endpoint_distance(vx: f32, vy: f32, radius: f32, angle: f32) -> f32 {
    let ex = radius * cos(angle);
    let ey = radius * sin(angle);
    return sqrt((vx - ex) * (vx - ex) + (vy - ey) * (vy - ey));
}

fn arc_cap_segment_distance(vx: f32, vy: f32, radius: f32, angle: f32, half: f32) -> f32 {
    let inner_radius = max(radius - half, 0.0);
    let outer_radius = radius + half;
    let co = cos(angle);
    let si = sin(angle);
    return distance_to_segment(
        vx,
        vy,
        inner_radius * co,
        inner_radius * si,
        outer_radius * co,
        outer_radius * si,
    );
}

fn arc_square_cap_distance(vx: f32, vy: f32, radius: f32, angle: f32, sweep_angle: f32, x0: f32, x1: f32, half: f32) -> f32 {
    var dir = 1.0;
    if (sweep_angle < 0.0) {
        dir = -1.0;
    }
    let co = cos(angle);
    let si = sin(angle);
    let ex = radius * co;
    let ey = radius * si;
    let tangent_x = -si * dir;
    let tangent_y = co * dir;
    let px = vx - ex;
    let py = vy - ey;
    let local_x = px * tangent_x + py * tangent_y;
    let local_y = -px * tangent_y + py * tangent_x;
    return local_line_rect_distance(local_x, local_y, x0, x1, half);
}

fn distance_to_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let dx = bx - ax;
    let dy = by - ay;
    let len2 = dx * dx + dy * dy;
    var dist = sqrt((px - ax) * (px - ax) + (py - ay) * (py - ay));
    if (len2 > 0.000001) {
        let t = clamp(((px - ax) * dx + (py - ay) * dy) / len2, 0.0, 1.0);
        let nx = ax + dx * t;
        let ny = ay + dy * t;
        dist = sqrt((px - nx) * (px - nx) + (py - ny) * (py - ny));
    }
    return dist;
}

fn rem_euclid_f32(value: f32, modulus: f32) -> f32 {
    return value - floor(value / modulus) * modulus;
}

fn local_line_rect_distance(axis: f32, normal: f32, x0: f32, x1: f32, half_height: f32) -> f32 {
    let center = (x0 + x1) * 0.5;
    let half_width = (x1 - x0) * 0.5;
    let dx = abs(axis - center) - half_width;
    let dy = abs(normal) - half_height;
    return sqrt(max(dx, 0.0) * max(dx, 0.0) + max(dy, 0.0) * max(dy, 0.0)) + min(max(dx, dy), 0.0);
}

fn sdf_coverage_from_dist(dist: f32) -> f32 {
    return clamp(0.5 - dist, 0.0, 1.0);
}

fn sdf_shadow_coverage_from_dist(dist: f32, expand: f32, intensity: f32) -> f32 {
    let clamped_intensity = clamp(intensity, 0.0, 1.0);
    var coverage = sdf_coverage_from_dist(dist) * clamped_intensity;
    if (expand > 0.0) {
        coverage = exp(-max(dist, 0.0) / expand) * clamped_intensity;
    }
    return clamp(coverage, 0.0, 1.0);
}

fn circle_sdf_distance(x: f32, y: f32, cx: f32, cy: f32, radius: f32) -> f32 {
    let dx = x - cx;
    let dy = y - cy;
    return sqrt(dx * dx + dy * dy) - radius;
}

fn rect_sdf_distance(
    x: f32,
    y: f32,
    x0_raw: f32,
    y0_raw: f32,
    x1_raw: f32,
    y1_raw: f32,
    top_left: f32,
    top_right: f32,
    bottom_left: f32,
    bottom_right: f32,
) -> f32 {
    let x0 = min(x0_raw, x1_raw);
    let y0 = min(y0_raw, y1_raw);
    let x1 = max(x0_raw, x1_raw);
    let y1 = max(y0_raw, y1_raw);
    let cx = (x0 + x1) * 0.5;
    let cy = (y0 + y1) * 0.5;
    let hx = (x1 - x0) * 0.5;
    let hy = (y1 - y0) * 0.5;
    let px = x - cx;
    let py = y - cy;
    var radius = top_left;
    if (px >= 0.0) {
        if (py <= 0.0) {
            radius = top_right;
        } else {
            radius = bottom_right;
        }
    } else if (py > 0.0) {
        radius = bottom_left;
    }
    let r = max(min(min(radius, hx), hy), 0.0);
    let ax = abs(px);
    let ay = abs(py);

    if (r <= 0.0) {
        let dx = ax - hx;
        let dy = ay - hy;
        return sqrt(max(dx, 0.0) * max(dx, 0.0) + max(dy, 0.0) * max(dy, 0.0)) + min(max(dx, dy), 0.0);
    }

    let qx = ax - hx + r;
    let qy = ay - hy + r;
    return min(max(qx, qy), 0.0) + sqrt(max(qx, 0.0) * max(qx, 0.0) + max(qy, 0.0) * max(qy, 0.0)) - r;
}
