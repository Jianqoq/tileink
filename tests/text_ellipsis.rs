//! Upstream shaping must preserve soft-break boundaries consumed by native UI text runs.
use cosmic_text::{
    Attrs, Buffer, Ellipsize, EllipsizeHeightLimit, FontSystem, Metrics, Shaping, Wrap,
};
#[test]
fn ellipsis_does_not_reintroduce_the_discarded_wrap_space() {
    let copy = "This long description tests the native second-line ellipsis. Its source must remain complete.";
    for wrap in [Wrap::Word, Wrap::WordOrGlyph] {
        let mut fonts = FontSystem::new();
        let mut buffer = Buffer::new(&mut fonts, Metrics::new(14., 21.));
        buffer.set_text(copy, &Attrs::new(), Shaping::Advanced, None);
        buffer.set_size(Some(160.), None);
        buffer.set_wrap(wrap);
        buffer.shape_until_scroll(&mut fonts, false);
        let expected: Vec<_> = tileink::text_layout_runs(&buffer)
            .take(2)
            .map(|r| r.glyphs.iter().map(|g| g.start).min().unwrap())
            .collect();
        buffer.set_ellipsize(Ellipsize::End(EllipsizeHeightLimit::Lines(2)));
        buffer.shape_until_scroll(&mut fonts, false);
        let actual: Vec<_> = tileink::text_layout_runs(&buffer)
            .map(|r| {
                r.glyphs
                    .iter()
                    .filter(|g| g.start != g.end)
                    .map(|g| g.start)
                    .min()
                    .unwrap()
            })
            .collect();
        assert_eq!(
            actual, expected,
            "limiting displayed lines must not shift a word by a discarded space"
        );
        assert_eq!(buffer.lines[0].text(), copy);
        assert!(
            tileink::text_layout_runs(&buffer).any(|r| r.glyphs.iter().any(|g| g.start == g.end))
        );
    }
}
