use super::*;
use cosmic_text::{Buffer, Metrics, Scroll, Shaping};

fn buffer(fonts: &mut FontSystem, text: &str, height: f32, width: Option<f32>) -> Buffer {
    let mut buffer = Buffer::new(fonts, Metrics::new(14.0, height));
    buffer.set_size(width, None);
    buffer.set_text(text, &Attrs::new(), Shaping::Advanced, None);
    buffer.shape_until_scroll(fonts, false);
    buffer
}

#[test]
fn fractional_and_empty_lines_preserve_boxes_and_borrowed_shaping() {
    let mut fonts = font_system();
    let buffer = buffer(&mut fonts, "Heading\n\nTail", 19.25, None);
    let raw: Vec<_> = buffer.layout_runs().collect();
    let runs: Vec<_> = text_layout_runs(&buffer).collect();
    assert_eq!(runs.len(), 3);
    assert_eq!(runs[0].line_y, 15.0);
    assert_eq!(runs[2].line_y, 53.5);
    for (actual, original) in runs.iter().zip(&raw) {
        assert_eq!(actual.line_top, original.line_top);
        assert_eq!(actual.line_height, 19.25);
        assert_eq!(actual.line_w, original.line_w);
        assert_eq!(actual.line_i, original.line_i);
        assert!(std::ptr::eq(actual.glyphs, original.glyphs));
    }
    assert_eq!(buffer.layout_runs().next().unwrap().line_y, raw[0].line_y);
}

#[test]
fn wrapped_scrolled_and_rtl_runs_keep_their_own_font_metrics() {
    let mut fonts = font_system();
    let mut buffer = buffer(
        &mut fonts,
        "one two three four five six seven eight nine\nאבגדה",
        21.0,
        Some(45.0),
    );
    for offset in [0.0, 21.0, 64.25] {
        buffer.set_scroll(Scroll {
            line: 0,
            vertical: offset,
            horizontal: 0.0,
        });
        buffer.shape_until_scroll(&mut fonts, false);
        let raw: Vec<_> = buffer.layout_runs().collect();
        let runs: Vec<_> = text_layout_runs(&buffer).collect();
        assert_eq!(runs.len(), raw.len());
        assert!(!runs.is_empty());
        for (actual, original) in runs.iter().zip(raw) {
            assert_eq!(actual.line_top, original.line_top);
            assert_eq!(actual.rtl, original.rtl);
            assert_eq!(actual.line_y, actual.line_top + 16.0);
            assert!(std::ptr::eq(actual.glyphs, original.glyphs));
        }
    }
}

#[test]
fn negative_leading_and_fractional_scroll_do_not_truncate_the_resolved_baseline() {
    let mut fonts = font_system();
    let mut buffer = buffer(&mut fonts, "H\nH", 10.0, None);
    buffer.set_scroll(Scroll {
        line: 0,
        vertical: 0.25,
        horizontal: 0.0,
    });
    buffer.shape_until_scroll(&mut fonts, false);
    let runs: Vec<_> = text_layout_runs(&buffer).collect();
    assert_eq!(runs[0].line_y, 9.75);
    assert_eq!(runs[1].line_y - runs[0].line_y, 10.0);
    let layout = TextContext::new().layout_buffer(&mut fonts, &mut buffer);
    assert_eq!(layout.glyphs()[0].outline_origin.y, 9.75);
    assert_eq!(
        layout.glyphs()[0].cache_key.y_bin,
        cosmic_text::SubpixelBin::Three
    );
}

#[test]
fn fallback_and_combining_offsets_share_raster_and_outline_line_placement() {
    let mut fonts = font_system();
    let mut buffer = buffer(&mut fonts, "Hi😀 e\u{301}\nNext", 21.25, None);
    let origins: Vec<_> = text_layout_runs(&buffer)
        .flat_map(|run| {
            run.glyphs.iter().map(move |glyph| {
                let offset_y = glyph.y - glyph.font_size * glyph.y_offset;
                Point::new(
                    f64::from(glyph.x + glyph.font_size * glyph.x_offset),
                    f64::from(run.line_y + offset_y),
                )
            })
        })
        .collect();
    let layout = TextContext::new().layout_buffer(&mut fonts, &mut buffer);
    assert_eq!(layout.glyphs().len(), origins.len());
    let ids: std::collections::HashSet<_> = layout
        .glyphs()
        .iter()
        .map(|g| g.cache_key.font_id)
        .collect();
    assert!(ids.len() > 1, "the test must exercise font fallback");
    for (glyph, origin) in layout.glyphs().iter().zip(origins) {
        assert_eq!(glyph.outline_origin, origin);
        assert!((f64::from(glyph.y) - origin.y).abs() <= 1.0);
    }
}
