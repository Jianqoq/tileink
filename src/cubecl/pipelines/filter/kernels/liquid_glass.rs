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
    let base = source[ix];
    let surface_height = image_height.max(1) as f32;
    let distance_norm = distance / surface_height;
    if distance_norm >= LIQUID_GLASS_ACTIVE_DISTANCE_NORM {
        target[ix] = base;
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
    let inside_distance = -distance;
    let edge = liquid_glass_edge(inside_distance, refraction_thickness, refraction_factor);
    let mut blur_mix = inside_distance / refraction_thickness.max(LIQUID_GLASS_EPSILON);
    if blur_edge == 1 {
        blur_mix = 1.0;
    }
    blur_mix = blur_mix.clamp(0.0, 1.0);

    let normal_len = LIQUID_GLASS_NORMAL_LENGTH_SCALE / surface_height;
    let mut r = liquid_glass_sample_straight_channel(
        blurred,
        x as f32,
        y as f32,
        image_width,
        image_height,
        0,
    );
    let mut g = liquid_glass_sample_straight_channel(
        blurred,
        x as f32,
        y as f32,
        image_width,
        image_height,
        1,
    );
    let mut b = liquid_glass_sample_straight_channel(
        blurred,
        x as f32,
        y as f32,
        image_width,
        image_height,
        2,
    );
    let mut a = liquid_glass_sample_straight_channel(
        blurred,
        x as f32,
        y as f32,
        image_width,
        image_height,
        3,
    );

    if edge <= 0.0 {
        r = liquid_glass_mix(r, tint_r, tint_a * LIQUID_GLASS_TINT_MIX);
        g = liquid_glass_mix(g, tint_g, tint_a * LIQUID_GLASS_TINT_MIX);
        b = liquid_glass_mix(b, tint_b, tint_a * LIQUID_GLASS_TINT_MIX);
        a = liquid_glass_mix(a, 1.0, tint_a * LIQUID_GLASS_TINT_MIX);
    } else {
        let offset_x = -nx * edge * LIQUID_GLASS_REFRACTION_PIXEL_SCALE;
        let offset_y = -ny * edge * LIQUID_GLASS_REFRACTION_PIXEL_SCALE;
        r = liquid_glass_dispersion_channel(
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
        g = liquid_glass_dispersion_channel(
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
        b = liquid_glass_dispersion_channel(
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
        a = liquid_glass_sample_alpha(
            source,
            blurred,
            x as f32 + offset_x,
            y as f32 + offset_y,
            image_width,
            image_height,
        );
        let blurred_r = r;
        let blurred_g = g;
        let blurred_b = b;
        r = liquid_glass_mix(r, tint_r, tint_a * LIQUID_GLASS_TINT_MIX);
        g = liquid_glass_mix(g, tint_g, tint_a * LIQUID_GLASS_TINT_MIX);
        b = liquid_glass_mix(b, tint_b, tint_a * LIQUID_GLASS_TINT_MIX);
        a = liquid_glass_mix(a, 1.0, tint_a * LIQUID_GLASS_TINT_MIX);

        let fresnel = liquid_glass_fresnel(distance, fresnel_range, fresnel_hardness);
        let fresnel_base_r = liquid_glass_mix(1.0, tint_r, tint_a * LIQUID_GLASS_TINT_BASE_MIX);
        let fresnel_base_g = liquid_glass_mix(1.0, tint_g, tint_a * LIQUID_GLASS_TINT_BASE_MIX);
        let fresnel_base_b = liquid_glass_mix(1.0, tint_b, tint_a * LIQUID_GLASS_TINT_BASE_MIX);
        let mut fresnel_l = liquid_glass_srgb_to_lch_l(fresnel_base_r, fresnel_base_g, fresnel_base_b);
        let fresnel_c = liquid_glass_srgb_to_lch_c(fresnel_base_r, fresnel_base_g, fresnel_base_b);
        let fresnel_h = liquid_glass_srgb_to_lch_h(fresnel_base_r, fresnel_base_g, fresnel_base_b);
        fresnel_l = (fresnel_l + LIQUID_GLASS_FRESNEL_LIGHTNESS_GAIN * fresnel * fresnel_factor).clamp(0.0, 100.0);
        let fresnel_mix = fresnel * fresnel_factor * LIQUID_GLASS_FRESNEL_MIX_SCALE * normal_len;
        r = liquid_glass_mix(r, liquid_glass_lch_to_srgb_r(fresnel_l, fresnel_c, fresnel_h), fresnel_mix);
        g = liquid_glass_mix(g, liquid_glass_lch_to_srgb_g(fresnel_l, fresnel_c, fresnel_h), fresnel_mix);
        b = liquid_glass_mix(b, liquid_glass_lch_to_srgb_b(fresnel_l, fresnel_c, fresnel_h), fresnel_mix);
        a = liquid_glass_mix(a, 1.0, fresnel_mix);

        let glare_geo = liquid_glass_glare_geometry(distance, glare_range, glare_hardness);
        let glare_angle_factor = liquid_glass_glare_angle(
            nx,
            ny,
            glare_convergence,
            glare_opposite_factor,
            glare_factor,
            glare_angle,
        );
        let glare_base_r = liquid_glass_mix(blurred_r, tint_r, tint_a * LIQUID_GLASS_TINT_BASE_MIX);
        let glare_base_g = liquid_glass_mix(blurred_g, tint_g, tint_a * LIQUID_GLASS_TINT_BASE_MIX);
        let glare_base_b = liquid_glass_mix(blurred_b, tint_b, tint_a * LIQUID_GLASS_TINT_BASE_MIX);
        let mut glare_l = liquid_glass_srgb_to_lch_l(glare_base_r, glare_base_g, glare_base_b);
        let mut glare_c = liquid_glass_srgb_to_lch_c(glare_base_r, glare_base_g, glare_base_b);
        let glare_h = liquid_glass_srgb_to_lch_h(glare_base_r, glare_base_g, glare_base_b);
        glare_l = (glare_l + LIQUID_GLASS_GLARE_LIGHTNESS_GAIN * glare_angle_factor * glare_geo).clamp(0.0, 120.0);
        glare_c += LIQUID_GLASS_GLARE_CHROMA_GAIN * glare_angle_factor * glare_geo;
        let glare_mix = glare_angle_factor * glare_geo * normal_len;
        r = liquid_glass_mix(r, liquid_glass_lch_to_srgb_r(glare_l, glare_c, glare_h), glare_mix);
        g = liquid_glass_mix(g, liquid_glass_lch_to_srgb_g(glare_l, glare_c, glare_h), glare_mix);
        b = liquid_glass_mix(b, liquid_glass_lch_to_srgb_b(glare_l, glare_c, glare_h), glare_mix);
        a = liquid_glass_mix(a, 1.0, glare_mix);
    }

    let edge_mix = liquid_glass_smoothstep(LIQUID_GLASS_EDGE_BLEND_START, LIQUID_GLASS_EDGE_BLEND_END, distance_norm);
    r = liquid_glass_mix(r, liquid_glass_pixel_straight_channel(base, 0), edge_mix);
    g = liquid_glass_mix(g, liquid_glass_pixel_straight_channel(base, 1), edge_mix);
    b = liquid_glass_mix(b, liquid_glass_pixel_straight_channel(base, 2), edge_mix);
    a = liquid_glass_mix(a, liquid_glass_pixel_straight_channel(base, 3), edge_mix);
    target[ix] = liquid_glass_pack_straight_rgba8(r, g, b, a);
}

#[cube]
fn liquid_glass_edge(inside_distance: f32, refraction_thickness: f32, refraction_factor: f32) -> f32 {
    let thickness = refraction_thickness.max(LIQUID_GLASS_EPSILON);
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
    (1.0 + distance / LIQUID_GLASS_GEOMETRY_DISTANCE_SCALE * (LIQUID_GLASS_GEOMETRY_RANGE_SCALE / fresnel_range.max(LIQUID_GLASS_EPSILON)).powf(2.0) + fresnel_hardness)
        .powf(5.0)
        .clamp(0.0, 1.0)
}

#[cube]
fn liquid_glass_glare_geometry(distance: f32, glare_range: f32, glare_hardness: f32) -> f32 {
    (1.0 + distance / LIQUID_GLASS_GEOMETRY_DISTANCE_SCALE * (LIQUID_GLASS_GEOMETRY_RANGE_SCALE / glare_range.max(LIQUID_GLASS_EPSILON)).powf(2.0) + glare_hardness)
        .powf(5.0)
        .clamp(0.0, 1.0)
}

#[cube]
#[allow(clippy::useless_conversion)]
fn liquid_glass_glare_angle(
    nx: f32,
    ny: f32,
    glare_convergence: f32,
    glare_opposite_factor: f32,
    glare_factor: f32,
    glare_angle: f32,
) -> f32 {
    let angle = (liquid_glass_vec2_angle(nx, ny) - LIQUID_GLASS_PI * 0.25 + glare_angle) * 2.0;
    let side = if (angle > LIQUID_GLASS_PI * 1.5 && angle < LIQUID_GLASS_PI * 3.5)
        || angle < -LIQUID_GLASS_PI * 0.5
    {
        LIQUID_GLASS_GLARE_SIDE_SCALE * glare_opposite_factor
    } else {
        // CubeCL needs this branch expanded to a shader value; plain clippy sees
        // the conversion before macro expansion and flags it as redundant.
        LIQUID_GLASS_GLARE_SIDE_SCALE.into()
    };
    ((0.5 + angle.sin() * 0.5) * side * glare_factor)
        .powf(LIQUID_GLASS_GLARE_POWER_BASE + glare_convergence * LIQUID_GLASS_GLARE_POWER_SCALE)
        .clamp(0.0, 1.0)
}

#[cube]
fn liquid_glass_vec2_angle(x: f32, y: f32) -> f32 {
    let len = (x * x + y * y).sqrt();
    let mut angle = 0.0;
    if len >= 0.000_000_01 {
        angle = y.atan2(x);
        if angle < 0.0 {
            angle += 2.0 * LIQUID_GLASS_PI;
        }
    }
    angle
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
    let src = liquid_glass_sample_straight_channel(source, sx, sy, image_width, image_height, channel);
    let blur = liquid_glass_sample_straight_channel(blurred, sx, sy, image_width, image_height, channel);
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
    liquid_glass_sample_straight_channel(source, x, y, image_width, image_height, 3)
        .max(liquid_glass_sample_straight_channel(blurred, x, y, image_width, image_height, 3))
}

#[cube]
fn liquid_glass_sample_straight_channel(
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
    let tl = liquid_glass_pixel_straight_channel(image[(y0 * image_width + x0) as usize], channel);
    let tr = liquid_glass_pixel_straight_channel(image[(y0 * image_width + x1) as usize], channel);
    let bl = liquid_glass_pixel_straight_channel(image[(y1 * image_width + x0) as usize], channel);
    let br = liquid_glass_pixel_straight_channel(image[(y1 * image_width + x1) as usize], channel);
    liquid_glass_mix(liquid_glass_mix(tl, tr, tx), liquid_glass_mix(bl, br, tx), ty)
}

#[cube]
fn liquid_glass_pixel_straight_channel(px: u32, channel: u32) -> f32 {
    let a = ((px >> 24) & 255) as f32 / 255.0;
    let mut value = px & 255;
    if channel == 1 {
        value = (px >> 8) & 255;
    } else if channel == 2 {
        value = (px >> 16) & 255;
    } else if channel == 3 {
        value = (px >> 24) & 255;
    }
    let mut out = value as f32 / 255.0;
    if channel != 3 && a > LIQUID_GLASS_EPSILON {
        out /= a;
    }
    out
}

#[cube]
fn liquid_glass_pack_straight_rgba8(r: f32, g: f32, b: f32, a: f32) -> u32 {
    let a = a.clamp(0.0, 1.0);
    pack_premul_rgba8(
        r.clamp(0.0, 1.0) * a,
        g.clamp(0.0, 1.0) * a,
        b.clamp(0.0, 1.0) * a,
        a,
    )
}

#[cube]
fn liquid_glass_mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cube]
fn liquid_glass_smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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
    let eps = 1.0;
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
    if len > LIQUID_GLASS_EPSILON {
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
    let eps = 1.0;
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
    if len > LIQUID_GLASS_EPSILON {
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

#[cube]
fn liquid_glass_srgb_to_lch_l(r: f32, g: f32, b: f32) -> f32 {
    let y = liquid_glass_xyz_to_lab_f(liquid_glass_srgb_to_xyz_y(r, g, b) / LIQUID_GLASS_D65_Y);
    116.0 * y - 16.0
}

#[cube]
fn liquid_glass_srgb_to_lch_c(r: f32, g: f32, b: f32) -> f32 {
    let l_a = liquid_glass_srgb_to_lab_a(r, g, b);
    let l_b = liquid_glass_srgb_to_lab_b(r, g, b);
    (l_a * l_a + l_b * l_b).sqrt()
}

#[cube]
fn liquid_glass_srgb_to_lch_h(r: f32, g: f32, b: f32) -> f32 {
    liquid_glass_srgb_to_lab_b(r, g, b).atan2(liquid_glass_srgb_to_lab_a(r, g, b)) * 57.295_78
}

#[cube]
fn liquid_glass_srgb_to_lab_a(r: f32, g: f32, b: f32) -> f32 {
    let x = liquid_glass_xyz_to_lab_f(liquid_glass_srgb_to_xyz_x(r, g, b) / LIQUID_GLASS_D65_X);
    let y = liquid_glass_xyz_to_lab_f(liquid_glass_srgb_to_xyz_y(r, g, b) / LIQUID_GLASS_D65_Y);
    500.0 * (x - y)
}

#[cube]
fn liquid_glass_srgb_to_lab_b(r: f32, g: f32, b: f32) -> f32 {
    let y = liquid_glass_xyz_to_lab_f(liquid_glass_srgb_to_xyz_y(r, g, b) / LIQUID_GLASS_D65_Y);
    let z = liquid_glass_xyz_to_lab_f(liquid_glass_srgb_to_xyz_z(r, g, b) / LIQUID_GLASS_D65_Z);
    200.0 * (y - z)
}

#[cube]
fn liquid_glass_srgb_to_xyz_x(r: f32, g: f32, b: f32) -> f32 {
    let lr = liquid_glass_uncompand_srgb(r);
    let lg = liquid_glass_uncompand_srgb(g);
    let lb = liquid_glass_uncompand_srgb(b);
    lr * 0.4124 + lg * 0.3576 + lb * 0.1805
}

#[cube]
fn liquid_glass_srgb_to_xyz_y(r: f32, g: f32, b: f32) -> f32 {
    let lr = liquid_glass_uncompand_srgb(r);
    let lg = liquid_glass_uncompand_srgb(g);
    let lb = liquid_glass_uncompand_srgb(b);
    lr * 0.2126 + lg * 0.7152 + lb * 0.0722
}

#[cube]
fn liquid_glass_srgb_to_xyz_z(r: f32, g: f32, b: f32) -> f32 {
    let lr = liquid_glass_uncompand_srgb(r);
    let lg = liquid_glass_uncompand_srgb(g);
    let lb = liquid_glass_uncompand_srgb(b);
    lr * 0.0193 + lg * 0.1192 + lb * 0.9505
}

#[cube]
fn liquid_glass_lch_to_srgb_r(l: f32, c: f32, h: f32) -> f32 {
    let xyz_x = liquid_glass_lch_to_xyz_x(l, c, h);
    let xyz_y = liquid_glass_lch_to_xyz_y(l, c, h);
    let xyz_z = liquid_glass_lch_to_xyz_z(l, c, h);
    liquid_glass_compand_rgb(xyz_x * 3.240_625_5 + xyz_y * -1.537_208 + xyz_z * -0.498_628_6)
}

#[cube]
fn liquid_glass_lch_to_srgb_g(l: f32, c: f32, h: f32) -> f32 {
    let xyz_x = liquid_glass_lch_to_xyz_x(l, c, h);
    let xyz_y = liquid_glass_lch_to_xyz_y(l, c, h);
    let xyz_z = liquid_glass_lch_to_xyz_z(l, c, h);
    liquid_glass_compand_rgb(xyz_x * -0.968_930_7 + xyz_y * 1.875_756_1 + xyz_z * 0.041_517_5)
}

#[cube]
fn liquid_glass_lch_to_srgb_b(l: f32, c: f32, h: f32) -> f32 {
    let xyz_x = liquid_glass_lch_to_xyz_x(l, c, h);
    let xyz_y = liquid_glass_lch_to_xyz_y(l, c, h);
    let xyz_z = liquid_glass_lch_to_xyz_z(l, c, h);
    liquid_glass_compand_rgb(xyz_x * 0.055_710_1 + xyz_y * -0.204_021_1 + xyz_z * 1.056_995_9)
}

#[cube]
fn liquid_glass_lch_to_xyz_x(l: f32, c: f32, h: f32) -> f32 {
    let hue = h * 0.017_453_292;
    let lab_a = c * hue.cos();
    let w = (l + 16.0) / 116.0;
    LIQUID_GLASS_D65_X * liquid_glass_lab_to_xyz_f(w + lab_a / 500.0)
}

#[cube]
fn liquid_glass_lch_to_xyz_y(l: f32, _c: f32, _h: f32) -> f32 {
    let w = (l + 16.0) / 116.0;
    LIQUID_GLASS_D65_Y * liquid_glass_lab_to_xyz_f(w)
}

#[cube]
fn liquid_glass_lch_to_xyz_z(l: f32, c: f32, h: f32) -> f32 {
    let hue = h * 0.017_453_292;
    let lab_b = c * hue.sin();
    let w = (l + 16.0) / 116.0;
    LIQUID_GLASS_D65_Z * liquid_glass_lab_to_xyz_f(w - lab_b / 200.0)
}

#[cube]
fn liquid_glass_xyz_to_lab_f(x: f32) -> f32 {
    let mut out = 7.787_037 * x + 0.137_931_03;
    if x > 0.008_856_452 {
        out = x.powf(0.333_333_34);
    }
    out
}

#[cube]
fn liquid_glass_lab_to_xyz_f(x: f32) -> f32 {
    let mut out = 0.128_418_55 * (x - 0.137_931_03);
    if x > 0.206_897 {
        out = x * x * x;
    }
    out
}

#[cube]
fn liquid_glass_uncompand_srgb(a: f32) -> f32 {
    let mut out = a / 12.92;
    if a > 0.04045 {
        out = ((a + 0.055) / 1.055).powf(2.4);
    }
    out
}

#[cube]
fn liquid_glass_compand_rgb(a: f32) -> f32 {
    let mut out = 12.92 * a;
    if a > 0.003_130_8 {
        out = 1.055 * a.powf(0.416_666_66) - 0.055;
    }
    out
}
