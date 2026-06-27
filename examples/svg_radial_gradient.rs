mod common;

use peniko::{
    Color, Gradient,
    color::{AlphaColor, palette::css},
    kurbo::{Affine, BezPath, Rect, Stroke},
};
use tileink::{Brush, FillRule, Radius, Scene};

fn curve_path() -> BezPath {
    let mut path = BezPath::new();
    path.move_to((24.0, 118.0));
    path.curve_to((72.0, 82.0), (120.0, 154.0), (216.0, 56.0));
    path
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut scene = Scene::new(480, 280);
    let scale = Affine::scale(2.0);

    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 480.0, 280.0),
        Radius::all(0.0),
        Color::from_rgb8(17, 24, 39),
    );

    let ellipse =
        Gradient::new_two_point_radial((80.0, 70.0), 0.0, (80.0, 70.0), 64.0).with_stops([
            AlphaColor::from_rgb8(255, 255, 255),
            AlphaColor::from_rgb8(34, 197, 94),
            AlphaColor::from_rgb8(15, 23, 42),
        ]);
    scene.push_path(
        common::rect_path(Rect::new(24.0, 24.0, 136.0, 116.0), Radius::all(12.0)),
        &ellipse,
        scale,
        FillRule::NonZero,
        0.1,
    );

    let stroke = Gradient::new_linear((20.0, 0.0), (220.0, 0.0)).with_stops([
        css::RED,
        css::YELLOW,
        css::BLUE,
    ]);
    scene.push_stroke(
        curve_path(),
        Stroke::new(12.0),
        Brush::from_gradient(&stroke),
        scale,
        FillRule::NonZero,
        0.1,
    );

    common::render_to_png("svg_radial_gradient", &scene, 480, 280, Color::TRANSPARENT)
}
