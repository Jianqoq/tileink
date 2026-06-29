use peniko::{
    Color,
    kurbo::{Point, Rect},
};
use tileink::{FillRule, Scene, SdfLine, SdfLineCap};

pub fn sdf_line_scene() -> (Scene, u32, u32) {
    let width = 520;
    let height = 320;
    let mut scene = Scene::new(width, height);

    scene.push_rect(
        Rect::new(0.0, 0.0, width as f64, height as f64),
        Color::from_rgb8(248, 249, 251),
        FillRule::NonZero,
    );

    for y in [72.0, 144.0, 216.0] {
        scene.push_line(
            SdfLine::new(
                Point::new(52.0, y + 0.5),
                Point::new(468.0, y + 0.5),
                1.0,
                SdfLineCap::Butt,
            ),
            Color::from_rgb8(222, 228, 236),
            FillRule::NonZero,
        );
    }

    let red = Color::from_rgb8(220, 64, 72);
    let blue = Color::from_rgb8(37, 99, 235);
    let green = Color::from_rgb8(22, 163, 74);
    let amber = Color::from_rgb8(217, 119, 6);
    let violet = Color::from_rgb8(124, 58, 237);

    scene.push_line(
        SdfLine::new(
            Point::new(72.0, 72.5),
            Point::new(220.0, 72.5),
            1.0,
            SdfLineCap::Butt,
        ),
        red,
        FillRule::NonZero,
    );
    scene.push_line(
        SdfLine::new(
            Point::new(300.0, 72.5),
            Point::new(448.0, 72.5),
            8.0,
            SdfLineCap::Butt,
        ),
        blue,
        FillRule::NonZero,
    );
    scene.push_line(
        SdfLine::new(
            Point::new(72.0, 144.5),
            Point::new(220.0, 144.5),
            12.0,
            SdfLineCap::Square,
        ),
        green,
        FillRule::NonZero,
    );
    scene.push_line(
        SdfLine::new(
            Point::new(300.0, 144.5),
            Point::new(448.0, 144.5),
            12.0,
            SdfLineCap::Round,
        ),
        amber,
        FillRule::NonZero,
    );
    scene.push_line(
        SdfLine::new(
            Point::new(86.0, 252.0),
            Point::new(438.0, 196.0),
            10.0,
            SdfLineCap::Round,
        ),
        violet,
        FillRule::NonZero,
    );

    (scene, width, height)
}
