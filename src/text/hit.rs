//! Hit testing over the same visible line boxes used by text painting and editing geometry.
use cosmic_text::{Affinity, Buffer, Cursor, LayoutGlyph, LayoutRun};
use unicode_segmentation::UnicodeSegmentation;

use super::text_layout_runs;

/// Finds a shaped cursor using Tileink's shared UI line geometry.
///
/// Nonfinite pointer coordinates are rejected. Ligature clusters are divided by grapheme, and
/// physical edge positions honor each glyph's direction, including aligned and mixed-BiDi text.
#[must_use]
pub fn text_hit(buffer: &Buffer, x: f32, y: f32) -> Option<Cursor> {
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    let mut last = None;
    for run in text_layout_runs(buffer) {
        if y < run.line_top {
            return Some(Cursor::new(run.line_i, 0));
        }
        if y < run.line_top + run.line_height {
            return Some(hit_run(&run, x));
        }
        last = Some(Cursor::new_with_affinity(
            run.line_i,
            run.text.len(),
            Affinity::Before,
        ));
    }
    last
}

fn edge(run: &LayoutRun<'_>, glyph: &LayoutGlyph, right: bool) -> Cursor {
    let (index, affinity) = if right == glyph.level.is_rtl() {
        (glyph.start, Affinity::After)
    } else {
        (glyph.end, Affinity::Before)
    };
    Cursor::new_with_affinity(run.line_i, index, affinity)
}

fn hit_run(run: &LayoutRun<'_>, x: f32) -> Cursor {
    let mut nearest = (f32::INFINITY, Cursor::new(run.line_i, 0));
    for glyph in run.glyphs {
        let right = glyph.x + glyph.w;
        if x >= glyph.x && x <= right {
            let parts = run.text[glyph.start..glyph.end].grapheme_indices(true);
            let count = parts.clone().count();
            if count == 0 {
                return edge(run, glyph, x >= glyph.x.midpoint(right));
            }
            let advance = glyph.w / count as f32;
            // Graphemes are logical order; measure from the right for an RTL ligature.
            let progress = if glyph.level.is_rtl() {
                right - x
            } else {
                x - glyph.x
            };
            let mut start = 0.0;
            for (offset, part) in parts {
                if progress <= start + advance {
                    let (index, affinity) = if progress >= start + advance * 0.5 {
                        (glyph.start + offset + part.len(), Affinity::Before)
                    } else {
                        (glyph.start + offset, Affinity::After)
                    };
                    return Cursor::new_with_affinity(run.line_i, index, affinity);
                }
                start += advance;
            }
            // Accumulated division can leave the final logical subinterval just short.
            return edge(run, glyph, !glyph.level.is_rtl());
        }
        for (coordinate, is_right) in [(glyph.x, false), (right, true)] {
            let distance = (x - coordinate).abs();
            if distance < nearest.0 {
                nearest = (distance, edge(run, glyph, is_right));
            }
        }
    }
    nearest.1
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rtl_ligature_hits_follow_logical_graphemes_from_the_right() {
        let mut fonts = crate::text::tests::font_system();
        let mut buffer = Buffer::new(&mut fonts, cosmic_text::Metrics::new(14.0, 20.0));
        buffer.set_text(
            "لا",
            &cosmic_text::Attrs::new(),
            cosmic_text::Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut fonts, false);
        let mut run = buffer.layout_runs().next().unwrap();
        // One valid multi-grapheme RTL glyph. Test its cursor mapping independently of a
        // particular font's GSUB repertoire; real shaped buffers are covered by the hit suite.
        let mut glyph = run.glyphs[0].clone();
        glyph.start = 0;
        glyph.end = run.text.len();
        glyph.x = 0.0;
        glyph.w = 20.0;
        assert!(glyph.level.is_rtl());
        let glyphs = [glyph];
        run.glyphs = &glyphs;
        assert_eq!(hit_run(&run, 0.01).index, run.text.len());
        assert_eq!(hit_run(&run, 19.99).index, 0);
        assert_eq!(hit_run(&run, 10.0).index, 2);
    }
}
