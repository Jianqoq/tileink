use peniko::{Color, kurbo};

use crate::shared::{
    bounds::Bounds,
    brush::Brush,
    image::Image,
    layer::{backdrop::BackdropRegion, blend::Blend, mask::MaskMode},
    pixel::{pack_premul_rgba8, src_over, unpack_premul_rgba8},
};

/// Static liquid-glass appearance inspired by `liquid-glass-react`.
///
/// The effect combines a frosted backdrop, edge-only refraction, chromatic
/// aberration, deterministic multi-sample scattering, a subtle tint, and
/// directional rim lighting.
#[derive(Clone, Copy, Debug)]
pub struct LiquidGlass {
    pub blur_radius: f32,
    pub saturation: f32,
    pub displacement_scale: f32,
    pub aberration_intensity: f32,
    pub edge_width: f32,
    /// Maximum disk radius, in pixels, used to scatter refracted backdrop samples.
    pub scatter_radius: f32,
    /// Blend amount between the refracted sample and the scattered average.
    pub scatter_strength: f32,
    /// Number of deterministic disk samples. Values above 16 are clamped.
    pub scatter_samples: u32,
    /// Surface roughness multiplier applied to `scatter_radius`.
    pub roughness: f32,
    pub tint: Color,
    pub tint_strength: f32,
    pub highlight_strength: f32,
    pub shadow_strength: f32,
    pub light_direction: [f32; 2],
}

impl Default for LiquidGlass {
    fn default() -> Self {
        Self {
            blur_radius: 6.0,
            saturation: 1.4,
            displacement_scale: 12.0,
            aberration_intensity: 2.0,
            edge_width: 18.0,
            scatter_radius: 3.0,
            scatter_strength: 0.28,
            scatter_samples: 8,
            roughness: 0.75,
            tint: Color::from_rgb8(220, 238, 255),
            tint_strength: 0.07,
            highlight_strength: 0.36,
            shadow_strength: 0.12,
            light_direction: [-0.65, -0.76],
        }
    }
}

#[derive(Clone, Debug)]
pub enum Filter {
    Blur(f32),
    Brightness(f32),
    Contrast(f32),
    Grayscale(f32),
    HueRotate(f32),
    Invert(f32),
    Opacity(f32),
    Saturate(f32),
    Sepia(f32),
    DropShadow {
        offset_x: f32,
        offset_y: f32,
        radius: f32,
        brush: Brush,
    },
    LiquidGlass(LiquidGlass),
}

impl Filter {
    pub(crate) fn apply(&self, image: &mut Image) {
        match self {
            Filter::Blur(radius) => blur(image, *radius),
            Filter::Brightness(amount) => map_image_color(image, |rgb, alpha| {
                ([rgb[0] * amount, rgb[1] * amount, rgb[2] * amount], alpha)
            }),
            Filter::Contrast(amount) => map_image_color(image, |rgb, alpha| {
                (
                    [
                        (rgb[0] - 0.5) * amount + 0.5,
                        (rgb[1] - 0.5) * amount + 0.5,
                        (rgb[2] - 0.5) * amount + 0.5,
                    ],
                    alpha,
                )
            }),
            Filter::Grayscale(amount) => map_image_color(image, |rgb, alpha| {
                let amount = amount.clamp(0.0, 1.0);
                let gray = luminance(rgb);
                (
                    [
                        mix(rgb[0], gray, amount),
                        mix(rgb[1], gray, amount),
                        mix(rgb[2], gray, amount),
                    ],
                    alpha,
                )
            }),
            Filter::HueRotate(degrees) => {
                map_image_color(image, |rgb, alpha| (hue_rotate(rgb, *degrees), alpha))
            }
            Filter::Invert(amount) => map_image_color(image, |rgb, alpha| {
                let amount = amount.clamp(0.0, 1.0);
                (
                    [
                        mix(rgb[0], 1.0 - rgb[0], amount),
                        mix(rgb[1], 1.0 - rgb[1], amount),
                        mix(rgb[2], 1.0 - rgb[2], amount),
                    ],
                    alpha,
                )
            }),
            Filter::Opacity(amount) => map_image_premul(image, |mut px| {
                let amount = amount.clamp(0.0, 1.0);
                for c in &mut px {
                    *c *= amount;
                }
                px
            }),
            Filter::Saturate(amount) => map_image_color(image, |rgb, alpha| {
                let gray = luminance(rgb);
                (
                    [
                        gray + (rgb[0] - gray) * amount,
                        gray + (rgb[1] - gray) * amount,
                        gray + (rgb[2] - gray) * amount,
                    ],
                    alpha,
                )
            }),
            Filter::Sepia(amount) => map_image_color(image, |rgb, alpha| {
                let amount = amount.clamp(0.0, 1.0);
                let sepia = [
                    rgb[0] * 0.393 + rgb[1] * 0.769 + rgb[2] * 0.189,
                    rgb[0] * 0.349 + rgb[1] * 0.686 + rgb[2] * 0.168,
                    rgb[0] * 0.272 + rgb[1] * 0.534 + rgb[2] * 0.131,
                ];
                (
                    [
                        mix(rgb[0], sepia[0], amount),
                        mix(rgb[1], sepia[1], amount),
                        mix(rgb[2], sepia[2], amount),
                    ],
                    alpha,
                )
            }),
            Filter::DropShadow {
                offset_x,
                offset_y,
                radius,
                brush,
            } => apply_drop_shadow(image, *offset_x, *offset_y, *radius, brush),
            Filter::LiquidGlass(glass) => {
                blur(image, glass.blur_radius);
                apply_saturation(image, glass.saturation);
            }
        }
    }

    pub(crate) fn apply_backdrop(&self, image: &mut Image, region: &BackdropRegion, mask: &Image) {
        match self {
            Filter::LiquidGlass(glass) => apply_liquid_glass(image, region, mask, *glass),
            _ => self.apply(image),
        }
    }
}

fn blur(image: &mut Image, radius: f32) {
    let radius = radius.ceil().max(0.0) as i32;
    if radius == 0 || image.width == 0 || image.height == 0 {
        return;
    }

    let width = image.width as i32;
    let height = image.height as i32;
    let kernel = gaussian_kernel(radius);
    let mut tmp = vec![[0.0f32; 4]; image.pixels.len()];

    for y in 0..height {
        for x in 0..width {
            let mut acc = [0.0; 4];
            for dx in -radius..=radius {
                let sx = x + dx;
                if sx >= 0 && sx < width {
                    let weight = kernel[(dx + radius) as usize];
                    let px = unpack_premul_rgba8(image.pixels[(y * width + sx) as usize]);
                    for c in 0..4 {
                        acc[c] += px[c] * weight;
                    }
                }
            }
            tmp[(y * width + x) as usize] = acc;
        }
    }

    let mut out = vec![0u32; image.pixels.len()];
    for y in 0..height {
        for x in 0..width {
            let mut acc = [0.0; 4];
            for dy in -radius..=radius {
                let sy = y + dy;
                if sy >= 0 && sy < height {
                    let weight = kernel[(dy + radius) as usize];
                    let px = tmp[(sy * width + x) as usize];
                    for c in 0..4 {
                        acc[c] += px[c] * weight;
                    }
                }
            }
            out[(y * width + x) as usize] = pack_premul_rgba8(acc);
        }
    }
    image.pixels = out;
}

fn gaussian_kernel(radius: i32) -> Vec<f32> {
    let sigma = (radius as f32 / 3.0).max(1.0e-3);
    let two_sigma_sq = 2.0 * sigma * sigma;
    let mut kernel = Vec::with_capacity((radius * 2 + 1) as usize);
    let mut sum = 0.0;

    for i in -radius..=radius {
        let x = i as f32;
        let weight = (-x * x / two_sigma_sq).exp();
        kernel.push(weight);
        sum += weight;
    }
    for weight in &mut kernel {
        *weight /= sum;
    }

    kernel
}

fn apply_saturation(image: &mut Image, amount: f32) {
    map_image_color(image, |rgb, alpha| {
        let gray = luminance(rgb);
        (
            [
                gray + (rgb[0] - gray) * amount,
                gray + (rgb[1] - gray) * amount,
                gray + (rgb[2] - gray) * amount,
            ],
            alpha,
        )
    });
}

pub(crate) fn apply_liquid_glass(
    image: &mut Image,
    region: &BackdropRegion,
    mask: &Image,
    glass: LiquidGlass,
) {
    blur(image, glass.blur_radius);
    apply_saturation(image, glass.saturation);
    let frosted = image.clone();

    let BackdropRegion::Rect { rect, radius } = region else {
        return;
    };
    let shape = crate::shared::sdf::rect::Rect {
        start: kurbo::Point::new(rect.x0, rect.y0),
        end: kurbo::Point::new(rect.x1, rect.y1),
        radius: *radius,
    };
    let tint = glass.tint.to_rgba8();
    let tint_rgb = [
        tint.r as f32 / 255.0,
        tint.g as f32 / 255.0,
        tint.b as f32 / 255.0,
    ];
    let light = normalize(glass.light_direction);
    let edge_width = glass.edge_width.max(1.0);
    let width = image.width;
    let height = image.height;

    for y in 0..height {
        for x in 0..width {
            let ix = (y * width + x) as usize;
            let coverage = unpack_premul_rgba8(mask.pixels[ix])[3];
            if coverage <= 0.0 {
                continue;
            }

            let px = x as f64 + 0.5;
            let py = y as f64 + 0.5;
            let distance = shape.distance(px, py) as f32;
            let inward = (-distance).max(0.0);
            let edge = 1.0 - smoothstep(0.0, edge_width, inward);
            let edge = edge * edge;

            let dx = (shape.distance(px + 0.5, py) - shape.distance(px - 0.5, py)) as f32;
            let dy = (shape.distance(px, py + 0.5) - shape.distance(px, py - 0.5)) as f32;
            let normal = normalize([dx, dy]);
            let bend = glass.displacement_scale.max(0.0) * edge;
            let aberration = glass.aberration_intensity.max(0.0);
            let red_pos = [px as f32 - normal[0] * bend, py as f32 - normal[1] * bend];
            let green_pos = [
                px as f32 - normal[0] * bend * (1.0 - aberration * 0.05),
                py as f32 - normal[1] * bend * (1.0 - aberration * 0.05),
            ];
            let blue_pos = [
                px as f32 - normal[0] * bend * (1.0 - aberration * 0.10),
                py as f32 - normal[1] * bend * (1.0 - aberration * 0.10),
            ];
            let red = sample_bilinear(&frosted, red_pos[0], red_pos[1]);
            let green = sample_bilinear(&frosted, green_pos[0], green_pos[1]);
            let blue = sample_bilinear(&frosted, blue_pos[0], blue_pos[1]);

            let scatter_strength = glass.scatter_strength.clamp(0.0, 1.0) * (0.35 + 0.65 * edge);
            let scatter_radius = glass.scatter_radius.max(0.0)
                * glass.roughness.clamp(0.0, 1.0)
                * (0.55 + 0.45 * edge);
            let scattered =
                if scatter_strength > 0.0 && scatter_radius > 0.0 && glass.scatter_samples > 0 {
                    let scattered_red = sample_scattered(
                        &frosted,
                        red_pos[0],
                        red_pos[1],
                        scatter_radius,
                        glass.scatter_samples,
                    );
                    let scattered_green = sample_scattered(
                        &frosted,
                        green_pos[0],
                        green_pos[1],
                        scatter_radius,
                        glass.scatter_samples,
                    );
                    let scattered_blue = sample_scattered(
                        &frosted,
                        blue_pos[0],
                        blue_pos[1],
                        scatter_radius,
                        glass.scatter_samples,
                    );
                    Some([
                        scattered_red[0],
                        scattered_green[1],
                        scattered_blue[2],
                        (scattered_red[3] + scattered_green[3] + scattered_blue[3]) / 3.0,
                    ])
                } else {
                    None
                };

            let mut base = unpack_premul_rgba8(image.pixels[ix]);
            let refracted = [
                red[0],
                green[1],
                blue[2],
                (red[3] + green[3] + blue[3]) / 3.0,
            ];
            for channel in 0..4 {
                base[channel] = mix(base[channel], refracted[channel], edge);
            }
            if let Some(scattered) = scattered {
                for channel in 0..4 {
                    base[channel] = mix(base[channel], scattered[channel], scatter_strength);
                }
            }

            let alpha = base[3];
            let tint_amount = glass.tint_strength.clamp(0.0, 1.0) * (tint.a as f32 / 255.0);
            for channel in 0..3 {
                base[channel] = mix(base[channel], tint_rgb[channel] * alpha, tint_amount);
            }

            let facing_light = (normal[0] * light[0] + normal[1] * light[1]).clamp(-1.0, 1.0);
            let highlight =
                facing_light.max(0.0).powi(3) * edge * glass.highlight_strength.max(0.0);
            let shadow = (-facing_light).max(0.0).powi(2) * edge * glass.shadow_strength.max(0.0);
            for channel in 0..3 {
                base[channel] = (base[channel] + highlight * alpha) * (1.0 - shadow);
            }
            image.pixels[ix] = pack_premul_rgba8(base);
        }
    }
}

const SCATTER_DISK: [[f32; 2]; 16] = [
    [0.0, 0.0],
    [0.5278, 0.0859],
    [-0.5278, -0.0859],
    [0.1467, 0.6214],
    [-0.1467, -0.6214],
    [-0.6223, 0.3489],
    [0.6223, -0.3489],
    [0.7041, 0.6182],
    [-0.7041, -0.6182],
    [-0.2074, 0.9131],
    [0.2074, -0.9131],
    [-0.9296, -0.2368],
    [0.9296, 0.2368],
    [-0.7842, 0.5897],
    [0.7842, -0.5897],
    [0.3211, 0.9392],
];

fn sample_scattered(image: &Image, x: f32, y: f32, radius: f32, samples: u32) -> [f32; 4] {
    let sample_count = samples.clamp(1, SCATTER_DISK.len() as u32) as usize;
    let mut out = [0.0; 4];
    for offset in &SCATTER_DISK[..sample_count] {
        let sample = sample_bilinear(image, x + offset[0] * radius, y + offset[1] * radius);
        for channel in 0..4 {
            out[channel] += sample[channel];
        }
    }
    let scale = 1.0 / sample_count as f32;
    for channel in &mut out {
        *channel *= scale;
    }
    out
}

fn sample_bilinear(image: &Image, x: f32, y: f32) -> [f32; 4] {
    let x = (x - 0.5).clamp(0.0, image.width.saturating_sub(1) as f32);
    let y = (y - 0.5).clamp(0.0, image.height.saturating_sub(1) as f32);
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(image.width.saturating_sub(1));
    let y1 = (y0 + 1).min(image.height.saturating_sub(1));
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let p00 = unpack_premul_rgba8(image.pixels[(y0 * image.width + x0) as usize]);
    let p10 = unpack_premul_rgba8(image.pixels[(y0 * image.width + x1) as usize]);
    let p01 = unpack_premul_rgba8(image.pixels[(y1 * image.width + x0) as usize]);
    let p11 = unpack_premul_rgba8(image.pixels[(y1 * image.width + x1) as usize]);
    let mut out = [0.0; 4];
    for channel in 0..4 {
        out[channel] = mix(
            mix(p00[channel], p10[channel], tx),
            mix(p01[channel], p11[channel], tx),
            ty,
        );
    }
    out
}

fn normalize(v: [f32; 2]) -> [f32; 2] {
    let length = v[0].hypot(v[1]);
    if length <= 1.0e-6 {
        [0.0, -1.0]
    } else {
        [v[0] / length, v[1] / length]
    }
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn apply_drop_shadow(image: &mut Image, offset_x: f32, offset_y: f32, radius: f32, brush: &Brush) {
    let width = image.width as i32;
    let height = image.height as i32;

    let mut shadow = Image::new(image.width, image.height, Color::TRANSPARENT);
    for y in 0..height {
        for x in 0..width {
            let alpha = unpack_premul_rgba8(image.pixels[(y * width + x) as usize])[3];
            shadow.pixels[(y * width + x) as usize] = pack_premul_rgba8([0.0, 0.0, 0.0, alpha]);
        }
    }

    blur(&mut shadow, radius);

    let dx = offset_x.round() as i32;
    let dy = offset_y.round() as i32;
    let mut offset_shadow = Image::new(image.width, image.height, Color::TRANSPARENT);
    for y in 0..height {
        for x in 0..width {
            let sx = x - dx;
            let sy = y - dy;
            if sx >= 0 && sx < width && sy >= 0 && sy < height {
                let mask = unpack_premul_rgba8(shadow.pixels[(sy * width + sx) as usize])[3];
                let color = unpack_premul_rgba8(brush.sample(x as f32 + 0.5, y as f32 + 0.5));
                offset_shadow.pixels[(y * width + x) as usize] = pack_premul_rgba8([
                    color[0] * mask,
                    color[1] * mask,
                    color[2] * mask,
                    color[3] * mask,
                ]);
            }
        }
    }

    composite_image(&mut offset_shadow, image);
    *image = offset_shadow;
}

fn map_image_color(image: &mut Image, mut f: impl FnMut([f32; 3], f32) -> ([f32; 3], f32)) {
    for px in &mut image.pixels {
        let premul = unpack_premul_rgba8(*px);
        let alpha = premul[3];
        if alpha <= 0.0 {
            continue;
        }

        let rgb = [premul[0] / alpha, premul[1] / alpha, premul[2] / alpha];
        let (rgb, alpha) = f(rgb, alpha);
        let alpha = alpha.clamp(0.0, 1.0);
        *px = pack_premul_rgba8([
            rgb[0].clamp(0.0, 1.0) * alpha,
            rgb[1].clamp(0.0, 1.0) * alpha,
            rgb[2].clamp(0.0, 1.0) * alpha,
            alpha,
        ]);
    }
}

fn map_image_premul(image: &mut Image, mut f: impl FnMut([f32; 4]) -> [f32; 4]) {
    for px in &mut image.pixels {
        *px = pack_premul_rgba8(f(unpack_premul_rgba8(*px)));
    }
}

fn luminance(rgb: [f32; 3]) -> f32 {
    0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]
}

fn hue_rotate(rgb: [f32; 3], degrees: f32) -> [f32; 3] {
    let angle = degrees.to_radians();
    let cos = angle.cos();
    let sin = angle.sin();
    [
        rgb[0] * (0.213 + cos * 0.787 - sin * 0.213)
            + rgb[1] * (0.715 - cos * 0.715 - sin * 0.715)
            + rgb[2] * (0.072 - cos * 0.072 + sin * 0.928),
        rgb[0] * (0.213 - cos * 0.213 + sin * 0.143)
            + rgb[1] * (0.715 + cos * 0.285 + sin * 0.140)
            + rgb[2] * (0.072 - cos * 0.072 - sin * 0.283),
        rgb[0] * (0.213 - cos * 0.213 - sin * 0.787)
            + rgb[1] * (0.715 - cos * 0.715 + sin * 0.715)
            + rgb[2] * (0.072 + cos * 0.928 + sin * 0.072),
    ]
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub(crate) fn composite_image(dst: &mut Image, src: &Image) {
    debug_assert_eq!(dst.width, src.width);
    debug_assert_eq!(dst.height, src.height);
    for ix in 0..dst.pixels.len() {
        let dst_px = unpack_premul_rgba8(dst.pixels[ix]);
        let src_px = unpack_premul_rgba8(src.pixels[ix]);
        dst.pixels[ix] = pack_premul_rgba8(src_over(dst_px, src_px));
    }
}

pub(crate) fn composite_image_at(dst: &mut Image, src: &Image, bounds: Bounds) {
    for src_y in 0..src.height {
        let dst_y = bounds.y0 + src_y as i32;
        if dst_y < 0 || dst_y >= dst.height as i32 {
            continue;
        }
        for src_x in 0..src.width {
            let dst_x = bounds.x0 + src_x as i32;
            if dst_x < 0 || dst_x >= dst.width as i32 {
                continue;
            }
            let src_ix = (src_y * src.width + src_x) as usize;
            let dst_ix = (dst_y as u32 * dst.width + dst_x as u32) as usize;
            let dst_px = unpack_premul_rgba8(dst.pixels[dst_ix]);
            let src_px = unpack_premul_rgba8(src.pixels[src_ix]);
            dst.pixels[dst_ix] = pack_premul_rgba8(src_over(dst_px, src_px));
        }
    }
}

pub(crate) fn composite_blend_image_at(
    dst: &mut Image,
    src: &Image,
    bounds: Bounds,
    blend: &Blend,
) {
    for src_y in 0..src.height {
        let dst_y = bounds.y0 + src_y as i32;
        if dst_y < 0 || dst_y >= dst.height as i32 {
            continue;
        }
        for src_x in 0..src.width {
            let dst_x = bounds.x0 + src_x as i32;
            if dst_x < 0 || dst_x >= dst.width as i32 {
                continue;
            }
            let src_ix = (src_y * src.width + src_x) as usize;
            let dst_ix = (dst_y as u32 * dst.width + dst_x as u32) as usize;
            let dst_px = unpack_premul_rgba8(dst.pixels[dst_ix]);
            let src_px = unpack_premul_rgba8(src.pixels[src_ix]);
            dst.pixels[dst_ix] = pack_premul_rgba8(blend.blend(src_px, dst_px));
        }
    }
}

pub(crate) fn copy_image_region(src: &Image, bounds: Bounds) -> Image {
    let mut out = Image::new(bounds.width(), bounds.height(), Color::TRANSPARENT);
    for y in 0..out.height {
        let src_y = bounds.y0 + y as i32;
        if src_y < 0 || src_y >= src.height as i32 {
            continue;
        }
        for x in 0..out.width {
            let src_x = bounds.x0 + x as i32;
            if src_x < 0 || src_x >= src.width as i32 {
                continue;
            }
            out.pixels[(y * out.width + x) as usize] =
                src.pixels[(src_y as u32 * src.width + src_x as u32) as usize];
        }
    }
    out
}

pub(crate) fn composite_backdrop_filter(dst: &mut Image, filtered_backdrop: &Image, mask: &Image) {
    debug_assert_eq!(dst.width, filtered_backdrop.width);
    debug_assert_eq!(dst.height, filtered_backdrop.height);
    debug_assert_eq!(dst.width, mask.width);
    debug_assert_eq!(dst.height, mask.height);

    for ix in 0..dst.pixels.len() {
        // Mask alpha is geometry coverage only. Foreground content alpha does
        // not affect backdrop-filter strength.
        let alpha = unpack_premul_rgba8(mask.pixels[ix])[3].clamp(0.0, 1.0);
        if alpha <= 0.0 {
            continue;
        }

        let original = unpack_premul_rgba8(dst.pixels[ix]);
        let filtered = unpack_premul_rgba8(filtered_backdrop.pixels[ix]);
        dst.pixels[ix] = pack_premul_rgba8([
            mix(original[0], filtered[0], alpha),
            mix(original[1], filtered[1], alpha),
            mix(original[2], filtered[2], alpha),
            mix(original[3], filtered[3], alpha),
        ]);
    }
}

pub(crate) fn composite_backdrop_filter_at(
    dst: &mut Image,
    filtered_backdrop: &Image,
    mask: &Image,
    bounds: Bounds,
) {
    debug_assert_eq!(filtered_backdrop.width, mask.width);
    debug_assert_eq!(filtered_backdrop.height, mask.height);
    for y in 0..filtered_backdrop.height {
        let dst_y = bounds.y0 + y as i32;
        if dst_y < 0 || dst_y >= dst.height as i32 {
            continue;
        }
        for x in 0..filtered_backdrop.width {
            let dst_x = bounds.x0 + x as i32;
            if dst_x < 0 || dst_x >= dst.width as i32 {
                continue;
            }
            let ix = (y * filtered_backdrop.width + x) as usize;
            let alpha = unpack_premul_rgba8(mask.pixels[ix])[3].clamp(0.0, 1.0);
            if alpha <= 0.0 {
                continue;
            }
            let dst_ix = (dst_y as u32 * dst.width + dst_x as u32) as usize;
            let original = unpack_premul_rgba8(dst.pixels[dst_ix]);
            let filtered = unpack_premul_rgba8(filtered_backdrop.pixels[ix]);
            dst.pixels[dst_ix] = pack_premul_rgba8([
                mix(original[0], filtered[0], alpha),
                mix(original[1], filtered[1], alpha),
                mix(original[2], filtered[2], alpha),
                mix(original[3], filtered[3], alpha),
            ]);
        }
    }
}

pub(crate) fn composite_masked_image(
    dst: &mut Image,
    content: &Image,
    mask: &Image,
    mode: MaskMode,
) {
    debug_assert_eq!(dst.width, content.width);
    debug_assert_eq!(dst.height, content.height);
    debug_assert_eq!(dst.width, mask.width);
    debug_assert_eq!(dst.height, mask.height);

    for ix in 0..dst.pixels.len() {
        let coverage = mask_coverage(mask.pixels[ix], mode);
        if coverage <= 0.0 {
            continue;
        }

        let dst_px = unpack_premul_rgba8(dst.pixels[ix]);
        let mut content_px = unpack_premul_rgba8(content.pixels[ix]);
        for channel in &mut content_px {
            *channel *= coverage;
        }
        dst.pixels[ix] = pack_premul_rgba8(src_over(dst_px, content_px));
    }
}

pub(crate) fn composite_masked_image_at(
    dst: &mut Image,
    content: &Image,
    mask: &Image,
    bounds: Bounds,
    mode: MaskMode,
) {
    debug_assert_eq!(content.width, mask.width);
    debug_assert_eq!(content.height, mask.height);
    for y in 0..content.height {
        let dst_y = bounds.y0 + y as i32;
        if dst_y < 0 || dst_y >= dst.height as i32 {
            continue;
        }
        for x in 0..content.width {
            let dst_x = bounds.x0 + x as i32;
            if dst_x < 0 || dst_x >= dst.width as i32 {
                continue;
            }
            let ix = (y * content.width + x) as usize;
            let coverage = mask_coverage(mask.pixels[ix], mode);
            if coverage <= 0.0 {
                continue;
            }
            let dst_ix = (dst_y as u32 * dst.width + dst_x as u32) as usize;
            let dst_px = unpack_premul_rgba8(dst.pixels[dst_ix]);
            let mut content_px = unpack_premul_rgba8(content.pixels[ix]);
            for channel in &mut content_px {
                *channel *= coverage;
            }
            dst.pixels[dst_ix] = pack_premul_rgba8(src_over(dst_px, content_px));
        }
    }
}

fn mask_coverage(mask: u32, mode: MaskMode) -> f32 {
    let px = unpack_premul_rgba8(mask);
    match mode {
        MaskMode::Alpha => px[3],
        MaskMode::Luminance => {
            if px[3] <= 0.0 {
                0.0
            } else {
                let rgb = [px[0] / px[3], px[1] / px[3], px[2] / px[3]];
                px[3] * luminance(rgb)
            }
        }
    }
}
