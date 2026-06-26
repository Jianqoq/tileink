use peniko::{
    BlendMode, Compose, Mix,
    kurbo::{Affine, BezPath},
};

use crate::shared::bounds::Bounds;

#[derive(Clone, Debug)]
pub struct Blend {
    pub(crate) path: BezPath,
    pub(crate) bounds: Bounds,
    pub(crate) transform: Affine,
    pub(crate) tolerance: f64,
    pub(crate) mode: BlendMode,
}

impl Blend {
    pub(crate) fn new(mix: Mix, compose: Compose) -> Self {
        Self {
            path: BezPath::new(),
            bounds: Bounds::new(0, 0, 0, 0),
            transform: Affine::IDENTITY,
            tolerance: 0.0,
            mode: BlendMode::new(mix, compose),
        }
    }

    pub(crate) fn with_geometry(
        path: BezPath,
        bounds: Bounds,
        transform: Affine,
        tolerance: f64,
        mix: Mix,
        compose: Compose,
    ) -> Self {
        Self {
            path,
            bounds,
            transform,
            tolerance,
            mode: BlendMode::new(mix, compose),
        }
    }

    pub(crate) fn is_normal_src_over(&self) -> bool {
        matches!(
            (self.mode.mix, self.mode.compose),
            (Mix::Normal, Compose::SrcOver)
        )
    }

    pub(crate) fn blend(&self, src: [f32; 4], dst: [f32; 4]) -> [f32; 4] {
        match (self.mode.mix, self.mode.compose) {
            (Mix::Normal, Compose::SrcOver) => src_over_premul(dst, src),
            (Mix::Normal, Compose::Dest) => dst,
            (Mix::Normal, Compose::Clear) => [0.0; 4],
            (Mix::Normal, Compose::Copy) => src,
            (Mix::Normal, compose) => blend_normal_compose(dst, src, compose),
            _ => blend_premul(dst, src, self.mode),
        }
    }
}

#[inline]
pub(crate) fn src_over_premul(dst: [f32; 4], src: [f32; 4]) -> [f32; 4] {
    let dst_factor = 1.0 - src[3];
    [
        src[0] + dst[0] * dst_factor,
        src[1] + dst[1] * dst_factor,
        src[2] + dst[2] * dst_factor,
        src[3] + dst[3] * dst_factor,
    ]
}

#[inline]
pub(crate) fn scale_premul(src: [f32; 4], factor: f32) -> [f32; 4] {
    [
        src[0] * factor,
        src[1] * factor,
        src[2] * factor,
        src[3] * factor,
    ]
}

fn blend_normal_compose(dst: [f32; 4], src: [f32; 4], compose: Compose) -> [f32; 4] {
    let src_alpha = src[3];
    let dst_alpha = dst[3];
    let (src_factor, dst_factor) = compose_factors(compose, src_alpha, dst_alpha);
    let mut out = [
        src[0] * src_factor + dst[0] * dst_factor,
        src[1] * src_factor + dst[1] * dst_factor,
        src[2] * src_factor + dst[2] * dst_factor,
        src[3] * src_factor + dst[3] * dst_factor,
    ];
    if matches!(compose, Compose::PlusLighter) {
        out = out.map(|c| c.min(1.0));
    }
    out
}

fn blend_premul(dst: [f32; 4], src: [f32; 4], blend: BlendMode) -> [f32; 4] {
    if matches!(blend.compose, Compose::SrcOver) {
        if let Some(result) = blend_src_over_highp(dst, src, blend.mix) {
            return result;
        }
    }
    let src_alpha = src[3].clamp(0.0, 1.0);
    let dst_alpha = dst[3].clamp(0.0, 1.0);
    let src_rgb = unpremul(src);
    let dst_rgb = unpremul(dst);
    let mixed_rgb = mix_rgb(dst_rgb, src_rgb, blend.mix);
    let effective_src = [
        src_alpha * ((1.0 - dst_alpha) * src_rgb[0] + dst_alpha * mixed_rgb[0]),
        src_alpha * ((1.0 - dst_alpha) * src_rgb[1] + dst_alpha * mixed_rgb[1]),
        src_alpha * ((1.0 - dst_alpha) * src_rgb[2] + dst_alpha * mixed_rgb[2]),
        src_alpha,
    ];
    let (src_factor, dst_factor) = compose_factors(blend.compose, src_alpha, dst_alpha);

    [
        effective_src[0] * src_factor + dst[0] * dst_factor,
        effective_src[1] * src_factor + dst[1] * dst_factor,
        effective_src[2] * src_factor + dst[2] * dst_factor,
        effective_src[3] * src_factor + dst[3] * dst_factor,
    ]
}

fn blend_src_over_highp(dst: [f32; 4], src: [f32; 4], mix: Mix) -> Option<[f32; 4]> {
    let [sr, sg, sb, sa] = src;
    let [dr, dg, db, da] = dst;
    let rgb = match mix {
        Mix::ColorDodge => [
            color_dodge_premul(sr, dr, sa, da),
            color_dodge_premul(sg, dg, sa, da),
            color_dodge_premul(sb, db, sa, da),
        ],
        Mix::ColorBurn => [
            color_burn_premul(sr, dr, sa, da),
            color_burn_premul(sg, dg, sa, da),
            color_burn_premul(sb, db, sa, da),
        ],
        _ => return None,
    };
    Some([rgb[0], rgb[1], rgb[2], sa + da * (1.0 - sa)])
}

fn compose_factors(compose: Compose, src_alpha: f32, dst_alpha: f32) -> (f32, f32) {
    match compose {
        Compose::Clear => (0.0, 0.0),
        Compose::Copy => (1.0, 0.0),
        Compose::Dest => (0.0, 1.0),
        Compose::SrcOver => (1.0, 1.0 - src_alpha),
        Compose::DestOver => (1.0 - dst_alpha, 1.0),
        Compose::SrcIn => (dst_alpha, 0.0),
        Compose::DestIn => (0.0, src_alpha),
        Compose::SrcOut => (1.0 - dst_alpha, 0.0),
        Compose::DestOut => (0.0, 1.0 - src_alpha),
        Compose::SrcAtop => (dst_alpha, 1.0 - src_alpha),
        Compose::DestAtop => (1.0 - dst_alpha, src_alpha),
        Compose::Xor => (1.0 - dst_alpha, 1.0 - src_alpha),
        Compose::Plus | Compose::PlusLighter => (1.0, 1.0),
    }
}

fn mix_rgb(dst: [f32; 3], src: [f32; 3], mix: Mix) -> [f32; 3] {
    match mix {
        Mix::Normal => src,
        Mix::Multiply => [dst[0] * src[0], dst[1] * src[1], dst[2] * src[2]],
        Mix::Screen => [
            dst[0] + src[0] - dst[0] * src[0],
            dst[1] + src[1] - dst[1] * src[1],
            dst[2] + src[2] - dst[2] * src[2],
        ],
        Mix::Overlay => [
            overlay(dst[0], src[0]),
            overlay(dst[1], src[1]),
            overlay(dst[2], src[2]),
        ],
        Mix::Darken => [dst[0].min(src[0]), dst[1].min(src[1]), dst[2].min(src[2])],
        Mix::Lighten => [dst[0].max(src[0]), dst[1].max(src[1]), dst[2].max(src[2])],
        Mix::ColorDodge => [
            color_dodge(dst[0], src[0]),
            color_dodge(dst[1], src[1]),
            color_dodge(dst[2], src[2]),
        ],
        Mix::ColorBurn => [
            color_burn(dst[0], src[0]),
            color_burn(dst[1], src[1]),
            color_burn(dst[2], src[2]),
        ],
        Mix::HardLight => [
            overlay(src[0], dst[0]),
            overlay(src[1], dst[1]),
            overlay(src[2], dst[2]),
        ],
        Mix::SoftLight => [
            soft_light(dst[0], src[0]),
            soft_light(dst[1], src[1]),
            soft_light(dst[2], src[2]),
        ],
        Mix::Difference => [
            (dst[0] - src[0]).abs(),
            (dst[1] - src[1]).abs(),
            (dst[2] - src[2]).abs(),
        ],
        Mix::Exclusion => [
            dst[0] + src[0] - 2.0 * dst[0] * src[0],
            dst[1] + src[1] - 2.0 * dst[1] * src[1],
            dst[2] + src[2] - 2.0 * dst[2] * src[2],
        ],
        Mix::Hue | Mix::Saturation | Mix::Color | Mix::Luminosity => {
            mix_nonseparable(dst, src, mix)
        }
    }
}

fn unpremul(px: [f32; 4]) -> [f32; 3] {
    if px[3] <= 0.0 {
        [0.0; 3]
    } else {
        [px[0] / px[3], px[1] / px[3], px[2] / px[3]]
    }
}

fn overlay(dst: f32, src: f32) -> f32 {
    if dst <= 0.5 {
        2.0 * dst * src
    } else {
        1.0 - 2.0 * (1.0 - dst) * (1.0 - src)
    }
}

fn color_dodge(dst: f32, src: f32) -> f32 {
    if src >= 1.0 {
        1.0
    } else {
        (dst / (1.0 - src)).min(1.0)
    }
}

fn color_dodge_premul(src: f32, dst: f32, src_alpha: f32, dst_alpha: f32) -> f32 {
    if dst <= 0.0 {
        src * (1.0 - dst_alpha)
    } else if src >= src_alpha {
        src + dst * (1.0 - src_alpha)
    } else {
        src_alpha * dst_alpha.min((dst * src_alpha) / (src_alpha - src))
            + src * (1.0 - dst_alpha)
            + dst * (1.0 - src_alpha)
    }
}

fn color_burn(dst: f32, src: f32) -> f32 {
    if src <= 0.0 {
        0.0
    } else {
        1.0 - ((1.0 - dst) / src).min(1.0)
    }
}

fn color_burn_premul(src: f32, dst: f32, src_alpha: f32, dst_alpha: f32) -> f32 {
    if dst >= dst_alpha {
        dst + src * (1.0 - dst_alpha)
    } else if src <= 0.0 {
        dst * (1.0 - src_alpha)
    } else {
        src_alpha * (dst_alpha - dst_alpha.min(((dst_alpha - dst) * src_alpha) / src))
            + src * (1.0 - dst_alpha)
            + dst * (1.0 - src_alpha)
    }
}

fn soft_light(dst: f32, src: f32) -> f32 {
    if src <= 0.5 {
        dst - (1.0 - 2.0 * src) * dst * (1.0 - dst)
    } else {
        let d = if dst <= 0.25 {
            ((16.0 * dst - 12.0) * dst + 4.0) * dst
        } else {
            dst.sqrt()
        };
        dst + (2.0 * src - 1.0) * (d - dst)
    }
}

fn mix_nonseparable(dst: [f32; 3], src: [f32; 3], mix: Mix) -> [f32; 3] {
    match mix {
        Mix::Hue => set_lum(set_sat(src, sat(dst)), lum(dst)),
        Mix::Saturation => set_lum(set_sat(dst, sat(src)), lum(dst)),
        Mix::Color => set_lum(src, lum(dst)),
        Mix::Luminosity => set_lum(dst, lum(src)),
        _ => unreachable!(),
    }
}

fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color([c[0] + d, c[1] + d, c[2] + d])
}

fn clip_color(mut c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    if n < 0.0 {
        c = [
            l + (c[0] - l) * l / (l - n),
            l + (c[1] - l) * l / (l - n),
            l + (c[2] - l) * l / (l - n),
        ];
    }
    if x > 1.0 {
        c = [
            l + (c[0] - l) * (1.0 - l) / (x - l),
            l + (c[1] - l) * (1.0 - l) / (x - l),
            l + (c[2] - l) * (1.0 - l) / (x - l),
        ];
    }
    c
}

fn set_sat(mut c: [f32; 3], s: f32) -> [f32; 3] {
    let min = if c[0] <= c[1] && c[0] <= c[2] {
        0
    } else if c[1] <= c[2] {
        1
    } else {
        2
    };
    let max = if c[0] >= c[1] && c[0] >= c[2] {
        0
    } else if c[1] >= c[2] {
        1
    } else {
        2
    };
    if min == max {
        return [0.0; 3];
    }
    let mid = 3 - min - max;

    if c[max] > c[min] {
        c[mid] = (c[mid] - c[min]) * s / (c[max] - c[min]);
        c[max] = s;
    } else {
        c[mid] = 0.0;
        c[max] = 0.0;
    }
    c[min] = 0.0;
    c
}
