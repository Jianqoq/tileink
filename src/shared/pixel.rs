use crate::{BLOCK_SIZE, shared::image::rgba8_pack};

pub type TileBuffer = [u32; BLOCK_SIZE as usize];

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
    src_over_mask_linear_auto_with_params_u8(dst, src, coverage, TextCoverageParams::DEFAULT)
}

pub(crate) fn src_over_mask_linear_auto_with_params_u8(
    dst: u32,
    src: u32,
    coverage: u8,
    params: TextCoverageParams,
) -> u32 {
    let coverage = auto_text_coverage(
        dst,
        src,
        coverage,
        params,
        TextCoverageMode {
            chroma_scale: params.alpha_mask_chroma_scale,
            low_luma_chroma_reduction: params.alpha_mask_low_luma_chroma_reduction
                * (params.alpha_mask_chroma_scale - params.subpixel_mask_chroma_scale).max(0.0),
            low_luma_contrast_limit: params.alpha_mask_low_luma_contrast_limit,
            apparent_axis_strength: params.alpha_mask_apparent_axis_strength,
            apparent_axis_luma_limit: params.alpha_mask_apparent_axis_luma_limit,
            destination_chroma_boost: false,
        },
    );
    src_over_mask_linear_u8(dst, src, coverage)
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
    src_over_subpixel_mask_linear_auto_with_params_u8(
        dst,
        src,
        mask,
        clip,
        TextCoverageParams::DEFAULT,
    )
}

pub(crate) fn src_over_subpixel_mask_linear_auto_with_params_u8(
    dst: u32,
    src: u32,
    mask: [u8; 3],
    clip: u8,
    params: TextCoverageParams,
) -> u32 {
    let clipped = [
        mul_div255(mask[0], clip),
        mul_div255(mask[1], clip),
        mul_div255(mask[2], clip),
    ];
    let subpixel_mode = TextCoverageMode::subpixel(params);
    let compensated = [
        auto_text_coverage(dst, src, clipped[0], params, subpixel_mode),
        auto_text_coverage(dst, src, clipped[1], params, subpixel_mode),
        auto_text_coverage(dst, src, clipped[2], params, subpixel_mode),
    ];
    src_over_subpixel_mask_linear_u8(
        dst,
        src,
        subpixel_axis_corrected_mask(
            dst,
            src,
            compensated,
            TextCoverageMode::subpixel_axis(params),
        ),
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

#[derive(Clone, Copy)]
struct TextCoverageMode {
    chroma_scale: f32,
    low_luma_chroma_reduction: f32,
    low_luma_contrast_limit: f32,
    apparent_axis_strength: f32,
    apparent_axis_luma_limit: f32,
    destination_chroma_boost: bool,
}

impl TextCoverageMode {
    fn subpixel(params: TextCoverageParams) -> Self {
        Self {
            chroma_scale: params.subpixel_mask_chroma_scale,
            low_luma_chroma_reduction: params.subpixel_mask_low_luma_chroma_reduction,
            low_luma_contrast_limit: params.subpixel_mask_low_luma_contrast_limit,
            apparent_axis_strength: 0.0,
            apparent_axis_luma_limit: params.subpixel_mask_apparent_axis_luma_limit,
            destination_chroma_boost: true,
        }
    }

    fn subpixel_axis(params: TextCoverageParams) -> Self {
        Self {
            chroma_scale: params.subpixel_mask_chroma_scale,
            low_luma_chroma_reduction: params.subpixel_mask_low_luma_chroma_reduction,
            low_luma_contrast_limit: params.subpixel_mask_low_luma_contrast_limit,
            apparent_axis_strength: params.subpixel_mask_apparent_axis_strength,
            apparent_axis_luma_limit: params.subpixel_mask_apparent_axis_luma_limit,
            destination_chroma_boost: true,
        }
    }
}

#[derive(Clone, Copy)]
struct TextColorStats {
    linear: [f32; 3],
    srgb: [f32; 3],
    luma: f32,
    perceptual_luma: f32,
    chroma: f32,
    max: f32,
}

fn auto_text_coverage(
    dst: u32,
    src: u32,
    coverage: u8,
    params: TextCoverageParams,
    mode: TextCoverageMode,
) -> u8 {
    if coverage == 0 || coverage == MASK_OPAQUE {
        return coverage;
    }

    let src_stats = text_color_stats(src);
    let dst_stats = text_color_stats(dst);
    let channel_contrast = (src_stats.linear[0] - dst_stats.linear[0])
        .abs()
        .max((src_stats.linear[1] - dst_stats.linear[1]).abs())
        .max((src_stats.linear[2] - dst_stats.linear[2]).abs());
    let alpha_mask_chroma_excess = (mode.chroma_scale - params.subpixel_mask_chroma_scale).max(0.0);
    let luma_contrast = (src_stats.luma - dst_stats.luma).abs();
    let low_luma_contrast = if mode.low_luma_contrast_limit > 0.0 {
        ((mode.low_luma_contrast_limit - luma_contrast) / mode.low_luma_contrast_limit)
            .clamp(0.0, 1.0)
    } else {
        0.0
    };
    // DirectWrite stays much lighter on very low-luma-contrast, high-chroma
    // light-on-dark text. The squared falloff keeps suppression concentrated
    // near that case instead of thinning medium-contrast text.
    let low_luma_contrast = low_luma_contrast * low_luma_contrast;
    // Saturated blue/purple text can have higher linear luminance while still
    // reading darker in sRGB. Do not apply the light-on-dark thinning gate to
    // those source-dominant dark-on-light cases.
    let perceptual_light_on_dark_gate = if src_stats.perceptual_luma >= dst_stats.perceptual_luma {
        1.0
    } else {
        0.0
    };
    let low_luma_chroma_suppression = mode.low_luma_chroma_reduction
        * low_luma_contrast
        * perceptual_light_on_dark_gate
        * channel_contrast
        * ((src_stats.chroma + dst_stats.chroma) * 0.5).clamp(0.0, 1.0);
    let src_chroma_dominance = ((src_stats.chroma - dst_stats.chroma) * 2.0).clamp(0.0, 1.0);
    let source_chroma_contrast_gate = if params.source_chroma_coverage_contrast_limit > 0.0 {
        ((params.source_chroma_coverage_contrast_limit - luma_contrast)
            / params.source_chroma_coverage_contrast_limit)
            .clamp(0.0, 1.0)
    } else {
        0.0
    };
    // Foreground-dominant saturated text can look too thin against a less
    // saturated surface. Apply this as a separate exponent reduction so it is
    // not capped by the generic dark-on-light curve.
    let source_chroma_coverage_boost = params.source_chroma_coverage_boost
        * source_chroma_contrast_gate
        * src_chroma_dominance
        * src_stats.chroma
        * channel_contrast;
    if src_stats.luma < dst_stats.luma {
        let contrast = (dst_stats.luma - src_stats.luma).clamp(0.0, 1.0);
        let hidden_chroma_contrast = (channel_contrast - contrast).max(0.0);
        let dst_chroma_dominance = ((dst_stats.chroma - src_stats.chroma) * 2.0).clamp(0.0, 1.0);
        // Very dark colored foreground has little linear chroma, but LCD masks
        // on saturated light backgrounds still need to preserve hidden channel
        // contrast. Keep this out of grayscale alpha masks to avoid over-bold
        // low-contrast colored text.
        let dark_on_light_chroma = if mode.destination_chroma_boost {
            src_stats
                .chroma
                .max(dst_stats.chroma * (1.0 - src_stats.max))
        } else {
            src_stats.chroma
        };
        let curve = (contrast
            * (params.dark_on_light_luma_base - params.dark_on_light_luma_taper * dst_stats.luma)
            + params.dark_on_light_chroma_boost
                * mode.chroma_scale
                * hidden_chroma_contrast
                * dark_on_light_chroma)
            .clamp(0.0, 1.0);
        let exponent =
            1.0 - params.dark_on_light_coverage_strength * curve - source_chroma_coverage_boost
                + low_luma_chroma_suppression * dst_chroma_dominance;
        let exponent = exponent.max(0.03);
        let compensated = (coverage as f32 * (1.0 / 255.0)).powf(exponent);
        return apparent_axis_corrected_coverage(compensated, src_stats, dst_stats, mode);
    }

    let contrast = (src_stats.luma - dst_stats.luma).clamp(0.0, 1.0);
    let black_surface = ((params.light_on_dark_black_luma_limit - dst_stats.luma)
        / params.light_on_dark_black_luma_limit)
        .clamp(0.0, 1.0);
    let high_luma_chroma = if src_stats.max > 0.0 {
        src_stats.chroma
            * (src_stats.luma / src_stats.max - params.light_on_dark_high_luma_threshold).max(0.0)
    } else {
        0.0
    };
    let colored_dark_surface = ((params.light_on_colored_dark_luma_limit - dst_stats.luma)
        / params.light_on_colored_dark_luma_limit)
        .clamp(0.0, 1.0)
        * (dst_stats.chroma * 4.0).clamp(0.0, 1.0);
    let exponent = 1.0
        + black_surface
            * (params.light_on_dark_coverage_reduction * contrast * src_stats.luma
                + params.light_on_dark_chroma_reduction
                    * mode.chroma_scale
                    * src_stats.chroma
                    * src_stats.max
                + params.light_on_dark_high_luma_chroma_reduction
                    * mode.chroma_scale
                    * high_luma_chroma)
        + params.light_on_colored_dark_chroma_reduction
            * alpha_mask_chroma_excess
            * colored_dark_surface
            * src_stats.chroma
            * channel_contrast
        + low_luma_chroma_suppression
        - source_chroma_coverage_boost * (1.0 - black_surface);
    let exponent = exponent.max(0.03);
    let compensated = (coverage as f32 * (1.0 / 255.0)).powf(exponent);
    apparent_axis_corrected_coverage(compensated, src_stats, dst_stats, mode)
}

fn apparent_axis_corrected_coverage(
    coverage: f32,
    src: TextColorStats,
    dst: TextColorStats,
    mode: TextCoverageMode,
) -> u8 {
    let coverage = coverage.clamp(0.0, 1.0);
    if mode.apparent_axis_strength <= 0.0 || mode.apparent_axis_luma_limit <= 0.0 {
        return coverage_f32_to_u8(coverage);
    }

    let perceptual_luma_contrast = (src.perceptual_luma - dst.perceptual_luma).abs();
    let luma_gate = ((mode.apparent_axis_luma_limit - perceptual_luma_contrast)
        / mode.apparent_axis_luma_limit)
        .clamp(0.0, 1.0);
    if luma_gate <= 0.0 {
        return coverage_f32_to_u8(coverage);
    }

    let chroma_gate = src.chroma.max(dst.chroma).clamp(0.0, 1.0);
    if chroma_gate <= 0.0 {
        return coverage_f32_to_u8(coverage);
    }

    let (projection, derivative) =
        apparent_axis_projection_at_coverage(src.linear, dst.linear, src.srgb, dst.srgb, coverage);
    if derivative <= 1.0e-4 {
        return coverage_f32_to_u8(coverage);
    }
    // The projection curve is the visible sRGB coverage produced by
    // linear-light compositing. Correct along the local inverse slope instead
    // of assuming projection changes one-for-one with mask coverage.
    let inverse_delta = (coverage - projection) / derivative.clamp(0.2, 5.0);
    let correction =
        (mode.apparent_axis_strength * luma_gate * chroma_gate).clamp(0.0, 1.0) * inverse_delta;
    if correction < 0.0 && perceptual_luma_contrast < mode.apparent_axis_luma_limit * 0.05 {
        return coverage_f32_to_u8(coverage);
    }
    coverage_f32_to_u8(coverage + correction)
}

fn subpixel_axis_corrected_mask(
    dst: u32,
    src: u32,
    mask: [u8; 3],
    mode: TextCoverageMode,
) -> [u8; 3] {
    if mode.apparent_axis_strength <= 0.0 || mode.apparent_axis_luma_limit <= 0.0 {
        return mask;
    }

    let src_stats = text_color_stats(src);
    let dst_stats = text_color_stats(dst);
    let perceptual_luma_contrast = (src_stats.perceptual_luma - dst_stats.perceptual_luma).abs();
    let luma_gate = ((mode.apparent_axis_luma_limit - perceptual_luma_contrast)
        / mode.apparent_axis_luma_limit)
        .clamp(0.0, 1.0);
    let chroma_gate = src_stats.chroma.max(dst_stats.chroma).clamp(0.0, 1.0);
    if luma_gate <= 0.0 || chroma_gate <= 0.0 {
        return mask;
    }

    let target = (f32::from(mask[0]) + f32::from(mask[1]) + f32::from(mask[2])) * (1.0 / 765.0);
    let projected = apparent_axis_coverage_of_srgb8(
        src_over_subpixel_mask_linear_u8(dst, src, mask, MASK_OPAQUE),
        src_stats.srgb,
        dst_stats.srgb,
    );
    let correction =
        mode.apparent_axis_strength * luma_gate * chroma_gate * (target - projected).max(0.0);
    [
        coverage_f32_to_u8(f32::from(mask[0]) * (1.0 / 255.0) + correction),
        coverage_f32_to_u8(f32::from(mask[1]) * (1.0 / 255.0) + correction),
        coverage_f32_to_u8(f32::from(mask[2]) * (1.0 / 255.0) + correction),
    ]
}

fn apparent_axis_projection_at_coverage(
    src_linear: [f32; 3],
    dst_linear: [f32; 3],
    src_srgb: [f32; 3],
    dst_srgb: [f32; 3],
    coverage: f32,
) -> (f32, f32) {
    let axis = [
        src_srgb[0] - dst_srgb[0],
        src_srgb[1] - dst_srgb[1],
        src_srgb[2] - dst_srgb[2],
    ];
    let denom = axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2];
    if denom <= 1.0e-6 {
        return (0.5, 0.0);
    }

    let mixed_linear = [
        dst_linear[0] + (src_linear[0] - dst_linear[0]) * coverage,
        dst_linear[1] + (src_linear[1] - dst_linear[1]) * coverage,
        dst_linear[2] + (src_linear[2] - dst_linear[2]) * coverage,
    ];
    let mixed = [
        linear_to_srgb(mixed_linear[0]),
        linear_to_srgb(mixed_linear[1]),
        linear_to_srgb(mixed_linear[2]),
    ];
    let projection = (((mixed[0] - dst_srgb[0]) * axis[0]
        + (mixed[1] - dst_srgb[1]) * axis[1]
        + (mixed[2] - dst_srgb[2]) * axis[2])
        / denom)
        .clamp(0.0, 1.0);
    let derivative = (axis[0]
        * linear_to_srgb_derivative(mixed_linear[0])
        * (src_linear[0] - dst_linear[0])
        + axis[1] * linear_to_srgb_derivative(mixed_linear[1]) * (src_linear[1] - dst_linear[1])
        + axis[2] * linear_to_srgb_derivative(mixed_linear[2]) * (src_linear[2] - dst_linear[2]))
        / denom;
    (projection, derivative.max(0.0))
}

fn apparent_axis_coverage_of_srgb8(px: u32, src_srgb: [f32; 3], dst_srgb: [f32; 3]) -> f32 {
    let px_srgb = straight_srgb_rgb_from_srgb8(px);
    let axis = [
        src_srgb[0] - dst_srgb[0],
        src_srgb[1] - dst_srgb[1],
        src_srgb[2] - dst_srgb[2],
    ];
    let denom = axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2];
    if denom <= 1.0e-6 {
        return 0.0;
    }
    (((px_srgb[0] - dst_srgb[0]) * axis[0]
        + (px_srgb[1] - dst_srgb[1]) * axis[1]
        + (px_srgb[2] - dst_srgb[2]) * axis[2])
        / denom)
        .clamp(0.0, 1.0)
}

fn text_color_stats(px: u32) -> TextColorStats {
    let linear = straight_linear_rgb_from_srgb8(px);
    let srgb = straight_srgb_rgb_from_srgb8(px);
    let max = linear[0].max(linear[1]).max(linear[2]);
    let min = linear[0].min(linear[1]).min(linear[2]);
    TextColorStats {
        linear,
        srgb,
        luma: linear_luminance(linear),
        perceptual_luma: linear_luminance(srgb),
        chroma: (max - min).clamp(0.0, 1.0),
        max,
    }
}

fn straight_linear_rgb_from_srgb8(px: u32) -> [f32; 3] {
    let alpha = ((px >> 24) & 0xff) as f32 * (1.0 / 255.0);
    if alpha == 0.0 {
        return [0.0; 3];
    }

    let r = srgb_to_linear(((px & 0xff) as f32 * (1.0 / 255.0)) / alpha);
    let g = srgb_to_linear((((px >> 8) & 0xff) as f32 * (1.0 / 255.0)) / alpha);
    let b = srgb_to_linear((((px >> 16) & 0xff) as f32 * (1.0 / 255.0)) / alpha);
    [r, g, b]
}

fn straight_srgb_rgb_from_srgb8(px: u32) -> [f32; 3] {
    let alpha = ((px >> 24) & 0xff) as f32 * (1.0 / 255.0);
    if alpha == 0.0 {
        return [0.0; 3];
    }

    let inv_alpha = 1.0 / (alpha * 255.0);
    [
        ((px & 0xff) as f32 * inv_alpha).clamp(0.0, 1.0),
        (((px >> 8) & 0xff) as f32 * inv_alpha).clamp(0.0, 1.0),
        (((px >> 16) & 0xff) as f32 * inv_alpha).clamp(0.0, 1.0),
    ]
}

fn linear_luminance(rgb: [f32; 3]) -> f32 {
    0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]
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

fn linear_to_srgb_derivative(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    if value <= 0.003_130_8 {
        12.92
    } else {
        (1.055 / 2.4) * value.powf(1.0 / 2.4 - 1.0)
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
            TextCoverageParams, src_over_mask_linear_auto_u8,
            src_over_mask_linear_auto_with_params_u8, src_over_mask_linear_u8,
            src_over_subpixel_mask_linear_auto_u8,
            src_over_subpixel_mask_linear_auto_with_params_u8, src_over_subpixel_mask_linear_u8,
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
            [157, 157, 157, 255]
        );
    }

    #[test]
    fn auto_linear_mask_compensates_dark_text_more_on_light_gray_than_white_curve() {
        let white = rgba8_pack([255, 255, 255, 255]);
        let light_gray = rgba8_pack([224, 224, 224, 255]);
        let src = rgba8_pack([0, 0, 0, 255]);

        assert_eq!(
            unpack_rgba8(src_over_mask_linear_auto_u8(light_gray, src, 128)),
            [127, 127, 127, 255]
        );
        assert!(
            (src_over_mask_linear_auto_u8(light_gray, src, 128) & 0xff)
                < (src_over_mask_linear_auto_u8(white, src, 128) & 0xff)
        );
    }

    #[test]
    fn auto_linear_mask_reduces_light_text_on_dark_background() {
        let dst = rgba8_pack([0, 0, 0, 255]);
        let src = rgba8_pack([255, 255, 255, 255]);

        assert_eq!(
            unpack_rgba8(src_over_mask_linear_auto_u8(dst, src, 128)),
            [176, 176, 176, 255]
        );
    }

    #[test]
    fn auto_linear_mask_reduces_saturated_light_text_on_black_background() {
        let dst = rgba8_pack([0, 0, 0, 255]);
        let src = rgba8_pack([120, 160, 255, 255]);

        assert_eq!(
            unpack_rgba8(src_over_mask_linear_auto_u8(dst, src, 128)),
            [78, 106, 171, 255]
        );
        assert!(
            (src_over_mask_linear_auto_u8(dst, src, 128) >> 16)
                < (src_over_mask_linear_u8(dst, src, 128) >> 16)
        );
    }

    #[test]
    fn auto_linear_mask_reduces_high_luma_saturated_text_on_black_background() {
        let dst = rgba8_pack([0, 0, 0, 255]);
        let src = rgba8_pack([80, 220, 120, 255]);

        assert_eq!(
            unpack_rgba8(src_over_mask_linear_auto_u8(dst, src, 128)),
            [50, 146, 77, 255]
        );
    }

    #[test]
    fn auto_linear_mask_reduces_saturated_text_on_colored_dark_background() {
        let dst = rgba8_pack([24, 68, 160, 255]);
        let src = rgba8_pack([240, 72, 72, 255]);

        assert_eq!(
            unpack_rgba8(src_over_mask_linear_auto_u8(dst, src, 128)),
            [150, 69, 138, 255]
        );
    }

    #[test]
    fn auto_linear_mask_boosts_saturated_dark_text_on_light_background() {
        let dst = rgba8_pack([255, 255, 255, 255]);
        let src = rgba8_pack([32, 180, 72, 255]);

        assert_eq!(
            unpack_rgba8(src_over_mask_linear_auto_u8(dst, src, 128)),
            [158, 209, 165, 255]
        );
        assert!(
            (src_over_mask_linear_auto_u8(dst, src, 128) & 0xff)
                < (src_over_mask_linear_u8(dst, src, 128) & 0xff)
        );
    }

    #[test]
    fn auto_linear_alpha_mask_reduces_low_luma_contrast_chroma_without_touching_subpixel() {
        let dst = rgba8_pack([28, 61, 41, 255]);
        let src = rgba8_pack([255, 5, 115, 255]);
        let mut params = TextCoverageParams::DEFAULT;
        params.alpha_mask_chroma_scale = 2.1;
        params.alpha_mask_low_luma_chroma_reduction = 12.0;
        params.alpha_mask_low_luma_contrast_limit = 0.30;

        let default_alpha = src_over_mask_linear_auto_u8(dst, src, 128);
        let reduced_alpha = src_over_mask_linear_auto_with_params_u8(dst, src, 128, params);
        let default_rgb_delta = rgb_delta_sum(unpack_rgba8(dst), unpack_rgba8(default_alpha));
        let reduced_rgb_delta = rgb_delta_sum(unpack_rgba8(dst), unpack_rgba8(reduced_alpha));
        assert!(reduced_rgb_delta < default_rgb_delta);

        assert_eq!(
            src_over_subpixel_mask_linear_auto_with_params_u8(
                dst,
                src,
                [128, 128, 128],
                255,
                params
            ),
            src_over_subpixel_mask_linear_auto_u8(dst, src, [128, 128, 128], 255)
        );
    }

    #[test]
    fn auto_linear_alpha_mask_reduces_dark_text_only_when_destination_chroma_dominates() {
        let dst_high_chroma = rgba8_pack([195, 13, 219, 255]);
        let src_lower_chroma = rgba8_pack([2, 112, 6, 255]);
        let mut params = TextCoverageParams::DEFAULT;
        params.alpha_mask_low_luma_chroma_reduction = 12.0;
        params.alpha_mask_low_luma_contrast_limit = 0.30;

        let default_alpha = src_over_mask_linear_auto_u8(dst_high_chroma, src_lower_chroma, 128);
        let reduced_alpha = src_over_mask_linear_auto_with_params_u8(
            dst_high_chroma,
            src_lower_chroma,
            128,
            params,
        );
        assert!(
            rgb_delta_sum(unpack_rgba8(dst_high_chroma), unpack_rgba8(reduced_alpha))
                < rgb_delta_sum(unpack_rgba8(dst_high_chroma), unpack_rgba8(default_alpha))
        );

        let dst_lower_chroma = rgba8_pack([120, 120, 20, 255]);
        let src_high_chroma = rgba8_pack([0, 0, 180, 255]);
        assert_eq!(
            src_over_mask_linear_auto_with_params_u8(
                dst_lower_chroma,
                src_high_chroma,
                128,
                params
            ),
            src_over_mask_linear_auto_u8(dst_lower_chroma, src_high_chroma, 128)
        );
    }

    #[test]
    fn auto_linear_alpha_mask_does_not_thin_perceptually_dark_source_chroma_text() {
        let dst = rgba8_pack([0x52, 0x4e, 0x0f, 255]);
        let src = rgba8_pack([0x56, 0x05, 0xff, 255]);
        let mut params = TextCoverageParams::DEFAULT;
        params.alpha_mask_chroma_scale = 2.15;
        params.subpixel_mask_chroma_scale = 1.11;
        params.alpha_mask_low_luma_chroma_reduction = 10.6;
        params.alpha_mask_low_luma_contrast_limit = 0.29;
        let with_low_luma_gate = src_over_mask_linear_auto_with_params_u8(dst, src, 128, params);

        params.alpha_mask_low_luma_chroma_reduction = 0.0;
        let without_low_luma_gate = src_over_mask_linear_auto_with_params_u8(dst, src, 128, params);

        assert_eq!(with_low_luma_gate, without_low_luma_gate);
    }

    #[test]
    fn auto_linear_alpha_mask_apparent_axis_boosts_under_projected_colored_text() {
        let dst = rgba8_pack([0x58, 0x27, 0xdb, 255]);
        let src = rgba8_pack([0x70, 0x6e, 0x02, 255]);
        let base = src_over_mask_linear_auto_u8(dst, src, 128);
        let mut params = TextCoverageParams::DEFAULT;
        params.alpha_mask_apparent_axis_strength = 4.0;
        params.alpha_mask_apparent_axis_luma_limit = 0.45;
        let corrected = src_over_mask_linear_auto_with_params_u8(dst, src, 128, params);

        let target = 0.5;
        let base_coverage =
            apparent_axis_coverage(unpack_rgba8(base), [0x70, 0x6e, 0x02], [0x58, 0x27, 0xdb]);
        let corrected_coverage = apparent_axis_coverage(
            unpack_rgba8(corrected),
            [0x70, 0x6e, 0x02],
            [0x58, 0x27, 0xdb],
        );
        assert!(corrected_coverage > base_coverage);
        assert!((corrected_coverage - target).abs() < (base_coverage - target).abs());
    }

    #[test]
    fn auto_linear_alpha_mask_apparent_axis_reduces_over_projected_colored_text() {
        let dst = rgba8_pack([0x11, 0x52, 0x05, 255]);
        let src = rgba8_pack([0x15, 0x28, 0xb3, 255]);
        let base = src_over_mask_linear_auto_u8(dst, src, 128);
        let mut params = TextCoverageParams::DEFAULT;
        params.alpha_mask_apparent_axis_strength = 4.0;
        params.alpha_mask_apparent_axis_luma_limit = 0.45;
        let corrected = src_over_mask_linear_auto_with_params_u8(dst, src, 128, params);

        let target = 0.5;
        let base_coverage =
            apparent_axis_coverage(unpack_rgba8(base), [0x15, 0x28, 0xb3], [0x11, 0x52, 0x05]);
        let corrected_coverage = apparent_axis_coverage(
            unpack_rgba8(corrected),
            [0x15, 0x28, 0xb3],
            [0x11, 0x52, 0x05],
        );
        assert!(corrected_coverage < base_coverage);
        assert!((corrected_coverage - target).abs() < (base_coverage - target).abs());
    }

    #[test]
    fn auto_linear_alpha_mask_apparent_axis_ignores_neutral_text() {
        let dst = rgba8_pack([0, 0, 0, 255]);
        let src = rgba8_pack([255, 255, 255, 255]);
        let mut params = TextCoverageParams::DEFAULT;
        params.alpha_mask_apparent_axis_strength = 6.0;
        params.alpha_mask_apparent_axis_luma_limit = 0.45;

        assert_eq!(
            src_over_mask_linear_auto_with_params_u8(dst, src, 128, params),
            src_over_mask_linear_auto_u8(dst, src, 128)
        );
    }

    #[test]
    fn auto_linear_mask_boosts_dark_text_when_source_chroma_dominates() {
        let dst = rgba8_pack([76, 107, 19, 255]);
        let src = rgba8_pack([61, 4, 204, 255]);
        let mut params = TextCoverageParams::DEFAULT;
        params.dark_on_light_chroma_boost = 0.0;
        let without_source_boost = src_over_mask_linear_auto_with_params_u8(dst, src, 128, params);
        params.source_chroma_coverage_boost = 0.6;
        params.source_chroma_coverage_contrast_limit = 0.35;

        let boosted_alpha = src_over_mask_linear_auto_with_params_u8(dst, src, 128, params);
        assert!(
            rgb_delta_sum(unpack_rgba8(dst), unpack_rgba8(without_source_boost))
                < rgb_delta_sum(unpack_rgba8(dst), unpack_rgba8(boosted_alpha))
        );
    }

    #[test]
    fn auto_linear_subpixel_mask_reduces_low_luma_contrast_chroma_without_touching_alpha() {
        let dst = rgba8_pack([28, 61, 35, 255]);
        let src = rgba8_pack([230, 5, 99, 255]);
        let mut params = TextCoverageParams::DEFAULT;
        params.subpixel_mask_low_luma_chroma_reduction = 6.0;
        params.subpixel_mask_low_luma_contrast_limit = 0.30;

        let default_subpixel =
            src_over_subpixel_mask_linear_auto_u8(dst, src, [128, 128, 128], 255);
        let reduced_subpixel = src_over_subpixel_mask_linear_auto_with_params_u8(
            dst,
            src,
            [128, 128, 128],
            255,
            params,
        );
        let default_rgb_delta = rgb_delta_sum(unpack_rgba8(dst), unpack_rgba8(default_subpixel));
        let reduced_rgb_delta = rgb_delta_sum(unpack_rgba8(dst), unpack_rgba8(reduced_subpixel));
        assert!(reduced_rgb_delta < default_rgb_delta);

        assert_eq!(
            src_over_mask_linear_auto_with_params_u8(dst, src, 128, params),
            src_over_mask_linear_auto_u8(dst, src, 128)
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
            [157, 255, 0, 255]
        );
    }

    #[test]
    fn auto_linear_subpixel_mask_boosts_dark_text_on_saturated_light_background() {
        let dst = rgba8_pack([0x3e, 0x4f, 0xc2, 255]);
        let src = rgba8_pack([0x2e, 0x01, 0x05, 255]);
        let mask = [128, 128, 128];
        let base = src_over_subpixel_mask_linear_u8(dst, src, mask, 255);
        let boosted = src_over_subpixel_mask_linear_auto_u8(dst, src, mask, 255);
        let base_coverage =
            apparent_axis_coverage(unpack_rgba8(base), [0x2e, 0x01, 0x05], [0x3e, 0x4f, 0xc2]);
        let boosted_coverage = apparent_axis_coverage(
            unpack_rgba8(boosted),
            [0x2e, 0x01, 0x05],
            [0x3e, 0x4f, 0xc2],
        );

        assert!(boosted_coverage > base_coverage);
    }

    #[test]
    fn auto_linear_alpha_mask_ignores_destination_chroma_for_dark_text_boost() {
        let dst = rgba8_pack([0x3e, 0x4f, 0xc2, 255]);
        let src = rgba8_pack([0x10, 0x10, 0x10, 255]);
        let mut params = TextCoverageParams::DEFAULT;
        params.dark_on_light_chroma_boost = 0.0;

        assert_eq!(
            src_over_mask_linear_auto_u8(dst, src, 128),
            src_over_mask_linear_auto_with_params_u8(dst, src, 128, params)
        );
    }

    fn rgb_delta_sum(a: [u8; 4], b: [u8; 4]) -> u16 {
        (i16::from(a[0]) - i16::from(b[0])).unsigned_abs()
            + (i16::from(a[1]) - i16::from(b[1])).unsigned_abs()
            + (i16::from(a[2]) - i16::from(b[2])).unsigned_abs()
    }

    fn apparent_axis_coverage(px: [u8; 4], src: [u8; 3], dst: [u8; 3]) -> f32 {
        let axis = [
            f32::from(src[0]) - f32::from(dst[0]),
            f32::from(src[1]) - f32::from(dst[1]),
            f32::from(src[2]) - f32::from(dst[2]),
        ];
        let denom = axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2];
        ((f32::from(px[0]) - f32::from(dst[0])) * axis[0]
            + (f32::from(px[1]) - f32::from(dst[1])) * axis[1]
            + (f32::from(px[2]) - f32::from(dst[2])) * axis[2])
            / denom
    }
}
