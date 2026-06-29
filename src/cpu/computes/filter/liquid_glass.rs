use crate::shared::{
    bounds::Bounds,
    image::Image,
    layer::filter::{LiquidGlass, LiquidGlassRegion},
    pixel::{pack_premul_rgba8, unpack_premul_rgba8},
};

const CHROMATIC_R: f32 = 0.98;
const CHROMATIC_G: f32 = 1.0;
const CHROMATIC_B: f32 = 1.02;

pub(super) fn apply(
    image: &mut Image,
    bounds: Bounds,
    glass: LiquidGlass,
    region: LiquidGlassRegion,
) {
    if image.width == 0 || image.height == 0 {
        return;
    }

    let source = image.clone();
    let mut blurred = source.clone();
    super::apply_gaussian_blur(&mut blurred, glass.blur_std_dev, glass.blur_std_dev);

    let tint = glass.tint.premultiply().components;
    for y in 0..image.height {
        let world_y = bounds.y0 as f32 + y as f32 + 0.5;
        for x in 0..image.width {
            let world_x = bounds.x0 as f32 + x as f32 + 0.5;
            let distance = round_rect_distance(world_x, world_y, region);
            let ix = (y * image.width + x) as usize;

            // The backdrop layer applies the exact region mask after filtering.
            // We still write sensible outside pixels so standalone LiquidGlass
            // filters remain deterministic when no backdrop mask is present.
            if distance >= 0.5 {
                image.pixels[ix] = source.pixels[ix];
                continue;
            }

            let normal = round_rect_normal(world_x, world_y, region);
            let inside_distance = (-distance).max(0.0);
            let edge = glass_edge(glass, inside_distance);
            let edge_mix = if glass.blur_edge {
                1.0
            } else {
                (inside_distance / glass.refraction_thickness.max(1e-6)).clamp(0.0, 1.0)
            };
            let offset = [
                -normal[0] * edge * glass.refraction_strength.max(0.0),
                -normal[1] * edge * glass.refraction_strength.max(0.0),
            ];

            let mut out = dispersion_sample(
                &source, &blurred, x as f32, y as f32, offset, edge_mix, glass,
            );
            out = mix_premul(out, tint, tint[3] * 0.8);

            let fresnel = fresnel_factor(glass, distance);
            out = mix_premul(
                out,
                [1.0, 1.0, 1.0, 1.0],
                fresnel * glass.fresnel_factor * 0.007,
            );

            let glare = glare_factor(glass, distance, normal);
            out = mix_premul(out, [1.0, 1.0, 1.0, 1.0], glare);
            out[3] = out[3].max(source_alpha_at(&source, x, y));
            image.pixels[ix] = pack_premul_rgba8(out);
        }
    }
}

fn glass_edge(glass: LiquidGlass, inside_distance: f32) -> f32 {
    let thickness = glass.refraction_thickness.max(1e-6);
    if inside_distance >= thickness {
        return 0.0;
    }
    let ratio = 1.0 - inside_distance / thickness;
    let theta_i = safe_asin(ratio.powi(2));
    let theta_t = safe_asin((theta_i.sin() / glass.refraction_factor.max(1.0)).clamp(-1.0, 1.0));
    (-((theta_t - theta_i).tan())).max(0.0)
}

fn fresnel_factor(glass: LiquidGlass, distance: f32) -> f32 {
    let range = glass.fresnel_range.max(1e-6);
    (1.0 + distance / 1500.0 * (500.0 / range).powi(2) + glass.fresnel_hardness * 0.01)
        .max(0.0)
        .powi(5)
        .clamp(0.0, 1.0)
}

fn glare_factor(glass: LiquidGlass, distance: f32, normal: [f32; 2]) -> f32 {
    let range = glass.glare_range.max(1e-6);
    let geometry =
        (1.0 + distance / 1500.0 * (500.0 / range).powi(2) + glass.glare_hardness * 0.01)
            .max(0.0)
            .powi(5)
            .clamp(0.0, 1.0);
    let angle = (normal[1].atan2(normal[0]) - std::f32::consts::FRAC_PI_4 + glass.glare_angle)
        .rem_euclid(std::f32::consts::TAU)
        * 2.0;
    let far_side = angle > std::f32::consts::PI * 1.5 || angle < -std::f32::consts::PI * 0.5;
    let side = if far_side {
        1.2 * glass.glare_opposite_factor * 0.01
    } else {
        1.2
    };
    let angular = ((0.5 + angle.sin() * 0.5) * side * glass.glare_factor * 0.01)
        .clamp(0.0, 1.0)
        .powf(0.1 + glass.glare_convergence * 0.02);
    angular * geometry
}

fn dispersion_sample(
    source: &Image,
    blurred: &Image,
    x: f32,
    y: f32,
    offset: [f32; 2],
    blur_mix: f32,
    glass: LiquidGlass,
) -> [f32; 4] {
    let sampler = DispersionSampler {
        source,
        blurred,
        offset,
        blur_mix,
        glass,
    };
    let r = sampler.channel(x, y, CHROMATIC_R, 0);
    let g = sampler.channel(x, y, CHROMATIC_G, 1);
    let b = sampler.channel(x, y, CHROMATIC_B, 2);
    let alpha = sample_premul_bilinear(source, x + offset[0], y + offset[1])[3]
        .max(sample_premul_bilinear(blurred, x + offset[0], y + offset[1])[3]);
    [r.min(alpha), g.min(alpha), b.min(alpha), alpha]
}

struct DispersionSampler<'a> {
    source: &'a Image,
    blurred: &'a Image,
    offset: [f32; 2],
    blur_mix: f32,
    glass: LiquidGlass,
}

impl DispersionSampler<'_> {
    fn channel(&self, x: f32, y: f32, chromatic: f32, channel: usize) -> f32 {
        let factor = 1.0 - (chromatic - 1.0) * self.glass.refraction_dispersion;
        let sx = x + self.offset[0] * factor;
        let sy = y + self.offset[1] * factor;
        let src = sample_premul_bilinear(self.source, sx, sy)[channel];
        let blur = sample_premul_bilinear(self.blurred, sx, sy)[channel];
        src + (blur - src) * self.blur_mix.clamp(0.0, 1.0)
    }
}

fn sample_premul_bilinear(image: &Image, x: f32, y: f32) -> [f32; 4] {
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
    let top = mix_premul(tl, tr, tx);
    let bottom = mix_premul(bl, br, tx);
    mix_premul(top, bottom, ty)
}

fn source_alpha_at(source: &Image, x: u32, y: u32) -> f32 {
    ((source.pixels[(y * source.width + x) as usize] >> 24) & 255) as f32 / 255.0
}

fn mix_premul(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

fn round_rect_normal(x: f32, y: f32, region: LiquidGlassRegion) -> [f32; 2] {
    let eps = 0.5;
    let dx = round_rect_distance(x + eps, y, region) - round_rect_distance(x - eps, y, region);
    let dy = round_rect_distance(x, y + eps, region) - round_rect_distance(x, y - eps, region);
    let len = (dx * dx + dy * dy).sqrt();
    if len <= f32::EPSILON {
        [0.0, -1.0]
    } else {
        [dx / len, dy / len]
    }
}

fn round_rect_distance(x: f32, y: f32, region: LiquidGlassRegion) -> f32 {
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

fn corner_radius(px: f32, py: f32, region: LiquidGlassRegion) -> f32 {
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

fn safe_asin(value: f32) -> f32 {
    value.clamp(-1.0, 1.0).asin()
}
