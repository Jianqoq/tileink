use super::*;
use cosmic_text::{Buffer, Metrics, Scroll, Shaping};

fn buffer(fonts: &mut FontSystem, value: &str, width: f32) -> Buffer {
    let mut buffer = Buffer::new(fonts, Metrics::new(14.0, 19.25));
    buffer.set_size(Some(width), None);
    buffer.set_text(value, &Attrs::new(), Shaping::Advanced, Some(Align::Center));
    buffer.shape_until_scroll(fonts, false);
    buffer
}

#[test]
fn aligned_text_uses_the_closest_physical_edge_in_both_directions() {
    let mut fonts = font_system();
    for (value, left, right) in [("abc", 0, 3), ("אבג", 6, 0)] {
        let buffer = buffer(&mut fonts, value, 200.0);
        assert_eq!(text_hit(&buffer, 1.0, 10.0).unwrap().index, left);
        assert_eq!(text_hit(&buffer, 199.0, 10.0).unwrap().index, right);
        for run in text_layout_runs(&buffer) {
            for glyph in run.glyphs.iter().filter(|g| g.w > 1.0) {
                let left = text_hit(&buffer, glyph.x + 0.01, 10.0).unwrap();
                let right = text_hit(&buffer, glyph.x + glyph.w - 0.01, 10.0).unwrap();
                assert_eq!(
                    left.index,
                    if glyph.level.is_rtl() {
                        glyph.end
                    } else {
                        glyph.start
                    }
                );
                assert_eq!(
                    right.index,
                    if glyph.level.is_rtl() {
                        glyph.start
                    } else {
                        glyph.end
                    }
                );
            }
        }
    }
}

#[test]
fn partially_visible_empty_paragraphs_remain_hittable() {
    let mut fonts = font_system();
    let mut buffer = buffer(&mut fonts, "\nH", 200.0);
    buffer.set_scroll(Scroll {
        line: 0,
        vertical: 10.25,
        horizontal: 0.0,
    });
    buffer.shape_until_scroll(&mut fonts, false);
    let runs: Vec<_> = text_layout_runs(&buffer).collect();
    assert_eq!(runs[0].line_i, 0);
    assert!(runs[0].glyphs.is_empty());
    assert!(runs[0].line_top < 0.0);
    assert_eq!(text_hit(&buffer, 5.0, 1.0).unwrap().line, 0);
    assert_eq!(text_hit(&buffer, 5.0, 10.0).unwrap().line, 1);
}

#[test]
fn cluster_hits_preserve_grapheme_and_utf8_boundaries() {
    let mut fonts = font_system();
    let buffer = buffer(&mut fonts, "office e\u{301}", 200.0);
    for run in text_layout_runs(&buffer) {
        for glyph in run.glyphs {
            for fraction in [0.1, 0.4, 0.6, 0.9] {
                let cursor = text_hit(&buffer, glyph.x + glyph.w * fraction, 10.0).unwrap();
                assert!(run.text.is_char_boundary(cursor.index));
                assert_ne!(
                    cursor.index,
                    "office e".len(),
                    "a combining cluster must not split"
                );
            }
        }
    }
    for (x, y) in [(f32::NAN, 0.0), (0.0, f32::INFINITY)] {
        assert!(text_hit(&buffer, x, y).is_none());
    }
}
