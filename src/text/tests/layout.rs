use super::*;

#[test]
fn load_font_file_makes_font_available_to_layout() {
    let mut font_system = FontSystem::new_with_locale_and_db("en-US".into(), Default::default());
    let mut context = TextContext::new();
    let font_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/svg/fonts/NotoSans-Regular.ttf");

    font_system
        .db_mut()
        .load_font_file(font_path)
        .expect("load font file");
    context.clear_glyph_caches();
    let layout = context.layout(
        &mut font_system,
        TextLayoutOptions::new("Noto", 20.0)
            .with_attrs(Attrs::new().family(Family::Name("Noto Sans"))),
    );

    assert!(!layout.is_empty());
}

#[test]
fn layout_options_pass_cosmic_attrs_to_shaping() {
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(
        &mut font_system,
        TextLayoutOptions::new("A", 20.0).with_attrs(Attrs::new().weight(Weight::BOLD)),
    );
    assert!(!layout.glyphs.is_empty());

    assert_eq!(layout.glyphs[0].cache_key.font_weight, Weight::BOLD);
}

#[test]
fn layout_options_pass_hinting_flags_to_glyph_keys() {
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(
        &mut font_system,
        TextLayoutOptions::new("A", 20.0)
            .with_attrs(Attrs::new().cache_key_flags(CacheKeyFlags::DISABLE_HINTING)),
    );
    assert!(!layout.glyphs.is_empty());

    assert!(
        layout.glyphs[0]
            .cache_key
            .flags
            .contains(CacheKeyFlags::DISABLE_HINTING)
    );
}

#[test]
fn layout_options_pass_alignment_to_cosmic_buffer() {
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let left = context.layout(
        &mut font_system,
        TextLayoutOptions::new("A", 20.0)
            .with_size(Some(200.0), None)
            .with_wrap(Wrap::None),
    );
    let center = context.layout(
        &mut font_system,
        TextLayoutOptions::new("A", 20.0)
            .with_size(Some(200.0), None)
            .with_alignment(Some(Align::Center))
            .with_wrap(Wrap::None),
    );
    assert!(!left.glyphs.is_empty() && !center.glyphs.is_empty());

    assert!(center.glyphs[0].x > left.glyphs[0].x);
}

#[test]
fn layout_options_preserve_cosmic_text_default_wrap() {
    assert_eq!(TextLayoutOptions::new("text", 16.0).wrap, Wrap::WordOrGlyph);
}

#[test]
fn bounded_none_wrap_keeps_text_on_one_line() {
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let text = "one two three four";
    let unbounded = context.layout(
        &mut font_system,
        TextLayoutOptions::new(text, 20.0).with_wrap(Wrap::None),
    );
    let options = TextLayoutOptions::new(text, 20.0)
        .with_size(Some(35.0), None)
        .with_wrap(Wrap::None);
    let layout = context.layout(&mut font_system, options);
    assert!(!layout.glyphs.is_empty());

    let first_y = layout.glyphs[0].y;
    assert!(layout.glyphs.iter().all(|glyph| glyph.y == first_y));
    assert_eq!(layout.glyphs.len(), unbounded.glyphs.len());
}

#[test]
fn layout_handles_emoji_sequences_without_panicking() {
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(
        &mut font_system,
        TextLayoutOptions::new("Emoji 😀 👍🏽 👨‍👩‍👧‍👦 🇺🇸", 32.0),
    );
    assert!(!layout.glyphs.is_empty());

    assert!(!layout.bounds().is_empty());
}

#[test]
fn layout_outline_path_extracts_scalable_glyph_paths() {
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("Outline", 42.0));
    assert!(!layout.is_empty());

    let path = context.layout_outline_path(&mut font_system, &layout, Point::new(8.0, 48.0));

    assert!(!path.is_empty());
    assert!(path.bounding_box().width() > 1.0);
    assert!(path.bounding_box().height() > 1.0);
}

fn assert_shaped_cosmic_buffer_matches_owned_layout(options: TextLayoutOptions<'_>) {
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let expected = context.layout(&mut font_system, options.clone());
    let mut buffer = cosmic_text::Buffer::new(
        &mut font_system,
        cosmic_text::Metrics::new(options.font_size, options.line_height),
    );
    buffer.set_size(options.width, options.height);
    buffer.set_wrap(options.wrap);
    buffer.set_text(
        options.text,
        &options.attrs,
        cosmic_text::Shaping::Advanced,
        options.alignment,
    );
    let actual = context.layout_buffer(&mut font_system, &mut buffer);

    assert!(
        !expected.is_empty(),
        "bundled font must shape the comparison text"
    );
    assert!(!actual.is_empty());
    assert_eq!(actual.bounds, expected.bounds);
    assert_eq!(actual.glyphs.len(), expected.glyphs.len());
    for (actual, expected) in actual.glyphs.iter().zip(&expected.glyphs) {
        assert_eq!(actual.cache_key, expected.cache_key);
        assert_eq!((actual.x, actual.y), (expected.x, expected.y));
        assert_eq!(actual.outline_origin, expected.outline_origin);
    }
}

#[test]
fn shaped_cosmic_buffer_layout_matches_owned_layout() {
    assert_shaped_cosmic_buffer_matches_owned_layout(
        TextLayoutOptions::new("Market Watch", 12.0)
            .with_line_height(16.0)
            .with_size(Some(180.0), Some(24.0)),
    );
}

#[test]
fn shaped_cosmic_buffer_preserves_alignment_and_no_wrap() {
    assert_shaped_cosmic_buffer_matches_owned_layout(
        TextLayoutOptions::new("Market Watch", 12.0)
            .with_line_height(16.0)
            .with_size(Some(180.0), Some(24.0))
            .with_alignment(Some(cosmic_text::Align::Center))
            .with_wrap(cosmic_text::Wrap::None),
    );
}
