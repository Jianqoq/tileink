use ::cubecl::prelude::*;

use crate::cubecl::{
    brush::{
        GPU_BRUSH_FOUR_CORNER, GPU_BRUSH_LINEAR, GPU_BRUSH_PARAM_STRIDE, GPU_BRUSH_PATTERN,
        GPU_BRUSH_RADIAL, GPU_BRUSH_SWEEP, GPU_BRUSH_U32_STRIDE, GPU_EXTEND_REFLECT,
        GPU_EXTEND_REPEAT, GpuBrushResources,
    },
    buffer::CubeBuffer,
    renderer::{CoarseBuffers, ScanBuffers},
    types::{
        CUBE_PTCL_BEGIN_BLEND, CUBE_PTCL_BEGIN_CLIP, CUBE_PTCL_BEGIN_OPACITY, CUBE_PTCL_COLOR,
        CUBE_PTCL_END, CUBE_PTCL_END_BLEND, CUBE_PTCL_END_CLIP, CUBE_PTCL_END_OPACITY,
        CUBE_PTCL_FILL, CubeBufferLengths,
    },
};

const FINE_WORKGROUP_SIZE: u32 = 256;

#[derive(Clone, Copy)]
pub(crate) struct FineRenderConfig {
    pub(crate) lengths: CubeBufferLengths,
    pub(crate) size: (u32, u32),
    pub(crate) clear_color: u32,
    pub(crate) max_clip_depth: usize,
    pub(crate) max_group_depth: usize,
}

pub(crate) struct FinePipeline;

impl FinePipeline {
    pub(crate) fn run<R: Runtime>(
        client: &ComputeClient<R>,
        scan: &ScanBuffers,
        coarse: &CoarseBuffers,
        brushes: GpuBrushResources<'_>,
        target: &mut CubeBuffer<u32>,
        config: FineRenderConfig,
    ) {
        Self::clear(client, target, config.lengths, config.clear_color);
        Self::render(client, scan, coarse, brushes, target, config);
    }

    pub(crate) fn clear<R: Runtime>(
        client: &ComputeClient<R>,
        target: &mut CubeBuffer<u32>,
        lengths: CubeBufferLengths,
        clear_color: u32,
    ) {
        let image_pixels = lengths.image_pixels as u32;
        if image_pixels == 0 {
            return;
        }

        fine_clear::launch::<R>(
            client,
            cube_count(image_pixels),
            CubeDim::new_1d(FINE_WORKGROUP_SIZE),
            image_pixels,
            clear_color,
            unsafe { target.arg() },
        );
    }

    pub(crate) fn render<R: Runtime>(
        client: &ComputeClient<R>,
        scan: &ScanBuffers,
        coarse: &CoarseBuffers,
        brushes: GpuBrushResources<'_>,
        target: &mut CubeBuffer<u32>,
        config: FineRenderConfig,
    ) {
        let lengths = config.lengths;
        let tile_count = lengths.tile_count as u32;
        if tile_count == 0 || lengths.coarse_ptcl_capacity == 0 {
            return;
        }

        fine_render::launch::<R>(
            client,
            CubeCount::Static(tile_count, 1, 1),
            CubeDim::new_1d(FINE_WORKGROUP_SIZE),
            FINE_WORKGROUP_SIZE as usize,
            config.max_clip_depth.max(1),
            config.max_group_depth.max(1),
            tile_count,
            lengths.tiles_width as u32,
            lengths.tiles_height as u32,
            lengths.image_pixels as u32,
            config.size.0,
            config.size.1,
            unsafe { coarse.tile_ptcl_range_starts.arg() },
            unsafe { coarse.tile_ptcl_range_ends.arg() },
            unsafe { coarse.ptcl_tags.arg() },
            unsafe { coarse.ptcl_backdrops.arg() },
            unsafe { coarse.ptcl_fill_rules.arg() },
            unsafe { coarse.ptcl_segment_starts.arg() },
            unsafe { coarse.ptcl_segment_ends.arg() },
            unsafe { coarse.ptcl_colors.arg() },
            unsafe { scan.segment_p0x.arg() },
            unsafe { scan.segment_p0y.arg() },
            unsafe { scan.segment_p1x.arg() },
            unsafe { scan.segment_p1y.arg() },
            unsafe { scan.segment_y_edge.arg() },
            unsafe { brushes.data.arg() },
            unsafe { brushes.params.arg() },
            unsafe { brushes.payloads.arg() },
            unsafe { target.arg() },
        );
    }
}

fn cube_count(items: u32) -> CubeCount {
    CubeCount::Static(items.div_ceil(FINE_WORKGROUP_SIZE), 1, 1)
}

#[cube(launch)]
fn fine_clear(image_pixels: u32, clear_color: u32, target: &mut Array<u32>) {
    let ix = ABSOLUTE_POS as u32;
    if ix >= image_pixels {
        terminate!();
    }
    target[ix as usize] = clear_color;
}

#[cube(launch)]
fn fine_render(
    #[comptime] workgroup_size: usize,
    #[comptime] clip_stack_capacity: usize,
    #[comptime] group_stack_capacity: usize,
    tile_count: u32,
    tiles_width: u32,
    tiles_height: u32,
    image_pixels: u32,
    image_width: u32,
    image_height: u32,
    tile_range_starts: &Array<u32>,
    tile_range_ends: &Array<u32>,
    ptcl_tags: &Array<u32>,
    ptcl_backdrops: &Array<i32>,
    ptcl_fill_rules: &Array<u32>,
    ptcl_segment_starts: &Array<u32>,
    ptcl_segment_ends: &Array<u32>,
    ptcl_colors: &Array<u32>,
    segment_p0x: &Array<f32>,
    segment_p0y: &Array<f32>,
    segment_p1x: &Array<f32>,
    segment_p1y: &Array<f32>,
    segment_y_edge: &Array<f32>,
    brush_data: &Array<u32>,
    brush_params: &Array<f32>,
    brush_payloads: &Array<u32>,
    target: &mut Array<u32>,
) {
    let tile_ix = CUBE_POS as u32;
    if tile_ix >= tile_count {
        terminate!();
    }

    let local_ix = UNIT_POS;
    let local_x = local_ix % 16;
    let local_y = local_ix / 16;
    let tile_x = tile_ix % tiles_width;
    let tile_y = tile_ix / tiles_width;
    if tile_y >= tiles_height {
        terminate!();
    }

    let global_x = tile_x * 16 + local_x;
    let global_y = tile_y * 16 + local_y;
    if global_x >= image_width || global_y >= image_height {
        terminate!();
    }

    let target_ix = global_y * image_width + global_x;
    if target_ix >= image_pixels {
        terminate!();
    }

    let mut pixel = target[target_ix as usize];
    let mut clip_mask = 255u32;
    let mut clip_depth = 0u32;
    let mut group_depth = 0u32;
    let mut clip_stack = SharedMemory::<u32>::new(workgroup_size * clip_stack_capacity);
    let mut group_kinds = SharedMemory::<u32>::new(workgroup_size * group_stack_capacity);
    let mut group_parent_pixels = SharedMemory::<u32>::new(workgroup_size * group_stack_capacity);
    let mut group_parent_clips = SharedMemory::<u32>::new(workgroup_size * group_stack_capacity);
    let mut group_layer_alphas = SharedMemory::<u32>::new(workgroup_size * group_stack_capacity);
    let mut group_payloads = SharedMemory::<u32>::new(workgroup_size * group_stack_capacity);
    let mut ptcl_ix = tile_range_starts[tile_ix as usize];
    let range_end = tile_range_ends[tile_ix as usize];

    while ptcl_ix < range_end {
        let ptcl_i = ptcl_ix as usize;
        let tag = ptcl_tags[ptcl_i];
        if tag == CUBE_PTCL_END {
            ptcl_ix = range_end;
        } else {
            if tag == CUBE_PTCL_COLOR {
                pixel = src_over_premul_u8(pixel, scale_premul_u8(ptcl_colors[ptcl_i], clip_mask));
            } else if tag == CUBE_PTCL_END_CLIP {
                if clip_depth > 0 {
                    clip_depth -= 1;
                    let stack_ix = (clip_depth * workgroup_size as u32 + UNIT_POS) as usize;
                    clip_mask = clip_stack[stack_ix];
                } else {
                    clip_mask = 255;
                }
            } else if tag == CUBE_PTCL_END_OPACITY || tag == CUBE_PTCL_END_BLEND {
                if group_depth > 0 {
                    group_depth -= 1;
                    let stack_ix = (group_depth * workgroup_size as u32 + UNIT_POS) as usize;
                    let parent = group_parent_pixels[stack_ix];
                    let parent_clip = group_parent_clips[stack_ix];
                    let layer_alpha = group_layer_alphas[stack_ix];
                    let payload = group_payloads[stack_ix];
                    let group_kind = group_kinds[stack_ix];
                    let mut alpha = combine_alpha(layer_alpha, parent_clip);
                    if group_kind == CUBE_PTCL_BEGIN_OPACITY {
                        alpha = combine_alpha(alpha, payload);
                        pixel = src_over_premul_u8(parent, scale_premul_u8(pixel, alpha));
                    } else {
                        let src = scale_premul_u8(pixel, alpha);
                        if src >> 24 == 0 {
                            pixel = parent;
                        } else {
                            pixel = blend_premul_u8(parent, src, payload);
                        }
                    }
                }
            } else if tag == CUBE_PTCL_FILL
                || tag == CUBE_PTCL_BEGIN_CLIP
                || tag == CUBE_PTCL_BEGIN_OPACITY
                || tag == CUBE_PTCL_BEGIN_BLEND
            {
                let alpha = fill_alpha_at(
                    ptcl_backdrops[ptcl_i],
                    ptcl_fill_rules[ptcl_i],
                    ptcl_segment_starts[ptcl_i],
                    ptcl_segment_ends[ptcl_i],
                    local_x,
                    local_y,
                    segment_p0x,
                    segment_p0y,
                    segment_p1x,
                    segment_p1y,
                    segment_y_edge,
                );
                if tag == CUBE_PTCL_BEGIN_CLIP {
                    if clip_depth < clip_stack_capacity as u32 {
                        let stack_ix = (clip_depth * workgroup_size as u32 + UNIT_POS) as usize;
                        clip_stack[stack_ix] = clip_mask;
                        clip_depth += 1;
                    }
                    clip_mask = combine_alpha(clip_mask, alpha);
                } else if tag == CUBE_PTCL_BEGIN_OPACITY || tag == CUBE_PTCL_BEGIN_BLEND {
                    if group_depth < group_stack_capacity as u32 {
                        let stack_ix = (group_depth * workgroup_size as u32 + UNIT_POS) as usize;
                        group_kinds[stack_ix] = tag;
                        group_parent_pixels[stack_ix] = pixel;
                        group_parent_clips[stack_ix] = clip_mask;
                        group_layer_alphas[stack_ix] = alpha;
                        group_payloads[stack_ix] = ptcl_colors[ptcl_i];
                        group_depth += 1;
                        pixel = 0;
                    }
                } else {
                    let alpha = combine_alpha(alpha, clip_mask);
                    let color = sample_brush(
                        ptcl_colors[ptcl_i],
                        global_x as f32 + 0.5,
                        global_y as f32 + 0.5,
                        brush_data,
                        brush_params,
                        brush_payloads,
                    );
                    let src = scale_premul_u8(color, alpha);
                    pixel = src_over_premul_u8(pixel, src);
                }
            }
            ptcl_ix += 1;
        }
    }

    target[target_ix as usize] = pixel;
}

#[cube]
fn fill_alpha_at(
    backdrop: i32,
    fill_rule: u32,
    segment_start: u32,
    segment_end: u32,
    x: u32,
    y: u32,
    segment_p0x: &Array<f32>,
    segment_p0y: &Array<f32>,
    segment_p1x: &Array<f32>,
    segment_p1y: &Array<f32>,
    segment_y_edge: &Array<f32>,
) -> u32 {
    let mut coverage = backdrop as f32;
    let mut segment_ix = segment_start;
    while segment_ix < segment_end {
        let i = segment_ix as usize;
        coverage += segment_coverage_at(
            segment_p0x[i],
            segment_p0y[i],
            segment_p1x[i],
            segment_p1y[i],
            segment_y_edge[i],
            x,
            y,
        );
        segment_ix += 1;
    }
    coverage_to_alpha(coverage, fill_rule)
}

#[cube]
fn segment_coverage_at(p0x: f32, p0y: f32, p1x: f32, p1y: f32, y_edge: f32, x: u32, y: u32) -> f32 {
    let delta_x = p1x - p0x;
    let delta_y = p1y - p0y;
    let row_y = y as f32;
    let local_y = p0y - row_y;
    let y0 = local_y.clamp(0.0, 1.0);
    let y1 = (local_y + delta_y).clamp(0.0, 1.0);
    let dy = y0 - y1;
    let x_sign = signum_f32(delta_x);
    let mut coverage = x_sign * (row_y - y_edge + 1.0).clamp(0.0, 1.0);

    if dy != 0.0 {
        let recip = 1.0 / delta_y;
        let t0 = (y0 - local_y) * recip;
        let t1 = (y1 - local_y) * recip;
        let sx0 = p0x + t0 * delta_x;
        let sx1 = p0x + t1 * delta_x;
        let pixel_x = x as f32;
        let xmin = sx0.min(sx1) - pixel_x;
        let xmax = sx0.max(sx1) - pixel_x;
        let a_min = xmin.min(1.0) - f32::new(0.000001_f32);
        let b = xmax.min(1.0);
        let c = b.max(0.0);
        let d = a_min.max(0.0);
        let area = (b + f32::new(0.5_f32) * (d * d - c * c) - a_min) / (xmax - a_min);
        coverage += area * dy;
    }

    coverage
}

#[cube]
fn signum_f32(value: f32) -> f32 {
    // Match CPU f32::signum semantics for coverage: vertical edges add no y-edge term.
    let mut out = 0.0;
    if value > 0.0 {
        out = 1.0;
    } else if value < 0.0 {
        out = -1.0;
    }
    out
}

#[cube]
fn coverage_to_alpha(value: f32, fill_rule: u32) -> u32 {
    let mut alpha = value.abs().min(1.0);
    if fill_rule == 1 {
        alpha = (value - f32::new(2.0_f32) * (f32::new(0.5_f32) * value).round()).abs();
    }
    (alpha.clamp(0.0, 1.0) * 255.0 + 0.5) as u32
}

#[cube]
fn sample_brush(
    brush_index: u32,
    x: f32,
    y: f32,
    brush_data: &Array<u32>,
    brush_params: &Array<f32>,
    brush_payloads: &Array<u32>,
) -> u32 {
    let data_base = (brush_index * GPU_BRUSH_U32_STRIDE as u32) as usize;
    let kind = brush_data[data_base];
    let extend = brush_data[data_base + 1];
    let payload_offset = brush_data[data_base + 2];
    let payload_len = brush_data[data_base + 3];
    let base = (brush_index * GPU_BRUSH_PARAM_STRIDE as u32) as usize;
    let mut color = brush_data[data_base + 4];

    if kind == GPU_BRUSH_LINEAR {
        let sx = brush_params[base];
        let sy = brush_params[base + 1];
        let ex = brush_params[base + 2];
        let ey = brush_params[base + 3];
        let dx = ex - sx;
        let dy = ey - sy;
        let denominator = dx * dx + dy * dy;
        let mut t = 0.0;
        if denominator > f32::new(0.000_000_119_209_29_f32) {
            t = ((x - sx) * dx + (y - sy) * dy) / denominator;
        }
        color = sample_ramp(brush_payloads, payload_offset, payload_len, t, extend);
    } else if kind == GPU_BRUSH_RADIAL {
        color = sample_radial(
            x,
            y,
            base,
            extend,
            payload_offset,
            payload_len,
            brush_params,
            brush_payloads,
        );
    } else if kind == GPU_BRUSH_SWEEP {
        let cx = brush_params[base];
        let cy = brush_params[base + 1];
        let start_angle = brush_params[base + 2];
        let end_angle = brush_params[base + 3];
        let span = end_angle - start_angle;
        let mut t = 0.0;
        if span.abs() > f32::new(0.000_000_119_209_29_f32) {
            let tau = f32::new(6.283_185_5_f32);
            let mut angle = (y - cy).atan2(x - cx);
            if span > 0.0 {
                while angle < start_angle {
                    angle += tau;
                }
            } else {
                while angle > start_angle {
                    angle -= tau;
                }
            }
            t = (angle - start_angle) / span;
        }
        color = sample_ramp(brush_payloads, payload_offset, payload_len, t, extend);
    } else if kind == GPU_BRUSH_FOUR_CORNER {
        color = sample_four_corner(x, y, base, payload_offset, brush_params, brush_payloads);
    } else if kind == GPU_BRUSH_PATTERN {
        color = sample_pattern(
            x,
            y,
            base,
            payload_offset,
            payload_len,
            brush_data[data_base + 5],
            brush_data[data_base + 6],
            brush_data[data_base + 7],
            brush_params,
            brush_payloads,
        );
    }

    color
}

#[cube]
fn sample_radial(
    x: f32,
    y: f32,
    base: usize,
    extend: u32,
    payload_offset: u32,
    payload_len: u32,
    brush_params: &Array<f32>,
    brush_payloads: &Array<u32>,
) -> u32 {
    let tx = brush_params[base + 6] * x + brush_params[base + 8] * y + brush_params[base + 10];
    let ty = brush_params[base + 7] * x + brush_params[base + 9] * y + brush_params[base + 11];
    let sx = brush_params[base];
    let sy = brush_params[base + 1];
    let ex = brush_params[base + 2];
    let ey = brush_params[base + 3];
    let start_radius = brush_params[base + 4];
    let end_radius = brush_params[base + 5];
    let qx = tx - sx;
    let qy = ty - sy;
    let dcx = ex - sx;
    let dcy = ey - sy;
    let dr = end_radius - start_radius;
    let a = dcx * dcx + dcy * dcy - dr * dr;
    let b = -2.0 * (qx * dcx + qy * dcy + start_radius * dr);
    let c = qx * qx + qy * qy - start_radius * start_radius;
    let mut has_t = false;
    let mut t = 0.0;

    if a.abs() <= f32::new(0.000001_f32) {
        if b.abs() > f32::new(0.000001_f32) {
            let candidate = -c / b;
            if start_radius + candidate * dr >= 0.0 {
                has_t = true;
                t = candidate;
            }
        }
    } else {
        let discriminant = b * b - 4.0 * a * c;
        if discriminant >= 0.0 {
            let root = discriminant.sqrt();
            let t0 = (-b - root) / (2.0 * a);
            let t1 = (-b + root) / (2.0 * a);
            let valid0 = start_radius + t0 * dr >= 0.0;
            let valid1 = start_radius + t1 * dr >= 0.0;
            if valid0 {
                has_t = true;
                if valid1 {
                    t = t0.max(t1);
                } else {
                    t = t0;
                }
            } else if valid1 {
                has_t = true;
                t = t1;
            }
        }
    }

    let mut color = 0u32;
    if has_t {
        color = sample_ramp(brush_payloads, payload_offset, payload_len, t, extend);
    }
    color
}

#[cube]
fn sample_four_corner(
    x: f32,
    y: f32,
    base: usize,
    payload_offset: u32,
    brush_params: &Array<f32>,
    brush_payloads: &Array<u32>,
) -> u32 {
    let x0 = brush_params[base];
    let y0 = brush_params[base + 1];
    let x1 = brush_params[base + 2];
    let y1 = brush_params[base + 3];
    let width = x1 - x0;
    let height = y1 - y0;
    let mut u = 0.0;
    let mut v = 0.0;
    if width.abs() > f32::new(0.000_000_119_209_29_f32) {
        u = ((x - x0) / width).clamp(0.0, 1.0);
    }
    if height.abs() > f32::new(0.000_000_119_209_29_f32) {
        v = ((y - y0) / height).clamp(0.0, 1.0);
    }
    let tl = brush_payloads[payload_offset as usize];
    let tr = brush_payloads[(payload_offset + 1) as usize];
    let br = brush_payloads[(payload_offset + 2) as usize];
    let bl = brush_payloads[(payload_offset + 3) as usize];
    let top = lerp_premul_u8(tl, tr, u);
    let bottom = lerp_premul_u8(bl, br, u);
    lerp_premul_u8(top, bottom, v)
}

#[cube]
fn sample_pattern(
    x: f32,
    y: f32,
    base: usize,
    payload_offset: u32,
    payload_len: u32,
    width: u32,
    height: u32,
    opacity: u32,
    brush_params: &Array<f32>,
    brush_payloads: &Array<u32>,
) -> u32 {
    let mut color = 0u32;
    if payload_len > 0 && width > 0 && height > 0 {
        let tx = brush_params[base] * x + brush_params[base + 2] * y + brush_params[base + 4];
        let ty = brush_params[base + 1] * x + brush_params[base + 3] * y + brush_params[base + 5];
        let local_x = repeat_coord_i32(tx.floor() as i32, width);
        let local_y = repeat_coord_i32(ty.floor() as i32, height);
        let local_ix = (local_y * width + local_x).min(payload_len - 1);
        color = scale_premul_u8(
            brush_payloads[(payload_offset + local_ix) as usize],
            opacity,
        );
    }
    color
}

#[cube]
fn sample_ramp(
    brush_payloads: &Array<u32>,
    payload_offset: u32,
    payload_len: u32,
    t: f32,
    extend: u32,
) -> u32 {
    let mut color = 0u32;
    if payload_len > 0 {
        let last = payload_len - 1;
        let position = apply_extend(t, extend) * last as f32;
        let left_ix = position.floor() as u32;
        let right_ix = (left_ix + 1).min(last);
        let frac = position - left_ix as f32;
        let left = brush_payloads[(payload_offset + left_ix) as usize];
        let right = brush_payloads[(payload_offset + right_ix) as usize];
        if frac <= f32::new(0.000_000_119_209_29_f32) || left_ix == right_ix {
            color = left;
        } else {
            color = lerp_premul_u8(left, right, frac);
        }
    }
    color
}

#[cube]
fn apply_extend(t: f32, extend: u32) -> f32 {
    let mut out = t.clamp(0.0, 1.0);
    if extend == GPU_EXTEND_REPEAT {
        out = rem_euclid_f32(t, 1.0);
    } else if extend == GPU_EXTEND_REFLECT {
        let value = rem_euclid_f32(t, 2.0);
        if value <= 1.0 {
            out = value;
        } else {
            out = 2.0 - value;
        }
    }
    out
}

#[cube]
fn rem_euclid_f32(value: f32, modulus: f32) -> f32 {
    value - (value / modulus).floor() * modulus
}

#[cube]
fn repeat_coord_i32(value: i32, size: u32) -> u32 {
    let size_i = size as i32;
    let mut out = value % size_i;
    if out < 0 {
        out += size_i;
    }
    out as u32
}

#[cube]
fn lerp_premul_u8(a: u32, b: u32, t: f32) -> u32 {
    let inv = 1.0 / 255.0;
    let ar = (a & 255) as f32 * inv;
    let ag = ((a >> 8) & 255) as f32 * inv;
    let ab = ((a >> 16) & 255) as f32 * inv;
    let aa = ((a >> 24) & 255) as f32 * inv;
    let br = (b & 255) as f32 * inv;
    let bg = ((b >> 8) & 255) as f32 * inv;
    let bb = ((b >> 16) & 255) as f32 * inv;
    let ba = ((b >> 24) & 255) as f32 * inv;
    pack_premul_rgba8(
        ar + (br - ar) * t,
        ag + (bg - ag) * t,
        ab + (bb - ab) * t,
        aa + (ba - aa) * t,
    )
}

#[cube]
fn combine_alpha(a: u32, b: u32) -> u32 {
    (a * b + 127) / 255
}

#[cube]
fn scale_premul_u8(src: u32, factor: u32) -> u32 {
    let mut out = 0u32;
    if factor != 0 {
        if factor == 255 {
            out = src;
        } else {
            out = mul_div255(src & 255, factor)
                | (mul_div255((src >> 8) & 255, factor) << 8)
                | (mul_div255((src >> 16) & 255, factor) << 16)
                | (mul_div255((src >> 24) & 255, factor) << 24);
        }
    }
    out
}

#[cube]
fn src_over_premul_u8(dst: u32, src: u32) -> u32 {
    let sa = src >> 24;
    let mut out = dst;
    if sa != 0 {
        if sa == 255 {
            out = src;
        } else {
            let inv = 255 - sa;
            let r = (src & 255) + mul_div255(dst & 255, inv);
            let g = ((src >> 8) & 255) + mul_div255((dst >> 8) & 255, inv);
            let b = ((src >> 16) & 255) + mul_div255((dst >> 16) & 255, inv);
            let a = sa + mul_div255((dst >> 24) & 255, inv);
            out = r | (g << 8) | (b << 16) | (a << 24);
        }
    }
    out
}

#[cube]
fn blend_premul_u8(dst: u32, src: u32, mode: u32) -> u32 {
    let mix = mode & 255;
    let compose = (mode >> 8) & 255;
    let inv = 1.0 / 255.0;
    let sr = (src & 255) as f32 * inv;
    let sg = ((src >> 8) & 255) as f32 * inv;
    let sb = ((src >> 16) & 255) as f32 * inv;
    let sa = ((src >> 24) & 255) as f32 * inv;
    let dr = (dst & 255) as f32 * inv;
    let dg = ((dst >> 8) & 255) as f32 * inv;
    let db = ((dst >> 16) & 255) as f32 * inv;
    let da = ((dst >> 24) & 255) as f32 * inv;

    let mut out_r = sr + dr * (1.0 - sa);
    let mut out_g = sg + dg * (1.0 - sa);
    let mut out_b = sb + db * (1.0 - sa);
    let mut out_a = sa + da * (1.0 - sa);

    if mix == 0 && compose == 3 {
    } else if mix == 0 && compose == 2 {
        out_r = dr;
        out_g = dg;
        out_b = db;
        out_a = da;
    } else if mix == 0 && compose == 0 {
        out_r = 0.0;
        out_g = 0.0;
        out_b = 0.0;
        out_a = 0.0;
    } else if mix == 0 && compose == 1 {
        out_r = sr;
        out_g = sg;
        out_b = sb;
        out_a = sa;
    } else if mix == 0 {
        let src_factor = compose_src_factor(compose, sa, da);
        let dst_factor = compose_dst_factor(compose, sa, da);
        out_r = sr * src_factor + dr * dst_factor;
        out_g = sg * src_factor + dg * dst_factor;
        out_b = sb * src_factor + db * dst_factor;
        out_a = sa * src_factor + da * dst_factor;
        if compose == 13 {
            out_r = out_r.min(1.0);
            out_g = out_g.min(1.0);
            out_b = out_b.min(1.0);
            out_a = out_a.min(1.0);
        }
    } else if compose == 3 && mix == 6 {
        out_r = color_dodge_premul(sr, dr, sa, da);
        out_g = color_dodge_premul(sg, dg, sa, da);
        out_b = color_dodge_premul(sb, db, sa, da);
        out_a = sa + da * (1.0 - sa);
    } else if compose == 3 && mix == 7 {
        out_r = color_burn_premul(sr, dr, sa, da);
        out_g = color_burn_premul(sg, dg, sa, da);
        out_b = color_burn_premul(sb, db, sa, da);
        out_a = sa + da * (1.0 - sa);
    } else {
        let src_alpha = sa.clamp(0.0, 1.0);
        let dst_alpha = da.clamp(0.0, 1.0);
        let src_r = unpremul_channel(sr, src_alpha);
        let src_g = unpremul_channel(sg, src_alpha);
        let src_b = unpremul_channel(sb, src_alpha);
        let dst_r = unpremul_channel(dr, dst_alpha);
        let dst_g = unpremul_channel(dg, dst_alpha);
        let dst_b = unpremul_channel(db, dst_alpha);
        let mixed_r = mix_rgb_channel(dst_r, dst_g, dst_b, src_r, src_g, src_b, mix, 0);
        let mixed_g = mix_rgb_channel(dst_r, dst_g, dst_b, src_r, src_g, src_b, mix, 1);
        let mixed_b = mix_rgb_channel(dst_r, dst_g, dst_b, src_r, src_g, src_b, mix, 2);
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

    pack_premul_rgba8(out_r, out_g, out_b, out_a)
}

#[cube]
fn compose_src_factor(compose: u32, _src_alpha: f32, dst_alpha: f32) -> f32 {
    let mut factor = 1.0;
    if compose == 0 || compose == 2 || compose == 6 || compose == 8 {
        factor = 0.0;
    } else if compose == 4 {
        factor = 1.0 - dst_alpha;
    } else if compose == 5 || compose == 9 {
        factor = dst_alpha;
    } else if compose == 7 || compose == 10 || compose == 11 {
        factor = 1.0 - dst_alpha;
    }
    factor
}

#[cube]
fn compose_dst_factor(compose: u32, src_alpha: f32, _dst_alpha: f32) -> f32 {
    let mut factor = 1.0 - src_alpha;
    if compose == 0 || compose == 1 || compose == 5 || compose == 7 {
        factor = 0.0;
    } else if compose == 2 || compose == 4 {
        factor = 1.0;
    } else if compose == 6 || compose == 10 {
        factor = src_alpha;
    } else if compose == 8 || compose == 9 || compose == 11 {
        factor = 1.0 - src_alpha;
    } else if compose == 12 || compose == 13 {
        factor = 1.0;
    }
    factor
}

#[cube]
fn unpremul_channel(value: f32, alpha: f32) -> f32 {
    let mut out = 0.0;
    if alpha > 0.0 {
        out = value / alpha;
    }
    out
}

#[cube]
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
    let mut r = src_r;
    let mut g = src_g;
    let mut b = src_b;
    if mix == 1 {
        r = dst_r * src_r;
        g = dst_g * src_g;
        b = dst_b * src_b;
    } else if mix == 2 {
        r = dst_r + src_r - dst_r * src_r;
        g = dst_g + src_g - dst_g * src_g;
        b = dst_b + src_b - dst_b * src_b;
    } else if mix == 3 {
        r = overlay(dst_r, src_r);
        g = overlay(dst_g, src_g);
        b = overlay(dst_b, src_b);
    } else if mix == 4 {
        r = dst_r.min(src_r);
        g = dst_g.min(src_g);
        b = dst_b.min(src_b);
    } else if mix == 5 {
        r = dst_r.max(src_r);
        g = dst_g.max(src_g);
        b = dst_b.max(src_b);
    } else if mix == 6 {
        r = color_dodge(dst_r, src_r);
        g = color_dodge(dst_g, src_g);
        b = color_dodge(dst_b, src_b);
    } else if mix == 7 {
        r = color_burn(dst_r, src_r);
        g = color_burn(dst_g, src_g);
        b = color_burn(dst_b, src_b);
    } else if mix == 8 {
        r = overlay(src_r, dst_r);
        g = overlay(src_g, dst_g);
        b = overlay(src_b, dst_b);
    } else if mix == 9 {
        r = soft_light(dst_r, src_r);
        g = soft_light(dst_g, src_g);
        b = soft_light(dst_b, src_b);
    } else if mix == 10 {
        r = (dst_r - src_r).abs();
        g = (dst_g - src_g).abs();
        b = (dst_b - src_b).abs();
    } else if mix == 11 {
        r = dst_r + src_r - 2.0 * dst_r * src_r;
        g = dst_g + src_g - 2.0 * dst_g * src_g;
        b = dst_b + src_b - 2.0 * dst_b * src_b;
    } else if mix == 12 {
        let sat_dst = sat3(dst_r, dst_g, dst_b);
        let lum_dst = lum3(dst_r, dst_g, dst_b);
        let sr = set_sat_channel(src_r, src_g, src_b, sat_dst, 0);
        let sg = set_sat_channel(src_r, src_g, src_b, sat_dst, 1);
        let sb = set_sat_channel(src_r, src_g, src_b, sat_dst, 2);
        r = set_lum_channel(sr, sg, sb, lum_dst, 0);
        g = set_lum_channel(sr, sg, sb, lum_dst, 1);
        b = set_lum_channel(sr, sg, sb, lum_dst, 2);
    } else if mix == 13 {
        let sat_src = sat3(src_r, src_g, src_b);
        let lum_dst = lum3(dst_r, dst_g, dst_b);
        let dr = set_sat_channel(dst_r, dst_g, dst_b, sat_src, 0);
        let dg = set_sat_channel(dst_r, dst_g, dst_b, sat_src, 1);
        let db = set_sat_channel(dst_r, dst_g, dst_b, sat_src, 2);
        r = set_lum_channel(dr, dg, db, lum_dst, 0);
        g = set_lum_channel(dr, dg, db, lum_dst, 1);
        b = set_lum_channel(dr, dg, db, lum_dst, 2);
    } else if mix == 14 {
        let lum_dst = lum3(dst_r, dst_g, dst_b);
        r = set_lum_channel(src_r, src_g, src_b, lum_dst, 0);
        g = set_lum_channel(src_r, src_g, src_b, lum_dst, 1);
        b = set_lum_channel(src_r, src_g, src_b, lum_dst, 2);
    } else if mix == 15 {
        let lum_src = lum3(src_r, src_g, src_b);
        r = set_lum_channel(dst_r, dst_g, dst_b, lum_src, 0);
        g = set_lum_channel(dst_r, dst_g, dst_b, lum_src, 1);
        b = set_lum_channel(dst_r, dst_g, dst_b, lum_src, 2);
    }

    if channel == 0 {
        r
    } else if channel == 1 {
        g
    } else {
        b
    }
}

#[cube]
fn overlay(dst: f32, src: f32) -> f32 {
    if dst <= 0.5 {
        2.0 * dst * src
    } else {
        1.0 - 2.0 * (1.0 - dst) * (1.0 - src)
    }
}

#[cube]
fn color_dodge(dst: f32, src: f32) -> f32 {
    let mut out = 1.0;
    if src < 1.0 {
        out = (dst / (1.0 - src)).min(1.0);
    }
    out
}

#[cube]
fn color_burn(dst: f32, src: f32) -> f32 {
    let mut out = 0.0;
    if src > 0.0 {
        out = 1.0 - ((1.0 - dst) / src).min(1.0);
    }
    out
}

#[cube]
fn color_dodge_premul(src: f32, dst: f32, src_alpha: f32, dst_alpha: f32) -> f32 {
    let mut out = src * (1.0 - dst_alpha);
    if dst > 0.0 {
        if src >= src_alpha {
            out = src + dst * (1.0 - src_alpha);
        } else {
            out = src_alpha * dst_alpha.min((dst * src_alpha) / (src_alpha - src))
                + src * (1.0 - dst_alpha)
                + dst * (1.0 - src_alpha);
        }
    }
    out
}

#[cube]
fn color_burn_premul(src: f32, dst: f32, src_alpha: f32, dst_alpha: f32) -> f32 {
    let mut out = dst + src * (1.0 - dst_alpha);
    if dst < dst_alpha {
        if src <= 0.0 {
            out = dst * (1.0 - src_alpha);
        } else {
            out = src_alpha * (dst_alpha - dst_alpha.min(((dst_alpha - dst) * src_alpha) / src))
                + src * (1.0 - dst_alpha)
                + dst * (1.0 - src_alpha);
        }
    }
    out
}

#[cube]
fn soft_light(dst: f32, src: f32) -> f32 {
    let mut out = dst - (1.0 - 2.0 * src) * dst * (1.0 - dst);
    if src > 0.5 {
        let mut d = dst.sqrt();
        if dst <= 0.25 {
            d = ((16.0 * dst - 12.0) * dst + 4.0) * dst;
        }
        out = dst + (2.0 * src - 1.0) * (d - dst);
    }
    out
}

#[cube]
fn lum3(r: f32, g: f32, b: f32) -> f32 {
    0.3 * r + 0.59 * g + 0.11 * b
}

#[cube]
fn sat3(r: f32, g: f32, b: f32) -> f32 {
    r.max(g).max(b) - r.min(g).min(b)
}

#[cube]
fn set_lum_channel(r: f32, g: f32, b: f32, lum: f32, channel: u32) -> f32 {
    let d = lum - lum3(r, g, b);
    clip_color_channel(r + d, g + d, b + d, channel)
}

#[cube]
fn clip_color_channel(r: f32, g: f32, b: f32, channel: u32) -> f32 {
    let lum = lum3(r, g, b);
    let min_c = r.min(g).min(b);
    let max_c = r.max(g).max(b);
    let mut out_r = r;
    let mut out_g = g;
    let mut out_b = b;
    if min_c < 0.0 {
        out_r = lum + (out_r - lum) * lum / (lum - min_c);
        out_g = lum + (out_g - lum) * lum / (lum - min_c);
        out_b = lum + (out_b - lum) * lum / (lum - min_c);
    }
    if max_c > 1.0 {
        out_r = lum + (out_r - lum) * (1.0 - lum) / (max_c - lum);
        out_g = lum + (out_g - lum) * (1.0 - lum) / (max_c - lum);
        out_b = lum + (out_b - lum) * (1.0 - lum) / (max_c - lum);
    }
    if channel == 0 {
        out_r
    } else if channel == 1 {
        out_g
    } else {
        out_b
    }
}

#[cube]
fn set_sat_channel(r: f32, g: f32, b: f32, sat: f32, channel: u32) -> f32 {
    let mut min_ix = 0u32;
    if r <= g && r <= b {
    } else if g <= b {
        min_ix = 1;
    } else {
        min_ix = 2;
    }

    let mut max_ix = 0u32;
    if r >= g && r >= b {
    } else if g >= b {
        max_ix = 1;
    } else {
        max_ix = 2;
    }

    let mut out_r = 0.0;
    let mut out_g = 0.0;
    let mut out_b = 0.0;
    if min_ix != max_ix {
        let mid_ix = 3 - min_ix - max_ix;
        let min_v = channel_value(r, g, b, min_ix);
        let mid_v = channel_value(r, g, b, mid_ix);
        let max_v = channel_value(r, g, b, max_ix);
        let mut new_mid = 0.0;
        let mut new_max = 0.0;
        if max_v > min_v {
            new_mid = (mid_v - min_v) * sat / (max_v - min_v);
            new_max = sat;
        }
        out_r = set_channel_value(out_r, new_mid, mid_ix, 0);
        out_g = set_channel_value(out_g, new_mid, mid_ix, 1);
        out_b = set_channel_value(out_b, new_mid, mid_ix, 2);
        out_r = set_channel_value(out_r, new_max, max_ix, 0);
        out_g = set_channel_value(out_g, new_max, max_ix, 1);
        out_b = set_channel_value(out_b, new_max, max_ix, 2);
    }

    if channel == 0 {
        out_r
    } else if channel == 1 {
        out_g
    } else {
        out_b
    }
}

#[cube]
fn channel_value(r: f32, g: f32, b: f32, channel: u32) -> f32 {
    if channel == 0 {
        r
    } else if channel == 1 {
        g
    } else {
        b
    }
}

#[cube]
fn set_channel_value(current: f32, value: f32, src_channel: u32, dst_channel: u32) -> f32 {
    if src_channel == dst_channel {
        value
    } else {
        current
    }
}

#[cube]
fn pack_premul_rgba8(r: f32, g: f32, b: f32, a: f32) -> u32 {
    ((r.clamp(0.0, 1.0) * 255.0 + 0.5) as u32)
        | (((g.clamp(0.0, 1.0) * 255.0 + 0.5) as u32) << 8)
        | (((b.clamp(0.0, 1.0) * 255.0 + 0.5) as u32) << 16)
        | (((a.clamp(0.0, 1.0) * 255.0 + 0.5) as u32) << 24)
}

#[cube]
fn mul_div255(a: u32, b: u32) -> u32 {
    (a * b + 127) / 255
}
