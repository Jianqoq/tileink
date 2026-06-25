use std::ops::{BitOr, BitOrAssign};

use crate::{
    BLOCK_SIZE, TILE_SIZE,
    shared::{bounds::Bounds, image::rgba8_pack},
};
use wide::u32x4;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TileMask {
    words: [u64; 4],
}

impl TileMask {
    pub const fn new() -> Self {
        Self { words: [0; 4] }
    }

    pub const fn from_words(words: [u64; 4]) -> Self {
        Self { words }
    }

    pub const fn words(&self) -> &[u64; 4] {
        &self.words
    }

    #[inline]
    pub fn set(&mut self, index: usize) {
        debug_assert!(index < BLOCK_SIZE as usize);
        let word = index / u64::BITS as usize;
        let bit = index % u64::BITS as usize;
        self.words[word] |= 1u64 << bit;
    }

    #[inline]
    pub fn clear(&mut self, index: usize) {
        debug_assert!(index < BLOCK_SIZE as usize);
        let word = index / u64::BITS as usize;
        let bit = index % u64::BITS as usize;
        self.words[word] &= !(1u64 << bit);
    }

    #[inline]
    pub fn get(&self, index: usize) -> bool {
        debug_assert!(index < BLOCK_SIZE as usize);
        let word = index / u64::BITS as usize;
        let bit = index % u64::BITS as usize;
        (self.words[word] & (1u64 << bit)) != 0
    }

    #[inline]
    pub fn any(&self) -> bool {
        self.words.iter().any(|&word| word != 0)
    }

    #[inline]
    pub fn union(self, other: Self) -> Self {
        Self {
            words: std::array::from_fn(|i| self.words[i] | other.words[i]),
        }
    }

    #[inline]
    pub fn iter_ones(self) -> TileMaskIter {
        TileMaskIter {
            words: self.words,
            word_index: 0,
        }
    }
}

impl From<[u64; 4]> for TileMask {
    fn from(words: [u64; 4]) -> Self {
        Self::from_words(words)
    }
}

impl BitOr for TileMask {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        self.union(rhs)
    }
}

impl BitOrAssign for TileMask {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.union(rhs);
    }
}

impl IntoIterator for TileMask {
    type Item = usize;
    type IntoIter = TileMaskIter;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_ones()
    }
}

pub struct TileMaskIter {
    words: [u64; 4],
    word_index: usize,
}

impl Iterator for TileMaskIter {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        while self.word_index < self.words.len() {
            let word = &mut self.words[self.word_index];
            if *word == 0 {
                self.word_index += 1;
                continue;
            }

            let bit = word.trailing_zeros() as usize;
            *word &= *word - 1;
            return Some(self.word_index * u64::BITS as usize + bit);
        }

        None
    }
}

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

#[inline]
pub(crate) fn src_over_premul_u8x4(dst: u32x4, src: u32) -> u32x4 {
    let sa = src >> 24;
    if sa == 0 {
        return dst;
    }
    if sa == u32::from(MASK_OPAQUE) {
        return u32x4::splat(src);
    }

    let inv = u32x4::splat(255 - sa);
    let mask = u32x4::splat(0xff);
    let src_r = u32x4::splat(src & 0xff);
    let src_g = u32x4::splat((src >> 8) & 0xff);
    let src_b = u32x4::splat((src >> 16) & 0xff);
    let src_a = u32x4::splat(sa);

    let r = src_r + mul_div255_u32x4(dst & mask, inv);
    let g = src_g + mul_div255_u32x4((dst >> 8) & mask, inv);
    let b = src_b + mul_div255_u32x4((dst >> 16) & mask, inv);
    let a = src_a + mul_div255_u32x4((dst >> 24) & mask, inv);

    r | (g << 8) | (b << 16) | (a << 24)
}

#[inline]
fn mul_div255_u32x4(a: u32x4, b: u32x4) -> u32x4 {
    ((a * b + u32x4::splat(128)) * u32x4::splat(257)) >> 16
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

/// # Safety
/// `pixels` must point at a valid `image_width * image_height` RGBA8 buffer.
pub(crate) unsafe fn load_tile_buffer_raw(
    pixels: *const u32,
    image_width: u32,
    tile_bounds: Bounds,
) -> TileBuffer {
    let mut out = [0_u32; BLOCK_SIZE as usize];
    for y in tile_bounds.y0..tile_bounds.y1 {
        let local_y = (y - tile_bounds.y0) as usize;
        let local_ix = local_y * TILE_SIZE as usize;
        let image_ix = (y as u32 * image_width + tile_bounds.x0 as u32) as usize;
        let width = (tile_bounds.x1 - tile_bounds.x0) as usize;
        out[local_ix..local_ix + width]
            .copy_from_slice(unsafe { std::slice::from_raw_parts(pixels.add(image_ix), width) });
    }
    out
}

pub(crate) unsafe fn store_tile_buffer_raw(
    pixels: *mut u32,
    image_width: u32,
    tile_bounds: Bounds,
    tile_pixels: &TileBuffer,
) {
    for y in tile_bounds.y0..tile_bounds.y1 {
        let local_y = (y - tile_bounds.y0) as usize;
        let local_ix = local_y * TILE_SIZE as usize;
        let image_ix = (y as u32 * image_width + tile_bounds.x0 as u32) as usize;
        let width = (tile_bounds.x1 - tile_bounds.x0) as usize;
        unsafe {
            std::slice::from_raw_parts_mut(pixels.add(image_ix), width)
                .copy_from_slice(&tile_pixels[local_ix..local_ix + width])
        };
    }
}

/// Write a solid premultiplied color into a tile region (parallel-safe: disjoint tile bounds).
///
/// # Safety
/// `pixels` must point at a valid `image_width * image_height` RGBA8 buffer.
pub(crate) unsafe fn store_solid_tile_raw(
    pixels: *mut u32,
    image_width: u32,
    tile_bounds: Bounds,
    color: u32,
) {
    for y in tile_bounds.y0..tile_bounds.y1 {
        let image_ix = (y as u32 * image_width + tile_bounds.x0 as u32) as usize;
        let width = (tile_bounds.x1 - tile_bounds.x0) as usize;
        unsafe { std::slice::from_raw_parts_mut(pixels.add(image_ix), width).fill(color) };
    }
}

/// Tile-local or framebuffer-backed pixel surface for fine compositing.
pub(crate) struct PixelSurface<'a> {
    pixels: &'a mut [u32],
    stride: usize,
    origin_x: usize,
    origin_y: usize,
}

impl<'a> PixelSurface<'a> {
    pub(crate) fn tile(pixels: &'a mut TileBuffer) -> Self {
        Self {
            pixels,
            stride: TILE_SIZE as usize,
            origin_x: 0,
            origin_y: 0,
        }
    }

    #[inline]
    fn ix(&self, local_x: usize, local_y: usize) -> usize {
        (self.origin_y + local_y) * self.stride + self.origin_x + local_x
    }

    #[inline]
    pub(crate) fn get(&self, local_x: usize, local_y: usize) -> u32 {
        self.pixels[self.ix(local_x, local_y)]
    }

    #[inline]
    pub(crate) fn set(&mut self, local_x: usize, local_y: usize, value: u32) {
        self.pixels[self.ix(local_x, local_y)] = value;
    }

    pub(crate) fn fill_row(
        &mut self,
        local_y: usize,
        local_x0: usize,
        local_x1: usize,
        value: u32,
    ) {
        let start = self.ix(local_x0, local_y);
        let end = self.ix(local_x1, local_y);
        self.pixels[start..end].fill(value);
    }

    pub(crate) fn src_over_row(
        &mut self,
        local_y: usize,
        local_x0: usize,
        local_x1: usize,
        src: u32,
    ) {
        let start = self.ix(local_x0, local_y);
        let end = self.ix(local_x1, local_y);
        let row = &mut self.pixels[start..end];
        let mut chunks = row.chunks_exact_mut(4);
        for chunk in &mut chunks {
            let dst = u32x4::from([chunk[0], chunk[1], chunk[2], chunk[3]]);
            chunk.copy_from_slice(&src_over_premul_u8x4(dst, src).to_array());
        }
        for dst in chunks.into_remainder() {
            *dst = src_over_premul_u8(*dst, src);
        }
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
    use super::*;

    #[test]
    fn tile_mask_iterates_all_set_bits_in_order() {
        let mask = TileMask::from_words([
            (1u64 << 0) | (1u64 << 5) | (1u64 << 63),
            (1u64 << 0) | (1u64 << 7),
            0,
            1u64 << 63,
        ]);

        let bits: Vec<_> = mask.iter_ones().collect();
        assert_eq!(bits, vec![0, 5, 63, 64, 71, 255]);
    }

    #[test]
    fn tile_mask_set_get_clear_work() {
        let mut mask = TileMask::new();
        assert!(!mask.any());

        mask.set(3);
        mask.set(130);
        assert!(mask.get(3));
        assert!(mask.get(130));
        assert_eq!(mask.into_iter().collect::<Vec<_>>(), vec![3, 130]);

        mask.clear(3);
        assert!(!mask.get(3));
        assert!(mask.get(130));
    }

    #[test]
    fn src_over_u8x4_matches_scalar() {
        let dst = [0x0000_0000, 0x8040_2010, 0xff80_4020, 0x7f12_3456];
        for src in [0x0000_0000, 0xff44_2211, 0x8044_2211, 0x0180_6040] {
            let expected = dst.map(|dst| src_over_premul_u8(dst, src));
            let actual = src_over_premul_u8x4(u32x4::from(dst), src).to_array();
            assert_eq!(actual, expected, "src={src:#010x}");
        }
    }
}
