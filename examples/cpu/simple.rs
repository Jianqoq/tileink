use crate::common;

use peniko::{
    Color,
    kurbo::{Affine, Circle, Rect, Shape, Stroke},
};
use tileink::{Canvas, FillRule};

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let width = 360;
    let height = 260;
    let mut scene = Canvas::new(width, height);

    scene.push_rect(
        Rect::new(0.0, 0.0, width as f64, height as f64),
        tileink::Radius::ZERO,
        Color::from_rgb8(248, 249, 251),
    );
    scene.push_rect(
        Rect::new(42.0, 38.0, 178.0, 128.0),
        tileink::Radius::ZERO,
        Color::from_rgba8(37, 143, 93, 230),
    );
    scene.push_path(
        Circle::new((242.0, 86.0), 56.0).to_path(0.1),
        Color::from_rgba8(45, 111, 211, 220),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );

    scene.push_stroke(
        Rect::new(72.0, 156.0, 178.0, 218.0),
        Stroke::new(12.0),
        Color::from_rgb8(230, 89, 80),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    scene.push_stroke(
        Circle::new((260.0, 184.0), 40.0),
        Stroke::new(10.0),
        Color::from_rgb8(222, 178, 106),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );

    common::render_to_png(
        "simple",
        &scene,
        width,
        height,
        Color::from_rgb8(255, 255, 255),
    )
}
