const LIQUID_GLASS_CHROMATIC_R: f32 = 0.98;
const LIQUID_GLASS_CHROMATIC_G: f32 = 1.0;
const LIQUID_GLASS_CHROMATIC_B: f32 = 1.02;
const LIQUID_GLASS_PI: f32 = std::f32::consts::PI;
const LIQUID_GLASS_TAU: f32 = std::f32::consts::TAU;

#[cube(launch)]
#[allow(clippy::too_many_arguments)]
pub(super) fn filter_liquid_glass_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    image_height: u32,
    shape_x0: f32,
    shape_y0: f32,
    shape_x1: f32,
    shape_y1: f32,
    radius_top_left: f32,
    radius_top_right: f32,
    radius_bottom_left: f32,
    radius_bottom_right: f32,
    blur_edge: u32,
    tint_r: f32,
    tint_g: f32,
    tint_b: f32,
    tint_a: f32,
    refraction_thickness: f32,
    refraction_factor: f32,
    refraction_strength: f32,
    refraction_dispersion: f32,
    fresnel_range: f32,
    fresnel_hardness: f32,
    fresnel_factor: f32,
    glare_range: f32,
    glare_hardness: f32,
    glare_convergence: f32,
    glare_opposite_factor: f32,
    glare_factor: f32,
    glare_angle: f32,
    source: &Array<u32>,
    blurred: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    let world_x = x as f32 + 0.5;
    let world_y = y as f32 + 0.5;

    let distance = liquid_glass_round_rect_distance(
        world_x,
        world_y,
        shape_x0,
        shape_y0,
        shape_x1,
        shape_y1,
        radius_top_left,
        radius_top_right,
        radius_bottom_left,
        radius_bottom_right,
    );
    if distance >= 0.5 {
        target[ix] = source[ix];
        terminate!();
    }

    let nx = liquid_glass_normal_x(
        world_x,
        world_y,
        shape_x0,
        shape_y0,
        shape_x1,
        shape_y1,
        radius_top_left,
        radius_top_right,
        radius_bottom_left,
        radius_bottom_right,
    );
    let ny = liquid_glass_normal_y(
        world_x,
        world_y,
        shape_x0,
        shape_y0,
        shape_x1,
        shape_y1,
        radius_top_left,
        radius_top_right,
        radius_bottom_left,
        radius_bottom_right,
    );
    let inside_distance = (-distance).max(0.0);
    let edge = liquid_glass_edge(inside_distance, refraction_thickness, refraction_factor);
    let mut blur_mix = inside_distance / refraction_thickness.max(0.000_001);
    if blur_edge == 1 {
        blur_mix = 1.0;
    }
    blur_mix = blur_mix.clamp(0.0, 1.0);

    let offset_x = -nx * edge * refraction_strength.max(0.0);
    let offset_y = -ny * edge * refraction_strength.max(0.0);
    let out_r = liquid_glass_dispersion_channel(
        source,
        blurred,
        x as f32,
        y as f32,
        offset_x,
        offset_y,
        LIQUID_GLASS_CHROMATIC_R,
        0,
        blur_mix,
        refraction_dispersion,
        image_width,
        image_height,
    );
    let out_g = liquid_glass_dispersion_channel(
        source,
        blurred,
        x as f32,
        y as f32,
        offset_x,
        offset_y,
        LIQUID_GLASS_CHROMATIC_G,
        1,
        blur_mix,
        refraction_dispersion,
        image_width,
        image_height,
    );
    let out_b = liquid_glass_dispersion_channel(
        source,
        blurred,
        x as f32,
        y as f32,
        offset_x,
        offset_y,
        LIQUID_GLASS_CHROMATIC_B,
        2,
        blur_mix,
        refraction_dispersion,
        image_width,
        image_height,
    );
    let sample_alpha = liquid_glass_sample_alpha(
        source,
        blurred,
        x as f32 + offset_x,
        y as f32 + offset_y,
        image_width,
        image_height,
    );

    let tint_mix = (tint_a * 0.8).clamp(0.0, 1.0);
    let mut r = liquid_glass_mix(out_r, tint_r, tint_mix);
    let mut g = liquid_glass_mix(out_g, tint_g, tint_mix);
    let mut b = liquid_glass_mix(out_b, tint_b, tint_mix);
    let mut a = liquid_glass_mix(sample_alpha, tint_a, tint_mix);

    let fresnel = liquid_glass_fresnel(distance, fresnel_range, fresnel_hardness)
        * fresnel_factor
        * 0.007;
    r = liquid_glass_mix(r, 1.0, fresnel);
    g = liquid_glass_mix(g, 1.0, fresnel);
    b = liquid_glass_mix(b, 1.0, fresnel);
    a = liquid_glass_mix(a, 1.0, fresnel);

    let glare = liquid_glass_glare(
        distance,
        nx,
        ny,
        glare_range,
        glare_hardness,
        glare_convergence,
        glare_opposite_factor,
        glare_factor,
        glare_angle,
    );
    r = liquid_glass_mix(r, 1.0, glare);
    g = liquid_glass_mix(g, 1.0, glare);
    b = liquid_glass_mix(b, 1.0, glare);
    a = liquid_glass_mix(a, 1.0, glare).max((source[ix] >> 24) as f32 / 255.0);
    target[ix] = pack_premul_rgba8(r.min(a), g.min(a), b.min(a), a);
}

#[cube]
fn liquid_glass_edge(inside_distance: f32, refraction_thickness: f32, refraction_factor: f32) -> f32 {
    let thickness = refraction_thickness.max(0.000_001);
    let mut out = 0.0;
    if inside_distance < thickness {
        let ratio = 1.0 - inside_distance / thickness;
        let theta_i = ratio.powf(2.0).clamp(-1.0, 1.0).asin();
        let theta_t = (theta_i.sin() / refraction_factor.max(1.0)).clamp(-1.0, 1.0).asin();
        out = (-(theta_t - theta_i).tan()).max(0.0);
    }
    out
}

#[cube]
fn liquid_glass_fresnel(distance: f32, fresnel_range: f32, fresnel_hardness: f32) -> f32 {
    let range = fresnel_range.max(0.000_001);
    (1.0 + distance / 1500.0 * (500.0 / range).powf(2.0) + fresnel_hardness * 0.01)
        .max(0.0)
        .powf(5.0)
        .clamp(0.0, 1.0)
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn liquid_glass_glare(
    distance: f32,
    nx: f32,
    ny: f32,
    glare_range: f32,
    glare_hardness: f32,
    glare_convergence: f32,
    glare_opposite_factor: f32,
    glare_factor: f32,
    glare_angle: f32,
) -> f32 {
    let range = glare_range.max(0.000_001);
    let geometry = (1.0 + distance / 1500.0 * (500.0 / range).powf(2.0) + glare_hardness * 0.01)
        .max(0.0)
        .powf(5.0)
        .clamp(0.0, 1.0);
    let raw_angle = ny.atan2(nx) - LIQUID_GLASS_PI * 0.25 + glare_angle;
    let mut angle = raw_angle - (raw_angle / LIQUID_GLASS_TAU).floor() * LIQUID_GLASS_TAU;
    if angle < 0.0 {
        angle += LIQUID_GLASS_TAU;
    }
    angle *= 2.0;
    let mut side = 1.2;
    if angle > LIQUID_GLASS_PI * 1.5 || angle < -LIQUID_GLASS_PI * 0.5 {
        side = 1.2 * glare_opposite_factor * 0.01;
    }
    let angular = ((0.5 + angle.sin() * 0.5) * side * glare_factor * 0.01)
        .clamp(0.0, 1.0)
        .powf(0.1 + glare_convergence * 0.02);
    angular * geometry
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn liquid_glass_dispersion_channel(
    source: &Array<u32>,
    blurred: &Array<u32>,
    x: f32,
    y: f32,
    offset_x: f32,
    offset_y: f32,
    chromatic: f32,
    channel: u32,
    blur_mix: f32,
    refraction_dispersion: f32,
    image_width: u32,
    image_height: u32,
) -> f32 {
    let factor = 1.0 - (chromatic - 1.0) * refraction_dispersion;
    let sx = x + offset_x * factor;
    let sy = y + offset_y * factor;
    let src = liquid_glass_sample_channel(source, sx, sy, image_width, image_height, channel);
    let blur = liquid_glass_sample_channel(blurred, sx, sy, image_width, image_height, channel);
    liquid_glass_mix(src, blur, blur_mix)
}

#[cube]
fn liquid_glass_sample_alpha(
    source: &Array<u32>,
    blurred: &Array<u32>,
    x: f32,
    y: f32,
    image_width: u32,
    image_height: u32,
) -> f32 {
    liquid_glass_sample_channel(source, x, y, image_width, image_height, 3)
        .max(liquid_glass_sample_channel(blurred, x, y, image_width, image_height, 3))
}

#[cube]
fn liquid_glass_sample_channel(
    image: &Array<u32>,
    x: f32,
    y: f32,
    image_width: u32,
    image_height: u32,
    channel: u32,
) -> f32 {
    let sx = x.clamp(0.0, (image_width - 1) as f32);
    let sy = y.clamp(0.0, (image_height - 1) as f32);
    let x0 = sx.floor() as u32;
    let y0 = sy.floor() as u32;
    let x1 = (x0 + 1).min(image_width - 1);
    let y1 = (y0 + 1).min(image_height - 1);
    let tx = sx - x0 as f32;
    let ty = sy - y0 as f32;
    let tl = liquid_glass_pixel_channel(image[(y0 * image_width + x0) as usize], channel);
    let tr = liquid_glass_pixel_channel(image[(y0 * image_width + x1) as usize], channel);
    let bl = liquid_glass_pixel_channel(image[(y1 * image_width + x0) as usize], channel);
    let br = liquid_glass_pixel_channel(image[(y1 * image_width + x1) as usize], channel);
    liquid_glass_mix(liquid_glass_mix(tl, tr, tx), liquid_glass_mix(bl, br, tx), ty)
}

#[cube]
fn liquid_glass_pixel_channel(px: u32, channel: u32) -> f32 {
    let mut value = px & 255;
    if channel == 1 {
        value = (px >> 8) & 255;
    } else if channel == 2 {
        value = (px >> 16) & 255;
    } else if channel == 3 {
        value = (px >> 24) & 255;
    }
    value as f32 / 255.0
}

#[cube]
fn liquid_glass_mix(a: f32, b: f32, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    a + (b - a) * t
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn liquid_glass_normal_x(
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
    let eps = 0.5;
    let dx = liquid_glass_round_rect_distance(
        x + eps,
        y,
        x0,
        y0,
        x1,
        y1,
        radius_top_left,
        radius_top_right,
        radius_bottom_left,
        radius_bottom_right,
    ) - liquid_glass_round_rect_distance(
        x - eps,
        y,
        x0,
        y0,
        x1,
        y1,
        radius_top_left,
        radius_top_right,
        radius_bottom_left,
        radius_bottom_right,
    );
    let dy = liquid_glass_round_rect_distance(
        x,
        y + eps,
        x0,
        y0,
        x1,
        y1,
        radius_top_left,
        radius_top_right,
        radius_bottom_left,
        radius_bottom_right,
    ) - liquid_glass_round_rect_distance(
        x,
        y - eps,
        x0,
        y0,
        x1,
        y1,
        radius_top_left,
        radius_top_right,
        radius_bottom_left,
        radius_bottom_right,
    );
    let len = (dx * dx + dy * dy).sqrt();
    let mut out = 0.0;
    if len > 0.000_001 {
        out = dx / len;
    }
    out
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn liquid_glass_normal_y(
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
    let eps = 0.5;
    let dx = liquid_glass_round_rect_distance(
        x + eps,
        y,
        x0,
        y0,
        x1,
        y1,
        radius_top_left,
        radius_top_right,
        radius_bottom_left,
        radius_bottom_right,
    ) - liquid_glass_round_rect_distance(
        x - eps,
        y,
        x0,
        y0,
        x1,
        y1,
        radius_top_left,
        radius_top_right,
        radius_bottom_left,
        radius_bottom_right,
    );
    let dy = liquid_glass_round_rect_distance(
        x,
        y + eps,
        x0,
        y0,
        x1,
        y1,
        radius_top_left,
        radius_top_right,
        radius_bottom_left,
        radius_bottom_right,
    ) - liquid_glass_round_rect_distance(
        x,
        y - eps,
        x0,
        y0,
        x1,
        y1,
        radius_top_left,
        radius_top_right,
        radius_bottom_left,
        radius_bottom_right,
    );
    let len = (dx * dx + dy * dy).sqrt();
    let mut out = f32::new(-1.0_f32);
    if len > 0.000_001 {
        out = dy / len;
    }
    out
}

#[cube]
#[allow(clippy::too_many_arguments)]
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
    let hx = ((x1 - x0) * 0.5).max(0.0);
    let hy = ((y1 - y0) * 0.5).max(0.0);
    let px = x - cx;
    let py = y - cy;
    let radius = liquid_glass_corner_radius(
        px,
        py,
        radius_top_left,
        radius_top_right,
        radius_bottom_left,
        radius_bottom_right,
    )
    .min(hx)
    .min(hy)
    .max(0.0);
    let ax = px.abs();
    let ay = py.abs();
    let dx = ax - hx;
    let dy = ay - hy;
    let ox = dx.max(0.0);
    let oy = dy.max(0.0);
    let mut out = (ox * ox + oy * oy).sqrt() + dx.max(dy).min(0.0);
    if radius > 0.0 {
        let qx = ax - hx + radius;
        let qy = ay - hy + radius;
        let ox = qx.max(0.0);
        let oy = qy.max(0.0);
        out = qx.max(qy).min(0.0) + (ox * ox + oy * oy).sqrt() - radius;
    }
    out
}

#[cube]
fn liquid_glass_corner_radius(
    px: f32,
    py: f32,
    radius_top_left: f32,
    radius_top_right: f32,
    radius_bottom_left: f32,
    radius_bottom_right: f32,
) -> f32 {
    let mut radius = radius_top_left;
    if px >= 0.0 {
        if py <= 0.0 {
            radius = radius_top_right;
        } else {
            radius = radius_bottom_right;
        }
    } else if py > 0.0 {
        radius = radius_bottom_left;
    }
    radius
}
