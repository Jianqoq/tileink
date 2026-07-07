use crate::common;

use peniko::{Color, kurbo::Rect};
use tileink::{Canvas, Filter, Radius};

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut scene = Canvas::new(640, 360, 1.0);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 640.0, 360.0),
        Radius::ZERO,
        Color::from_rgb8(246, 248, 251),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(70.0, 80.0, 280.0, 280.0),
        Radius::ZERO,
        Color::from_rgb8(30, 64, 175),
    );
    scene.push_filter_layer(Filter::Brightness(1.65), common::canvas_region(640, 360));
    common::fill_rect(
        &mut scene,
        Rect::new(360.0, 80.0, 570.0, 280.0),
        Radius::ZERO,
        Color::from_rgb8(30, 64, 175),
    );
    scene.pop_layer();
    common::render_to_png_wgpu("brightness", &scene, 640, 360, Color::WHITE)
}
