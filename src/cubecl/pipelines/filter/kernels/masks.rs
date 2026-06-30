#[cube(launch)]
#[allow(clippy::too_many_arguments)]
pub(super) fn filter_layer_mask_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    tiles_width: u32,
    tiles_height: u32,
    draw_ix: u32,
    draw_path_ids: &Array<u32>,
    draw_tags: &Array<u32>,
    draw_fill_rules: &Array<u32>,
    draw_pixel_x0: &Array<i32>,
    draw_pixel_y0: &Array<i32>,
    draw_pixel_x1: &Array<i32>,
    draw_pixel_y1: &Array<i32>,
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
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }

    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let tile_x = x / 16;
    let tile_y = y / 16;
    let local_x = x - tile_x * 16;
    let local_y = y - tile_y * 16;
    let alpha = layer_stack_alpha_at(
        draw_ix,
        tile_x,
        tile_y,
        local_x,
        local_y,
        tiles_width,
        tiles_height,
        draw_path_ids,
        draw_tags,
        draw_fill_rules,
        draw_pixel_x0,
        draw_pixel_y0,
        draw_pixel_x1,
        draw_pixel_y1,
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
    let ix = (y * image_width + x) as usize;
    target[ix] = alpha | (alpha << 8) | (alpha << 16) | (alpha << 24);
}

#[cube(launch)]
#[allow(clippy::too_many_arguments)]
pub(super) fn filter_rect_mask_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    radius_top_left: f32,
    radius_top_right: f32,
    radius_bottom_left: f32,
    radius_bottom_right: f32,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }

    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let px = x as f32 + 0.5;
    let py = y as f32 + 0.5;
    let dist = gpu_rect_sdf_distance(
        px,
        py,
        x0,
        y0,
        x1,
        y1,
        radius_top_left,
        radius_top_right,
        radius_bottom_left,
        radius_bottom_right,
    );
    let alpha = ((0.5 - dist).clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
    let ix = (y * image_width + x) as usize;
    target[ix] = alpha | (alpha << 8) | (alpha << 16) | (alpha << 24);
}

#[cube(launch)]
#[allow(clippy::too_many_arguments)]
pub(super) fn filter_sdf_mask_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    kind: u32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    r0: f32,
    r1: f32,
    r2: f32,
    r3: f32,
    stroke_top: f32,
    stroke_right: f32,
    stroke_bottom: f32,
    stroke_left: f32,
    shadow_offset_x: f32,
    shadow_offset_y: f32,
    shadow_expand: f32,
    shadow_intensity: f32,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }

    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let alpha = gpu_sdf_alpha_from_encoded(
        kind,
        x as f32 + 0.5,
        y as f32 + 0.5,
        x0,
        y0,
        x1,
        y1,
        r0,
        r1,
        r2,
        r3,
        stroke_top,
        stroke_right,
        stroke_bottom,
        stroke_left,
        shadow_offset_x,
        shadow_offset_y,
        shadow_expand,
        shadow_intensity,
    );
    let ix = (y * image_width + x) as usize;
    target[ix] = alpha | (alpha << 8) | (alpha << 16) | (alpha << 24);
}

#[cube(launch)]
// Keep runtime bool conditions as nested branches here. Combined `&&`/`||`
// expressions have produced incorrect wgpu shader output in this CubeCL path.
#[allow(clippy::collapsible_if)]
pub(super) fn filter_path_mask_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    path_index: u32,
    path_range_starts: &Array<u32>,
    path_range_ends: &Array<u32>,
    path_p0x: &Array<i32>,
    path_p0y: &Array<i32>,
    path_p1x: &Array<i32>,
    path_p1y: &Array<i32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }

    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let px = x as f32 + 0.5;
    let py = y as f32 + 0.5;
    let path_i = path_index as usize;
    let mut winding = i32::new(0);
    if path_i < path_range_starts.len() {
        let mut line_ix = path_range_starts[path_i];
        let line_end = path_range_ends[path_i];
        while line_ix < line_end {
            let i = line_ix as usize;
            let inv_scale = f32::new(0.003_906_25_f32);
            let y0 = path_p0y[i] as f32 * inv_scale;
            let y1 = path_p1y[i] as f32 * inv_scale;
            let mut winding_delta = i32::new(0);
            if y0 <= py {
                if y1 > py {
                    winding_delta = i32::new(1);
                }
            }
            if y1 <= py {
                if y0 > py {
                    winding_delta = i32::new(-1);
                }
            }
            if winding_delta != 0 {
                let x0 = path_p0x[i] as f32 * inv_scale;
                let x1 = path_p1x[i] as f32 * inv_scale;
                let t = (py - y0) / (y1 - y0);
                let x_cross = x0 + (x1 - x0) * t;
                if x_cross > px {
                    winding += winding_delta;
                }
            }
            line_ix += 1;
        }
    }

    let mut alpha = u32::new(0);
    if winding != 0 {
        alpha = 255u32;
    }
    let ix = (y * image_width + x) as usize;
    target[ix] = alpha | (alpha << 8) | (alpha << 16) | (alpha << 24);
}

#[cube(launch)]
pub(super) fn filter_blur_region(
    pixel_count: u32,
    region_width: u32,
    region_height: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    std_dev: f32,
    axis: u32,
    source: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }

    let std_dev = std_dev.max(0.0);
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let dst_ix = (y * image_width + x) as usize;
    if std_dev <= 0.0 {
        target[dst_ix] = source[dst_ix];
        terminate!();
    }

    let half_width = (std_dev * 3.0).ceil().max(1.0) as i32;
    let sigma = std_dev.max(0.0001);
    let two_sigma_sq = 2.0 * sigma * sigma;
    let region_x1 = (region_x0 + region_width) as i32;
    let region_y1 = (region_y0 + region_height) as i32;
    let base_x = x as i32;
    let base_y = y as i32;
    let mut sum = 0.0;
    let mut r = 0.0;
    let mut g = 0.0;
    let mut b = 0.0;
    let mut a = 0.0;
    let mut d = -half_width;
    while d <= half_width {
        let df = d as f32;
        let weight = (-(df * df) / two_sigma_sq).exp();
        sum += weight;
        let mut sample_x = base_x;
        let mut sample_y = base_y;
        if axis == 0 {
            sample_x += d;
        } else {
            sample_y += d;
        }
        if sample_x >= region_x0 as i32
            && sample_x < region_x1
            && sample_y >= region_y0 as i32
            && sample_y < region_y1
        {
            let sample_ix = (sample_y as u32 * image_width + sample_x as u32) as usize;
            let px = source[sample_ix];
            r += (px & 255) as f32 * weight;
            g += ((px >> 8) & 255) as f32 * weight;
            b += ((px >> 16) & 255) as f32 * weight;
            a += ((px >> 24) & 255) as f32 * weight;
        }
        d += 1;
    }

    let mut scale = f32::new(0.0_f32);
    if sum > 0.0 {
        scale = 1.0 / (255.0 * sum);
    }
    target[dst_ix] = pack_premul_rgba8(r * scale, g * scale, b * scale, a * scale);
}

#[cube(launch)]
pub(super) fn filter_drop_shadow_mask_region(
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
    let source_ix = (y * image_width + x) as usize;
    let alpha = source[source_ix] >> 24;
    if alpha == 0 {
        terminate!();
    }

    let tx = x as i32 + dx;
    let ty = y as i32 + dy;
    let region_x1 = (region_x0 + region_width) as i32;
    let region_y1 = (region_y0 + region_height) as i32;
    if tx >= region_x0 as i32 && tx < region_x1 && ty >= region_y0 as i32 && ty < region_y1 {
        let target_ix = (ty as u32 * image_width + tx as u32) as usize;
        target[target_ix] = alpha | (alpha << 8) | (alpha << 16) | (alpha << 24);
    }
}

#[cube(launch)]
pub(super) fn filter_composite_drop_shadow_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    brush_index: u32,
    brush_data: &Array<u32>,
    brush_params: &Array<f32>,
    brush_payloads: &Array<u32>,
    shadow_mask: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }

    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    let alpha = shadow_mask[ix] >> 24;
    let shadow_color = sample_brush(
        brush_index,
        x as f32 + 0.5,
        y as f32 + 0.5,
        brush_data,
        brush_params,
        brush_payloads,
    );
    let shadow = scale_premul_u8(shadow_color, alpha);
    target[ix] = src_over_premul_u8(shadow, target[ix]);
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn layer_stack_alpha_at(
    draw_ix: u32,
    tile_x: u32,
    tile_y: u32,
    local_x: u32,
    local_y: u32,
    tiles_width: u32,
    tiles_height: u32,
    draw_path_ids: &Array<u32>,
    draw_tags: &Array<u32>,
    draw_fill_rules: &Array<u32>,
    draw_pixel_x0: &Array<i32>,
    draw_pixel_y0: &Array<i32>,
    draw_pixel_x1: &Array<i32>,
    draw_pixel_y1: &Array<i32>,
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
) -> u32 {
    let invalid = u32::new(-1);
    let backdrop_ix = filter_draw_backdrop_ix(
        draw_ix,
        tile_x,
        tile_y,
        tiles_width,
        tiles_height,
        draw_path_ids,
        draw_tags,
        draw_pixel_x0,
        draw_pixel_y0,
        draw_pixel_x1,
        draw_pixel_y1,
        backdrop_data_offsets,
        backdrop_tile_x0,
        backdrop_tile_y0,
        backdrop_tile_x1,
        backdrop_tile_y1,
    );
    let mut alpha = 0u32;
    if backdrop_ix != invalid {
        let i = backdrop_ix as usize;
        alpha = filter_fill_alpha_at(
            backdrops[i].load(),
            draw_fill_rules[draw_ix as usize],
            segment_starts[i],
            segment_ends[i],
            local_x,
            local_y,
            segment_p0x,
            segment_p0y,
            segment_p1x,
            segment_p1y,
            segment_y_edge,
        );
    }
    alpha
}

#[cube]
#[allow(clippy::too_many_arguments)]
// Keep runtime bool conditions as nested branches here. Combined `&&`/`||`
// expressions have produced incorrect wgpu shader output in this CubeCL path.
#[allow(clippy::collapsible_if)]
fn filter_draw_backdrop_ix(
    draw_ix: u32,
    tile_x: u32,
    tile_y: u32,
    tiles_width: u32,
    tiles_height: u32,
    draw_path_ids: &Array<u32>,
    draw_tags: &Array<u32>,
    draw_pixel_x0: &Array<i32>,
    draw_pixel_y0: &Array<i32>,
    draw_pixel_x1: &Array<i32>,
    draw_pixel_y1: &Array<i32>,
    backdrop_data_offsets: &Array<u32>,
    backdrop_tile_x0: &Array<u32>,
    backdrop_tile_y0: &Array<u32>,
    backdrop_tile_x1: &Array<u32>,
    backdrop_tile_y1: &Array<u32>,
) -> u32 {
    let invalid = u32::new(-1);
    let draw_i = draw_ix as usize;
    let path_id = draw_path_ids[draw_i];
    let draw_tag = draw_tags[draw_i];
    let mut result = invalid;

    let mut valid_draw = false;
    if draw_tag == CUBE_DRAW_BRUSH {
        valid_draw = true;
    }
    if draw_tag == CUBE_DRAW_PATH_GLYPH {
        valid_draw = true;
    }
    if draw_tag == CUBE_DRAW_CLIP {
        valid_draw = true;
    }
    if draw_tag == CUBE_DRAW_OPACITY {
        valid_draw = true;
    }
    if draw_tag == CUBE_DRAW_BLEND {
        valid_draw = true;
    }
    if draw_tag == CUBE_DRAW_ISOLATE {
        valid_draw = true;
    }

    if path_id != invalid {
        if valid_draw {
            let draw_x0 = filter_pixel_tile_min(draw_pixel_x0[draw_i], tiles_width);
            let draw_y0 = filter_pixel_tile_min(draw_pixel_y0[draw_i], tiles_height);
            let draw_x1 = filter_pixel_tile_max(draw_pixel_x1[draw_i], tiles_width);
            let draw_y1 = filter_pixel_tile_max(draw_pixel_y1[draw_i], tiles_height);
            let mut tile_in_draw = false;
            if tile_x >= draw_x0 {
                if tile_x < draw_x1 {
                    if tile_y >= draw_y0 {
                        if tile_y < draw_y1 {
                            tile_in_draw = true;
                        }
                    }
                }
            }
            if tile_in_draw {
                let path_i = path_id as usize;
                if path_i < backdrop_data_offsets.len() {
                    let bx0 = backdrop_tile_x0[path_i];
                    let by0 = backdrop_tile_y0[path_i];
                    let bx1 = backdrop_tile_x1[path_i];
                    let by1 = backdrop_tile_y1[path_i];
                    let stride = bx1 - bx0;
                    let mut tile_in_backdrop = false;
                    if stride > 0 {
                        if tile_x >= bx0 {
                            if tile_x < bx1 {
                                if tile_y >= by0 {
                                    if tile_y < by1 {
                                        tile_in_backdrop = true;
                                    }
                                }
                            }
                        }
                    }
                    if tile_in_backdrop {
                        result =
                            backdrop_data_offsets[path_i] + (tile_y - by0) * stride + tile_x - bx0;
                    }
                }
            }
        }
    }

    result
}

#[cube]
fn filter_pixel_tile_min(value: i32, limit: u32) -> u32 {
    let mut tile = 0u32;
    if value > 0 {
        tile = (value as u32 / 16).min(limit);
    }
    tile
}

#[cube]
fn filter_pixel_tile_max(value: i32, limit: u32) -> u32 {
    let mut tile = 0u32;
    if value > 0 {
        tile = (value as u32).div_ceil(16).min(limit);
    }
    tile
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn filter_fill_alpha_at(
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
        coverage += filter_segment_coverage_at(
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
    filter_coverage_to_alpha(coverage, fill_rule)
}

#[cube]
fn filter_segment_coverage_at(
    p0x: f32,
    p0y: f32,
    p1x: f32,
    p1y: f32,
    y_edge: f32,
    x: u32,
    y: u32,
) -> f32 {
    let delta_x = p1x - p0x;
    let delta_y = p1y - p0y;
    let row_y = y as f32;
    let local_y = p0y - row_y;
    let y0 = local_y.clamp(0.0, 1.0);
    let y1 = (local_y + delta_y).clamp(0.0, 1.0);
    let dy = y0 - y1;
    let x_sign = filter_signum_f32(delta_x);
    let mut coverage = x_sign * (row_y - y_edge + 1.0).clamp(0.0, 1.0);

    if dy != 0.0 {
        let recip = 1.0 / delta_y;
        let t0 = (y0 - local_y) * recip;
        let t1 = (y1 - local_y) * recip;
        let sx0 = p0x + t0 * delta_x;
        let sx1 = p1x + (t1 - 1.0) * delta_x;
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
fn filter_signum_f32(value: f32) -> f32 {
    // CPU coverage uses Rust f32::signum(), which returns +1 for +0.0.
    // Filter compositing reuses scan/fine coverage for masks and outer stacks,
    // so keep the same boundary-edge ownership here.
    let mut out = 1.0;
    if value < 0.0 {
        out = -1.0;
    }
    out
}

#[cube]
fn filter_coverage_to_alpha(value: f32, fill_rule: u32) -> u32 {
    let mut alpha = value.abs().min(1.0);
    if fill_rule == 1 {
        alpha = (value - f32::new(2.0_f32) * (f32::new(0.5_f32) * value).round()).abs();
    }
    (alpha.clamp(0.0, 1.0) * 255.0 + 0.5) as u32
}

#[cube]
fn apply_color_filter_pixel(px: u32, filter_kind: u32, amount: f32) -> u32 {
    let inv_255 = 1.0 / 255.0;
    let mut r = (px & 255) as f32 * inv_255;
    let mut g = ((px >> 8) & 255) as f32 * inv_255;
    let mut b = ((px >> 16) & 255) as f32 * inv_255;
    let mut a = ((px >> 24) & 255) as f32 * inv_255;

    if filter_kind == FILTER_OPACITY {
        let opacity = amount.clamp(0.0, 1.0);
        r *= opacity;
        g *= opacity;
        b *= opacity;
        a *= opacity;
    } else if a > 0.0 {
        let alpha = a;
        let mut ur = r / alpha;
        let mut ug = g / alpha;
        let mut ub = b / alpha;

        if filter_kind == FILTER_BRIGHTNESS {
            ur *= amount;
            ug *= amount;
            ub *= amount;
        } else if filter_kind == FILTER_CONTRAST {
            ur = (ur - 0.5) * amount + 0.5;
            ug = (ug - 0.5) * amount + 0.5;
            ub = (ub - 0.5) * amount + 0.5;
        } else if filter_kind == FILTER_GRAYSCALE {
            let t = amount.clamp(0.0, 1.0);
            let l = lum(ur, ug, ub);
            ur = lerp(ur, l, t);
            ug = lerp(ug, l, t);
            ub = lerp(ub, l, t);
        } else if filter_kind == FILTER_HUE_ROTATE {
            let angle = amount * f32::new(0.017_453_292_f32);
            let co = angle.cos();
            let si = angle.sin();
            let nr = (0.213 + co * 0.787 - si * 0.213) * ur
                + (0.715 - co * 0.715 - si * 0.715) * ug
                + (0.072 - co * 0.072 + si * 0.928) * ub;
            let ng = (0.213 - co * 0.213 + si * 0.143) * ur
                + (0.715 + co * 0.285 + si * 0.140) * ug
                + (0.072 - co * 0.072 - si * 0.283) * ub;
            let nb = (0.213 - co * 0.213 - si * 0.787) * ur
                + (0.715 - co * 0.715 + si * 0.715) * ug
                + (0.072 + co * 0.928 + si * 0.072) * ub;
            ur = nr;
            ug = ng;
            ub = nb;
        } else if filter_kind == FILTER_INVERT {
            let t = amount.clamp(0.0, 1.0);
            ur = lerp(ur, 1.0 - ur, t);
            ug = lerp(ug, 1.0 - ug, t);
            ub = lerp(ub, 1.0 - ub, t);
        } else if filter_kind == FILTER_SATURATE {
            let l = lum(ur, ug, ub);
            ur = l + (ur - l) * amount;
            ug = l + (ug - l) * amount;
            ub = l + (ub - l) * amount;
        } else if filter_kind == FILTER_SEPIA {
            let t = amount.clamp(0.0, 1.0);
            let sr = ur * 0.393 + ug * 0.769 + ub * 0.189;
            let sg = ur * 0.349 + ug * 0.686 + ub * 0.168;
            let sb = ur * 0.272 + ug * 0.534 + ub * 0.131;
            ur = lerp(ur, sr, t);
            ug = lerp(ug, sg, t);
            ub = lerp(ub, sb, t);
        }

        r = ur.clamp(0.0, 1.0) * alpha;
        g = ug.clamp(0.0, 1.0) * alpha;
        b = ub.clamp(0.0, 1.0) * alpha;
    }

    pack_premul_rgba8(r, g, b, a)
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn apply_color_matrix_pixel(
    px: u32,
    m00: f32,
    m01: f32,
    m02: f32,
    m03: f32,
    m04: f32,
    m10: f32,
    m11: f32,
    m12: f32,
    m13: f32,
    m14: f32,
    m20: f32,
    m21: f32,
    m22: f32,
    m23: f32,
    m24: f32,
    m30: f32,
    m31: f32,
    m32: f32,
    m33: f32,
    m34: f32,
) -> u32 {
    // SVG filter matrices operate on straight RGBA, while render buffers are premultiplied.
    let inv_255 = 1.0 / 255.0;
    let premul_r = (px & 255) as f32 * inv_255;
    let premul_g = ((px >> 8) & 255) as f32 * inv_255;
    let premul_b = ((px >> 16) & 255) as f32 * inv_255;
    let a = ((px >> 24) & 255) as f32 * inv_255;

    let mut r = 0.0;
    let mut g = 0.0;
    let mut b = 0.0;
    if a > 0.0 {
        r = premul_r / a;
        g = premul_g / a;
        b = premul_b / a;
    }

    let out_r = m00 * r + m01 * g + m02 * b + m03 * a + m04;
    let out_g = m10 * r + m11 * g + m12 * b + m13 * a + m14;
    let out_b = m20 * r + m21 * g + m22 * b + m23 * a + m24;
    let out_a = (m30 * r + m31 * g + m32 * b + m33 * a + m34).clamp(0.0, 1.0);
    pack_premul_rgba8(
        out_r.clamp(0.0, 1.0) * out_a,
        out_g.clamp(0.0, 1.0) * out_a,
        out_b.clamp(0.0, 1.0) * out_a,
        out_a,
    )
}

#[cube]
fn apply_component_transfer_pixel(px: u32, table_index: u32, transfer_tables: &Array<u32>) -> u32 {
    let alpha = (px >> 24) & 255;
    let base = table_index * COMPONENT_TRANSFER_TABLE_LEN_U32;
    let r_index = straight_component_index(px & 255, alpha);
    let g_index = straight_component_index((px >> 8) & 255, alpha);
    let b_index = straight_component_index((px >> 16) & 255, alpha);
    let inv_255 = 1.0 / 255.0;
    let r = transfer_tables[(base + r_index) as usize] as f32 * inv_255;
    let g = transfer_tables[(base + COMPONENT_TRANSFER_TABLE_SIZE_U32 + g_index) as usize] as f32
        * inv_255;
    let b = transfer_tables[(base + 2 * COMPONENT_TRANSFER_TABLE_SIZE_U32 + b_index) as usize]
        as f32
        * inv_255;
    let a = transfer_tables[(base + 3 * COMPONENT_TRANSFER_TABLE_SIZE_U32 + alpha) as usize] as f32
        * inv_255;
    pack_premul_rgba8(r * a, g * a, b * a, a)
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn composite_inputs_pixel(
    input1: u32,
    input2: u32,
    operator: u32,
    k1: f32,
    k2: f32,
    k3: f32,
    k4: f32,
) -> u32 {
    let mut out = blend_premul_u8(input2, input1, 3 << 8);
    if operator == 1 {
        out = blend_premul_u8(input2, input1, 5 << 8);
    } else if operator == 2 {
        out = blend_premul_u8(input2, input1, 7 << 8);
    } else if operator == 3 {
        out = blend_premul_u8(input2, input1, 9 << 8);
    } else if operator == 4 {
        out = blend_premul_u8(input2, input1, 11 << 8);
    } else if operator == 5 {
        out = arithmetic_composite_pixel(input1, input2, k1, k2, k3, k4);
    }
    out
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn arithmetic_composite_pixel(input1: u32, input2: u32, k1: f32, k2: f32, k3: f32, k4: f32) -> u32 {
    let a_r = (input1 & 255) as f32 / 255.0;
    let a_g = ((input1 >> 8) & 255) as f32 / 255.0;
    let a_b = ((input1 >> 16) & 255) as f32 / 255.0;
    let a_a = ((input1 >> 24) & 255) as f32 / 255.0;
    let b_r = (input2 & 255) as f32 / 255.0;
    let b_g = ((input2 >> 8) & 255) as f32 / 255.0;
    let b_b = ((input2 >> 16) & 255) as f32 / 255.0;
    let b_a = ((input2 >> 24) & 255) as f32 / 255.0;

    let out_r = arithmetic_channel(a_r, b_r, k1, k2, k3, k4);
    let out_g = arithmetic_channel(a_g, b_g, k1, k2, k3, k4);
    let out_b = arithmetic_channel(a_b, b_b, k1, k2, k3, k4);
    let out_a = arithmetic_channel(a_a, b_a, k1, k2, k3, k4);
    pack_premul_rgba8(out_r, out_g, out_b, out_a)
}

#[cube]
fn straight_channel(premul: u32, alpha: u32) -> f32 {
    let mut out = 0.0;
    if alpha != 0 {
        out = premul as f32 / alpha as f32;
    }
    out
}

#[cube]
fn filter_displacement_channel(px: u32, channel: u32, linear_rgb: u32) -> f32 {
    let alpha = (px >> 24) & 255;
    let mut value = alpha as f32 / 255.0;
    if channel != 3 {
        let mut premul = px & 255;
        if channel == 1 {
            premul = (px >> 8) & 255;
        } else if channel == 2 {
            premul = (px >> 16) & 255;
        }
        value = straight_channel(premul, alpha);
        if linear_rgb != 0 {
            value = filter_srgb_to_linear(value);
        }
    }
    value
}

#[cube]
fn source_alpha_at(source: &Array<u32>, x: u32, y: u32, image_width: u32) -> f32 {
    ((source[(y * image_width + x) as usize] >> 24) & 255) as f32 / 255.0
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn alpha_gradient_x(
    source: &Array<u32>,
    x: u32,
    y: u32,
    image_width: u32,
    region_x0: u32,
    region_width: u32,
    region_y0: u32,
    region_height: u32,
) -> f32 {
    let mut out = 0.0;
    if region_width >= 2 {
        let weighted_diff = alpha_gradient_x_sample(
            source,
            x,
            y,
            image_width,
            region_x0,
            region_width,
            region_y0,
            region_height,
            -1,
            1.0,
        ) + alpha_gradient_x_sample(
            source,
            x,
            y,
            image_width,
            region_x0,
            region_width,
            region_y0,
            region_height,
            0,
            2.0,
        ) + alpha_gradient_x_sample(
            source,
            x,
            y,
            image_width,
            region_x0,
            region_width,
            region_y0,
            region_height,
            1,
            1.0,
        );
        let weight_sum = gradient_sample_weight(y, region_y0, region_height, -1, 1.0)
            + gradient_sample_weight(y, region_y0, region_height, 0, 2.0)
            + gradient_sample_weight(y, region_y0, region_height, 1, 1.0);
        let one_sided = x == region_x0 || x == region_x0 + region_width - 1;
        let edge_scale = if one_sided { 2.0 } else { 1.0 };
        out = weighted_diff * edge_scale / weight_sum.max(0.000_001);
    }
    out
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn alpha_gradient_y(
    source: &Array<u32>,
    x: u32,
    y: u32,
    image_width: u32,
    region_x0: u32,
    region_width: u32,
    region_y0: u32,
    region_height: u32,
) -> f32 {
    let mut out = 0.0;
    if region_height >= 2 {
        let weighted_diff = alpha_gradient_y_sample(
            source,
            x,
            y,
            image_width,
            region_x0,
            region_width,
            region_y0,
            region_height,
            -1,
            1.0,
        ) + alpha_gradient_y_sample(
            source,
            x,
            y,
            image_width,
            region_x0,
            region_width,
            region_y0,
            region_height,
            0,
            2.0,
        ) + alpha_gradient_y_sample(
            source,
            x,
            y,
            image_width,
            region_x0,
            region_width,
            region_y0,
            region_height,
            1,
            1.0,
        );
        let weight_sum = gradient_sample_weight(x, region_x0, region_width, -1, 1.0)
            + gradient_sample_weight(x, region_x0, region_width, 0, 2.0)
            + gradient_sample_weight(x, region_x0, region_width, 1, 1.0);
        let one_sided = y == region_y0 || y == region_y0 + region_height - 1;
        let edge_scale = if one_sided { 2.0 } else { 1.0 };
        out = weighted_diff * edge_scale / weight_sum.max(0.000_001);
    }
    out
}

#[cube]
fn gradient_sample_weight(pos: u32, start: u32, len: u32, offset: i32, weight: f32) -> f32 {
    let sample = pos as i32 + offset;
    let end = start + len;
    let mut out = 0.0;
    if sample >= start as i32 && sample < end as i32 {
        out = weight;
    }
    out
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn alpha_gradient_x_sample(
    source: &Array<u32>,
    x: u32,
    y: u32,
    image_width: u32,
    region_x0: u32,
    region_width: u32,
    region_y0: u32,
    region_height: u32,
    offset: i32,
    weight: f32,
) -> f32 {
    let sy = y as i32 + offset;
    let region_x1 = region_x0 + region_width - 1;
    let region_y1 = region_y0 + region_height;
    let mut out = 0.0;
    if sy >= region_y0 as i32 && sy < region_y1 as i32 {
        let syu = sy as u32;
        let left = if x > region_x0 { x - 1 } else { x };
        let right = if x < region_x1 { x + 1 } else { x };
        let center = source_alpha_at(source, x, syu, image_width);
        let diff = if x == region_x0 {
            source_alpha_at(source, right, syu, image_width) - center
        } else if x == region_x1 {
            center - source_alpha_at(source, left, syu, image_width)
        } else {
            source_alpha_at(source, right, syu, image_width)
                - source_alpha_at(source, left, syu, image_width)
        };
        out = weight * diff;
    }
    out
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn alpha_gradient_y_sample(
    source: &Array<u32>,
    x: u32,
    y: u32,
    image_width: u32,
    region_x0: u32,
    region_width: u32,
    region_y0: u32,
    region_height: u32,
    offset: i32,
    weight: f32,
) -> f32 {
    let sx = x as i32 + offset;
    let region_x1 = region_x0 + region_width;
    let region_y1 = region_y0 + region_height - 1;
    let mut out = 0.0;
    if sx >= region_x0 as i32 && sx < region_x1 as i32 {
        let sxu = sx as u32;
        let top = if y > region_y0 { y - 1 } else { y };
        let bottom = if y < region_y1 { y + 1 } else { y };
        let center = source_alpha_at(source, sxu, y, image_width);
        let diff = if y == region_y0 {
            source_alpha_at(source, sxu, bottom, image_width) - center
        } else if y == region_y1 {
            center - source_alpha_at(source, sxu, top, image_width)
        } else {
            source_alpha_at(source, sxu, bottom, image_width)
                - source_alpha_at(source, sxu, top, image_width)
        };
        out = weight * diff;
    }
    out
}

#[cube]
fn arithmetic_channel(a: f32, b: f32, k1: f32, k2: f32, k3: f32, k4: f32) -> f32 {
    (k1 * a * b + k2 * a + k3 * b + k4).clamp(0.0, 1.0)
}

#[cube]
fn straight_component_index(premul: u32, alpha: u32) -> u32 {
    let safe_alpha = alpha.max(1);
    let mut index = ((premul * 255 + safe_alpha / 2) / safe_alpha).min(255);
    if alpha == 0 {
        index = 0;
    }
    index
}

#[cube]
fn lum(r: f32, g: f32, b: f32) -> f32 {
    r * 0.2126 + g * 0.7152 + b * 0.0722
}

#[cube]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
