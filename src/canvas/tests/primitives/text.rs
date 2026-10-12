use super::*;

#[test]
fn append_fast_path_offsets_text_runs_without_mutating_child() {
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("AA", 20.0));
    assert!(
        !layout.is_empty(),
        "the checked-in font must shape the test text"
    );

    let mut child = test_scene();
    child
        .push_text_layout(&layout, Point::new(0.0, 20.0), Brush::Solid(rgb(0, 0, 0)))
        .expect("layout should produce a text draw");
    let child_runs = child.text_runs.clone();
    let child_glyphs = child.text_glyphs.clone();

    let mut parent = test_scene();
    parent.append(&child, Point::new(4.0, 8.0));
    parent.append(&child, Point::new(40.0, 80.0));

    assert_eq!(child.text_runs.len(), child_runs.len());
    for (actual, expected) in child.text_runs.iter().zip(&child_runs) {
        assert_eq!(actual.glyph_start, expected.glyph_start);
        assert_eq!(actual.glyph_count, expected.glyph_count);
    }
    assert_eq!(child.text_glyphs.len(), child_glyphs.len());
    for (actual, expected) in child.text_glyphs.iter().zip(&child_glyphs) {
        assert_eq!(actual.x, expected.x);
        assert_eq!(actual.y, expected.y);
    }
    assert_eq!(parent.text_runs.len(), 2);
    assert_eq!(parent.text_runs[0].glyph_start, 0);
    assert_eq!(parent.text_runs[1].glyph_start, child_glyphs.len() as u32);
}

#[test]
fn text_draw_tracks_run_and_glyph_positions() {
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("Cache", 20.0));
    assert!(
        !layout.is_empty(),
        "the checked-in font must shape the test text"
    );

    let mut canvas = test_scene();
    let draw = canvas
        .push_text_layout(&layout, Point::new(8.0, 32.0), Brush::Solid(rgb(0, 0, 0)))
        .expect("layout should produce a text draw");

    assert_eq!(canvas.draw_records[draw.index()].glyph_run_id(), Some(0));
    assert_eq!(canvas.text_runs[0].glyph_start, 0);
    assert_eq!(
        canvas.text_runs[0].glyph_count,
        canvas.text_glyphs.len() as u32
    );
}

#[test]
fn push_text_layout_scales_glyph_cache_keys_to_physical_size() {
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("Scale", 20.0));
    assert!(
        !layout.is_empty(),
        "the checked-in font must shape the test text"
    );

    let mut canvas = Canvas::new(128, 64, 2.0);
    canvas
        .push_text_layout(&layout, Point::new(4.0, 12.0), Brush::Solid(rgb(0, 0, 0)))
        .expect("layout should produce a text draw");

    assert!(!canvas.text_glyphs.is_empty());
    let glyph = canvas.text_glyphs[0];
    assert_eq!(f32::from_bits(glyph.cache_key.font_size_bits), 40.0);
    assert!(glyph.x >= 8);
}

// Keep text recording tests independent of system fonts and never silently skip assertions.
fn font_system() -> TextFontSystem {
    let mut fonts = TextFontSystem::new_with_locale_and_db(
        "en-US".into(),
        cosmic_text::fontdb::Database::default(),
    );
    fonts
        .db_mut()
        .load_font_data(include_bytes!("../../../svg/fonts/NotoSans-Regular.ttf").to_vec());
    fonts.db_mut().set_sans_serif_family("Noto Sans");
    fonts
}
