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

const GPU_SDF_RECT: u32 = 0u;
const GPU_SDF_CIRCLE: u32 = 1u;
const GPU_SDF_RECT_STROKE: u32 = 2u;
const MAX_LAYER_DEPTH: u32 = 16u;
const TILE_PIXELS: u32 = 256u;
const GROUP_SPILL_WORDS_PER_LAYER: u32 = TILE_PIXELS * 4u;

struct FineParams {
    width: u32,
    height: u32,
    width_in_tiles: u32,
    tile_count: u32,
    tiles_off: u32,
    ptcl_off: u32,
    fill_aux_off: u32,
    fill_alpha_off: u32,
    solid_spans_off: u32,
    clip_spill_starts_off: u32,
    group_spill_starts_off: u32,
    clip_spill_off: u32,
    group_spill_off: u32,
    image_off: u32,
    dispatch_row: u32,
    _pad0: u32,
    _pad1: u32,
}

@group(0) @binding(0) var<uniform> params: FineParams;
@group(0) @binding(1) var<storage, read_write> arena: array<atomic<u32>>;

var<workgroup> fill_mask: array<u32, 256>;

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

fn load_u8(byte_offset: u32) -> u32 {
    let value = load_u32(byte_offset & ~3u);
    return (value >> ((byte_offset & 3u) * 8u)) & 255u;
}

fn store_u32(byte_offset: u32, value: u32) {
    atomicStore(&arena[word(byte_offset)], value);
}

fn clip_spill_byte_offset(tile_spill_start: u32, spill_layer: u32, local_ix: u32) -> u32 {
    return params.clip_spill_off + ((tile_spill_start + spill_layer) * TILE_PIXELS + local_ix) * 4u;
}

fn group_spill_byte_offset(tile_spill_start: u32, spill_layer: u32, local_ix: u32, slot: u32) -> u32 {
    return params.group_spill_off +
        ((tile_spill_start + spill_layer) * GROUP_SPILL_WORDS_PER_LAYER + local_ix * 4u + slot) * 4u;
}

fn mul_div255(a: u32, b: u32) -> u32 {
    return (a * b + 127u) / 255u;
}

fn scale_premul_u8(src: u32, factor: u32) -> u32 {
    if factor == 0u {
        return 0u;
    }
    if factor == 255u {
        return src;
    }
    let r = mul_div255(src & 255u, factor);
    let g = mul_div255((src >> 8u) & 255u, factor);
    let b = mul_div255((src >> 16u) & 255u, factor);
    let a = mul_div255(src >> 24u, factor);
    return r | (g << 8u) | (b << 16u) | (a << 24u);
}

fn src_over_u8(dst: u32, src: u32) -> u32 {
    let sa = src >> 24u;
    if sa == 0u {
        return dst;
    }
    if sa == 255u {
        return src;
    }
    let inv = 255u - sa;
    let r = (src & 255u) + mul_div255(dst & 255u, inv);
    let g = ((src >> 8u) & 255u) + mul_div255((dst >> 8u) & 255u, inv);
    let b = ((src >> 16u) & 255u) + mul_div255((dst >> 16u) & 255u, inv);
    let a = sa + mul_div255(dst >> 24u, inv);
    return r | (g << 8u) | (b << 16u) | (a << 24u);
}

fn brush_load_u32(byte_offset: u32) -> u32 {
    return load_u32(byte_offset);
}

fn brush_load_f32(byte_offset: u32) -> f32 {
    return load_f32(byte_offset);
}

fn brush_unpack(px: u32) -> vec4<f32> {
    return unpack(px);
}

fn brush_pack(c: vec4<f32>) -> u32 {
    return pack(c);
}

fn blend_pixel(dst_px: u32, src_px: u32, mode: u32, compose: u32) -> u32 {
    if mode == 0u && compose == 3u {
        return src_over_u8(dst_px, src_px);
    }
    let dst = unpack(dst_px);
    let src = unpack(src_px);
    let sa = clamp(src.w, 0.0, 1.0);
    let da = clamp(dst.w, 0.0, 1.0);
    var src_rgb = vec3<f32>(0.0);
    var dst_rgb = vec3<f32>(0.0);
    if sa > 0.0 { src_rgb = src.xyz / sa; }
    if da > 0.0 { dst_rgb = dst.xyz / da; }
    var effective = src;
    if mode != 0u {
        let mixed = blend_mix(dst_rgb, src_rgb, mode);
        let rgb = sa * ((1.0 - da) * src_rgb + da * mixed);
        effective = vec4<f32>(rgb, sa);
    }
    let factors = compose_factors(compose, sa, da);
    var out = effective * factors.x + dst * factors.y;
    if compose >= 12u {
        out = min(out, vec4<f32>(1.0));
    }
    return pack(out);
}

fn apply_source(dst: u32, color: u32, coverage: u32, mode: u32, compose: u32) -> u32 {
    if coverage == 0u {
        return dst;
    }
    return blend_pixel(dst, scale_premul_u8(color, coverage), mode, compose);
}

fn rect_pixel_coverage(px: vec2<f32>, p0: vec2<f32>, p1: vec2<f32>) -> f32 {
    let lo = min(p0, p1);
    let hi = max(p0, p1);
    let pixel_lo = px - vec2<f32>(0.5);
    let pixel_hi = px + vec2<f32>(0.5);
    let overlap = max(min(pixel_hi, hi) - max(pixel_lo, lo), vec2<f32>(0.0));
    return clamp(overlap.x * overlap.y, 0.0, 1.0);
}

fn sdf_coverage(rec: u32, px: vec2<f32>) -> u32 {
    let sdf = rec + 40u;
    let kind = load_u32(sdf);
    var dist = 1e9;
    if kind == GPU_SDF_CIRCLE {
        let center = vec2<f32>(load_f32(sdf + 4u), load_f32(sdf + 8u));
        dist = distance(px, center) - load_f32(sdf + 12u);
    } else if kind == GPU_SDF_RECT {
        let p0 = vec2<f32>(load_f32(sdf + 4u), load_f32(sdf + 8u));
        let p1 = vec2<f32>(load_f32(sdf + 12u), load_f32(sdf + 16u));
        let lo = min(p0, p1);
        let hi = max(p0, p1);
        let center = (lo + hi) * 0.5;
        let half_size = (hi - lo) * 0.5;
        let local = px - center;
        var radius = load_f32(sdf + 20u);
        if local.x >= 0.0 {
            if local.y <= 0.0 {
                radius = load_f32(sdf + 24u);
            } else {
                radius = load_f32(sdf + 32u);
            }
        } else if local.y > 0.0 {
            radius = load_f32(sdf + 28u);
        }
        radius = clamp(radius, 0.0, min(half_size.x, half_size.y));
        let q = abs(local) - half_size + vec2<f32>(radius);
        dist = min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - radius;
    } else if kind == GPU_SDF_RECT_STROKE {
        let outer0 = vec2<f32>(load_f32(sdf + 4u), load_f32(sdf + 8u));
        let outer1 = vec2<f32>(load_f32(sdf + 12u), load_f32(sdf + 16u));
        let inner0 = vec2<f32>(load_f32(sdf + 20u), load_f32(sdf + 24u));
        let inner1 = vec2<f32>(load_f32(sdf + 28u), load_f32(sdf + 32u));
        let coverage = rect_pixel_coverage(px, outer0, outer1) - rect_pixel_coverage(px, inner0, inner1);
        return u32(clamp(coverage, 0.0, 1.0) * 255.0 + 0.5);
    }
    return u32(clamp(0.5 - dist, 0.0, 1.0) * 255.0 + 0.5);
}

fn build_fill_mask(aux: u32, local_id: u32, generation: u32) {
    let tagged_solid = (generation << 8u) | 255u;
    let span_off = load_u32(aux + 36u);
    let span_len = load_u32(aux + 40u);
    var i = local_id;
    while i < span_len {
        let span = params.solid_spans_off + (span_off + i) * 12u;
        let y = load_u32(span);
        let x0 = load_u32(span + 4u);
        let x1 = load_u32(span + 8u);
        var x = x0;
        while x < x1 {
            fill_mask[y * 16u + x] = tagged_solid;
            x += 1u;
        }
        i += 256u;
    }

    let alpha_idx_off = load_u32(aux + 20u);
    let alpha_len = load_u32(aux + 24u);
    let pixel_alpha_off = load_u32(aux + 28u);
    i = local_id;
    while i < alpha_len {
        let ix = load_u8(params.fill_alpha_off + alpha_idx_off + i);
        let alpha = load_u8(params.fill_alpha_off + pixel_alpha_off + i);
        fill_mask[ix] = (generation << 8u) | alpha;
        i += 256u;
    }
}

fn fill_coverage(local_ix: u32, generation: u32) -> u32 {
    let tagged = fill_mask[local_ix];
    if tagged >> 8u == generation {
        return tagged & 255u;
    }
    return 0u;
}

@compute @workgroup_size(16, 16, 1)
fn main(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id_3d: vec3<u32>,
    @builtin(local_invocation_index) local_ix: u32,
) {
    let tile_ix = workgroup_id.x + workgroup_id.y * params.dispatch_row;
    if tile_ix >= params.tile_count {
        return;
    }
    let tile = params.tiles_off + tile_ix * 28u;
    if load_u32(tile + 24u) == 0u {
        return;
    }

    let tile_x = tile_ix % params.width_in_tiles;
    let tile_y = tile_ix / params.width_in_tiles;
    let pixel_x = tile_x * 16u + local_id_3d.x;
    let pixel_y = tile_y * 16u + local_id_3d.y;
    let valid_pixel = pixel_x < params.width && pixel_y < params.height;
    let image_byte = params.image_off + (pixel_y * params.width + pixel_x) * 4u;
    let bg = load_u32(tile);
    var pixel = 0u;
    if valid_pixel {
        pixel = load_u32(image_byte);
        if bg != GPU_TILE_BG_NONE {
            pixel = bg;
        }
    }

    var current_clip = 255u;
    var clip_stack: array<u32, 16>;
    var clip_depth = 0u;
    var group_parent_stack: array<u32, 16>;
    var group_opacity_stack: array<u32, 16>;
    var group_mix_stack: array<u32, 16>;
    var group_compose_stack: array<u32, 16>;
    var group_depth = 0u;
    let clip_spill_start = load_u32(params.clip_spill_starts_off + tile_ix * 4u);
    let group_spill_start = load_u32(params.group_spill_starts_off + tile_ix * 4u);

    let ptcl_start = load_u32(tile + 16u);
    let ptcl_count = load_u32(tile + 20u);
    var command_ix = 0u;
    loop {
        if command_ix >= ptcl_count {
            break;
        }
        let rec = params.ptcl_off + (ptcl_start + command_ix) * 80u;
        let tag = load_u32(rec);
        let is_fill = tag == GPU_PTCL_FILL || tag == GPU_PTCL_BEGIN_CLIP;
        var fill_aux = 0u;
        let generation = command_ix + 1u;

        // FXC requires barriers to occur in uniform control flow. Every lane executes
        // both barriers for every command, including non-fill and end commands.
        workgroupBarrier();
        if is_fill {
            fill_aux = params.fill_aux_off + load_u32(rec + 4u) * 48u;
            build_fill_mask(fill_aux, local_ix, generation);
        }
        workgroupBarrier();

        if is_fill {
            let shape_alpha = fill_coverage(local_ix, generation);
            if tag == GPU_PTCL_FILL {
                if valid_pixel {
                    let coverage = mul_div255(shape_alpha, current_clip);
                    pixel = apply_source(
                        pixel,
                        sample_brush_u32(
                            load_u32(fill_aux + 44u),
                            vec2<f32>(f32(pixel_x) + 0.5, f32(pixel_y) + 0.5),
                        ),
                        coverage,
                        0u,
                        3u,
                    );
                }
            } else {
                if clip_depth < MAX_LAYER_DEPTH {
                    clip_stack[clip_depth] = current_clip;
                } else {
                    store_u32(
                        clip_spill_byte_offset(
                            clip_spill_start,
                            clip_depth - MAX_LAYER_DEPTH,
                            local_ix,
                        ),
                        current_clip,
                    );
                }
                clip_depth += 1u;
                current_clip = mul_div255(current_clip, shape_alpha);
            }
        } else if tag == GPU_PTCL_SDF || tag == GPU_PTCL_BEGIN_CLIP_SDF {
            var shape_alpha = 0u;
            if valid_pixel {
                shape_alpha = sdf_coverage(rec, vec2<f32>(f32(pixel_x) + 0.5, f32(pixel_y) + 0.5));
                let in_bounds =
                    i32(pixel_x) >= load_i32(rec + 8u) &&
                    i32(pixel_y) >= load_i32(rec + 12u) &&
                    i32(pixel_x) < load_i32(rec + 16u) &&
                    i32(pixel_y) < load_i32(rec + 20u);
                if !in_bounds {
                    shape_alpha = 0u;
                }
            }
            if tag == GPU_PTCL_SDF {
                if valid_pixel {
                    let coverage = mul_div255(shape_alpha, current_clip);
                    pixel = apply_source(
                        pixel,
                        sample_brush_u32(
                            load_u32(rec + 76u),
                            vec2<f32>(f32(pixel_x) + 0.5, f32(pixel_y) + 0.5),
                        ),
                        coverage,
                        0u,
                        3u,
                    );
                }
            } else {
                if clip_depth < MAX_LAYER_DEPTH {
                    clip_stack[clip_depth] = current_clip;
                } else {
                    store_u32(
                        clip_spill_byte_offset(
                            clip_spill_start,
                            clip_depth - MAX_LAYER_DEPTH,
                            local_ix,
                        ),
                        current_clip,
                    );
                }
                clip_depth += 1u;
                current_clip = mul_div255(current_clip, shape_alpha);
            }
        } else if tag == GPU_PTCL_COLOR {
            if valid_pixel {
                let in_bounds =
                    i32(pixel_x) >= load_i32(rec + 8u) &&
                    i32(pixel_y) >= load_i32(rec + 12u) &&
                    i32(pixel_x) < load_i32(rec + 16u) &&
                    i32(pixel_y) < load_i32(rec + 20u);
                if in_bounds {
                    let coverage = current_clip;
                    pixel = apply_source(
                        pixel,
                        load_u32(rec + 24u),
                        coverage,
                        0u,
                        3u,
                    );
                }
            }
        } else if tag == GPU_PTCL_END_CLIP {
            if clip_depth > 0u {
                clip_depth -= 1u;
                if clip_depth < MAX_LAYER_DEPTH {
                    current_clip = clip_stack[clip_depth];
                } else {
                    current_clip = load_u32(
                        clip_spill_byte_offset(
                            clip_spill_start,
                            clip_depth - MAX_LAYER_DEPTH,
                            local_ix,
                        ),
                    );
                }
            }
        } else if tag == GPU_PTCL_BEGIN_OPACITY {
            if group_depth < MAX_LAYER_DEPTH {
                group_parent_stack[group_depth] = pixel;
                group_opacity_stack[group_depth] = load_u32(rec + 28u);
            } else {
                let spill_layer = group_depth - MAX_LAYER_DEPTH;
                store_u32(group_spill_byte_offset(group_spill_start, spill_layer, local_ix, 0u), pixel);
                store_u32(
                    group_spill_byte_offset(group_spill_start, spill_layer, local_ix, 1u),
                    load_u32(rec + 28u),
                );
                store_u32(group_spill_byte_offset(group_spill_start, spill_layer, local_ix, 2u), 0u);
                store_u32(group_spill_byte_offset(group_spill_start, spill_layer, local_ix, 3u), 0u);
            }
            group_depth += 1u;
            pixel = 0u;
        } else if tag == GPU_PTCL_END_OPACITY {
            if group_depth > 0u {
                group_depth -= 1u;
                var parent = 0u;
                var opacity_bits = 0u;
                if group_depth < MAX_LAYER_DEPTH {
                    parent = group_parent_stack[group_depth];
                    opacity_bits = group_opacity_stack[group_depth];
                } else {
                    let spill_layer = group_depth - MAX_LAYER_DEPTH;
                    parent = load_u32(
                        group_spill_byte_offset(group_spill_start, spill_layer, local_ix, 0u),
                    );
                    opacity_bits = load_u32(
                        group_spill_byte_offset(group_spill_start, spill_layer, local_ix, 1u),
                    );
                }
                let opacity = u32(clamp(bitcast<f32>(opacity_bits), 0.0, 1.0) * 255.0 + 0.5);
                pixel = src_over_u8(
                    parent,
                    select(scale_premul_u8(pixel, opacity), pixel, opacity == 255u),
                );
            }
        } else if tag == GPU_PTCL_BEGIN_BLEND {
            if group_depth < MAX_LAYER_DEPTH {
                group_parent_stack[group_depth] = pixel;
                group_mix_stack[group_depth] = load_u32(rec + 32u);
                group_compose_stack[group_depth] = load_u32(rec + 36u);
            } else {
                let spill_layer = group_depth - MAX_LAYER_DEPTH;
                store_u32(group_spill_byte_offset(group_spill_start, spill_layer, local_ix, 0u), pixel);
                store_u32(group_spill_byte_offset(group_spill_start, spill_layer, local_ix, 1u), 0u);
                store_u32(
                    group_spill_byte_offset(group_spill_start, spill_layer, local_ix, 2u),
                    load_u32(rec + 32u),
                );
                store_u32(
                    group_spill_byte_offset(group_spill_start, spill_layer, local_ix, 3u),
                    load_u32(rec + 36u),
                );
            }
            group_depth += 1u;
            pixel = 0u;
        } else if tag == GPU_PTCL_END_BLEND {
            if group_depth > 0u {
                group_depth -= 1u;
                var parent = 0u;
                var mix = 0u;
                var compose = 0u;
                if group_depth < MAX_LAYER_DEPTH {
                    parent = group_parent_stack[group_depth];
                    mix = group_mix_stack[group_depth];
                    compose = group_compose_stack[group_depth];
                } else {
                    let spill_layer = group_depth - MAX_LAYER_DEPTH;
                    parent = load_u32(
                        group_spill_byte_offset(group_spill_start, spill_layer, local_ix, 0u),
                    );
                    mix = load_u32(
                        group_spill_byte_offset(group_spill_start, spill_layer, local_ix, 2u),
                    );
                    compose = load_u32(
                        group_spill_byte_offset(group_spill_start, spill_layer, local_ix, 3u),
                    );
                }
                pixel = blend_pixel(parent, pixel, mix, compose);
            }
        }
        command_ix += 1u;
    }

    if valid_pixel {
        store_u32(image_byte, pixel);
    }
}
