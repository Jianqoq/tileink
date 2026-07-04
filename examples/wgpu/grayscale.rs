use crate::common;

use peniko::{
    Color,
    kurbo::{Circle, Rect},
};
use tileink::{Canvas, Filter, Radius};

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut scene = Canvas::new(640, 360);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 640.0, 360.0),
        Radius::ZERO,
        Color::from_rgb8(246, 248, 251),
    );
    scene.push_filter_layer(Filter::Grayscale(1.0), common::canvas_region(640, 360));
    common::fill_rect(
        &mut scene,
        Rect::new(120.0, 80.0, 330.0, 260.0),
        Radius::ZERO,
        Color::from_rgb8(220, 38, 38),
    );
    common::fill_circle(
        &mut scene,
        Circle::new((360.0, 180.0), 88.0),
        Color::from_rgb8(37, 99, 235),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(360.0, 110.0, 530.0, 250.0),
        Radius::ZERO,
        Color::from_rgb8(22, 163, 74),
    );
    scene.pop_layer();
    common::render_to_png_wgpu("grayscale", &scene, 640, 360, Color::WHITE)
}
