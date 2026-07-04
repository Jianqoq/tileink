use cosmic_text::{CacheKey, CacheKeyFlags, FontSystem};
use swash::{
    scale::ScaleContext,
    zeno::{Angle, Transform},
};

pub(super) fn with_glyph_scaler<R>(
    font_system: &mut FontSystem,
    context: &mut ScaleContext,
    cache_key: CacheKey,
    f: impl FnOnce(&mut swash::scale::Scaler<'_>) -> R,
) -> Option<R> {
    let font = font_system.get_font(cache_key.font_id, cache_key.font_weight)?;

    let swash_font = font.as_swash();
    let weight_tag = swash::Tag::from_be_bytes(*b"wght");
    let variable_width = swash_font.variations().find_by_tag(weight_tag);

    let mut scaler = context
        .builder(swash_font)
        .size(f32::from_bits(cache_key.font_size_bits))
        .hint(!cache_key.flags.contains(CacheKeyFlags::DISABLE_HINTING));
    if let Some(variation) = variable_width {
        scaler = scaler.normalized_coords(swash_font.variations().normalized_coords([(
            weight_tag,
            f32::from(cache_key.font_weight.0).clamp(variation.min_value(), variation.max_value()),
        )]));
    }
    let mut scaler = scaler.build();
    Some(f(&mut scaler))
}

pub(super) fn fake_italic_transform(cache_key: CacheKey) -> Option<Transform> {
    cache_key
        .flags
        .contains(CacheKeyFlags::FAKE_ITALIC)
        .then(|| Transform::skew(Angle::from_degrees(14.0), Angle::from_degrees(0.0)))
}
