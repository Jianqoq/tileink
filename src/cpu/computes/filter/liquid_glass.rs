use crate::shared::{
    bounds::Bounds,
    image::Image,
    layer::filter::{
        LIQUID_GLASS_ACTIVE_DISTANCE_NORM, LIQUID_GLASS_BLUR_STD_DEV_SCALE,
        LIQUID_GLASS_CHROMATIC_B, LIQUID_GLASS_CHROMATIC_G, LIQUID_GLASS_CHROMATIC_R,
        LIQUID_GLASS_D65_WHITE, LIQUID_GLASS_EDGE_BLEND_END, LIQUID_GLASS_EDGE_BLEND_START,
        LIQUID_GLASS_EPSILON, LIQUID_GLASS_FRESNEL_LIGHTNESS_GAIN, LIQUID_GLASS_FRESNEL_MIX_SCALE,
        LIQUID_GLASS_GEOMETRY_DISTANCE_SCALE, LIQUID_GLASS_GEOMETRY_RANGE_SCALE,
        LIQUID_GLASS_GLARE_CHROMA_GAIN, LIQUID_GLASS_GLARE_LIGHTNESS_GAIN,
        LIQUID_GLASS_GLARE_POWER_BASE, LIQUID_GLASS_GLARE_POWER_SCALE,
        LIQUID_GLASS_GLARE_SIDE_SCALE, LIQUID_GLASS_NORMAL_LENGTH_SCALE,
        LIQUID_GLASS_REFRACTION_PIXEL_SCALE, LIQUID_GLASS_TINT_BASE_MIX, LIQUID_GLASS_TINT_MIX,
        RectLiquidGlass, RectLiquidGlassRegion,
    },
    pixel::{pack_premul_rgba8, unpack_premul_rgba8},
};

pub(super) fn apply(
    image: &mut Image,
    bounds: Bounds,
    surface_size: (u32, u32),
    glass: RectLiquidGlass,
    region: RectLiquidGlassRegion,
) {
    if image.width == 0 || image.height == 0 {
        return;
    }

    let source = image.clone();
    let mut blurred = source.clone();
    super::apply_gaussian_blur_with_sampling(
        &mut blurred,
        glass.blur_radius as f32 * LIQUID_GLASS_BLUR_STD_DEV_SCALE,
        glass.blur_radius as f32 * LIQUID_GLASS_BLUR_STD_DEV_SCALE,
        glass.blur_sampling,
        bounds,
    );

    let surface_height = surface_size.1.max(1) as f32;
    let normal_len = LIQUID_GLASS_NORMAL_LENGTH_SCALE / surface_height;
    let tint = glass.tint.components;
    for y in 0..image.height {
        let world_y = bounds.y0 as f32 + y as f32 + 0.5;
        for x in 0..image.width {
            let world_x = bounds.x0 as f32 + x as f32 + 0.5;
            let distance = round_rect_distance(world_x, world_y, region);
            let distance_norm = distance / surface_height;
            let base = sample_straight_at(&source, x, y);
            let mut out = base;

            if distance_norm < LIQUID_GLASS_ACTIVE_DISTANCE_NORM {
                let inside_distance = -distance;
                let edge = glass_edge(glass, inside_distance);
                if edge <= 0.0 {
                    out = sample_straight_bilinear(&blurred, x as f32, y as f32);
                    out = mix_straight(
                        out,
                        [tint[0], tint[1], tint[2], 1.0],
                        tint[3] * LIQUID_GLASS_TINT_MIX,
                    );
                } else {
                    let edge_h =
                        inside_distance / glass.refraction_thickness.max(LIQUID_GLASS_EPSILON);
                    let blur_mix = if glass.blur_edge { 1.0 } else { edge_h };
                    let normal = round_rect_unit_normal(world_x, world_y, region);
                    let offset = [
                        -normal[0] * edge * LIQUID_GLASS_REFRACTION_PIXEL_SCALE,
                        -normal[1] * edge * LIQUID_GLASS_REFRACTION_PIXEL_SCALE,
                    ];
                    out = dispersion_sample(
                        &source, &blurred, x as f32, y as f32, offset, blur_mix, glass,
                    );
                    let refracted = out;
                    out = mix_straight(
                        out,
                        [tint[0], tint[1], tint[2], 1.0],
                        tint[3] * LIQUID_GLASS_TINT_MIX,
                    );

                    let fresnel = fresnel_factor(glass, distance);
                    let mut fresnel_tint = srgb_to_lch(mix_rgb(
                        [1.0; 3],
                        [tint[0], tint[1], tint[2]],
                        tint[3] * LIQUID_GLASS_TINT_BASE_MIX,
                    ));
                    fresnel_tint[0] = (fresnel_tint[0]
                        + LIQUID_GLASS_FRESNEL_LIGHTNESS_GAIN
                            * fresnel
                            * percent(glass.fresnel_factor))
                    .clamp(0.0, 100.0);
                    let fresnel_color = lch_to_srgb(fresnel_tint);
                    out = mix_straight(
                        out,
                        [fresnel_color[0], fresnel_color[1], fresnel_color[2], 1.0],
                        fresnel
                            * percent(glass.fresnel_factor)
                            * LIQUID_GLASS_FRESNEL_MIX_SCALE
                            * normal_len,
                    );

                    let glare_geo = glare_geometry(glass, distance);
                    let glare_angle = (vec2_angle(normal) - std::f32::consts::FRAC_PI_4
                        + glass.glare_angle)
                        * 2.0;
                    let far_side = (glare_angle > std::f32::consts::PI * 1.5
                        && glare_angle < std::f32::consts::PI * 3.5)
                        || glare_angle < -std::f32::consts::FRAC_PI_2;
                    let side = if far_side {
                        LIQUID_GLASS_GLARE_SIDE_SCALE * percent(glass.glare_opposite_factor)
                    } else {
                        LIQUID_GLASS_GLARE_SIDE_SCALE
                    };
                    let glare_angle_factor = ((0.5 + glare_angle.sin() * 0.5)
                        * side
                        * percent(glass.glare_factor))
                    .powf(
                        LIQUID_GLASS_GLARE_POWER_BASE
                            + percent(glass.glare_convergence) * LIQUID_GLASS_GLARE_POWER_SCALE,
                    )
                    .clamp(0.0, 1.0);
                    let mut glare_tint = srgb_to_lch(mix_rgb(
                        [refracted[0], refracted[1], refracted[2]],
                        [tint[0], tint[1], tint[2]],
                        tint[3] * LIQUID_GLASS_TINT_BASE_MIX,
                    ));
                    glare_tint[0] = (glare_tint[0]
                        + LIQUID_GLASS_GLARE_LIGHTNESS_GAIN * glare_angle_factor * glare_geo)
                        .clamp(0.0, 120.0);
                    glare_tint[1] +=
                        LIQUID_GLASS_GLARE_CHROMA_GAIN * glare_angle_factor * glare_geo;
                    let glare_color = lch_to_srgb(glare_tint);
                    out = mix_straight(
                        out,
                        [glare_color[0], glare_color[1], glare_color[2], 1.0],
                        glare_angle_factor * glare_geo * normal_len,
                    );
                }
            }

            let edge_mix = smoothstep(
                LIQUID_GLASS_EDGE_BLEND_START,
                LIQUID_GLASS_EDGE_BLEND_END,
                distance_norm,
            );
            out = mix_straight(out, base, edge_mix);
            image.pixels[(y * image.width + x) as usize] = pack_straight_rgba8(out);
        }
    }
}

fn glass_edge(glass: RectLiquidGlass, inside_distance: f32) -> f32 {
    let ratio = 1.0 - inside_distance / glass.refraction_thickness.max(LIQUID_GLASS_EPSILON);
    let theta_i = safe_asin(ratio.powi(2));
    let theta_t = safe_asin(theta_i.sin() / glass.refraction_factor.max(1.0));
    let mut edge = -((theta_t - theta_i).tan());
    if inside_distance >= glass.refraction_thickness {
        edge = 0.0;
    }
    edge.max(0.0)
}

fn fresnel_factor(glass: RectLiquidGlass, distance: f32) -> f32 {
    (1.0 + distance / LIQUID_GLASS_GEOMETRY_DISTANCE_SCALE
        * (LIQUID_GLASS_GEOMETRY_RANGE_SCALE / glass.fresnel_range.max(LIQUID_GLASS_EPSILON))
            .powi(2)
        + percent(glass.fresnel_hardness))
    .powi(5)
    .clamp(0.0, 1.0)
}

fn glare_geometry(glass: RectLiquidGlass, distance: f32) -> f32 {
    (1.0 + distance / LIQUID_GLASS_GEOMETRY_DISTANCE_SCALE
        * (LIQUID_GLASS_GEOMETRY_RANGE_SCALE / glass.glare_range.max(LIQUID_GLASS_EPSILON)).powi(2)
        + percent(glass.glare_hardness))
    .powi(5)
    .clamp(0.0, 1.0)
}

fn dispersion_sample(
    source: &Image,
    blurred: &Image,
    x: f32,
    y: f32,
    offset: [f32; 2],
    blur_mix: f32,
    glass: RectLiquidGlass,
) -> [f32; 4] {
    let r = dispersion_channel(
        source,
        blurred,
        x,
        y,
        offset,
        LIQUID_GLASS_CHROMATIC_R,
        0,
        blur_mix,
        glass,
    );
    let g = dispersion_channel(
        source,
        blurred,
        x,
        y,
        offset,
        LIQUID_GLASS_CHROMATIC_G,
        1,
        blur_mix,
        glass,
    );
    let b = dispersion_channel(
        source,
        blurred,
        x,
        y,
        offset,
        LIQUID_GLASS_CHROMATIC_B,
        2,
        blur_mix,
        glass,
    );
    let alpha = sample_straight_bilinear(source, x + offset[0], y + offset[1])[3]
        .max(sample_straight_bilinear(blurred, x + offset[0], y + offset[1])[3]);
    [r, g, b, alpha]
}

#[allow(clippy::too_many_arguments)]
fn dispersion_channel(
    source: &Image,
    blurred: &Image,
    x: f32,
    y: f32,
    offset: [f32; 2],
    chromatic: f32,
    channel: usize,
    blur_mix: f32,
    glass: RectLiquidGlass,
) -> f32 {
    let factor = 1.0 - (chromatic - 1.0) * glass.refraction_dispersion;
    let sx = x + offset[0] * factor;
    let sy = y + offset[1] * factor;
    let src = sample_straight_bilinear(source, sx, sy)[channel];
    let blur = sample_straight_bilinear(blurred, sx, sy)[channel];
    src + (blur - src) * blur_mix
}

fn sample_straight_at(image: &Image, x: u32, y: u32) -> [f32; 4] {
    premul_to_straight(unpack_premul_rgba8(
        image.pixels[(y * image.width + x) as usize],
    ))
}

fn sample_straight_bilinear(image: &Image, x: f32, y: f32) -> [f32; 4] {
    let sx = x.clamp(0.0, image.width.saturating_sub(1) as f32);
    let sy = y.clamp(0.0, image.height.saturating_sub(1) as f32);
    let x0 = sx.floor() as u32;
    let y0 = sy.floor() as u32;
    let x1 = (x0 + 1).min(image.width - 1);
    let y1 = (y0 + 1).min(image.height - 1);
    let tx = sx - x0 as f32;
    let ty = sy - y0 as f32;
    let tl = unpack_premul_rgba8(image.pixels[(y0 * image.width + x0) as usize]);
    let tr = unpack_premul_rgba8(image.pixels[(y0 * image.width + x1) as usize]);
    let bl = unpack_premul_rgba8(image.pixels[(y1 * image.width + x0) as usize]);
    let br = unpack_premul_rgba8(image.pixels[(y1 * image.width + x1) as usize]);
    premul_to_straight(mix_premul(
        mix_premul(tl, tr, tx),
        mix_premul(bl, br, tx),
        ty,
    ))
}

fn premul_to_straight(mut rgba: [f32; 4]) -> [f32; 4] {
    if rgba[3] > LIQUID_GLASS_EPSILON {
        rgba[0] /= rgba[3];
        rgba[1] /= rgba[3];
        rgba[2] /= rgba[3];
    }
    rgba
}

fn pack_straight_rgba8(rgba: [f32; 4]) -> u32 {
    let a = rgba[3].clamp(0.0, 1.0);
    pack_premul_rgba8([
        rgba[0].clamp(0.0, 1.0) * a,
        rgba[1].clamp(0.0, 1.0) * a,
        rgba[2].clamp(0.0, 1.0) * a,
        a,
    ])
}

fn mix_straight(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

fn mix_rgb(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn mix_premul(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

fn round_rect_unit_normal(x: f32, y: f32, region: RectLiquidGlassRegion) -> [f32; 2] {
    let eps = 1.0;
    let dx = (round_rect_distance(x + eps, y, region) - round_rect_distance(x - eps, y, region))
        / (2.0 * eps);
    let dy = (round_rect_distance(x, y + eps, region) - round_rect_distance(x, y - eps, region))
        / (2.0 * eps);
    let len = (dx * dx + dy * dy).sqrt();
    if len <= LIQUID_GLASS_EPSILON {
        [0.0, -1.0]
    } else {
        [dx / len, dy / len]
    }
}

fn round_rect_distance(x: f32, y: f32, region: RectLiquidGlassRegion) -> f32 {
    let cx = (region.x0 + region.x1) * 0.5;
    let cy = (region.y0 + region.y1) * 0.5;
    let hx = ((region.x1 - region.x0) * 0.5).max(0.0);
    let hy = ((region.y1 - region.y0) * 0.5).max(0.0);
    let px = x - cx;
    let py = y - cy;
    let radius = corner_radius(px, py, region).min(hx).min(hy).max(0.0);
    let ax = px.abs();
    let ay = py.abs();
    if radius <= 0.0 {
        let dx = ax - hx;
        let dy = ay - hy;
        return dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0);
    }
    let qx = ax - hx + radius;
    let qy = ay - hy + radius;
    qx.max(qy).min(0.0) + qx.max(0.0).hypot(qy.max(0.0)) - radius
}

fn corner_radius(px: f32, py: f32, region: RectLiquidGlassRegion) -> f32 {
    if px >= 0.0 {
        if py <= 0.0 {
            region.radius_top_right
        } else {
            region.radius_bottom_right
        }
    } else if py <= 0.0 {
        region.radius_top_left
    } else {
        region.radius_bottom_left
    }
}

fn srgb_to_lch(srgb: [f32; 3]) -> [f32; 3] {
    let lab = xyz_to_lab(rgb_to_xyz([
        uncompand_srgb(srgb[0]),
        uncompand_srgb(srgb[1]),
        uncompand_srgb(srgb[2]),
    ]));
    [
        lab[0],
        (lab[1] * lab[1] + lab[2] * lab[2]).sqrt(),
        lab[2].atan2(lab[1]).to_degrees(),
    ]
}

fn lch_to_srgb(lch: [f32; 3]) -> [f32; 3] {
    let hue = lch[2].to_radians();
    let lab = [lch[0], lch[1] * hue.cos(), lch[1] * hue.sin()];
    xyz_to_srgb(lab_to_xyz(lab))
}

fn rgb_to_xyz(rgb: [f32; 3]) -> [f32; 3] {
    [
        rgb[0] * 0.4124 + rgb[1] * 0.3576 + rgb[2] * 0.1805,
        rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722,
        rgb[0] * 0.0193 + rgb[1] * 0.1192 + rgb[2] * 0.9505,
    ]
}

fn xyz_to_srgb(xyz: [f32; 3]) -> [f32; 3] {
    [
        compand_rgb(xyz[0] * 3.2406255 + xyz[1] * -1.537208 + xyz[2] * -0.4986286),
        compand_rgb(xyz[0] * -0.9689307 + xyz[1] * 1.8757561 + xyz[2] * 0.0415175),
        compand_rgb(xyz[0] * 0.0557101 + xyz[1] * -0.2040211 + xyz[2] * 1.0569959),
    ]
}

fn xyz_to_lab(xyz: [f32; 3]) -> [f32; 3] {
    let x = xyz_to_lab_f(xyz[0] / LIQUID_GLASS_D65_WHITE[0]);
    let y = xyz_to_lab_f(xyz[1] / LIQUID_GLASS_D65_WHITE[1]);
    let z = xyz_to_lab_f(xyz[2] / LIQUID_GLASS_D65_WHITE[2]);
    [116.0 * y - 16.0, 500.0 * (x - y), 200.0 * (y - z)]
}

fn lab_to_xyz(lab: [f32; 3]) -> [f32; 3] {
    let w = (lab[0] + 16.0) / 116.0;
    [
        LIQUID_GLASS_D65_WHITE[0] * lab_to_xyz_f(w + lab[1] / 500.0),
        LIQUID_GLASS_D65_WHITE[1] * lab_to_xyz_f(w),
        LIQUID_GLASS_D65_WHITE[2] * lab_to_xyz_f(w - lab[2] / 200.0),
    ]
}

fn xyz_to_lab_f(x: f32) -> f32 {
    if x > 0.008_856_452 {
        x.powf(1.0 / 3.0)
    } else {
        7.787037 * x + 0.13793103
    }
}

fn lab_to_xyz_f(x: f32) -> f32 {
    if x > 0.206897 {
        x * x * x
    } else {
        0.12841855 * (x - 0.13793103)
    }
}

fn uncompand_srgb(a: f32) -> f32 {
    if a > 0.04045 {
        ((a + 0.055) / 1.055).powf(2.4)
    } else {
        a / 12.92
    }
}

fn compand_rgb(a: f32) -> f32 {
    if a <= 0.0031308 {
        12.92 * a
    } else {
        1.055 * a.max(0.0).powf(1.0 / 2.4) - 0.055
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn vec2_angle(v: [f32; 2]) -> f32 {
    if v[0].hypot(v[1]) < 1e-8 {
        return 0.0;
    }
    let angle = v[1].atan2(v[0]);
    if angle < 0.0 {
        angle + std::f32::consts::TAU
    } else {
        angle
    }
}

fn percent(value: f32) -> f32 {
    value * 0.01
}

fn safe_asin(value: f32) -> f32 {
    value.clamp(-1.0, 1.0).asin()
}
