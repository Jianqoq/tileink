use peniko::{
    Color,
    kurbo::{Affine, Line, Rect, Shape, Stroke},
};
use tileink::{FillRule, Radius, Canvas};

pub const TOLERANCE: f64 = 0.25;
pub const CHART_WIDTH: f64 = 652.0;
pub const CHART_HEIGHT: f64 = 515.0;
pub const CHART_X: f64 = 387.0;
pub const CHART_Y: f64 = 104.0;
pub const CLOSE_Y: f64 = 216.5;

pub fn path_current_close_dash_scene() -> (Canvas, u32, u32) {
    let width = (CHART_X + CHART_WIDTH + 32.0).ceil() as u32;
    let height = (CHART_Y + CHART_HEIGHT + 32.0).ceil() as u32;
    let mut scene = Canvas::new(width, height);

    scene.push_rect(
        Rect::new(0.0, 0.0, width as f64, height as f64),
        Radius::ZERO,
        Color::from_rgb8(248, 249, 251),
    );
    scene.push_rect(
        Rect::new(
            CHART_X,
            CHART_Y,
            CHART_X + CHART_WIDTH,
            CHART_Y + CHART_HEIGHT,
        ),
        Radius::ZERO,
        Color::from_rgb8(244, 245, 247),
    );

    let chart_transform = Affine::translate((CHART_X, CHART_Y));
    let grid_brush = Color::from_rgb8(224, 229, 238);
    for x in [0.0, 163.0, 326.0, 489.0, CHART_WIDTH] {
        push_path_stroke(
            &mut scene,
            Line::new((x, 0.0), (x, CHART_HEIGHT)),
            Stroke::new(1.0),
            chart_transform,
            grid_brush,
        );
    }
    for y in [0.0, 103.0, 206.0, 309.0, 412.0, CHART_HEIGHT] {
        push_path_stroke(
            &mut scene,
            Line::new((0.0, y + 0.5), (CHART_WIDTH, y + 0.5)),
            Stroke::new(1.0),
            chart_transform,
            grid_brush,
        );
    }

    push_path_stroke(
        &mut scene,
        Line::new((0.0, CLOSE_Y), (CHART_WIDTH, CLOSE_Y)),
        Stroke::new(1.0).with_dashes(0.0, [1.0_f64, 2.0_f64]),
        chart_transform,
        Color::BLACK,
    );

    (scene, width, height)
}

fn push_path_stroke(
    scene: &mut Canvas,
    line: Line,
    stroke: Stroke,
    transform: Affine,
    color: Color,
) {
    scene.push_stroke(
        line.to_path(TOLERANCE),
        stroke,
        color,
        transform,
        FillRule::NonZero,
        TOLERANCE,
    );
}
