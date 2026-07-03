use peniko::{
    Color,
    kurbo::{Circle, Point, Rect},
};
use tileink::{Canvas, Radius, RectShadowOptions, SdfArc, SdfLine, SdfLineCap, ShadowOptions};

pub fn sdf_shape_shadows_scene() -> (Canvas, u32, u32) {
    let width = 640;
    let height = 360;
    let mut scene = Canvas::new(width, height);

    scene.push_rect(
        Rect::new(0.0, 0.0, width as f64, height as f64),
        Radius::ZERO,
        Color::from_rgb8(248, 250, 252),
    );

    let circle = Circle::new((132.0, 144.0), 64.0);
    scene.push_circle_shadow(
        circle,
        ShadowOptions::new(16.0, 20.0, 20.0, 0.28),
        Color::BLACK,
    );
    scene.push_circle(circle, Color::from_rgb8(59, 130, 246));

    let arc = SdfArc::new(
        Point::new(336.0, 164.0),
        84.0,
        -2.75,
        4.2,
        26.0,
        SdfLineCap::Round,
    );
    scene.push_arc_shadow(
        arc,
        RectShadowOptions::new(16.0, 20.0, 18.0, 0.32),
        Color::BLACK,
    );
    scene.push_sdf_arc(arc, Color::from_rgb8(239, 68, 68));

    let line = SdfLine::new(
        Point::new(116.0, 285.0),
        Point::new(516.0, 237.0),
        10.0,
        SdfLineCap::Round,
    );
    scene.push_line_shadow(
        line,
        RectShadowOptions::new(10.0, 14.0, 14.0, 0.3),
        Color::BLACK,
    );
    scene.push_line(line, Color::from_rgb8(20, 184, 166));

    (scene, width, height)
}
