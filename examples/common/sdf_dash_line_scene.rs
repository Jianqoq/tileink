use peniko::{
    Color,
    kurbo::{Point, Rect},
};
use tileink::{FillRule, Radius, Scene, SdfDashLine, SdfLineCap};

pub fn sdf_dash_line_scene() -> (Scene, u32, u32) {
    let width = 640;
    let height = 360;
    let mut scene = Scene::new(width, height);

    scene.push_rect(
        Rect::new(0.0, 0.0, width as f64, height as f64),
        Radius::ZERO,
        Color::from_rgb8(248, 249, 251),
        FillRule::NonZero,
    );

    for y in [72.0, 144.0, 216.0, 288.0] {
        scene.push_dash_line(
            SdfDashLine::new(
                Point::new(56.0, y + 0.5),
                Point::new(584.0, y + 0.5),
                1.0,
                SdfLineCap::Butt,
                2.0,
                8.0,
            ),
            Color::from_rgb8(226, 232, 240),
            FillRule::NonZero,
        );
    }

    scene.push_dash_line(
        SdfDashLine::new(
            Point::new(72.0, 72.5),
            Point::new(568.0, 72.5),
            2.0,
            SdfLineCap::Butt,
            14.0,
            8.0,
        ),
        Color::from_rgb8(220, 64, 72),
        FillRule::NonZero,
    );

    scene.push_dash_line(
        SdfDashLine::new(
            Point::new(72.0, 144.5),
            Point::new(568.0, 144.5),
            10.0,
            SdfLineCap::Square,
            22.0,
            12.0,
        ),
        Color::from_rgb8(37, 99, 235),
        FillRule::NonZero,
    );

    scene.push_dash_line(
        SdfDashLine::with_offset(
            Point::new(72.0, 216.5),
            Point::new(568.0, 216.5),
            14.0,
            SdfLineCap::Round,
            6.0,
            12.0,
            9.0,
        ),
        Color::from_rgb8(22, 163, 74),
        FillRule::NonZero,
    );

    scene.push_dash_line(
        SdfDashLine::with_offset(
            Point::new(88.0, 310.0),
            Point::new(552.0, 258.0),
            12.0,
            SdfLineCap::Round,
            28.0,
            18.0,
            14.0,
        ),
        Color::from_rgb8(217, 119, 6),
        FillRule::NonZero,
    );

    (scene, width, height)
}
