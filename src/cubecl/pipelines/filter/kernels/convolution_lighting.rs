#[cube(launch)]
#[allow(clippy::too_many_arguments)]
pub(super) fn filter_convolve_matrix_region(
    pixel_count: u32,
    region_width: u32,
    region_height: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    kernel_offset: u32,
    columns: u32,
    rows: u32,
    target_x: u32,
    target_y: u32,
    divisor: f32,
    bias: f32,
    edge_mode: u32,
    preserve_alpha: u32,
    kernels: &Array<f32>,
    source: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let dst_ix = (y * image_width + x) as usize;

    if columns == 0 || rows == 0 || divisor == 0.0 {
        target[dst_ix] = source[dst_ix];
        terminate!();
    }

    let region_x1 = (region_x0 + region_width) as i32;
    let region_y1 = (region_y0 + region_height) as i32;
    let mut out_r = 0.0;
    let mut out_g = 0.0;
    let mut out_b = 0.0;
    let mut out_a = 0.0;
    let mut ky = 0;
    while ky < rows {
        let mut kx = 0;
        while kx < columns {
            let kernel_ix = kernel_offset + (rows - 1 - ky) * columns + (columns - 1 - kx);
            let weight = kernels[kernel_ix as usize];
            let mut sx = x as i32 + kx as i32 - target_x as i32;
            let mut sy = y as i32 + ky as i32 - target_y as i32;
            let mut sample = u32::new(0);
            if edge_mode == 1 {
                sx = sx.clamp(region_x0 as i32, region_x1 - 1);
                sy = sy.clamp(region_y0 as i32, region_y1 - 1);
                sample = source[(sy as u32 * image_width + sx as u32) as usize];
            } else if edge_mode == 2 {
                while sx < region_x0 as i32 {
                    sx += region_width as i32;
                }
                while sx >= region_x1 {
                    sx -= region_width as i32;
                }
                while sy < region_y0 as i32 {
                    sy += region_height as i32;
                }
                while sy >= region_y1 {
                    sy -= region_height as i32;
                }
                sample = source[(sy as u32 * image_width + sx as u32) as usize];
            } else if sx >= region_x0 as i32
                && sx < region_x1
                && sy >= region_y0 as i32
                && sy < region_y1
            {
                sample = source[(sy as u32 * image_width + sx as u32) as usize];
            }

            let alpha = (sample >> 24) & 255;
            out_r += straight_channel(sample & 255, alpha) * weight;
            out_g += straight_channel((sample >> 8) & 255, alpha) * weight;
            out_b += straight_channel((sample >> 16) & 255, alpha) * weight;
            out_a += (alpha as f32 / 255.0) * weight;
            kx += 1;
        }
        ky += 1;
    }

    let base_alpha = (source[dst_ix] >> 24) as f32 / 255.0;
    let mut alpha = (out_a / divisor + bias).clamp(0.0, 1.0);
    if preserve_alpha == 1 {
        alpha = base_alpha;
    }
    let r = (out_r / divisor + bias).clamp(0.0, 1.0);
    let g = (out_g / divisor + bias).clamp(0.0, 1.0);
    let b = (out_b / divisor + bias).clamp(0.0, 1.0);
    target[dst_ix] = pack_premul_rgba8(r * alpha, g * alpha, b * alpha, alpha);
}

#[cube(launch)]
#[allow(clippy::too_many_arguments)]
pub(super) fn filter_lighting_region(
    pixel_count: u32,
    region_width: u32,
    region_height: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    output_kind: u32,
    surface_scale: f32,
    light_constant: f32,
    specular_exponent: f32,
    light_r: f32,
    light_g: f32,
    light_b: f32,
    surface_origin_x: i32,
    surface_origin_y: i32,
    light_kind: u32,
    p0: f32,
    p1: f32,
    p2: f32,
    p3: f32,
    p4: f32,
    p5: f32,
    p6: f32,
    p7: f32,
    _p8: f32,
    source: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let dst_ix = (y * image_width + x) as usize;
    let mut no_light = u32::new(0);
    if output_kind == 0 {
        no_light = u32::new(0xff00_0000i64);
    }

    // Match SVG lighting semantics: source alpha is the surface height. Diffuse
    // writes opaque lit RGB, while specular derives alpha from the highlight.
    let alpha = source_alpha_at(source, x, y, image_width);
    let z = alpha * surface_scale;
    let dx = alpha_gradient_x(
        source,
        x,
        y,
        image_width,
        region_x0,
        region_width,
        region_y0,
        region_height,
    ) * surface_scale;
    let dy = alpha_gradient_y(
        source,
        x,
        y,
        image_width,
        region_x0,
        region_width,
        region_y0,
        region_height,
    ) * surface_scale;
    let normal_len = (dx * dx + dy * dy + 1.0).sqrt();
    let nx = -dx / normal_len;
    let ny = -dy / normal_len;
    let nz = 1.0 / normal_len;

    // Filter surfaces are often rendered in local offscreen coordinates while
    // SVG light positions stay in the original user-space coordinate system.
    let world_x = surface_origin_x as f32 + x as f32 + 0.5;
    let world_y = surface_origin_y as f32 + y as f32 + 0.5;
    let mut lx = p0 - world_x;
    let mut ly = p1 - world_y;
    let mut lz = p2 - z;
    let mut attenuation = 1.0;
    let eps = 0.000_001;

    if light_kind == 0 {
        let azimuth = p0 * f32::new(0.017_453_292_f32);
        let elevation = p1 * f32::new(0.017_453_292_f32);
        lx = azimuth.cos() * elevation.cos();
        ly = azimuth.sin() * elevation.cos();
        lz = elevation.sin();
    } else {
        let len = (lx * lx + ly * ly + lz * lz).sqrt();
        if len <= eps {
            target[dst_ix] = no_light;
            terminate!();
        }
        lx /= len;
        ly /= len;
        lz /= len;

        if light_kind == 2 {
            let mut sx = p3 - p0;
            let mut sy = p4 - p1;
            let mut sz = p5 - p2;
            let slen = (sx * sx + sy * sy + sz * sz).sqrt();
            if slen <= eps {
                target[dst_ix] = no_light;
                terminate!();
            }
            sx /= slen;
            sy /= slen;
            sz /= slen;
            let focus = (-(lx * sx + ly * sy + lz * sz)).max(0.0);
            if p7 >= 0.0 && focus < (p7 * f32::new(0.017_453_292_f32)).cos() {
                target[dst_ix] = no_light;
                terminate!();
            }
            attenuation = focus.powf(p6.max(0.0));
        }
    }

    if output_kind == 0 {
        let amount = light_constant * attenuation * (nx * lx + ny * ly + nz * lz).max(0.0);
        target[dst_ix] = pack_premul_rgba8(
            (light_r * amount).clamp(0.0, 1.0),
            (light_g * amount).clamp(0.0, 1.0),
            (light_b * amount).clamp(0.0, 1.0),
            1.0,
        );
    } else {
        let mut hx = lx;
        let mut hy = ly;
        let mut hz = lz + 1.0;
        let hlen = (hx * hx + hy * hy + hz * hz).sqrt();
        if hlen <= eps {
            target[dst_ix] = no_light;
            terminate!();
        }
        hx /= hlen;
        hy /= hlen;
        hz /= hlen;

        let normal_dot_half = (nx * hx + ny * hy + nz * hz).max(0.0);
        let amount =
            light_constant * attenuation * normal_dot_half.powf(specular_exponent.max(0.0));
        let r = (light_r * amount).clamp(0.0, 1.0);
        let g = (light_g * amount).clamp(0.0, 1.0);
        let b = (light_b * amount).clamp(0.0, 1.0);
        target[dst_ix] = pack_premul_rgba8(r, g, b, r.max(g).max(b));
    }
}
