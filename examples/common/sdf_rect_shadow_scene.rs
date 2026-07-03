use peniko::{
    Color,
    kurbo::{Point, Rect},
};
use tileink::{Canvas, Radius, RectShadowOptions, SdfLine, SdfLineCap};

pub fn sdf_rect_shadow_scene() -> (Canvas, u32, u32) {
    let width = 560;
    let height = 360;
    let mut scene = Canvas::new(width, height);

    scene.push_rect(
        Rect::new(0.0, 0.0, width as f64, height as f64),
        tileink::Radius::ZERO,
        Color::from_rgb8(246, 248, 252),
    );

    for x in (48..=512).step_by(32) {
        scene.push_line(
            SdfLine::new(
                Point::new(x as f64 + 0.5, 44.0),
                Point::new(x as f64 + 0.5, 316.0),
                1.0,
                SdfLineCap::Butt,
            ),
            Color::from_rgba8(196, 204, 216, 110),
        );
    }
    for y in (48..=304).step_by(32) {
        scene.push_line(
            SdfLine::new(
                Point::new(48.0, y as f64 + 0.5),
                Point::new(512.0, y as f64 + 0.5),
                1.0,
                SdfLineCap::Butt,
            ),
            Color::from_rgba8(196, 204, 216, 110),
        );
    }

    let rect = Rect::new(136.0, 84.0, 424.0, 268.0);
    let radius = Radius {
        top_left: 56.0,
        top_right: 10.0,
        bottom_left: 24.0,
        bottom_right: 76.0,
    };

    scene.push_rect_shadow(
        rect,
        radius,
        RectShadowOptions::new(18.0, 24.0, 18.0, 0.34),
        Color::BLACK,
    );
    scene.push_rect(rect, radius, Color::from_rgb8(42, 117, 216));
    scene.push_rect_stroke(
        rect,
        radius,
        peniko::kurbo::Stroke::new(3.0),
        Color::from_rgba8(255, 255, 255, 210),
    );

    (scene, width, height)
}
