use super::*;
use cosmic_text::{Buffer, LayoutLine, Metrics, Scroll, Shaping};

fn source(fonts: &mut FontSystem, text: &str) -> Buffer {
    let mut buffer = Buffer::new(fonts, Metrics::new(14.0, 19.25));
    buffer.set_text(
        text,
        &Attrs::new()
            .metadata(17)
            .color(cosmic_text::Color::rgba(30, 60, 90, 180)),
        Shaping::Advanced,
        None,
    );
    buffer.set_size(Some(220.0), None);
    buffer.shape_until_scroll(fonts, false);
    buffer.set_redraw(false);
    buffer
}

#[test]
fn supplied_layout_drives_borrowed_paint_and_hit_without_mutating_source() {
    let mut fonts = font_system();
    let buffer = source(&mut fonts, "abc");
    let original_x = buffer.lines[0].layout_opt().unwrap()[0].glyphs[0].x;
    let original_shape = std::ptr::from_ref(buffer.lines[0].shape_opt().unwrap());
    let mut lines = buffer.lines[0].layout_opt().unwrap().clone();
    for glyph in &mut lines[0].glyphs {
        glyph.x += 40.25;
    }
    let view = || text_layout_runs_from(&buffer, |line| (line == 0).then_some(lines.as_slice()));
    let run = view().next().unwrap();
    assert!(std::ptr::eq(run.glyphs, lines[0].glyphs.as_slice()));
    let glyph = &run.glyphs[1];
    let hit_x = glyph.x + glyph.w * 0.25;
    let hit_y = run.line_top + run.line_height * 0.5;
    assert_eq!(
        text_hit_runs(view(), hit_x, hit_y).unwrap().index,
        glyph.start
    );
    let mut calls = 0;
    let prepared = TextContext::new().layout_runs_filtered(&mut fonts, view(), |_, glyph| {
        calls += 1;
        glyph.start != 0
    });
    assert_eq!(calls, lines[0].glyphs.len());
    assert!(!prepared.is_empty());
    assert!(
        prepared
            .iter()
            .all(|run| run.line == 0 && run.metadata == 17 && run.color == Some([30, 60, 90, 180]))
    );
    let actual: Vec<_> = prepared
        .iter()
        .flat_map(|run| run.layout.glyphs())
        .collect();
    let expected: Vec<_> = run.glyphs.iter().filter(|g| g.start != 0).collect();
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert_eq!(
            actual.outline_origin.x,
            f64::from(expected.x + expected.font_size * expected.x_offset)
        );
        assert_eq!(
            actual.outline_origin.y,
            f64::from(run.line_y + (expected.y - expected.font_size * expected.y_offset))
        );
    }
    assert_eq!(
        buffer.lines[0].layout_opt().unwrap()[0].glyphs[0].x,
        original_x
    );
    assert_eq!(
        std::ptr::from_ref(buffer.lines[0].shape_opt().unwrap()),
        original_shape
    );
    assert_eq!(buffer.lines[0].text(), "abc");
    assert!(!buffer.redraw());
}

#[test]
fn omitted_paragraphs_keep_logical_indices_and_empty_line_boxes() {
    let mut fonts = font_system();
    let buffer = source(&mut fonts, "hidden\n\nvisible");
    let empty: &[LayoutLine] = &[];
    let view = || text_layout_runs_from(&buffer, |line| (line == 0).then_some(empty));
    let runs: Vec<_> = view().collect();
    assert_eq!(runs.len(), 2);
    assert_eq!((runs[0].line_i, runs[0].line_top), (1, 0.0));
    assert!(runs[0].glyphs.is_empty());
    assert_eq!((runs[1].line_i, runs[1].line_top), (2, 19.25));
    let painted = TextContext::new().layout_runs_filtered(&mut fonts, view(), |_, _| true);
    assert!(!painted.is_empty());
    assert!(painted.iter().all(|run| run.line == 1));
    assert_eq!(text_hit_runs(view(), 0.0, 1.0).unwrap().line, 1);
    assert_eq!(buffer.lines.len(), 3);
    assert!(!buffer.redraw());
}

#[test]
fn default_layout_and_supplied_fallback_have_identical_scrolled_geometry() {
    let mut fonts = font_system();
    let mut buffer = source(&mut fonts, "one two three four five six\n\nאבג");
    buffer.set_wrap(Wrap::Word);
    buffer.set_size(Some(45.0), Some(70.0));
    buffer.set_scroll(Scroll::new(0, 19.5, 0.0));
    buffer.shape_until_scroll(&mut fonts, false);
    let ordinary: Vec<_> = text_layout_runs(&buffer).collect();
    let supplied: Vec<_> = text_layout_runs_from(&buffer, |_| None).collect();
    assert!(!ordinary.is_empty());
    assert_eq!(ordinary.len(), supplied.len());
    for (a, b) in ordinary.iter().zip(&supplied) {
        assert_eq!(
            (
                a.line_i,
                a.rtl,
                a.line_y,
                a.line_top,
                a.line_height,
                a.line_w
            ),
            (
                b.line_i,
                b.rtl,
                b.line_y,
                b.line_top,
                b.line_height,
                b.line_w
            )
        );
        assert!(std::ptr::eq(a.glyphs, b.glyphs));
    }
    for (x, y) in [(0.0, 0.0), (8.5, 20.0), (100.0, 50.0), (-20.0, -10.0)] {
        assert_eq!(
            text_hit(&buffer, x, y),
            text_hit_runs(text_layout_runs_from(&buffer, |_| None), x, y)
        );
    }
    assert!(text_hit_runs(supplied, f32::NAN, 0.0).is_none());
    assert!(text_hit_runs(std::iter::empty(), 0.0, 0.0).is_none());
}

#[test]
fn reflowed_visual_lines_keep_source_cursors_and_the_shared_fractional_baseline() {
    let mut fonts = font_system();
    let mut buffer = source(&mut fonts, "abcd");
    let original = &buffer.lines[0].layout_opt().unwrap()[0];
    let mut first = original.clone();
    let mut second = original.clone();
    first.glyphs = original.glyphs[..2].to_vec();
    second.glyphs = original.glyphs[2..].to_vec();
    first.line_height_opt = Some(25.5);
    let source_x = second.glyphs[0].x;
    for glyph in &mut second.glyphs {
        glyph.x -= source_x;
    }
    let lines = [first, second];
    let view = || text_layout_runs_from(&buffer, |_| Some(&lines));
    let runs: Vec<_> = view().collect();
    assert_eq!(runs.len(), 2);
    assert_eq!((runs[0].line_i, runs[1].line_i), (0, 0));
    assert_eq!((runs[0].line_top, runs[0].line_height), (0.0, 25.5));
    assert_eq!(runs[1].line_top, 25.5);
    assert_eq!(runs[1].line_y, runs[1].line_top + 15.0);
    let g = &runs[1].glyphs[0];
    assert_eq!(
        text_hit_runs(view(), g.x + g.w * 0.25, 26.0).unwrap().index,
        g.start
    );
    let prepared = TextContext::new().layout_runs_filtered(&mut fonts, view(), |_, _| true);
    assert!(prepared.iter().any(|run| run.line == 1));
    assert_eq!(buffer.lines[0].text(), "abcd");
    assert_eq!(buffer.lines[0].layout_opt().unwrap().len(), 1);
    assert!(!buffer.redraw());

    buffer.set_size(Some(220.0), Some(20.0));
    buffer.shape_until_scroll(&mut fonts, false);
    assert_eq!(text_layout_runs_from(&buffer, |_| Some(&lines)).count(), 1);
}
