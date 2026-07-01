use crate::{BLOCK_SIZE, shared::image::rgba8_pack};

pub type TileBuffer = [u32; BLOCK_SIZE as usize];

pub(crate) const MASK_OPAQUE: u8 = 255;
// Linear-light compositing makes dark glyph edges on light backgrounds look too
// pale at small sizes; this remaps glyph coverage only for that contrast case.
const TEXT_DARK_ON_LIGHT_COVERAGE_BOOST: f32 = 0.75;

#[inline]
pub(crate) fn coverage_f32_to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

#[inline]
pub(crate) fn opacity_f32_to_u8(opacity: f32) -> u8 {
    (opacity.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

#[inline]
pub(crate) fn mul_div255(a: u8, b: u8) -> u8 {
    ((a as u16 * b as u16 + 127) / 255) as u8
}

#[inline]
pub(crate) fn premul_f32_to_u32(c: [f32; 4]) -> u32 {
    rgba8_pack([
        (c[0] * 255.0 + 0.5) as u8,
        (c[1] * 255.0 + 0.5) as u8,
        (c[2] * 255.0 + 0.5) as u8,
        (c[3] * 255.0 + 0.5) as u8,
    ])
}

pub(crate) fn unpack_premul_rgba8(px: u32) -> [f32; 4] {
    let inv = 1.0 / 255.0;
    [
        (px & 0xff) as f32 * inv,
        ((px >> 8) & 0xff) as f32 * inv,
        ((px >> 16) & 0xff) as f32 * inv,
        ((px >> 24) & 0xff) as f32 * inv,
    ]
}

pub(crate) fn pack_premul_rgba8(c: [f32; 4]) -> u32 {
    premul_f32_to_u32(c)
}

pub(crate) fn src_over_premul_u8(dst: u32, src: u32) -> u32 {
    let sa = (src >> 24) as u8;
    if sa == 0 {
        return dst;
    }
    if sa == MASK_OPAQUE {
        return src;
    }
    let inv = 255 - sa;
    let dr = (dst & 0xff) as u8;
    let dg = ((dst >> 8) & 0xff) as u8;
    let db = ((dst >> 16) & 0xff) as u8;
    let da = ((dst >> 24) & 0xff) as u8;
    let sr = (src & 0xff) as u8;
    let sg = ((src >> 8) & 0xff) as u8;
    let sb = ((src >> 16) & 0xff) as u8;
    rgba8_pack([
        sr + mul_div255(dr, inv),
        sg + mul_div255(dg, inv),
        sb + mul_div255(db, inv),
        sa + mul_div255(da, inv),
    ])
}

pub(crate) fn scale_premul_u8(src: u32, factor: u8) -> u32 {
    if factor == 0 {
        return 0;
    }
    if factor == MASK_OPAQUE {
        return src;
    }
    rgba8_pack([
        mul_div255((src & 0xff) as u8, factor),
        mul_div255(((src >> 8) & 0xff) as u8, factor),
        mul_div255(((src >> 16) & 0xff) as u8, factor),
        mul_div255(((src >> 24) & 0xff) as u8, factor),
    ])
}

pub(crate) fn src_over_mask_linear_u8(dst: u32, src: u32, coverage: u8) -> u32 {
    if coverage == 0 || (src >> 24) == 0 {
        return dst;
    }

    let coverage = coverage as f32 * (1.0 / 255.0);
    let (src_rgb, src_a) = unpack_linear_premul_srgb8(src);
    let (dst_rgb, dst_a) = unpack_linear_premul_srgb8(dst);
    let src_a = src_a * coverage;
    let out_a = src_a + dst_a * (1.0 - src_a);
    pack_linear_premul_to_srgb8(
        src_rgb[0] * coverage + dst_rgb[0] * (1.0 - src_a),
        src_rgb[1] * coverage + dst_rgb[1] * (1.0 - src_a),
        src_rgb[2] * coverage + dst_rgb[2] * (1.0 - src_a),
        out_a,
    )
}

pub(crate) fn src_over_mask_linear_auto_u8(dst: u32, src: u32, coverage: u8) -> u32 {
    src_over_mask_linear_u8(dst, src, auto_text_coverage(dst, src, coverage))
}

pub(crate) fn src_over_subpixel_mask_u8(dst: u32, src: u32, mask: [u8; 3], clip: u8) -> u32 {
    let sa = (src >> 24) as u8;
    if sa == 0 || clip == 0 {
        return dst;
    }

    let mr = mul_div255(mask[0], clip);
    let mg = mul_div255(mask[1], clip);
    let mb = mul_div255(mask[2], clip);
    if mr == 0 && mg == 0 && mb == 0 {
        return dst;
    }

    // LCD/subpixel glyph masks are channel coverage, not a single alpha mask.
    // Keep channel-specific destination attenuation for sharp text on opaque
    // surfaces, then store the maximum channel coverage as the representable
    // premultiplied alpha for later layers.
    let cr = mul_div255(sa, mr);
    let cg = mul_div255(sa, mg);
    let cb = mul_div255(sa, mb);
    let ca = cr.max(cg).max(cb);

    let sr = mul_div255((src & 0xff) as u8, mr);
    let sg = mul_div255(((src >> 8) & 0xff) as u8, mg);
    let sb = mul_div255(((src >> 16) & 0xff) as u8, mb);
    let dr = (dst & 0xff) as u8;
    let dg = ((dst >> 8) & 0xff) as u8;
    let db = ((dst >> 16) & 0xff) as u8;
    let da = ((dst >> 24) & 0xff) as u8;

    rgba8_pack([
        sr + mul_div255(dr, 255 - cr),
        sg + mul_div255(dg, 255 - cg),
        sb + mul_div255(db, 255 - cb),
        ca + mul_div255(da, 255 - ca),
    ])
}

pub(crate) fn src_over_subpixel_mask_linear_auto_u8(
    dst: u32,
    src: u32,
    mask: [u8; 3],
    clip: u8,
) -> u32 {
    let clipped = [
        mul_div255(mask[0], clip),
        mul_div255(mask[1], clip),
        mul_div255(mask[2], clip),
    ];
    src_over_subpixel_mask_linear_u8(
        dst,
        src,
        [
            auto_text_coverage(dst, src, clipped[0]),
            auto_text_coverage(dst, src, clipped[1]),
            auto_text_coverage(dst, src, clipped[2]),
        ],
        MASK_OPAQUE,
    )
}

pub(crate) fn src_over_subpixel_mask_linear_u8(dst: u32, src: u32, mask: [u8; 3], clip: u8) -> u32 {
    if (src >> 24) == 0 || clip == 0 {
        return dst;
    }

    let mr = mul_div255(mask[0], clip) as f32 * (1.0 / 255.0);
    let mg = mul_div255(mask[1], clip) as f32 * (1.0 / 255.0);
    let mb = mul_div255(mask[2], clip) as f32 * (1.0 / 255.0);
    if mr == 0.0 && mg == 0.0 && mb == 0.0 {
        return dst;
    }

    let (src_rgb, src_a) = unpack_linear_premul_srgb8(src);
    let (dst_rgb, dst_a) = unpack_linear_premul_srgb8(dst);
    let cr = src_a * mr;
    let cg = src_a * mg;
    let cb = src_a * mb;
    let ca = cr.max(cg).max(cb);
    let out_a = ca + dst_a * (1.0 - ca);

    pack_linear_premul_to_srgb8(
        src_rgb[0] * mr + dst_rgb[0] * (1.0 - cr),
        src_rgb[1] * mg + dst_rgb[1] * (1.0 - cg),
        src_rgb[2] * mb + dst_rgb[2] * (1.0 - cb),
        out_a,
    )
}

fn auto_text_coverage(dst: u32, src: u32, coverage: u8) -> u8 {
    if coverage == 0 || coverage == MASK_OPAQUE {
        return coverage;
    }

    let src_luma = linear_luminance_from_srgb8(src);
    let dst_luma = linear_luminance_from_srgb8(dst);
    if src_luma >= dst_luma {
        return coverage;
    }

    let contrast = (dst_luma - src_luma).clamp(0.0, 1.0);
    let exponent = 1.0 - TEXT_DARK_ON_LIGHT_COVERAGE_BOOST * contrast * dst_luma.clamp(0.0, 1.0);
    let compensated = (coverage as f32 * (1.0 / 255.0)).powf(exponent);
    (compensated * 255.0 + 0.5) as u8
}

fn linear_luminance_from_srgb8(px: u32) -> f32 {
    let alpha = ((px >> 24) & 0xff) as f32 * (1.0 / 255.0);
    if alpha == 0.0 {
        return 0.0;
    }

    let r = srgb_to_linear(((px & 0xff) as f32 * (1.0 / 255.0)) / alpha);
    let g = srgb_to_linear((((px >> 8) & 0xff) as f32 * (1.0 / 255.0)) / alpha);
    let b = srgb_to_linear((((px >> 16) & 0xff) as f32 * (1.0 / 255.0)) / alpha);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn unpack_linear_premul_srgb8(px: u32) -> ([f32; 3], f32) {
    let inv = 1.0 / 255.0;
    let a = ((px >> 24) & 0xff) as f32 * inv;
    if a == 0.0 {
        return ([0.0, 0.0, 0.0], 0.0);
    }

    (
        [
            srgb_to_linear(((px & 0xff) as f32 * inv) / a) * a,
            srgb_to_linear((((px >> 8) & 0xff) as f32 * inv) / a) * a,
            srgb_to_linear((((px >> 16) & 0xff) as f32 * inv) / a) * a,
        ],
        a,
    )
}

fn pack_linear_premul_to_srgb8(r: f32, g: f32, b: f32, a: f32) -> u32 {
    if a <= 0.0 {
        return 0;
    }
    let a = a.clamp(0.0, 1.0);
    rgba8_pack([
        linear_premul_channel_to_srgb8(r, a),
        linear_premul_channel_to_srgb8(g, a),
        linear_premul_channel_to_srgb8(b, a),
        (a * 255.0 + 0.5) as u8,
    ])
}

fn linear_premul_channel_to_srgb8(value: f32, alpha: f32) -> u8 {
    let straight = (value / alpha).clamp(0.0, 1.0);
    (linear_to_srgb(straight) * alpha * 255.0 + 0.5) as u8
}

fn srgb_to_linear(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

pub(crate) fn src_over(dst: [f32; 4], src: [f32; 4]) -> [f32; 4] {
    [
        src[0] + dst[0] * (1.0 - src[3]),
        src[1] + dst[1] * (1.0 - src[3]),
        src[2] + dst[2] * (1.0 - src[3]),
        src[3] + dst[3] * (1.0 - src[3]),
    ]
}

#[cfg(test)]
mod tests {
    use crate::shared::{
        image::{rgba8_pack, unpack_rgba8},
        pixel::{
            src_over_mask_linear_auto_u8, src_over_mask_linear_u8,
            src_over_subpixel_mask_linear_auto_u8, src_over_subpixel_mask_linear_u8,
            src_over_subpixel_mask_u8,
        },
    };

    #[test]
    fn subpixel_mask_preserves_independent_channel_coverage_on_opaque_background() {
        let dst = rgba8_pack([0, 0, 0, 255]);
        let src = rgba8_pack([255, 255, 255, 255]);

        assert_eq!(
            unpack_rgba8(src_over_subpixel_mask_u8(dst, src, [255, 0, 0], 255)),
            [255, 0, 0, 255]
        );
    }

    #[test]
    fn subpixel_mask_uses_channel_coverage_to_attenuate_destination() {
        let dst = rgba8_pack([255, 255, 255, 255]);
        let src = rgba8_pack([0, 0, 0, 255]);

        assert_eq!(
            unpack_rgba8(src_over_subpixel_mask_u8(dst, src, [255, 128, 0], 255)),
            [0, 127, 255, 255]
        );
    }

    #[test]
    fn linear_mask_composites_coverage_in_linear_light() {
        let dst = rgba8_pack([0, 0, 0, 255]);
        let src = rgba8_pack([255, 255, 255, 255]);

        assert_eq!(
            unpack_rgba8(src_over_mask_linear_u8(dst, src, 128)),
            [188, 188, 188, 255]
        );
    }

    #[test]
    fn linear_subpixel_mask_composites_each_channel_in_linear_light() {
        let dst = rgba8_pack([0, 0, 0, 255]);
        let src = rgba8_pack([255, 255, 255, 255]);

        assert_eq!(
            unpack_rgba8(src_over_subpixel_mask_linear_u8(
                dst,
                src,
                [128, 0, 255],
                255
            )),
            [188, 0, 255, 255]
        );
    }

    #[test]
    fn auto_linear_mask_compensates_dark_text_on_light_background() {
        let dst = rgba8_pack([255, 255, 255, 255]);
        let src = rgba8_pack([0, 0, 0, 255]);

        assert_eq!(
            unpack_rgba8(src_over_mask_linear_auto_u8(dst, src, 128)),
            [110, 110, 110, 255]
        );
    }

    #[test]
    fn auto_linear_mask_keeps_light_text_on_dark_background_exact() {
        let dst = rgba8_pack([0, 0, 0, 255]);
        let src = rgba8_pack([255, 255, 255, 255]);

        assert_eq!(
            unpack_rgba8(src_over_mask_linear_auto_u8(dst, src, 128)),
            [188, 188, 188, 255]
        );
    }

    #[test]
    fn auto_linear_subpixel_mask_compensates_dark_text_channels() {
        let dst = rgba8_pack([255, 255, 255, 255]);
        let src = rgba8_pack([0, 0, 0, 255]);

        assert_eq!(
            unpack_rgba8(src_over_subpixel_mask_linear_auto_u8(
                dst,
                src,
                [128, 0, 255],
                255
            )),
            [110, 255, 0, 255]
        );
    }
}
