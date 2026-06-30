#[path = "../common/mod.rs"]
mod common;

use peniko::{
    Color,
    kurbo::{Affine, BezPath, Rect, Stroke},
};
use tileink::{FillRule, Radius, Scene};

fn nested_rect_path(offset_x: f64) -> BezPath {
    let mut path = BezPath::new();
    path.move_to((offset_x + 30.0, 60.0));
    path.line_to((offset_x + 190.0, 60.0));
    path.line_to((offset_x + 190.0, 220.0));
    path.line_to((offset_x + 30.0, 220.0));
    path.close_path();
    path.move_to((offset_x + 78.0, 108.0));
    path.line_to((offset_x + 142.0, 108.0));
    path.line_to((offset_x + 142.0, 172.0));
    path.line_to((offset_x + 78.0, 172.0));
    path.close_path();
    path
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut scene = Scene::new(520, 280);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 520.0, 280.0),
        Radius::ZERO,
        Color::from_rgb8(248, 249, 251),
    );
    scene.push_path(
        nested_rect_path(30.0),
        Color::from_rgb8(37, 99, 235),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    common::stroke_rect(
        &mut scene,
        Rect::new(60.5, 60.5, 220.5, 220.5),
        Radius::ZERO,
        Stroke::new(3.0),
        Color::from_rgb8(15, 23, 42),
    );
    scene.push_path(
        nested_rect_path(270.0),
        Color::from_rgb8(220, 38, 38),
        Affine::IDENTITY,
        FillRule::EvenOdd,
        0.1,
    );
    common::stroke_rect(
        &mut scene,
        Rect::new(300.5, 60.5, 460.5, 220.5),
        Radius::ZERO,
        Stroke::new(3.0),
        Color::from_rgb8(15, 23, 42),
    );
    common::render_to_png("even_odd", &scene, 520, 280, Color::WHITE)
}
