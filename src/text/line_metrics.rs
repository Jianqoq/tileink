//! Shared UI line placement over the caller's retained, already-shaped buffer.
use cosmic_text::{Buffer, LayoutRun};

/// Iterates visible UI runs with rounded font extents and floored upper half-leading.
///
/// Glyph advances, line tops and fractional line heights stay exact. Empty paragraphs retain
/// their line boxes. Call after shaping; no glyphs are cloned or reshaped here. Positioned
/// canvas/SVG glyphs retain their explicitly authored coordinates.
pub fn text_layout_runs(buffer: &Buffer) -> impl Iterator<Item = LayoutRun<'_>> {
    let scroll = buffer.scroll();
    let mut paragraph = scroll.line;
    let mut visual_line = 0;
    let mut top = -scroll.vertical;
    let height = buffer.size().1;
    let default_height = buffer.metrics().line_height;
    std::iter::from_fn(move || {
        loop {
            let source = buffer.lines.get(paragraph)?;
            let shape = source.shape_opt()?;
            let lines = source.layout_opt()?;
            let current_line = visual_line;
            let Some(line) = lines.get(current_line) else {
                paragraph += 1;
                visual_line = 0;
                continue;
            };
            visual_line += 1;
            let line_height = line.line_height_opt.unwrap_or(default_height);
            let line_top = top;
            top += line_height;
            let ascent = line.max_ascent.round();
            let descent = line.max_descent.round();
            let baseline = line_top + ascent + ((line_height - ascent - descent) * 0.5).floor();
            // Visibility uses the same resolved baseline as painting, including overflow from short
            // line heights. The line box also counts: an empty, partly scrolled paragraph has a caret.
            if height.is_some_and(|height| line_top.min(baseline - ascent) > height) {
                paragraph = buffer.lines.len();
                return None;
            }
            if top.max(baseline + descent) <= 0.0 {
                continue;
            }
            return Some(LayoutRun {
                line_i: paragraph,
                text: source.text(),
                rtl: shape.rtl,
                glyphs: &line.glyphs,
                decorations: &line.decorations,
                line_y: baseline,
                line_top,
                line_height,
                line_w: line.w,
            });
        }
    })
}
