#[cube(launch)]
pub(super) fn filter_clear_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    target[ix] = 0;
}

#[cube(launch)]
pub(super) fn filter_copy_region(
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
    target[ix] = source[ix];
}

#[cube(launch)]
pub(super) fn filter_tile_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    source_x0: u32,
    source_y0: u32,
    source_width: u32,
    source_height: u32,
    source: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let sx = source_x0 + (x + source_width - source_x0 % source_width) % source_width;
    let sy = source_y0 + (y + source_height - source_y0 % source_height) % source_height;
    target[(y * image_width + x) as usize] = source[(sy * image_width + sx) as usize];
}

#[cube(launch)]
pub(super) fn filter_source_alpha_region(
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
    target[ix] = source[ix] & 0xff00_0000u32;
}

#[cube(launch)]
pub(super) fn filter_svg_mask_coverage_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    kind: u32,
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
    let px = source[ix];
    let a = px >> 24;
    let mut mask_alpha = a;
    if kind == SVG_MASK_LUMINANCE {
        let mut safe_a = a;
        if safe_a == 0 {
            safe_a = 1;
        }
        let r = px & 255;
        let g = (px >> 8) & 255;
        let b = (px >> 16) & 255;
        let straight_r = (r * 255 + safe_a / 2) / safe_a;
        let straight_g = (g * 255 + safe_a / 2) / safe_a;
        let straight_b = (b * 255 + safe_a / 2) / safe_a;
        mask_alpha = ((2126 * straight_r + 7152 * straight_g + 722 * straight_b) * a + 1_275_000)
            / 2_550_000;
    }
    target[ix] = (mask_alpha << 24) | (mask_alpha << 16) | (mask_alpha << 8) | mask_alpha;
}

#[cube(launch)]
pub(super) fn filter_apply_region_mask(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    mask: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    let alpha = combine_alpha(target[ix] >> 24, mask[ix] >> 24);
    target[ix] = (alpha << 24) | (alpha << 16) | (alpha << 8) | alpha;
}

#[cube(launch)]
pub(super) fn filter_source_over_region(
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
    target[ix] = blend_premul_u8(target[ix], source[ix], 3 << 8);
}

#[cube(launch)]
pub(super) fn filter_blend_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    mode: u32,
    input1: &Array<u32>,
    input2: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    target[ix] = blend_premul_u8(input2[ix], input1[ix], mode);
}

#[cube(launch)]
#[allow(clippy::too_many_arguments)]
pub(super) fn filter_composite_inputs_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    operator: u32,
    k1: f32,
    k2: f32,
    k3: f32,
    k4: f32,
    input1: &Array<u32>,
    input2: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    target[ix] = composite_inputs_pixel(input1[ix], input2[ix], operator, k1, k2, k3, k4);
}

#[cube(launch)]
#[allow(clippy::too_many_arguments)]
pub(super) fn filter_displacement_map_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    image_height: u32,
    scale_x: f32,
    scale_y: f32,
    x_channel: u32,
    y_channel: u32,
    linear_rgb: u32,
    input1: &Array<u32>,
    input2: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    let map = input2[ix];
    let dx = filter_displacement_channel(map, x_channel, linear_rgb) - 0.5;
    let dy = filter_displacement_channel(map, y_channel, linear_rgb) - 0.5;
    let sx = (x as f32 + dx * scale_x).round() as i32;
    let sy = (y as f32 + dy * scale_y).round() as i32;
    let mut out = 0u32;
    if sx >= 0 && sx < image_width as i32 && sy >= 0 && sy < image_height as i32 {
        out = input1[(sy as u32 * image_width + sx as u32) as usize];
    }
    target[ix] = out;
}

#[cube(launch)]
pub(super) fn filter_morphology_axis_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    image_height: u32,
    radius: u32,
    operator: u32,
    axis: u32,
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
    let pos = if axis == 0 { x } else { y };
    let line_len = if axis == 0 { image_width } else { image_height };

    if operator == 0 && (pos < radius || pos + radius >= line_len) {
        target[ix] = 0;
        terminate!();
    }

    let mut out_r = 1.0;
    let mut out_g = 1.0;
    let mut out_b = 1.0;
    let mut out_a = 1.0;
    if operator == 1 {
        out_r = 0.0;
        out_g = 0.0;
        out_b = 0.0;
        out_a = 0.0;
    }

    let mut start = 0;
    if pos > radius {
        start = pos - radius;
    }
    let mut end = line_len - 1;
    if pos + radius < end {
        end = pos + radius;
    }

    let mut sample_pos = start;
    while sample_pos <= end {
        let sx = if axis == 0 { sample_pos } else { x };
        let sy = if axis == 0 { y } else { sample_pos };
        let sample = source[(sy * image_width + sx) as usize];
        let alpha = (sample >> 24) & 255;
        let sample_r = straight_channel(sample & 255, alpha);
        let sample_g = straight_channel((sample >> 8) & 255, alpha);
        let sample_b = straight_channel((sample >> 16) & 255, alpha);
        let sample_a = alpha as f32 / 255.0;

        if operator == 1 {
            if sample_r > out_r {
                out_r = sample_r;
            }
            if sample_g > out_g {
                out_g = sample_g;
            }
            if sample_b > out_b {
                out_b = sample_b;
            }
            if sample_a > out_a {
                out_a = sample_a;
            }
        } else {
            if sample_r < out_r {
                out_r = sample_r;
            }
            if sample_g < out_g {
                out_g = sample_g;
            }
            if sample_b < out_b {
                out_b = sample_b;
            }
            if sample_a < out_a {
                out_a = sample_a;
            }
        }
        sample_pos += 1;
    }

    target[ix] = pack_premul_rgba8(out_r * out_a, out_g * out_a, out_b * out_a, out_a);
}

#[cube(launch)]
pub(super) fn filter_color_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    filter_kind: u32,
    amount: f32,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    target[ix] = apply_color_filter_pixel(target[ix], filter_kind, amount);
}

#[cube(launch)]
#[allow(clippy::too_many_arguments)]
pub(super) fn filter_color_matrix_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
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
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    target[ix] = apply_color_matrix_pixel(
        target[ix], m00, m01, m02, m03, m04, m10, m11, m12, m13, m14, m20, m21, m22, m23, m24, m30,
        m31, m32, m33, m34,
    );
}

#[cube(launch)]
pub(super) fn filter_component_transfer_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    table_index: u32,
    transfer_tables: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    target[ix] = apply_component_transfer_pixel(target[ix], table_index, transfer_tables);
}
