use crate::{BLOCK_SIZE, shared::image::rgba8_pack};

pub type TileBuffer = [u32; BLOCK_SIZE as usize];

pub(crate) const MASK_OPAQUE: u8 = 255;

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
        pixel::src_over_subpixel_mask_u8,
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
}
