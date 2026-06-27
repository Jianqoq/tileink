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

pub(crate) fn src_over(dst: [f32; 4], src: [f32; 4]) -> [f32; 4] {
    [
        src[0] + dst[0] * (1.0 - src[3]),
        src[1] + dst[1] * (1.0 - src[3]),
        src[2] + dst[2] * (1.0 - src[3]),
        src[3] + dst[3] * (1.0 - src[3]),
    ]
}
