use peniko::{
    Color,
    kurbo::{Circle, Point, Rect, Stroke},
};
use tileink::{Canvas, Radius, SdfLine, SdfLineCap};

pub fn sdf_clip_scene() -> (Canvas, u32, u32) {
    let width = 640;
    let height = 360;
    let mut scene = Canvas::new(width, height);

    scene.push_rect(
        Rect::new(0.0, 0.0, width as f64, height as f64),
        Radius::ZERO,
        Color::from_rgb8(248, 250, 252),
    );

    for x in (40..=600).step_by(40) {
        scene.push_line(
            SdfLine::new(
                Point::new(x as f64 + 0.5, 36.0),
                Point::new(x as f64 + 0.5, 324.0),
                1.0,
                SdfLineCap::Butt,
            ),
            Color::from_rgba8(203, 213, 225, 90),
        );
    }
    for y in (40..=320).step_by(40) {
        scene.push_line(
            SdfLine::new(
                Point::new(40.0, y as f64 + 0.5),
                Point::new(600.0, y as f64 + 0.5),
                1.0,
                SdfLineCap::Butt,
            ),
            Color::from_rgba8(203, 213, 225, 90),
        );
    }

    let outer = Rect::new(72.0, 48.0, 568.0, 312.0);
    let outer_radius = Radius {
        top_left: 64.0,
        top_right: 18.0,
        bottom_left: 36.0,
        bottom_right: 96.0,
    };

    scene.push_rect_stroke(
        outer,
        outer_radius,
        Stroke::new(4.0),
        Color::from_rgb8(15, 23, 42),
    );

    scene.push_clip_sdf_rect_layer(outer, outer_radius);
    scene.push_rect(
        Rect::new(36.0, 28.0, 610.0, 132.0),
        Radius::ZERO,
        Color::from_rgb8(14, 165, 233),
    );
    scene.push_rect(
        Rect::new(36.0, 132.0, 610.0, 224.0),
        Radius::ZERO,
        Color::from_rgb8(34, 197, 94),
    );
    scene.push_rect(
        Rect::new(36.0, 224.0, 610.0, 336.0),
        Radius::ZERO,
        Color::from_rgb8(249, 115, 22),
    );
    scene.push_circle(
        Circle::new((128.0, 88.0), 74.0),
        Color::from_rgba8(255, 255, 255, 170),
    );
    scene.push_circle(
        Circle::new((532.0, 268.0), 96.0),
        Color::from_rgba8(30, 41, 59, 115),
    );
    scene.push_line(
        SdfLine::new(
            Point::new(74.0, 292.0),
            Point::new(570.0, 78.0),
            14.0,
            SdfLineCap::Round,
        ),
        Color::from_rgba8(255, 255, 255, 190),
    );

    let inner = Rect::new(210.0, 96.0, 430.0, 264.0);
    let inner_radius = Radius {
        top_left: 44.0,
        top_right: 44.0,
        bottom_left: 12.0,
        bottom_right: 72.0,
    };
    scene.push_clip_sdf_rect_layer(inner, inner_radius);
    scene.push_rect(
        Rect::new(168.0, 76.0, 472.0, 284.0),
        Radius::ZERO,
        Color::from_rgba8(15, 23, 42, 190),
    );
    scene.push_circle(
        Circle::new((320.0, 180.0), 86.0),
        Color::from_rgba8(255, 255, 255, 190),
    );
    scene.pop_layer();
    scene.pop_layer();

    scene.push_rect_stroke(
        inner,
        inner_radius,
        Stroke::new(3.0),
        Color::from_rgba8(255, 255, 255, 235),
    );

    (scene, width, height)
}
