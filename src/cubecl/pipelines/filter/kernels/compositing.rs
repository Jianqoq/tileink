#[cube(launch)]
pub(super) fn filter_offset_region(
    pixel_count: u32,
    region_width: u32,
    region_height: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    dx: i32,
    dy: i32,
    source: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let sx = x as i32 - dx;
    let sy = y as i32 - dy;
    let region_x1 = (region_x0 + region_width) as i32;
    let region_y1 = (region_y0 + region_height) as i32;
    let ix = (y * image_width + x) as usize;
    let mut pixel = 0u32;
    if sx >= region_x0 as i32 && sx < region_x1 && sy >= region_y0 as i32 && sy < region_y1 {
        pixel = source[(sy as u32 * image_width + sx as u32) as usize];
    }
    target[ix] = pixel;
}

#[cube(launch)]
pub(super) fn filter_flood_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    brush_index: u32,
    brush_data: &Array<u32>,
    brush_params: &Array<f32>,
    brush_payloads: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }

    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    target[ix] = sample_brush(
        brush_index,
        x as f32 + 0.5,
        y as f32 + 0.5,
        brush_data,
        brush_params,
        brush_payloads,
    );
}

#[cube(launch)]
pub(super) fn filter_composite_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    source: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    target[ix] = src_over_premul_u8(target[ix], source[ix]);
}

#[cube(launch)]
#[allow(clippy::too_many_arguments)]
// Keep runtime bool conditions as nested branches here. Combined `&&`/`||`
// expressions have produced incorrect wgpu shader output in this CubeCL path.
#[allow(clippy::collapsible_if)]
pub(super) fn filter_composite_stack_region(
    #[comptime] workgroup_size: usize,
    #[comptime] group_stack_capacity: usize,
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    tiles_width: u32,
    tiles_height: u32,
    layer_stack_start: u32,
    layer_stack_end: u32,
    mask_enabled: u32,
    source: &Array<u32>,
    mask: &Array<u32>,
    draw_path_ids: &Array<u32>,
    draw_flags: &Array<u32>,
    draw_pixel_x0: &Array<i32>,
    draw_pixel_y0: &Array<i32>,
    draw_pixel_x1: &Array<i32>,
    draw_pixel_y1: &Array<i32>,
    draw_sdf_refs: &Array<u32>,
    sdf_kinds: &Array<u32>,
    sdf_x0: &Array<f32>,
    sdf_y0: &Array<f32>,
    sdf_x1: &Array<f32>,
    sdf_y1: &Array<f32>,
    sdf_r0: &Array<f32>,
    sdf_r1: &Array<f32>,
    sdf_r2: &Array<f32>,
    sdf_r3: &Array<f32>,
    sdf_stroke_top: &Array<f32>,
    sdf_stroke_right: &Array<f32>,
    sdf_stroke_bottom: &Array<f32>,
    sdf_stroke_left: &Array<f32>,
    sdf_shadow_offset_x: &Array<f32>,
    sdf_shadow_offset_y: &Array<f32>,
    sdf_shadow_expand: &Array<f32>,
    sdf_shadow_intensity: &Array<f32>,
    backdrop_data_offsets: &Array<u32>,
    backdrop_tile_x0: &Array<u32>,
    backdrop_tile_y0: &Array<u32>,
    backdrop_tile_x1: &Array<u32>,
    backdrop_tile_y1: &Array<u32>,
    backdrops: &Array<Atomic<i32>>,
    segment_starts: &Array<u32>,
    segment_ends: &Array<u32>,
    segment_p0x: &Array<f32>,
    segment_p0y: &Array<f32>,
    segment_p1x: &Array<f32>,
    segment_p1y: &Array<f32>,
    segment_y_edge: &Array<f32>,
    layer_stack_tags: &Array<u32>,
    layer_stack_draws: &Array<u32>,
    layer_stack_payloads: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }

    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    let tile_x = x / 16;
    let tile_y = y / 16;
    let local_x = x - tile_x * 16;
    let local_y = y - tile_y * 16;

    let mut pixel = target[ix];
    let mut clip_mask = 255u32;
    let mut group_depth = 0u32;
    let mut group_kinds = SharedMemory::<u32>::new(workgroup_size * group_stack_capacity);
    let mut group_parent_pixels = SharedMemory::<u32>::new(workgroup_size * group_stack_capacity);
    let mut group_parent_clips = SharedMemory::<u32>::new(workgroup_size * group_stack_capacity);
    let mut group_layer_alphas = SharedMemory::<u32>::new(workgroup_size * group_stack_capacity);
    let mut group_payloads = SharedMemory::<u32>::new(workgroup_size * group_stack_capacity);

    let mut stack_ix = layer_stack_start;
    while stack_ix < layer_stack_end {
        let stack_i = stack_ix as usize;
        let tag = layer_stack_tags[stack_i];
        let alpha = layer_stack_alpha_at(
            layer_stack_draws[stack_i],
            tile_x,
            tile_y,
            local_x,
            local_y,
            tiles_width,
            tiles_height,
            draw_path_ids,
            draw_flags,
            draw_pixel_x0,
            draw_pixel_y0,
            draw_pixel_x1,
            draw_pixel_y1,
            draw_sdf_refs,
            sdf_kinds,
            sdf_x0,
            sdf_y0,
            sdf_x1,
            sdf_y1,
            sdf_r0,
            sdf_r1,
            sdf_r2,
            sdf_r3,
            sdf_stroke_top,
            sdf_stroke_right,
            sdf_stroke_bottom,
            sdf_stroke_left,
            sdf_shadow_offset_x,
            sdf_shadow_offset_y,
            sdf_shadow_expand,
            sdf_shadow_intensity,
            backdrop_data_offsets,
            backdrop_tile_x0,
            backdrop_tile_y0,
            backdrop_tile_x1,
            backdrop_tile_y1,
            backdrops,
            segment_starts,
            segment_ends,
            segment_p0x,
            segment_p0y,
            segment_p1x,
            segment_p1y,
            segment_y_edge,
        );

        if tag == CUBE_LAYER_CLIP {
            clip_mask = combine_alpha(clip_mask, alpha);
        } else {
            let mut is_group = false;
            if tag == CUBE_LAYER_OPACITY {
                is_group = true;
            }
            if tag == CUBE_LAYER_BLEND {
                is_group = true;
            }
            if is_group {
                if group_depth < group_stack_capacity as u32 {
                    let group_ix = (group_depth * workgroup_size as u32 + UNIT_POS) as usize;
                    group_kinds[group_ix] = tag;
                    group_parent_pixels[group_ix] = pixel;
                    group_parent_clips[group_ix] = clip_mask;
                    group_layer_alphas[group_ix] = alpha;
                    group_payloads[group_ix] = layer_stack_payloads[stack_i];
                    group_depth += 1;
                    pixel = 0;
                }
            }
        }
        stack_ix += 1;
    }

    let mut source_alpha = clip_mask;
    if mask_enabled == 1 {
        source_alpha = combine_alpha(source_alpha, mask[ix] >> 24);
    }
    pixel = src_over_premul_u8(pixel, scale_premul_u8(source[ix], source_alpha));

    while group_depth > 0 {
        group_depth -= 1;
        let group_ix = (group_depth * workgroup_size as u32 + UNIT_POS) as usize;
        let parent = group_parent_pixels[group_ix];
        let parent_clip = group_parent_clips[group_ix];
        let layer_alpha = group_layer_alphas[group_ix];
        let payload = group_payloads[group_ix];
        let group_kind = group_kinds[group_ix];
        let mut alpha = combine_alpha(layer_alpha, parent_clip);
        if group_kind == CUBE_LAYER_OPACITY {
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

    target[ix] = pixel;
}

#[cube(launch)]
#[allow(clippy::too_many_arguments)]
#[allow(clippy::collapsible_if)]
pub(super) fn filter_composite_surface_stack_region(
    #[comptime] workgroup_size: usize,
    #[comptime] group_stack_capacity: usize,
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    target_width: u32,
    tiles_width: u32,
    tiles_height: u32,
    source_width: u32,
    source_height: u32,
    source_origin_x: i32,
    source_origin_y: i32,
    layer_stack_start: u32,
    layer_stack_end: u32,
    source: &Array<u32>,
    draw_path_ids: &Array<u32>,
    draw_flags: &Array<u32>,
    draw_pixel_x0: &Array<i32>,
    draw_pixel_y0: &Array<i32>,
    draw_pixel_x1: &Array<i32>,
    draw_pixel_y1: &Array<i32>,
    draw_sdf_refs: &Array<u32>,
    sdf_kinds: &Array<u32>,
    sdf_x0: &Array<f32>,
    sdf_y0: &Array<f32>,
    sdf_x1: &Array<f32>,
    sdf_y1: &Array<f32>,
    sdf_r0: &Array<f32>,
    sdf_r1: &Array<f32>,
    sdf_r2: &Array<f32>,
    sdf_r3: &Array<f32>,
    sdf_stroke_top: &Array<f32>,
    sdf_stroke_right: &Array<f32>,
    sdf_stroke_bottom: &Array<f32>,
    sdf_stroke_left: &Array<f32>,
    sdf_shadow_offset_x: &Array<f32>,
    sdf_shadow_offset_y: &Array<f32>,
    sdf_shadow_expand: &Array<f32>,
    sdf_shadow_intensity: &Array<f32>,
    backdrop_data_offsets: &Array<u32>,
    backdrop_tile_x0: &Array<u32>,
    backdrop_tile_y0: &Array<u32>,
    backdrop_tile_x1: &Array<u32>,
    backdrop_tile_y1: &Array<u32>,
    backdrops: &Array<Atomic<i32>>,
    segment_starts: &Array<u32>,
    segment_ends: &Array<u32>,
    segment_p0x: &Array<f32>,
    segment_p0y: &Array<f32>,
    segment_p1x: &Array<f32>,
    segment_p1y: &Array<f32>,
    segment_y_edge: &Array<f32>,
    layer_stack_tags: &Array<u32>,
    layer_stack_draws: &Array<u32>,
    layer_stack_payloads: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }

    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let sx_i = x as i32 - source_origin_x;
    let sy_i = y as i32 - source_origin_y;
    if sx_i < 0 || sy_i < 0 || sx_i >= source_width as i32 || sy_i >= source_height as i32 {
        terminate!();
    }

    let ix = (y * target_width + x) as usize;
    let source_ix = (sy_i as u32 * source_width + sx_i as u32) as usize;
    let tile_x = x / 16;
    let tile_y = y / 16;
    let local_x = x - tile_x * 16;
    let local_y = y - tile_y * 16;

    let mut pixel = target[ix];
    let mut clip_mask = 255u32;
    let mut group_depth = 0u32;
    let mut group_kinds = SharedMemory::<u32>::new(workgroup_size * group_stack_capacity);
    let mut group_parent_pixels = SharedMemory::<u32>::new(workgroup_size * group_stack_capacity);
    let mut group_parent_clips = SharedMemory::<u32>::new(workgroup_size * group_stack_capacity);
    let mut group_layer_alphas = SharedMemory::<u32>::new(workgroup_size * group_stack_capacity);
    let mut group_payloads = SharedMemory::<u32>::new(workgroup_size * group_stack_capacity);

    let mut stack_ix = layer_stack_start;
    while stack_ix < layer_stack_end {
        let stack_i = stack_ix as usize;
        let tag = layer_stack_tags[stack_i];
        let alpha = layer_stack_alpha_at(
            layer_stack_draws[stack_i],
            tile_x,
            tile_y,
            local_x,
            local_y,
            tiles_width,
            tiles_height,
            draw_path_ids,
            draw_flags,
            draw_pixel_x0,
            draw_pixel_y0,
            draw_pixel_x1,
            draw_pixel_y1,
            draw_sdf_refs,
            sdf_kinds,
            sdf_x0,
            sdf_y0,
            sdf_x1,
            sdf_y1,
            sdf_r0,
            sdf_r1,
            sdf_r2,
            sdf_r3,
            sdf_stroke_top,
            sdf_stroke_right,
            sdf_stroke_bottom,
            sdf_stroke_left,
            sdf_shadow_offset_x,
            sdf_shadow_offset_y,
            sdf_shadow_expand,
            sdf_shadow_intensity,
            backdrop_data_offsets,
            backdrop_tile_x0,
            backdrop_tile_y0,
            backdrop_tile_x1,
            backdrop_tile_y1,
            backdrops,
            segment_starts,
            segment_ends,
            segment_p0x,
            segment_p0y,
            segment_p1x,
            segment_p1y,
            segment_y_edge,
        );

        if tag == CUBE_LAYER_CLIP {
            clip_mask = combine_alpha(clip_mask, alpha);
        } else {
            let mut is_group = false;
            if tag == CUBE_LAYER_OPACITY {
                is_group = true;
            }
            if tag == CUBE_LAYER_BLEND {
                is_group = true;
            }
            if is_group {
                if group_depth < group_stack_capacity as u32 {
                    let group_ix = (group_depth * workgroup_size as u32 + UNIT_POS) as usize;
                    group_kinds[group_ix] = tag;
                    group_parent_pixels[group_ix] = pixel;
                    group_parent_clips[group_ix] = clip_mask;
                    group_layer_alphas[group_ix] = alpha;
                    group_payloads[group_ix] = layer_stack_payloads[stack_i];
                    group_depth += 1;
                    pixel = 0;
                }
            }
        }
        stack_ix += 1;
    }

    pixel = src_over_premul_u8(pixel, scale_premul_u8(source[source_ix], clip_mask));

    while group_depth > 0 {
        group_depth -= 1;
        let group_ix = (group_depth * workgroup_size as u32 + UNIT_POS) as usize;
        let parent = group_parent_pixels[group_ix];
        let parent_clip = group_parent_clips[group_ix];
        let layer_alpha = group_layer_alphas[group_ix];
        let payload = group_payloads[group_ix];
        let group_kind = group_kinds[group_ix];
        let mut alpha = combine_alpha(layer_alpha, parent_clip);
        if group_kind == CUBE_LAYER_OPACITY {
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

    target[ix] = pixel;
}

#[cube(launch)]
#[allow(clippy::too_many_arguments)]
// Keep runtime bool conditions as nested branches here. Combined `&&`/`||`
// expressions have produced incorrect wgpu shader output in this CubeCL path.
#[allow(clippy::collapsible_if)]
pub(super) fn filter_composite_blend_stack_region(
    #[comptime] workgroup_size: usize,
    #[comptime] group_stack_capacity: usize,
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    tiles_width: u32,
    tiles_height: u32,
    layer_stack_start: u32,
    layer_stack_end: u32,
    blend_mode: u32,
    source: &Array<u32>,
    mask: &Array<u32>,
    draw_path_ids: &Array<u32>,
    draw_flags: &Array<u32>,
    draw_pixel_x0: &Array<i32>,
    draw_pixel_y0: &Array<i32>,
    draw_pixel_x1: &Array<i32>,
    draw_pixel_y1: &Array<i32>,
    draw_sdf_refs: &Array<u32>,
    sdf_kinds: &Array<u32>,
    sdf_x0: &Array<f32>,
    sdf_y0: &Array<f32>,
    sdf_x1: &Array<f32>,
    sdf_y1: &Array<f32>,
    sdf_r0: &Array<f32>,
    sdf_r1: &Array<f32>,
    sdf_r2: &Array<f32>,
    sdf_r3: &Array<f32>,
    sdf_stroke_top: &Array<f32>,
    sdf_stroke_right: &Array<f32>,
    sdf_stroke_bottom: &Array<f32>,
    sdf_stroke_left: &Array<f32>,
    sdf_shadow_offset_x: &Array<f32>,
    sdf_shadow_offset_y: &Array<f32>,
    sdf_shadow_expand: &Array<f32>,
    sdf_shadow_intensity: &Array<f32>,
    backdrop_data_offsets: &Array<u32>,
    backdrop_tile_x0: &Array<u32>,
    backdrop_tile_y0: &Array<u32>,
    backdrop_tile_x1: &Array<u32>,
    backdrop_tile_y1: &Array<u32>,
    backdrops: &Array<Atomic<i32>>,
    segment_starts: &Array<u32>,
    segment_ends: &Array<u32>,
    segment_p0x: &Array<f32>,
    segment_p0y: &Array<f32>,
    segment_p1x: &Array<f32>,
    segment_p1y: &Array<f32>,
    segment_y_edge: &Array<f32>,
    layer_stack_tags: &Array<u32>,
    layer_stack_draws: &Array<u32>,
    layer_stack_payloads: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }

    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    let tile_x = x / 16;
    let tile_y = y / 16;
    let local_x = x - tile_x * 16;
    let local_y = y - tile_y * 16;

    let mut pixel = target[ix];
    let mut clip_mask = 255u32;
    let mut group_depth = 0u32;
    let mut group_kinds = SharedMemory::<u32>::new(workgroup_size * group_stack_capacity);
    let mut group_parent_pixels = SharedMemory::<u32>::new(workgroup_size * group_stack_capacity);
    let mut group_parent_clips = SharedMemory::<u32>::new(workgroup_size * group_stack_capacity);
    let mut group_layer_alphas = SharedMemory::<u32>::new(workgroup_size * group_stack_capacity);
    let mut group_payloads = SharedMemory::<u32>::new(workgroup_size * group_stack_capacity);

    let mut stack_ix = layer_stack_start;
    while stack_ix < layer_stack_end {
        let stack_i = stack_ix as usize;
        let tag = layer_stack_tags[stack_i];
        let alpha = layer_stack_alpha_at(
            layer_stack_draws[stack_i],
            tile_x,
            tile_y,
            local_x,
            local_y,
            tiles_width,
            tiles_height,
            draw_path_ids,
            draw_flags,
            draw_pixel_x0,
            draw_pixel_y0,
            draw_pixel_x1,
            draw_pixel_y1,
            draw_sdf_refs,
            sdf_kinds,
            sdf_x0,
            sdf_y0,
            sdf_x1,
            sdf_y1,
            sdf_r0,
            sdf_r1,
            sdf_r2,
            sdf_r3,
            sdf_stroke_top,
            sdf_stroke_right,
            sdf_stroke_bottom,
            sdf_stroke_left,
            sdf_shadow_offset_x,
            sdf_shadow_offset_y,
            sdf_shadow_expand,
            sdf_shadow_intensity,
            backdrop_data_offsets,
            backdrop_tile_x0,
            backdrop_tile_y0,
            backdrop_tile_x1,
            backdrop_tile_y1,
            backdrops,
            segment_starts,
            segment_ends,
            segment_p0x,
            segment_p0y,
            segment_p1x,
            segment_p1y,
            segment_y_edge,
        );

        if tag == CUBE_LAYER_CLIP {
            clip_mask = combine_alpha(clip_mask, alpha);
        } else {
            let mut is_group = false;
            if tag == CUBE_LAYER_OPACITY {
                is_group = true;
            }
            if tag == CUBE_LAYER_BLEND {
                is_group = true;
            }
            if is_group {
                if group_depth < group_stack_capacity as u32 {
                    let group_ix = (group_depth * workgroup_size as u32 + UNIT_POS) as usize;
                    group_kinds[group_ix] = tag;
                    group_parent_pixels[group_ix] = pixel;
                    group_parent_clips[group_ix] = clip_mask;
                    group_layer_alphas[group_ix] = alpha;
                    group_payloads[group_ix] = layer_stack_payloads[stack_i];
                    group_depth += 1;
                    pixel = 0;
                }
            }
        }
        stack_ix += 1;
    }

    let source_alpha = combine_alpha(clip_mask, mask[ix] >> 24);
    let src = scale_premul_u8(source[ix], source_alpha);
    if src >> 24 != 0 {
        pixel = blend_premul_u8(pixel, src, blend_mode);
    }

    while group_depth > 0 {
        group_depth -= 1;
        let group_ix = (group_depth * workgroup_size as u32 + UNIT_POS) as usize;
        let parent = group_parent_pixels[group_ix];
        let parent_clip = group_parent_clips[group_ix];
        let layer_alpha = group_layer_alphas[group_ix];
        let payload = group_payloads[group_ix];
        let group_kind = group_kinds[group_ix];
        let mut alpha = combine_alpha(layer_alpha, parent_clip);
        if group_kind == CUBE_LAYER_OPACITY {
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

    target[ix] = pixel;
}
