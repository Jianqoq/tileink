use crate::common;

use peniko::{
    Color,
    kurbo::{Circle, Rect},
};
use tileink::{Canvas, Filter, Radius};

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut scene = Canvas::new(640, 360, 1.0);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 640.0, 360.0),
        Radius::ZERO,
        Color::from_rgb8(246, 248, 251),
    );
    scene.push_filter_layer(Filter::Sepia(1.0), common::canvas_region(640, 360));
    common::fill_rect(
        &mut scene,
        Rect::new(120.0, 76.0, 520.0, 284.0),
        Radius::ZERO,
        Color::from_rgb8(37, 99, 235),
    );
    common::fill_circle(
        &mut scene,
        Circle::new((320.0, 180.0), 76.0),
        Color::from_rgb8(220, 38, 38),
    );
    scene.pop_layer();
    common::render_to_png("sepia", &scene, 640, 360, Color::WHITE)
}
