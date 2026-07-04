use crate::common;

use peniko::{
    Color,
    kurbo::{Affine, Circle, Stroke},
};
use tileink::{Canvas, FillRule};

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let width = 220;
    let height = 180;
    let mut scene = Canvas::new(width, height);

    scene.push_rect(
        peniko::kurbo::Rect::new(0.0, 0.0, width as f64, height as f64),
        tileink::Radius::ZERO,
        Color::from_rgb8(250, 250, 248),
    );
    scene.push_stroke(
        Circle::new((110.0, 90.0), 54.0),
        Stroke::new(16.0),
        Color::from_rgb8(222, 96, 72),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );

    common::render_to_png("stroke_circle", &scene, width, height, Color::WHITE)
}
