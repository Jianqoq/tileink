use crate::shared::image::rgba8_pack;

pub(crate) const MASK_OPAQUE: u8 = 255;

/// Parameters for contrast-dependent text coverage compensation.
///
/// The default values mirror the GPU constants. WGPU text rendering accepts this
/// as runtime data so the quality harness can search parameter candidates
/// without rebuilding shaders for every trial.
#[derive(Clone, Copy, Debug)]
pub struct TextCoverageParams {
    pub dark_on_light_coverage_strength: f32,
    pub dark_on_light_luma_base: f32,
    pub dark_on_light_luma_taper: f32,
    pub dark_on_light_chroma_boost: f32,
    pub source_chroma_coverage_boost: f32,
    pub source_chroma_coverage_contrast_limit: f32,
    pub light_on_dark_coverage_reduction: f32,
    pub light_on_dark_black_luma_limit: f32,
    pub light_on_dark_chroma_reduction: f32,
    pub light_on_dark_high_luma_chroma_reduction: f32,
    pub light_on_dark_high_luma_threshold: f32,
    pub light_on_colored_dark_chroma_reduction: f32,
    pub light_on_colored_dark_luma_limit: f32,
    pub alpha_mask_chroma_scale: f32,
    pub subpixel_mask_chroma_scale: f32,
    /// Corrects low-contrast colored alpha masks whose apparent sRGB coverage
    /// is distorted by linear-light compositing along the foreground/background
    /// color axis.
    pub alpha_mask_apparent_axis_strength: f32,
    pub alpha_mask_apparent_axis_luma_limit: f32,
    /// Same correction for LCD/subpixel masks. Kept separate because channel
    /// masks already alter perceived edge color differently from alpha masks.
    pub subpixel_mask_apparent_axis_strength: f32,
    pub subpixel_mask_apparent_axis_luma_limit: f32,
    /// Extra outline embolden, in raster pixels, before alpha mask generation.
    ///
    /// This models the small-stem weight that DirectWrite hinting gives to
    /// tiny grayscale text and is part of the glyph cache key.
    pub alpha_mask_embolden: f32,
    /// Extra outline embolden, in raster pixels, before LCD/subpixel mask
    /// generation. Kept separate because LCD masks already add horizontal stem
    /// energy through their channel filter.
    pub subpixel_mask_embolden: f32,
    pub alpha_mask_low_luma_chroma_reduction: f32,
    pub alpha_mask_low_luma_contrast_limit: f32,
    pub subpixel_mask_low_luma_chroma_reduction: f32,
    pub subpixel_mask_low_luma_contrast_limit: f32,
}

impl TextCoverageParams {
    pub const DEFAULT: Self = Self {
        dark_on_light_coverage_strength: 0.95,
        dark_on_light_luma_base: 1.572_846_5,
        dark_on_light_luma_taper: 1.15,
        dark_on_light_chroma_boost: 0.365_655_8,
        source_chroma_coverage_boost: 0.0,
        source_chroma_coverage_contrast_limit: 0.231_008_04,
        light_on_dark_coverage_reduction: 0.206_628_05,
        light_on_dark_black_luma_limit: 0.028_754_03,
        light_on_dark_chroma_reduction: 0.114_799_53,
        light_on_dark_high_luma_chroma_reduction: 0.449_235_4,
        light_on_dark_high_luma_threshold: 0.261_297_02,
        light_on_colored_dark_chroma_reduction: 0.487_289_64,
        light_on_colored_dark_luma_limit: 0.115_199_71,
        alpha_mask_chroma_scale: 1.356_680_2,
        subpixel_mask_chroma_scale: 0.948_365_9,
        alpha_mask_apparent_axis_strength: 1.036_124,
        alpha_mask_apparent_axis_luma_limit: 0.688_732_8,
        subpixel_mask_apparent_axis_strength: 1.447_765,
        subpixel_mask_apparent_axis_luma_limit: 0.168_284_2,
        alpha_mask_embolden: 0.119_441_95,
        subpixel_mask_embolden: 0.032_732_67,
        alpha_mask_low_luma_chroma_reduction: 0.413_020_64,
        alpha_mask_low_luma_contrast_limit: 0.101_238_72,
        subpixel_mask_low_luma_chroma_reduction: 0.0,
        subpixel_mask_low_luma_contrast_limit: 0.063_855_61,
    };
}

impl Default for TextCoverageParams {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl PartialEq for TextCoverageParams {
    fn eq(&self, other: &Self) -> bool {
        self.dark_on_light_coverage_strength.to_bits()
            == other.dark_on_light_coverage_strength.to_bits()
            && self.dark_on_light_luma_base.to_bits() == other.dark_on_light_luma_base.to_bits()
            && self.dark_on_light_luma_taper.to_bits() == other.dark_on_light_luma_taper.to_bits()
            && self.dark_on_light_chroma_boost.to_bits()
                == other.dark_on_light_chroma_boost.to_bits()
            && self.source_chroma_coverage_boost.to_bits()
                == other.source_chroma_coverage_boost.to_bits()
            && self.source_chroma_coverage_contrast_limit.to_bits()
                == other.source_chroma_coverage_contrast_limit.to_bits()
            && self.light_on_dark_coverage_reduction.to_bits()
                == other.light_on_dark_coverage_reduction.to_bits()
            && self.light_on_dark_black_luma_limit.to_bits()
                == other.light_on_dark_black_luma_limit.to_bits()
            && self.light_on_dark_chroma_reduction.to_bits()
                == other.light_on_dark_chroma_reduction.to_bits()
            && self.light_on_dark_high_luma_chroma_reduction.to_bits()
                == other.light_on_dark_high_luma_chroma_reduction.to_bits()
            && self.light_on_dark_high_luma_threshold.to_bits()
                == other.light_on_dark_high_luma_threshold.to_bits()
            && self.light_on_colored_dark_chroma_reduction.to_bits()
                == other.light_on_colored_dark_chroma_reduction.to_bits()
            && self.light_on_colored_dark_luma_limit.to_bits()
                == other.light_on_colored_dark_luma_limit.to_bits()
            && self.alpha_mask_chroma_scale.to_bits() == other.alpha_mask_chroma_scale.to_bits()
            && self.subpixel_mask_chroma_scale.to_bits()
                == other.subpixel_mask_chroma_scale.to_bits()
            && self.alpha_mask_apparent_axis_strength.to_bits()
                == other.alpha_mask_apparent_axis_strength.to_bits()
            && self.alpha_mask_apparent_axis_luma_limit.to_bits()
                == other.alpha_mask_apparent_axis_luma_limit.to_bits()
            && self.subpixel_mask_apparent_axis_strength.to_bits()
                == other.subpixel_mask_apparent_axis_strength.to_bits()
            && self.subpixel_mask_apparent_axis_luma_limit.to_bits()
                == other.subpixel_mask_apparent_axis_luma_limit.to_bits()
            && self.alpha_mask_embolden.to_bits() == other.alpha_mask_embolden.to_bits()
            && self.subpixel_mask_embolden.to_bits() == other.subpixel_mask_embolden.to_bits()
            && self.alpha_mask_low_luma_chroma_reduction.to_bits()
                == other.alpha_mask_low_luma_chroma_reduction.to_bits()
            && self.alpha_mask_low_luma_contrast_limit.to_bits()
                == other.alpha_mask_low_luma_contrast_limit.to_bits()
            && self.subpixel_mask_low_luma_chroma_reduction.to_bits()
                == other.subpixel_mask_low_luma_chroma_reduction.to_bits()
            && self.subpixel_mask_low_luma_contrast_limit.to_bits()
                == other.subpixel_mask_low_luma_contrast_limit.to_bits()
    }
}

impl Eq for TextCoverageParams {}

impl std::hash::Hash for TextCoverageParams {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u32(self.dark_on_light_coverage_strength.to_bits());
        state.write_u32(self.dark_on_light_luma_base.to_bits());
        state.write_u32(self.dark_on_light_luma_taper.to_bits());
        state.write_u32(self.dark_on_light_chroma_boost.to_bits());
        state.write_u32(self.source_chroma_coverage_boost.to_bits());
        state.write_u32(self.source_chroma_coverage_contrast_limit.to_bits());
        state.write_u32(self.light_on_dark_coverage_reduction.to_bits());
        state.write_u32(self.light_on_dark_black_luma_limit.to_bits());
        state.write_u32(self.light_on_dark_chroma_reduction.to_bits());
        state.write_u32(self.light_on_dark_high_luma_chroma_reduction.to_bits());
        state.write_u32(self.light_on_dark_high_luma_threshold.to_bits());
        state.write_u32(self.light_on_colored_dark_chroma_reduction.to_bits());
        state.write_u32(self.light_on_colored_dark_luma_limit.to_bits());
        state.write_u32(self.alpha_mask_chroma_scale.to_bits());
        state.write_u32(self.subpixel_mask_chroma_scale.to_bits());
        state.write_u32(self.alpha_mask_apparent_axis_strength.to_bits());
        state.write_u32(self.alpha_mask_apparent_axis_luma_limit.to_bits());
        state.write_u32(self.subpixel_mask_apparent_axis_strength.to_bits());
        state.write_u32(self.subpixel_mask_apparent_axis_luma_limit.to_bits());
        state.write_u32(self.alpha_mask_embolden.to_bits());
        state.write_u32(self.subpixel_mask_embolden.to_bits());
        state.write_u32(self.alpha_mask_low_luma_chroma_reduction.to_bits());
        state.write_u32(self.alpha_mask_low_luma_contrast_limit.to_bits());
        state.write_u32(self.subpixel_mask_low_luma_chroma_reduction.to_bits());
        state.write_u32(self.subpixel_mask_low_luma_contrast_limit.to_bits());
    }
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
