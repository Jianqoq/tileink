use crate::shared::image::rgba8_pack;

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
