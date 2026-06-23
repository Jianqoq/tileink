const TILE_SIZE: u32 = 16u;
const DRAW_PATH_NONE: u32 = 0xffffffffu;
const DRAW_FLAG_SOLID_RECT: u32 = 1u;
const DRAW_FLAG_HAS_SDF: u32 = 2u;
const DRAW_FLAG_EVEN_ODD: u32 = 4u;
const DRAW_FLAG_ALLOW_SOLID_OVERRIDE: u32 = 8u;

const GPU_TILE_BG_NONE: u32 = 0u;
const GPU_PTCL_END: u32 = 0u;
const GPU_PTCL_FILL: u32 = 1u;
const GPU_PTCL_SDF: u32 = 2u;
const GPU_PTCL_COLOR: u32 = 3u;
const GPU_PTCL_BEGIN_CLIP: u32 = 4u;
const GPU_PTCL_BEGIN_CLIP_SDF: u32 = 5u;
const GPU_PTCL_END_CLIP: u32 = 6u;
const GPU_PTCL_BEGIN_OPACITY: u32 = 7u;
const GPU_PTCL_END_OPACITY: u32 = 8u;
const GPU_PTCL_BEGIN_BLEND: u32 = 9u;
const GPU_PTCL_END_BLEND: u32 = 10u;
const BRUSH_KIND_SOLID: u32 = 0u;
const CLIP_KIND_PATH: u32 = 0u;
const GROUP_STACK_BLEND_BIT: u32 = 1u << 31u;
const MAX_LAYER_DEPTH: u32 = 16u;

struct CoarseParams {
    width: u32,
    height: u32,
    width_in_tiles: u32,
    height_in_tiles: u32,
    path_count: u32,
    draw_count: u32,
    ptcl_per_tile: u32,
    segment_capacity: u32,
    fill_aux_capacity: u32,
    fill_alpha_capacity: u32,
    draws_off: u32,
    backdrops_off: u32,
    backdrop_pool_off: u32,
    binned_off: u32,
    starts_off: u32,
    opacity_off: u32,
    blend_off: u32,
    tiles_off: u32,
    segments_off: u32,
    ptcl_off: u32,
    fill_aux_off: u32,
    fill_alpha_off: u32,
    counters_off: u32,
    clip_count: u32,
    clip_layers_off: u32,
    clip_backdrop_off: u32,
    clip_seg_start_off: u32,
    clip_seg_end_off: u32,
    clip_geom_off: u32,
    clip_geom_start_off: u32,
    clip_geom_count_off: u32,
    tile_draw_starts_off: u32,
    tile_draw_indices_off: u32,
    tile_ptcl_starts_off: u32,
    clip_stack_data_off: u32,
    group_stack_data_off: u32,
    clip_spill_starts_off: u32,
    group_spill_starts_off: u32,
    coarse_clip_spill_off: u32,
    coarse_group_spill_off: u32,
    fine_clip_spill_off: u32,
    fine_group_spill_off: u32,
    ptcl_capacity: u32,
    solid_spans_off: u32,
    solid_span_capacity: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
    _pad3: u32,
}

@group(0) @binding(0) var<uniform> params: CoarseParams;
@group(0) @binding(1) var<storage, read_write> arena: array<atomic<u32>>;
@group(0) @binding(2) var<storage, read_write> coverage_indirect: array<atomic<u32>>;

var<workgroup> coverage_alpha: array<u32, 256>;
var<workgroup> partial_counts: array<u32, 64>;
var<workgroup> partial_offsets: array<u32, 64>;
var<workgroup> span_counts: array<u32, 16>;
var<workgroup> span_offsets: array<u32, 16>;
var<workgroup> compact_meta: array<u32, 4>;

fn word(byte_offset: u32) -> u32 {
    return byte_offset >> 2u;
}

fn load_u32(byte_offset: u32) -> u32 {
    return atomicLoad(&arena[word(byte_offset)]);
}

fn load_i32(byte_offset: u32) -> i32 {
    return bitcast<i32>(load_u32(byte_offset));
}

fn load_f32(byte_offset: u32) -> f32 {
    return bitcast<f32>(load_u32(byte_offset));
}

fn store_u32(byte_offset: u32, value: u32) {
    atomicStore(&arena[word(byte_offset)], value);
}

fn store_i32(byte_offset: u32, value: i32) {
    store_u32(byte_offset, bitcast<u32>(value));
}

fn store_u8(byte_offset: u32, value: u32) {
    let target_word = word(byte_offset);
    let shift = (byte_offset & 3u) * 8u;
    let mask = 255u << shift;
    var old = atomicLoad(&arena[target_word]);
    loop {
        let next = (old & ~mask) | ((value & 255u) << shift);
        let result = atomicCompareExchangeWeak(&arena[target_word], old, next);
        if result.exchanged {
            break;
        }
        old = result.old_value;
    }
}

fn atomic_alloc(counter: u32, count: u32) -> u32 {
    let value = atomicAdd(&arena[word(params.counters_off) + counter], count);
    if counter == 1u {
        atomicMax(&coverage_indirect[0], value + count);
    }
    return value;
}

fn atomic_alloc_output(counter: u32, count: u32) -> u32 {
    return atomicAdd(&arena[word(params.counters_off) + counter], count);
}

fn tile_record_off(tile_ix: u32) -> u32 {
    return params.tiles_off + tile_ix * 28u;
}

fn clear_tile(tile_ix: u32) {
    let base = tile_record_off(tile_ix);
    store_u32(base, GPU_TILE_BG_NONE);
    store_i32(base + 4u, 0);
    store_u32(base + 8u, 0u);
    store_u32(base + 12u, 0u);
    store_u32(base + 16u, load_u32(params.tile_ptcl_starts_off + tile_ix * 4u));
    store_u32(base + 20u, 0u);
    store_u32(base + 24u, 0u);
}

fn set_tile_counts(tile_ix: u32, segment_start: u32, segment_count: u32, ptcl_count: u32, touched: bool) {
    let base = tile_record_off(tile_ix);
    store_u32(base + 8u, segment_start);
    store_u32(base + 12u, segment_count);
    store_u32(base + 20u, ptcl_count);
    store_u32(base + 24u, select(0u, 1u, touched));
}

fn ptcl_record_off(tile_ix: u32, local_ix: u32) -> u32 {
    let start = load_u32(tile_record_off(tile_ix) + 16u);
    return params.ptcl_off + (start + local_ix) * 80u;
}

fn tile_ptcl_capacity(tile_ix: u32) -> u32 {
    let start = load_u32(params.tile_ptcl_starts_off + tile_ix * 4u);
    let end = load_u32(params.tile_ptcl_starts_off + (tile_ix + 1u) * 4u);
    return end - start;
}

fn clear_ptcl(base: u32) {
    for (var i = 0u; i < 20u; i++) {
        store_u32(base + i * 4u, 0u);
    }
}

fn push_simple(tile_ix: u32, count: ptr<function, u32>, tag: u32) {
    if *count >= tile_ptcl_capacity(tile_ix) {
        return;
    }
    let base = ptcl_record_off(tile_ix, *count);
    clear_ptcl(base);
    store_u32(base, tag);
    *count += 1u;
}

fn push_opacity(tile_ix: u32, count: ptr<function, u32>, layer_ix: u32) {
    if *count >= tile_ptcl_capacity(tile_ix) {
        return;
    }
    let base = ptcl_record_off(tile_ix, *count);
    clear_ptcl(base);
    store_u32(base, GPU_PTCL_BEGIN_OPACITY);
    store_u32(base + 28u, load_u32(params.opacity_off + layer_ix * 4u));
    *count += 1u;
}

fn push_blend(tile_ix: u32, count: ptr<function, u32>, layer_ix: u32) {
    if *count >= tile_ptcl_capacity(tile_ix) {
        return;
    }
    let packed = load_u32(params.blend_off + layer_ix * 4u);
    let base = ptcl_record_off(tile_ix, *count);
    clear_ptcl(base);
    store_u32(base, GPU_PTCL_BEGIN_BLEND);
    store_u32(base + 32u, packed & 255u);
    store_u32(base + 36u, (packed >> 8u) & 255u);
    *count += 1u;
}

fn push_bounds_command(
    tile_ix: u32,
    count: ptr<function, u32>,
    tag: u32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    color: u32,
    draw_base: u32,
) {
    if *count >= tile_ptcl_capacity(tile_ix) {
        return;
    }
    let base = ptcl_record_off(tile_ix, *count);
    clear_ptcl(base);
    store_u32(base, tag);
    store_i32(base + 8u, x0);
    store_i32(base + 12u, y0);
    store_i32(base + 16u, x1);
    store_i32(base + 20u, y1);
    store_u32(base + 24u, color);
    store_u32(base + 76u, load_u32(draw_base + 68u));
    if tag == GPU_PTCL_SDF {
        for (var i = 0u; i < 9u; i++) {
            store_u32(base + 40u + i * 4u, load_u32(draw_base + 32u + i * 4u));
        }
    }
    *count += 1u;
}

fn push_fill(tile_ix: u32, count: ptr<function, u32>, aux_ix: u32) {
    if *count >= tile_ptcl_capacity(tile_ix) {
        return;
    }
    let base = ptcl_record_off(tile_ix, *count);
    clear_ptcl(base);
    store_u32(base, GPU_PTCL_FILL);
    store_u32(base + 4u, aux_ix);
    *count += 1u;
}

fn push_clip_fill(tile_ix: u32, count: ptr<function, u32>, aux_ix: u32) {
    if *count >= tile_ptcl_capacity(tile_ix) {
        return;
    }
    let base = ptcl_record_off(tile_ix, *count);
    clear_ptcl(base);
    store_u32(base, GPU_PTCL_BEGIN_CLIP);
    store_u32(base + 4u, aux_ix);
    *count += 1u;
}

fn push_clip_sdf(
    tile_ix: u32,
    count: ptr<function, u32>,
    clip_base: u32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
) {
    if *count >= tile_ptcl_capacity(tile_ix) {
        return;
    }
    let base = ptcl_record_off(tile_ix, *count);
    clear_ptcl(base);
    store_u32(base, GPU_PTCL_BEGIN_CLIP_SDF);
    store_i32(base + 8u, x0);
    store_i32(base + 12u, y0);
    store_i32(base + 16u, x1);
    store_i32(base + 20u, y1);
    for (var i = 0u; i < 9u; i++) {
        store_u32(base + 40u + i * 4u, load_u32(clip_base + 32u + i * 4u));
    }
    *count += 1u;
}

fn segment_value(segment_ix: u32, field: u32) -> u32 {
    return load_u32(params.binned_off + segment_ix * 28u + field * 4u);
}

fn copy_segment(src_ix: u32, dst_ix: u32) {
    let dst = params.segments_off + dst_ix * 20u;
    store_u32(dst, segment_value(src_ix, 3u));
    store_u32(dst + 4u, segment_value(src_ix, 4u));
    store_u32(dst + 8u, segment_value(src_ix, 5u));
    store_u32(dst + 12u, segment_value(src_ix, 6u));
    store_u32(dst + 16u, segment_value(src_ix, 2u));
}

fn output_segment_value(segment_ix: u32, field: u32) -> u32 {
    return load_u32(params.segments_off + segment_ix * 20u + field * 4u);
}

fn copy_clip_segment(src_ix: u32, dst_ix: u32) {
    let src = params.clip_geom_off + src_ix * 20u;
    let dst = params.segments_off + dst_ix * 20u;
    for (var i = 0u; i < 5u; i++) {
        store_u32(dst + i * 4u, load_u32(src + i * 4u));
    }
}

fn apply_rule(value: f32, even_odd: bool) -> f32 {
    if even_odd {
        return abs(value - 2.0 * round(0.5 * value));
    }
    return min(abs(value), 1.0);
}

fn pixel_coverage(seg_start: u32, seg_end: u32, backdrop: i32, even_odd: bool, x: u32, y: u32) -> u32 {
    var coverage = f32(backdrop);
    for (var seg_ix = seg_start; seg_ix < seg_end; seg_ix++) {
        let p0 = vec2f(
            bitcast<f32>(segment_value(seg_ix, 3u)),
            bitcast<f32>(segment_value(seg_ix, 4u)),
        );
        let p1 = vec2f(
            bitcast<f32>(segment_value(seg_ix, 5u)),
            bitcast<f32>(segment_value(seg_ix, 6u)),
        );
        let y_edge_value = bitcast<f32>(segment_value(seg_ix, 2u));
        let delta = p1 - p0;
        let row_y = f32(y);
        let local_y = p0.y - row_y;
        let y0 = clamp(local_y, 0.0, 1.0);
        let y1 = clamp(local_y + delta.y, 0.0, 1.0);
        let dy = y0 - y1;
        let y_edge = sign(delta.x) * clamp(row_y - y_edge_value + 1.0, 0.0, 1.0);
        if dy != 0.0 {
            let recip = 1.0 / delta.y;
            let t0 = (y0 - local_y) * recip;
            let t1 = (y1 - local_y) * recip;
            let sx0 = p0.x + t0 * delta.x;
            let sx1 = p0.x + t1 * delta.x;
            let xmin = min(sx0, sx1) - f32(x);
            let xmax = max(sx0, sx1) - f32(x);
            let a_min = min(xmin, 1.0) - 1e-6;
            let b = min(xmax, 1.0);
            let c = max(b, 0.0);
            let d = max(a_min, 0.0);
            let a = (b + 0.5 * (d * d - c * c) - a_min) / (xmax - a_min);
            coverage += y_edge + a * dy;
        } else {
            coverage += y_edge;
        }
    }
    let normalized = clamp(apply_rule(coverage, even_odd), 0.0, 1.0);
    return u32(normalized * 255.0 + 0.5);
}

fn output_pixel_coverage(
    seg_start: u32,
    seg_end: u32,
    backdrop: i32,
    even_odd: bool,
    x: u32,
    y: u32,
) -> u32 {
    var coverage = f32(backdrop);
    for (var seg_ix = seg_start; seg_ix < seg_end; seg_ix++) {
        let p0 = vec2f(
            bitcast<f32>(output_segment_value(seg_ix, 0u)),
            bitcast<f32>(output_segment_value(seg_ix, 1u)),
        );
        let p1 = vec2f(
            bitcast<f32>(output_segment_value(seg_ix, 2u)),
            bitcast<f32>(output_segment_value(seg_ix, 3u)),
        );
        let y_edge_value = bitcast<f32>(output_segment_value(seg_ix, 4u));
        let delta = p1 - p0;
        let row_y = f32(y);
        let local_y = p0.y - row_y;
        let y0 = clamp(local_y, 0.0, 1.0);
        let y1 = clamp(local_y + delta.y, 0.0, 1.0);
        let dy = y0 - y1;
        let y_edge = sign(delta.x) * clamp(row_y - y_edge_value + 1.0, 0.0, 1.0);
        if dy != 0.0 {
            let recip = 1.0 / delta.y;
            let t0 = (y0 - local_y) * recip;
            let t1 = (y1 - local_y) * recip;
            let sx0 = p0.x + t0 * delta.x;
            let sx1 = p0.x + t1 * delta.x;
            let xmin = min(sx0, sx1) - f32(x);
            let xmax = max(sx0, sx1) - f32(x);
            let a_min = min(xmin, 1.0) - 1e-6;
            let b = min(xmax, 1.0);
            let c = max(b, 0.0);
            let d = max(a_min, 0.0);
            let a = (b + 0.5 * (d * d - c * c) - a_min) / (xmax - a_min);
            coverage += y_edge + a * dy;
        } else {
            coverage += y_edge;
        }
    }
    return u32(clamp(apply_rule(coverage, even_odd), 0.0, 1.0) * 255.0 + 0.5);
}

fn write_clip_fill(
    tile_ix: u32,
    ptcl_count: ptr<function, u32>,
    seg_start: u32,
    seg_end: u32,
    backdrop: i32,
) {
    let aux_ix = atomic_alloc(1u, 1u);
    if aux_ix >= params.fill_aux_capacity {
        return;
    }
    let aux = params.fill_aux_off + aux_ix * 48u;
    store_i32(aux, backdrop);
    store_u32(aux + 4u, seg_start);
    store_u32(aux + 8u, seg_end);
    store_u32(aux + 12u, 0u);
    store_u32(aux + 16u, 0u);
    store_u32(aux + 20u, 0u);
    store_u32(aux + 24u, 0u);
    store_u32(aux + 28u, 0u);
    store_u32(aux + 32u, 0u);
    store_u32(aux + 36u, 0u);
    store_u32(aux + 40u, 0u);
    store_u32(aux + 44u, 0u);
    push_clip_fill(tile_ix, ptcl_count, aux_ix);
}

fn begin_clip(
    tile_ix: u32,
    tile_x: u32,
    tile_y: u32,
    tx0: i32,
    ty0: i32,
    tx1: i32,
    ty1: i32,
    layer_ix: u32,
    clip_segment_base: u32,
    ptcl_count: ptr<function, u32>,
) {
    let clip = params.clip_layers_off + layer_ix * 84u;
    let kind = load_u32(clip);
    if kind == CLIP_KIND_PATH {
        let slot = layer_ix * (params.width_in_tiles * params.height_in_tiles) + tile_ix;
        var local_start = load_u32(params.clip_seg_start_off + slot * 4u);
        var local_end = local_start;
        var backdrop = 0;
        if local_start != 0xffffffffu {
            local_end = load_u32(params.clip_seg_end_off + slot * 4u);
            let scan_x0 = load_i32(clip + 4u);
            let scan_y0 = load_i32(clip + 8u);
            let local_ix = u32(i32(tile_y) - scan_y0) * load_u32(clip + 20u)
                + u32(i32(tile_x) - scan_x0);
            backdrop = load_i32(
                params.clip_backdrop_off
                    + (load_u32(clip + 24u) + local_ix) * 4u,
            );
        } else {
            local_start = 0u;
            local_end = 0u;
        }
        write_clip_fill(
            tile_ix,
            ptcl_count,
            clip_segment_base + local_start,
            clip_segment_base + local_end,
            backdrop,
        );
    } else {
        let x0 = max(tx0, load_i32(clip + 68u));
        let y0 = max(ty0, load_i32(clip + 72u));
        let x1 = min(tx1, load_i32(clip + 76u));
        let y1 = min(ty1, load_i32(clip + 80u));
        push_clip_sdf(tile_ix, ptcl_count, clip, x0, y0, x1, y1);
    }
}

fn write_full_fill(
    tile_ix: u32,
    ptcl_count: ptr<function, u32>,
    source_start: u32,
    source_end: u32,
    backdrop: i32,
    even_odd: bool,
    draw_base: u32,
) {
    let aux_ix = atomic_alloc(1u, 1u);
    if aux_ix >= params.fill_aux_capacity {
        return;
    }
    let aux = params.fill_aux_off + aux_ix * 48u;
    store_i32(aux, backdrop);
    store_u32(aux + 4u, source_start);
    store_u32(aux + 8u, source_end);
    // Bit 0 is the fill rule. Bit 1 means the segment range points into binned input.
    store_u32(aux + 12u, select(0u, 1u, even_odd) | 2u);
    store_u32(aux + 16u, load_u32(draw_base + 24u));
    store_u32(aux + 20u, 0u);
    store_u32(aux + 24u, 0u);
    store_u32(aux + 28u, 0u);
    store_u32(aux + 32u, 0u);
    store_u32(aux + 36u, 0u);
    store_u32(aux + 40u, 0u);
    store_u32(aux + 44u, load_u32(draw_base + 68u));
    push_fill(tile_ix, ptcl_count, aux_ix);
}

@compute @workgroup_size(64)
fn coverage(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let aux_ix = workgroup_id.x;
    let aux = params.fill_aux_off + aux_ix * 48u;
    let seg_start = load_u32(aux + 4u);
    let seg_end = load_u32(aux + 8u);
    let backdrop = load_i32(aux);
    let rule = load_u32(aux + 12u);
    let even_odd = (rule & 1u) != 0u;
    let source_is_binned = (rule & 2u) != 0u;
    let packed_ix = local_id.x;
    let base_pixel = packed_ix * 4u;
    for (var lane = 0u; lane < 4u; lane++) {
        let pixel = base_pixel + lane;
        if source_is_binned {
            coverage_alpha[pixel] = pixel_coverage(
                seg_start,
                seg_end,
                backdrop,
                even_odd,
                pixel % TILE_SIZE,
                pixel / TILE_SIZE,
            );
        } else {
            coverage_alpha[pixel] = output_pixel_coverage(
                seg_start,
                seg_end,
                backdrop,
                even_odd,
                pixel % TILE_SIZE,
                pixel / TILE_SIZE,
            );
        }
    }
    workgroupBarrier();
    var local_partial_count = 0u;
    for (var lane = 0u; lane < 4u; lane++) {
        let alpha = coverage_alpha[base_pixel + lane];
        if alpha > 0u && alpha < 255u {
            local_partial_count += 1u;
        }
    }
    partial_counts[local_id.x] = local_partial_count;
    if local_id.x < 16u {
        let y = local_id.x;
        var row_span_count = 0u;
        var x = 0u;
        loop {
            if x >= TILE_SIZE {
                break;
            }
            if coverage_alpha[y * TILE_SIZE + x] != 255u {
                x += 1u;
                continue;
            }
            row_span_count += 1u;
            loop {
                x += 1u;
                if x >= TILE_SIZE || coverage_alpha[y * TILE_SIZE + x] != 255u {
                    break;
                }
            }
        }
        span_counts[y] = row_span_count;
    }
    workgroupBarrier();

    if local_id.x == 0u {
        var partial_count = 0u;
        for (var lane = 0u; lane < 64u; lane++) {
            partial_offsets[lane] = partial_count;
            partial_count += partial_counts[lane];
        }
        var span_count = 0u;
        for (var y = 0u; y < 16u; y++) {
            span_offsets[y] = span_count;
            span_count += span_counts[y];
        }
        let alpha_bytes = partial_count * 2u;
        var alpha_offset = 0u;
        var span_offset = 0u;
        alpha_offset = atomic_alloc_output(2u, (alpha_bytes + 3u) & ~3u);
        span_offset = atomic_alloc_output(3u, span_count);
        compact_meta[0] = alpha_offset;
        compact_meta[1] = partial_count;
        compact_meta[2] = span_offset;
        compact_meta[3] = span_count;
        if alpha_offset + ((alpha_bytes + 3u) & ~3u) <= params.fill_alpha_capacity
            && span_offset + span_count <= params.solid_span_capacity
        {
            store_u32(aux + 20u, alpha_offset);
            store_u32(aux + 24u, partial_count);
            store_u32(aux + 28u, alpha_offset + partial_count);
            store_u32(aux + 32u, partial_count);
            store_u32(aux + 36u, span_offset);
            store_u32(aux + 40u, span_count);
        }
    }
    workgroupBarrier();

    let alpha_offset = compact_meta[0];
    let partial_count = compact_meta[1];
    if alpha_offset + ((partial_count * 2u + 3u) & ~3u) <= params.fill_alpha_capacity
    {
        var output_partial = partial_offsets[local_id.x];
        for (var lane = 0u; lane < 4u; lane++) {
            let pixel = base_pixel + lane;
            let alpha = coverage_alpha[pixel];
            if alpha > 0u && alpha < 255u {
                store_u8(params.fill_alpha_off + alpha_offset + output_partial, pixel);
                store_u8(
                    params.fill_alpha_off + alpha_offset + partial_count + output_partial,
                    alpha,
                );
                output_partial += 1u;
            }
        }
    }

    if local_id.x < 16u {
        let y = local_id.x;
        let span_offset = compact_meta[2];
        let span_count = compact_meta[3];
        if span_offset + span_count <= params.solid_span_capacity {
            var output_span = span_offsets[y];
            var x = 0u;
            loop {
                if x >= TILE_SIZE {
                    break;
                }
                if coverage_alpha[y * TILE_SIZE + x] != 255u {
                    x += 1u;
                    continue;
                }
                let x0 = x;
                loop {
                    x += 1u;
                    if x >= TILE_SIZE || coverage_alpha[y * TILE_SIZE + x] != 255u {
                        break;
                    }
                }
                let span = params.solid_spans_off + (span_offset + output_span) * 12u;
                store_u32(span, y);
                store_u32(span + 4u, x0);
                store_u32(span + 8u, x);
                output_span += 1u;
            }
        }
    }
    if local_id.x == 0u {
        // Fine consumes compact coverage only; keep CPU readback ranges self-contained.
        store_u32(aux + 4u, 0u);
        store_u32(aux + 8u, 0u);
        store_u32(aux + 12u, rule & 1u);
    }
}

@compute @workgroup_size(64)
fn clear(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let tile_count = params.width_in_tiles * params.height_in_tiles;
    if global_id.x < tile_count {
        clear_tile(global_id.x);
    }
    if global_id.x == 0u {
        store_u32(params.counters_off, 0u);
        store_u32(params.counters_off + 4u, 0u);
        store_u32(params.counters_off + 8u, 0u);
        store_u32(params.counters_off + 12u, 0u);
        atomicStore(&coverage_indirect[0], 0u);
        atomicStore(&coverage_indirect[1], 1u);
        atomicStore(&coverage_indirect[2], 1u);
    }
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let tile_ix = global_id.x;
    let tile_count = params.width_in_tiles * params.height_in_tiles;
    if tile_ix >= tile_count {
        return;
    }
    let tile_x = tile_ix % params.width_in_tiles;
    let tile_y = tile_ix / params.width_in_tiles;
    let tx0 = i32(tile_x * TILE_SIZE);
    let ty0 = i32(tile_y * TILE_SIZE);
    let tx1 = min(tx0 + i32(TILE_SIZE), i32(params.width));
    let ty1 = min(ty0 + i32(TILE_SIZE), i32(params.height));

    var ptcl_count = 0u;
    var segment_start = 0u;
    var segment_count = 0u;
    var group_depth = 0u;
    var clip_depth = 0u;
    var clip_ids: array<u32, MAX_LAYER_DEPTH>;
    var group_ids: array<u32, MAX_LAYER_DEPTH>;
    var touched = false;
    let clip_spill_start = load_u32(params.clip_spill_starts_off + tile_ix * 4u);
    let group_spill_start = load_u32(params.group_spill_starts_off + tile_ix * 4u);

    var clip_segment_base = 0u;
    if params.clip_count > 0u {
        let clip_count = load_u32(params.clip_geom_count_off + tile_ix * 4u);
        let clip_source = load_u32(params.clip_geom_start_off + tile_ix * 4u);
        clip_segment_base = atomic_alloc(0u, clip_count);
        if clip_segment_base + clip_count <= params.segment_capacity {
            for (var i = 0u; i < clip_count; i++) {
                copy_clip_segment(clip_source + i, clip_segment_base + i);
            }
            if clip_count > 0u {
                segment_start = clip_segment_base;
                segment_count = clip_count;
            }
        }
    }

    let tile_draw_start = load_u32(params.tile_draw_starts_off + tile_ix * 4u);
    let tile_draw_end = load_u32(params.tile_draw_starts_off + (tile_ix + 1u) * 4u);
    for (var tile_draw_ix = tile_draw_start; tile_draw_ix < tile_draw_end; tile_draw_ix++) {
        let draw_ix = load_u32(params.tile_draw_indices_off + tile_draw_ix * 4u);
        let draw = params.draws_off + draw_ix * 88u;
        let flags = load_u32(draw + 4u);
        let clip_stack_start = load_u32(draw + 72u);
        let clip_stack_len = load_u32(draw + 76u);
        let group_stack_start = load_u32(draw + 80u);
        let group_stack_len = load_u32(draw + 84u);

        let bx0 = max(load_i32(draw + 8u), tx0);
        let by0 = max(load_i32(draw + 12u), ty0);
        let bx1 = min(load_i32(draw + 16u), tx1);
        let by1 = min(load_i32(draw + 20u), ty1);
        if bx0 >= bx1 || by0 >= by1 {
            continue;
        }

        let path_ix = load_u32(draw);
        var source_start = 0u;
        var source_end = 0u;
        var backdrop = 0;
        var solid_backdrop = false;
        if path_ix != DRAW_PATH_NONE {
            if path_ix >= params.path_count {
                continue;
            }
            let bd = params.backdrops_off + path_ix * 36u;
            let tile_x0 = load_u32(bd + 8u);
            let tile_y0 = load_u32(bd + 12u);
            let tile_x1 = load_u32(bd + 16u);
            let tile_y1 = load_u32(bd + 20u);
            if tile_x < tile_x0 || tile_y < tile_y0 || tile_x >= tile_x1 || tile_y >= tile_y1 {
                continue;
            }
            let local_ix = (tile_y - tile_y0) * (tile_x1 - tile_x0) + tile_x - tile_x0;
            let starts_base = load_u32(bd + 4u) + path_ix;
            source_start = load_u32(params.starts_off + (starts_base + local_ix) * 4u);
            source_end = load_u32(params.starts_off + (starts_base + local_ix + 1u) * 4u);
            backdrop = load_i32(params.backdrop_pool_off + (load_u32(bd + 4u) + local_ix) * 4u);
            solid_backdrop = select(backdrop != 0, (backdrop & 1) != 0, (flags & DRAW_FLAG_EVEN_ODD) != 0u);
            if source_start == source_end && !solid_backdrop {
                continue;
            }
        }

        var shared_clip_depth = 0u;
        loop {
            if shared_clip_depth >= clip_depth || shared_clip_depth >= clip_stack_len {
                break;
            }
            let target_clip_ix = load_u32(
                params.clip_stack_data_off + (clip_stack_start + shared_clip_depth) * 4u,
            );
            var current_clip_ix = 0u;
            if shared_clip_depth < MAX_LAYER_DEPTH {
                current_clip_ix = clip_ids[shared_clip_depth];
            } else {
                current_clip_ix = load_u32(
                    params.coarse_clip_spill_off +
                    (clip_spill_start + shared_clip_depth - MAX_LAYER_DEPTH) * 4u,
                );
            }
            if current_clip_ix != target_clip_ix {
                break;
            }
            shared_clip_depth += 1u;
        }
        while clip_depth > shared_clip_depth {
            push_simple(tile_ix, &ptcl_count, GPU_PTCL_END_CLIP);
            clip_depth -= 1u;
        }
        var shared_group_depth = 0u;
        loop {
            if shared_group_depth >= group_depth || shared_group_depth >= group_stack_len {
                break;
            }
            let target_group_ix = load_u32(
                params.group_stack_data_off + (group_stack_start + shared_group_depth) * 4u,
            );
            var current_group_ix = 0u;
            if shared_group_depth < MAX_LAYER_DEPTH {
                current_group_ix = group_ids[shared_group_depth];
            } else {
                current_group_ix = load_u32(
                    params.coarse_group_spill_off +
                    (group_spill_start + shared_group_depth - MAX_LAYER_DEPTH) * 4u,
                );
            }
            if current_group_ix != target_group_ix {
                break;
            }
            shared_group_depth += 1u;
        }
        while group_depth > shared_group_depth {
            var current_group_ix = 0u;
            if group_depth <= MAX_LAYER_DEPTH {
                current_group_ix = group_ids[group_depth - 1u];
            } else {
                current_group_ix = load_u32(
                    params.coarse_group_spill_off +
                    (group_spill_start + (group_depth - 1u) - MAX_LAYER_DEPTH) * 4u,
                );
            }
            if (current_group_ix & GROUP_STACK_BLEND_BIT) != 0u {
                push_simple(tile_ix, &ptcl_count, GPU_PTCL_END_BLEND);
            } else {
                push_simple(tile_ix, &ptcl_count, GPU_PTCL_END_OPACITY);
            }
            group_depth -= 1u;
        }
        while clip_depth < clip_stack_len {
            let target_clip_ix =
                load_u32(params.clip_stack_data_off + (clip_stack_start + clip_depth) * 4u);
            begin_clip(
                tile_ix, tile_x, tile_y, tx0, ty0, tx1, ty1,
                target_clip_ix, clip_segment_base, &ptcl_count,
            );
            if clip_depth < MAX_LAYER_DEPTH {
                clip_ids[clip_depth] = target_clip_ix;
            } else {
                store_u32(
                    params.coarse_clip_spill_off +
                    (clip_spill_start + clip_depth - MAX_LAYER_DEPTH) * 4u,
                    target_clip_ix,
                );
            }
            clip_depth += 1u;
        }
        while group_depth < group_stack_len {
            let target_group_ix =
                load_u32(params.group_stack_data_off + (group_stack_start + group_depth) * 4u);
            if (target_group_ix & GROUP_STACK_BLEND_BIT) != 0u {
                push_blend(tile_ix, &ptcl_count, target_group_ix & ~GROUP_STACK_BLEND_BIT);
            } else {
                push_opacity(tile_ix, &ptcl_count, target_group_ix);
            }
            if group_depth < MAX_LAYER_DEPTH {
                group_ids[group_depth] = target_group_ix;
            } else {
                store_u32(
                    params.coarse_group_spill_off +
                    (group_spill_start + group_depth - MAX_LAYER_DEPTH) * 4u,
                    target_group_ix,
                );
            }
            group_depth += 1u;
        }

        touched = true;
        let color = load_u32(draw + 24u);
        let brush_kind = load_u32(load_u32(draw + 68u));
        let full_tile = bx0 == tx0 && by0 == ty0 && bx1 == tx1 && by1 == ty1;
        let opaque = (color >> 24u) == 255u;
        let allow_override = (flags & DRAW_FLAG_ALLOW_SOLID_OVERRIDE) != 0u
            && clip_stack_len == 0u
            && group_stack_len == 0u;

        if (flags & DRAW_FLAG_SOLID_RECT) != 0u
            || (path_ix != DRAW_PATH_NONE && source_start == source_end && solid_backdrop)
        {
            if brush_kind == BRUSH_KIND_SOLID && allow_override && opaque && full_tile {
                store_u32(tile_record_off(tile_ix), color);
                ptcl_count = 0u;
                segment_count = load_u32(params.clip_geom_count_off + tile_ix * 4u);
            } else {
                if brush_kind == BRUSH_KIND_SOLID {
                    push_bounds_command(tile_ix, &ptcl_count, GPU_PTCL_COLOR, bx0, by0, bx1, by1, color, draw);
                } else {
                    write_full_fill(
                        tile_ix,
                        &ptcl_count,
                        source_start,
                        source_end,
                        backdrop,
                        (flags & DRAW_FLAG_EVEN_ODD) != 0u,
                        draw,
                    );
                }
            }
        } else if (flags & DRAW_FLAG_HAS_SDF) != 0u {
            push_bounds_command(tile_ix, &ptcl_count, GPU_PTCL_SDF, bx0, by0, bx1, by1, color, draw);
        } else {
            write_full_fill(
                tile_ix,
                &ptcl_count,
                source_start,
                source_end,
                backdrop,
                (flags & DRAW_FLAG_EVEN_ODD) != 0u,
                draw,
            );
        }
    }

    if touched {
        while clip_depth > 0u {
            push_simple(tile_ix, &ptcl_count, GPU_PTCL_END_CLIP);
            clip_depth -= 1u;
        }
        while group_depth > 0u {
            var current_group_ix = 0u;
            if group_depth <= MAX_LAYER_DEPTH {
                current_group_ix = group_ids[group_depth - 1u];
            } else {
                current_group_ix = load_u32(
                    params.coarse_group_spill_off +
                    (group_spill_start + (group_depth - 1u) - MAX_LAYER_DEPTH) * 4u,
                );
            }
            if (current_group_ix & GROUP_STACK_BLEND_BIT) != 0u {
                push_simple(tile_ix, &ptcl_count, GPU_PTCL_END_BLEND);
            } else {
                push_simple(tile_ix, &ptcl_count, GPU_PTCL_END_OPACITY);
            }
            group_depth -= 1u;
        }
        push_simple(tile_ix, &ptcl_count, GPU_PTCL_END);
    }
    set_tile_counts(tile_ix, segment_start, segment_count, ptcl_count, touched);
}
