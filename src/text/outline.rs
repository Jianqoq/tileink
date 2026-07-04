use cosmic_text::{CacheKey, FontSystem, SubpixelBin};
use peniko::kurbo::{BezPath, Point};
use swash::{
    scale::ScaleContext,
    zeno::{Command as SwashPathCommand, PathData, Vector},
};

use super::scaler::{fake_italic_transform, with_glyph_scaler};

pub(super) fn outline_cache_key(cache_key: CacheKey) -> CacheKey {
    CacheKey {
        x_bin: SubpixelBin::Zero,
        y_bin: SubpixelBin::Zero,
        ..cache_key
    }
}

pub(super) fn outline_glyph_path(
    font_system: &mut FontSystem,
    context: &mut ScaleContext,
    cache_key: CacheKey,
) -> Option<BezPath> {
    with_glyph_scaler(font_system, context, cache_key, |scaler| {
        let mut outline = scaler
            .scale_outline(cache_key.glyph_id)
            .or_else(|| scaler.scale_color_outline(cache_key.glyph_id))?;
        if let Some(transform) = fake_italic_transform(cache_key) {
            outline.transform(&transform);
        }

        let mut path = BezPath::new();
        for command in outline.path().commands() {
            push_swash_command(&mut path, command, Point::ORIGIN);
        }
        Some(path)
    })
    .flatten()
}

pub(super) fn append_outline_path(path: &mut BezPath, outline: &BezPath, origin: Point) {
    for element in outline.elements() {
        match *element {
            peniko::kurbo::PathEl::MoveTo(p) => path.move_to((origin.x + p.x, origin.y + p.y)),
            peniko::kurbo::PathEl::LineTo(p) => path.line_to((origin.x + p.x, origin.y + p.y)),
            peniko::kurbo::PathEl::QuadTo(p0, p1) => path.quad_to(
                (origin.x + p0.x, origin.y + p0.y),
                (origin.x + p1.x, origin.y + p1.y),
            ),
            peniko::kurbo::PathEl::CurveTo(p0, p1, p2) => path.curve_to(
                (origin.x + p0.x, origin.y + p0.y),
                (origin.x + p1.x, origin.y + p1.y),
                (origin.x + p2.x, origin.y + p2.y),
            ),
            peniko::kurbo::PathEl::ClosePath => path.close_path(),
        }
    }
}

fn push_swash_command(path: &mut BezPath, command: SwashPathCommand, origin: Point) {
    let p = |p: Vector| (origin.x + p.x as f64, origin.y - p.y as f64);
    match command {
        SwashPathCommand::MoveTo(to) => path.move_to(p(to)),
        SwashPathCommand::LineTo(to) => path.line_to(p(to)),
        SwashPathCommand::QuadTo(control, to) => path.quad_to(p(control), p(to)),
        SwashPathCommand::CurveTo(control0, control1, to) => {
            path.curve_to(p(control0), p(control1), p(to));
        }
        SwashPathCommand::Close => path.close_path(),
    }
}
