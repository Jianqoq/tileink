use super::*;
use crate::text::{TextContext, TextLayoutOptions, TextRasterOptions, tests::font_system};
use cosmic_text::{Attrs, Family, SubpixelBin, Weight};

#[allow(
    clippy::unwrap_used,
    reason = "Bundled outline fixtures must produce visible glyphs"
)]
fn assert_channel_coverage(
    fonts: &mut FontSystem,
    scaler: &mut ScaleContext,
    key: CacheKey,
    mode: TextSubpixelMode,
    embolden: f32,
) {
    let actual = raster_glyph_image(fonts, scaler, key, mode, embolden).unwrap();
    let offset = raster_glyph_offset(key);
    let channels = harmony_lcd_outline_shifts(mode).map(|shift| {
        raster_glyph_image_with_sources(
            fonts,
            scaler,
            key,
            embolden,
            Vector::new(offset.x + shift, offset.y),
            &[Source::Outline],
        )
        .unwrap()
    });
    let expected = merge_harmony_lcd_masks(channels);
    assert_eq!(actual.content, expected.content);
    let placement = |p: Placement| (p.left, p.top, p.width, p.height);
    assert_eq!(
        placement(actual.placement),
        placement(expected.placement),
        "unexpected filter padding: {key:?} {mode:?} {embolden}"
    );
    assert_eq!(
        actual.data, expected.data,
        "channel coverage changed: {key:?} {mode:?} {embolden}"
    );
}

#[test]
fn harmony_preserves_independently_antialiased_channel_coverage() {
    // Harmony channels already integrate pixel coverage. A second spatial filter
    // spreads their edges again; compare the actual entry point with those samples.
    let mut fonts = font_system();
    fonts
        .db_mut()
        .load_font_data(include_bytes!("../../svg/fonts/NotoSerif-Regular.ttf").to_vec());
    let mut text = TextContext::new();
    let mut scaler = ScaleContext::new();
    for (family, weight) in [
        ("Noto Sans", Weight::NORMAL),
        ("Noto Sans", Weight::BOLD),
        ("Noto Serif", Weight::NORMAL),
    ] {
        for (size, scale) in [
            (12.0, 1.0),
            (12.0, 1.25),
            (12.0, 2.0),
            (16.0, 1.0),
            (16.0, 1.25),
            (16.0, 2.0),
        ] {
            let layout = text.layout(
                &mut fonts,
                TextLayoutOptions::new("Hnev", size * scale)
                    .with_attrs(Attrs::new().family(Family::Name(family)).weight(weight)),
            );
            assert_eq!(layout.glyphs().len(), 4);
            for glyph in layout.glyphs() {
                for (x_bin, y_bin) in [
                    (SubpixelBin::Zero, SubpixelBin::Zero),
                    (SubpixelBin::One, SubpixelBin::Two),
                    (SubpixelBin::Three, SubpixelBin::One),
                ] {
                    let key = CacheKey {
                        x_bin,
                        y_bin,
                        ..glyph.cache_key
                    };
                    for mode in [TextSubpixelMode::Rgb, TextSubpixelMode::Bgr] {
                        for embolden in [0.0, TextRasterOptions::default().mask_embolden()] {
                            assert_channel_coverage(&mut fonts, &mut scaler, key, mode, embolden);
                        }
                    }
                }
            }
        }
    }
}
