use ::cubecl::prelude::*;

use crate::cubecl::{
    brush::GpuBrushResources,
    buffer::CubeBuffer,
    pipelines::common::{
        blend_premul_u8, combine_alpha, sample_brush, scale_premul_u8,
        src_over_mask_linear_auto_u8, src_over_premul_u8, src_over_subpixel_mask_linear_auto_u8,
        src_over_subpixel_mask_u8,
    },
    renderer::{CoarseBuffers, ScanBuffers, SceneBuffers},
    types::{
        CUBE_GLYPH_COLOR, CUBE_GLYPH_LINEAR_COLOR, CUBE_GLYPH_LINEAR_MASK,
        CUBE_GLYPH_LINEAR_SUBPIXEL_MASK, CUBE_GLYPH_MASK, CUBE_GLYPH_SUBPIXEL_MASK,
        CUBE_PTCL_BEGIN_BLEND, CUBE_PTCL_BEGIN_CLIP, CUBE_PTCL_BEGIN_OPACITY, CUBE_PTCL_COLOR,
        CUBE_PTCL_END, CUBE_PTCL_END_BLEND, CUBE_PTCL_END_CLIP, CUBE_PTCL_END_OPACITY,
        CUBE_PTCL_FILL, CUBE_PTCL_GLYPH, CUBE_PTCL_PATH_GLYPH, CUBE_PTCL_SDF, CUBE_SDF_ARC,
        CUBE_SDF_ARC_SHADOW, CUBE_SDF_CANDLESTICK, CUBE_SDF_CIRCLE, CUBE_SDF_CIRCLE_SHADOW,
        CUBE_SDF_CIRCLE_STROKE, CUBE_SDF_LINE, CUBE_SDF_LINE_SHADOW, CUBE_SDF_RECT,
        CUBE_SDF_RECT_SHADOW, CUBE_SDF_RECT_STROKE, CubeBufferLengths,
    },
};

const FINE_WORKGROUP_SIZE: u32 = 256;

include!("sdf_kernels.rs");

#[derive(Clone, Copy)]
pub(crate) struct FineRenderConfig {
    pub(crate) lengths: CubeBufferLengths,
    pub(crate) size: (u32, u32),
    pub(crate) max_clip_depth: usize,
    pub(crate) max_group_depth: usize,
}

pub(crate) struct FinePipeline;

impl FinePipeline {
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
        scene: &SceneBuffers,
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
            unsafe { scene.draw_sdf_kinds.arg() },
            unsafe { scene.draw_sdf_x0.arg() },
            unsafe { scene.draw_sdf_y0.arg() },
            unsafe { scene.draw_sdf_x1.arg() },
            unsafe { scene.draw_sdf_y1.arg() },
            unsafe { scene.draw_sdf_r0.arg() },
            unsafe { scene.draw_sdf_r1.arg() },
            unsafe { scene.draw_sdf_r2.arg() },
            unsafe { scene.draw_sdf_r3.arg() },
            unsafe { scene.draw_sdf_stroke_top.arg() },
            unsafe { scene.draw_sdf_stroke_right.arg() },
            unsafe { scene.draw_sdf_stroke_bottom.arg() },
            unsafe { scene.draw_sdf_stroke_left.arg() },
            unsafe { scene.draw_sdf_shadow_offset_x.arg() },
            unsafe { scene.draw_sdf_shadow_offset_y.arg() },
            unsafe { scene.draw_sdf_shadow_expand.arg() },
            unsafe { scene.draw_sdf_shadow_intensity.arg() },
            unsafe { coarse.glyph_indices.arg() },
            unsafe { scene.glyph_image_ids.arg() },
            unsafe { scene.glyph_x.arg() },
            unsafe { scene.glyph_y.arg() },
            unsafe { scene.glyph_image_left.arg() },
            unsafe { scene.glyph_image_top.arg() },
            unsafe { scene.glyph_image_width.arg() },
            unsafe { scene.glyph_image_height.arg() },
            unsafe { scene.glyph_image_content.arg() },
            unsafe { scene.glyph_image_data_offsets.arg() },
            unsafe { scene.glyph_image_data.arg() },
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
    draw_sdf_kinds: &Array<u32>,
    draw_sdf_x0: &Array<f32>,
    draw_sdf_y0: &Array<f32>,
    draw_sdf_x1: &Array<f32>,
    draw_sdf_y1: &Array<f32>,
    draw_sdf_r0: &Array<f32>,
    draw_sdf_r1: &Array<f32>,
    draw_sdf_r2: &Array<f32>,
    draw_sdf_r3: &Array<f32>,
    draw_sdf_stroke_top: &Array<f32>,
    draw_sdf_stroke_right: &Array<f32>,
    draw_sdf_stroke_bottom: &Array<f32>,
    draw_sdf_stroke_left: &Array<f32>,
    draw_sdf_shadow_offset_x: &Array<f32>,
    draw_sdf_shadow_offset_y: &Array<f32>,
    draw_sdf_shadow_expand: &Array<f32>,
    draw_sdf_shadow_intensity: &Array<f32>,
    glyph_indices: &Array<u32>,
    glyph_image_ids: &Array<u32>,
    glyph_x: &Array<i32>,
    glyph_y: &Array<i32>,
    glyph_image_left: &Array<i32>,
    glyph_image_top: &Array<i32>,
    glyph_image_width: &Array<u32>,
    glyph_image_height: &Array<u32>,
    glyph_image_content: &Array<u32>,
    glyph_image_data_offsets: &Array<u32>,
    glyph_image_data: &Array<u32>,
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
            } else if tag == CUBE_PTCL_SDF {
                let draw_ix = ptcl_colors[ptcl_i];
                let alpha = combine_alpha(
                    sdf_alpha_at(
                        draw_ix,
                        global_x as f32 + 0.5,
                        global_y as f32 + 0.5,
                        draw_sdf_kinds,
                        draw_sdf_x0,
                        draw_sdf_y0,
                        draw_sdf_x1,
                        draw_sdf_y1,
                        draw_sdf_r0,
                        draw_sdf_r1,
                        draw_sdf_r2,
                        draw_sdf_r3,
                        draw_sdf_stroke_top,
                        draw_sdf_stroke_right,
                        draw_sdf_stroke_bottom,
                        draw_sdf_stroke_left,
                        draw_sdf_shadow_offset_x,
                        draw_sdf_shadow_offset_y,
                        draw_sdf_shadow_expand,
                        draw_sdf_shadow_intensity,
                    ),
                    clip_mask,
                );
                if alpha > 0 {
                    let color = sample_brush(
                        draw_ix,
                        global_x as f32 + 0.5,
                        global_y as f32 + 0.5,
                        brush_data,
                        brush_params,
                        brush_payloads,
                    );
                    pixel = src_over_premul_u8(pixel, scale_premul_u8(color, alpha));
                }
            } else if tag == CUBE_PTCL_GLYPH {
                pixel = composite_glyphs_at(
                    pixel,
                    ptcl_segment_starts[ptcl_i],
                    ptcl_segment_ends[ptcl_i],
                    ptcl_colors[ptcl_i],
                    global_x,
                    global_y,
                    clip_mask,
                    glyph_indices,
                    glyph_image_ids,
                    glyph_x,
                    glyph_y,
                    glyph_image_left,
                    glyph_image_top,
                    glyph_image_width,
                    glyph_image_height,
                    glyph_image_content,
                    glyph_image_data_offsets,
                    glyph_image_data,
                    brush_data,
                    brush_params,
                    brush_payloads,
                );
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
                || tag == CUBE_PTCL_PATH_GLYPH
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
                    if tag == CUBE_PTCL_PATH_GLYPH {
                        pixel = src_over_mask_linear_auto_u8(pixel, color, alpha);
                    } else {
                        pixel = src_over_premul_u8(pixel, scale_premul_u8(color, alpha));
                    }
                }
            }
            ptcl_ix += 1;
        }
    }

    target[target_ix as usize] = pixel;
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn composite_glyphs_at(
    mut pixel: u32,
    glyph_start: u32,
    glyph_end: u32,
    draw_ix: u32,
    global_x: u32,
    global_y: u32,
    clip_mask: u32,
    glyph_indices: &Array<u32>,
    glyph_image_ids: &Array<u32>,
    glyph_x: &Array<i32>,
    glyph_y: &Array<i32>,
    glyph_image_left: &Array<i32>,
    glyph_image_top: &Array<i32>,
    glyph_image_width: &Array<u32>,
    glyph_image_height: &Array<u32>,
    glyph_image_content: &Array<u32>,
    glyph_image_data_offsets: &Array<u32>,
    glyph_image_data: &Array<u32>,
    brush_data: &Array<u32>,
    brush_params: &Array<f32>,
    brush_payloads: &Array<u32>,
) -> u32 {
    let invalid = u32::new(-1);
    let mut glyph_list_ix = glyph_start;
    let px = global_x as i32;
    let py = global_y as i32;

    while glyph_list_ix < glyph_end {
        let glyph_i = glyph_indices[glyph_list_ix as usize] as usize;
        let image_id = glyph_image_ids[glyph_i];
        if image_id != invalid {
            let image_i = image_id as usize;
            let width = glyph_image_width[image_i];
            let height = glyph_image_height[image_i];
            let x0 = glyph_x[glyph_i] + glyph_image_left[image_i];
            let y0 = glyph_y[glyph_i] - glyph_image_top[image_i];
            let local_x = px - x0;
            let local_y = py - y0;
            if local_x >= 0 && local_y >= 0 && local_x < width as i32 && local_y < height as i32 {
                let data_ix =
                    glyph_image_data_offsets[image_i] + local_y as u32 * width + local_x as u32;
                let content = glyph_image_content[image_i];
                if content == CUBE_GLYPH_MASK {
                    let alpha = combine_alpha(glyph_image_data[data_ix as usize], clip_mask);
                    if alpha > 0 {
                        let color = sample_brush(
                            draw_ix,
                            global_x as f32 + 0.5,
                            global_y as f32 + 0.5,
                            brush_data,
                            brush_params,
                            brush_payloads,
                        );
                        pixel = src_over_premul_u8(pixel, scale_premul_u8(color, alpha));
                    }
                } else if content == CUBE_GLYPH_LINEAR_MASK {
                    let alpha = combine_alpha(glyph_image_data[data_ix as usize], clip_mask);
                    if alpha > 0 {
                        let color = sample_brush(
                            draw_ix,
                            global_x as f32 + 0.5,
                            global_y as f32 + 0.5,
                            brush_data,
                            brush_params,
                            brush_payloads,
                        );
                        pixel = src_over_mask_linear_auto_u8(pixel, color, alpha);
                    }
                } else if content == CUBE_GLYPH_COLOR {
                    let color = scale_premul_u8(glyph_image_data[data_ix as usize], clip_mask);
                    pixel = src_over_premul_u8(pixel, color);
                } else if content == CUBE_GLYPH_LINEAR_COLOR {
                    let color = glyph_image_data[data_ix as usize];
                    pixel = src_over_mask_linear_auto_u8(pixel, color, clip_mask);
                } else if content == CUBE_GLYPH_SUBPIXEL_MASK {
                    let color = sample_brush(
                        draw_ix,
                        global_x as f32 + 0.5,
                        global_y as f32 + 0.5,
                        brush_data,
                        brush_params,
                        brush_payloads,
                    );
                    pixel = src_over_subpixel_mask_u8(
                        pixel,
                        color,
                        glyph_image_data[data_ix as usize],
                        clip_mask,
                    );
                } else if content == CUBE_GLYPH_LINEAR_SUBPIXEL_MASK {
                    let color = sample_brush(
                        draw_ix,
                        global_x as f32 + 0.5,
                        global_y as f32 + 0.5,
                        brush_data,
                        brush_params,
                        brush_payloads,
                    );
                    pixel = src_over_subpixel_mask_linear_auto_u8(
                        pixel,
                        color,
                        glyph_image_data[data_ix as usize],
                        clip_mask,
                    );
                }
            }
        }
        glyph_list_ix += 1;
    }

    pixel
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
        let mut area = (f32::new(1.0_f32) - xmin).clamp(0.0, 1.0);
        if xmax - xmin > f32::new(0.000001_f32) {
            let a_min = xmin.min(1.0) - f32::new(0.000001_f32);
            let b = xmax.min(1.0);
            let c = b.max(0.0);
            let d = a_min.max(0.0);
            area = (b + f32::new(0.5_f32) * (d * d - c * c) - a_min) / (xmax - a_min);
        }
        coverage += area * dy;
    }

    coverage
}

#[cube]
fn signum_f32(value: f32) -> f32 {
    // CPU coverage uses Rust f32::signum(), which returns +1 for +0.0.
    // Scan canonicalizes clipped tile-boundary coordinates to +0.0, so this
    // branch keeps vertical boundary edges identical between CPU and GPU.
    let mut out = 1.0;
    if value < 0.0 {
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
#[allow(clippy::too_many_arguments)]
fn sdf_alpha_at(
    draw_ix: u32,
    x: f32,
    y: f32,
    draw_sdf_kinds: &Array<u32>,
    draw_sdf_x0: &Array<f32>,
    draw_sdf_y0: &Array<f32>,
    draw_sdf_x1: &Array<f32>,
    draw_sdf_y1: &Array<f32>,
    draw_sdf_r0: &Array<f32>,
    draw_sdf_r1: &Array<f32>,
    draw_sdf_r2: &Array<f32>,
    draw_sdf_r3: &Array<f32>,
    draw_sdf_stroke_top: &Array<f32>,
    draw_sdf_stroke_right: &Array<f32>,
    draw_sdf_stroke_bottom: &Array<f32>,
    draw_sdf_stroke_left: &Array<f32>,
    draw_sdf_shadow_offset_x: &Array<f32>,
    draw_sdf_shadow_offset_y: &Array<f32>,
    draw_sdf_shadow_expand: &Array<f32>,
    draw_sdf_shadow_intensity: &Array<f32>,
) -> u32 {
    let i = draw_ix as usize;
    gpu_sdf_alpha_from_encoded(
        draw_sdf_kinds[i],
        x,
        y,
        draw_sdf_x0[i],
        draw_sdf_y0[i],
        draw_sdf_x1[i],
        draw_sdf_y1[i],
        draw_sdf_r0[i],
        draw_sdf_r1[i],
        draw_sdf_r2[i],
        draw_sdf_r3[i],
        draw_sdf_stroke_top[i],
        draw_sdf_stroke_right[i],
        draw_sdf_stroke_bottom[i],
        draw_sdf_stroke_left[i],
        draw_sdf_shadow_offset_x[i],
        draw_sdf_shadow_offset_y[i],
        draw_sdf_shadow_expand[i],
        draw_sdf_shadow_intensity[i],
    )
}
