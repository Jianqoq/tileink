#[cube(launch)]
#[allow(clippy::too_many_arguments)]
pub(super) fn filter_turbulence_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    base_frequency_x: f32,
    base_frequency_y: f32,
    num_octaves: u32,
    stitch_tiles: u32,
    kind: u32,
    linear_rgb: u32,
    table_index: u32,
    transform_x: f32,
    transform_y: f32,
    scale_x: f32,
    scale_y: f32,
    tile_x: f32,
    tile_y: f32,
    tile_width: f32,
    tile_height: f32,
    selectors: &Array<u32>,
    gradients: &Array<f32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    let selector_offset = table_index * TURBULENCE_TABLE_LEN_U32;
    let gradient_offset = table_index * TURBULENCE_GRADIENT_LEN_U32;
    target[ix] = filter_turbulence_pixel(
        x as f32,
        y as f32,
        tile_width,
        tile_height,
        base_frequency_x,
        base_frequency_y,
        num_octaves,
        stitch_tiles,
        kind,
        linear_rgb,
        transform_x,
        transform_y,
        scale_x,
        scale_y,
        tile_x,
        tile_y,
        selector_offset,
        gradient_offset,
        selectors,
        gradients,
    );
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn filter_turbulence_pixel(
    x: f32,
    y: f32,
    tile_width: f32,
    tile_height: f32,
    base_frequency_x: f32,
    base_frequency_y: f32,
    num_octaves: u32,
    stitch_tiles: u32,
    kind: u32,
    linear_rgb: u32,
    transform_x: f32,
    transform_y: f32,
    scale_x: f32,
    scale_y: f32,
    tile_x: f32,
    tile_y: f32,
    selector_offset: u32,
    gradient_offset: u32,
    selectors: &Array<u32>,
    gradients: &Array<f32>,
) -> u32 {
    let mut result = u32::new(0);
    if scale_x.abs() > f32::new(0.000_000_119_209_29_f32)
        && scale_y.abs() > f32::new(0.000_000_119_209_29_f32)
    {
        let sample_base_x = (x - transform_x) / scale_x;
        let sample_base_y = (y - transform_y) / scale_y;
        let local_tile_x = x - tile_x;
        let local_tile_y = y - tile_y;
        let mut frequency_x = base_frequency_x;
        let mut frequency_y = base_frequency_y;
        let mut stitch_width = i32::new(0);
        let mut stitch_height = i32::new(0);
        let mut stitch_wrap_x = i32::new(0);
        let mut stitch_wrap_y = i32::new(0);
        if stitch_tiles == 1 {
            let tw = tile_width.max(1.0);
            let th = tile_height.max(1.0);
            frequency_x = filter_stitch_frequency(frequency_x, tw);
            frequency_y = filter_stitch_frequency(frequency_y, th);
            stitch_width = (tw * frequency_x + 0.5) as i32;
            stitch_height = (th * frequency_y + 0.5) as i32;
            stitch_wrap_x =
                (local_tile_x * frequency_x + f32::new(4096.0_f32) + stitch_width as f32) as i32;
            stitch_wrap_y =
                (local_tile_y * frequency_y + f32::new(4096.0_f32) + stitch_height as f32) as i32;
        }

        let mut ratio = 1.0;
        let mut out_r = 0.0;
        let mut out_g = 0.0;
        let mut out_b = 0.0;
        let mut out_a = 0.0;
        let mut octave = 0;
        while octave < num_octaves {
            let sample_x = sample_base_x * frequency_x;
            let sample_y = sample_base_y * frequency_y;
            let r = filter_turbulence_noise2(
                0,
                sample_x,
                sample_y,
                stitch_tiles,
                stitch_wrap_x,
                stitch_width,
                stitch_wrap_y,
                stitch_height,
                selector_offset,
                gradient_offset,
                selectors,
                gradients,
            );
            let g = filter_turbulence_noise2(
                1,
                sample_x,
                sample_y,
                stitch_tiles,
                stitch_wrap_x,
                stitch_width,
                stitch_wrap_y,
                stitch_height,
                selector_offset,
                gradient_offset,
                selectors,
                gradients,
            );
            let b = filter_turbulence_noise2(
                2,
                sample_x,
                sample_y,
                stitch_tiles,
                stitch_wrap_x,
                stitch_width,
                stitch_wrap_y,
                stitch_height,
                selector_offset,
                gradient_offset,
                selectors,
                gradients,
            );
            let a = filter_turbulence_noise2(
                3,
                sample_x,
                sample_y,
                stitch_tiles,
                stitch_wrap_x,
                stitch_width,
                stitch_wrap_y,
                stitch_height,
                selector_offset,
                gradient_offset,
                selectors,
                gradients,
            );
            if kind == 0 {
                out_r += r.abs() * ratio;
                out_g += g.abs() * ratio;
                out_b += b.abs() * ratio;
                out_a += a.abs() * ratio;
            } else {
                out_r += r * ratio;
                out_g += g * ratio;
                out_b += b * ratio;
                out_a += a * ratio;
            }
            frequency_x *= 2.0;
            frequency_y *= 2.0;
            ratio *= 0.5;
            if stitch_tiles == 1 {
                stitch_width *= 2;
                stitch_height *= 2;
                stitch_wrap_x = 2 * stitch_wrap_x - 4096;
                stitch_wrap_y = 2 * stitch_wrap_y - 4096;
            }
            octave += 1;
        }

        if kind == 1 {
            out_r = out_r * 0.5 + 0.5;
            out_g = out_g * 0.5 + 0.5;
            out_b = out_b * 0.5 + 0.5;
            out_a = out_a * 0.5 + 0.5;
        }
        out_r = out_r.clamp(0.0, 1.0);
        out_g = out_g.clamp(0.0, 1.0);
        out_b = out_b.clamp(0.0, 1.0);
        out_a = out_a.clamp(0.0, 1.0);
        if linear_rgb == 1 {
            out_r = filter_linear_rgb_to_srgb(out_r);
            out_g = filter_linear_rgb_to_srgb(out_g);
            out_b = filter_linear_rgb_to_srgb(out_b);
        }
        result = pack_premul_rgba8(out_r * out_a, out_g * out_a, out_b * out_a, out_a);
    }
    result
}

#[cube]
fn filter_stitch_frequency(frequency: f32, tile_size: f32) -> f32 {
    let mut out = 0.0;
    if frequency > 0.0 && tile_size > 0.0 {
        let low = (tile_size * frequency).floor() / tile_size;
        let high = (tile_size * frequency).ceil() / tile_size;
        if low != 0.0 && frequency / low < high / frequency {
            out = low;
        } else {
            out = high;
        }
    }
    out
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn filter_turbulence_noise2(
    channel: u32,
    x: f32,
    y: f32,
    stitch_tiles: u32,
    stitch_wrap_x: i32,
    stitch_width: i32,
    stitch_wrap_y: i32,
    stitch_height: i32,
    selector_offset: u32,
    gradient_offset: u32,
    selectors: &Array<u32>,
    gradients: &Array<f32>,
) -> f32 {
    let tx = x + f32::new(4096.0_f32);
    let ty = y + f32::new(4096.0_f32);
    let mut bx0 = tx.floor() as i32;
    let mut bx1 = bx0 + 1;
    let mut by0 = ty.floor() as i32;
    let mut by1 = by0 + 1;
    let rx0 = tx - bx0 as f32;
    let rx1 = rx0 - 1.0;
    let ry0 = ty - by0 as f32;
    let ry1 = ry0 - 1.0;
    if stitch_tiles == 1 {
        if bx0 >= stitch_wrap_x {
            bx0 -= stitch_width;
        }
        if bx1 >= stitch_wrap_x {
            bx1 -= stitch_width;
        }
        if by0 >= stitch_wrap_y {
            by0 -= stitch_height;
        }
        if by1 >= stitch_wrap_y {
            by1 -= stitch_height;
        }
    }
    let bx0 = (bx0 & 255) as u32;
    let bx1 = (bx1 & 255) as u32;
    let by0 = (by0 & 255) as u32;
    let by1 = (by1 & 255) as u32;
    let i = selectors[(selector_offset + bx0) as usize];
    let j = selectors[(selector_offset + bx1) as usize];
    let b00 = selectors[(selector_offset + i + by0) as usize];
    let b10 = selectors[(selector_offset + j + by0) as usize];
    let b01 = selectors[(selector_offset + i + by1) as usize];
    let b11 = selectors[(selector_offset + j + by1) as usize];
    let sx = filter_turbulence_curve(rx0);
    let sy = filter_turbulence_curve(ry0);
    let a = filter_turbulence_lerp(
        filter_turbulence_gradient_dot(gradient_offset, channel, b00, rx0, ry0, gradients),
        filter_turbulence_gradient_dot(gradient_offset, channel, b10, rx1, ry0, gradients),
        sx,
    );
    let b = filter_turbulence_lerp(
        filter_turbulence_gradient_dot(gradient_offset, channel, b01, rx0, ry1, gradients),
        filter_turbulence_gradient_dot(gradient_offset, channel, b11, rx1, ry1, gradients),
        sx,
    );
    filter_turbulence_lerp(a, b, sy)
}

#[cube]
fn filter_turbulence_curve(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

#[cube]
fn filter_linear_rgb_to_srgb(value: f32) -> f32 {
    let mut out = value * f32::new(12.92_f32);
    if value > f32::new(0.003_130_8_f32) {
        out = f32::new(1.055_f32) * value.powf(f32::new(1.0_f32 / 2.4_f32)) - f32::new(0.055_f32);
    }
    out
}

#[cube]
fn filter_srgb_to_linear(value: f32) -> f32 {
    let mut out = value / f32::new(12.92_f32);
    if value > f32::new(0.040_45_f32) {
        out = ((value + f32::new(0.055_f32)) / f32::new(1.055_f32)).powf(f32::new(2.4_f32));
    }
    out
}

#[cube]
fn filter_turbulence_lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cube]
fn filter_turbulence_gradient_dot(
    gradient_offset: u32,
    channel: u32,
    selector: u32,
    x: f32,
    y: f32,
    gradients: &Array<f32>,
) -> f32 {
    let ix = gradient_offset + (channel * TURBULENCE_TABLE_LEN_U32 + selector) * 2;
    gradients[ix as usize] * x + gradients[(ix + 1) as usize] * y
}
