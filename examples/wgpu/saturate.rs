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
    scene.push_filter_layer(Filter::Saturate(2.4), common::canvas_region(640, 360));
    common::fill_rect(
        &mut scene,
        Rect::new(90.0, 90.0, 250.0, 270.0),
        Radius::ZERO,
        Color::from_rgb8(129, 140, 248),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(250.0, 90.0, 410.0, 270.0),
        Radius::ZERO,
        Color::from_rgb8(45, 212, 191),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(410.0, 90.0, 550.0, 270.0),
        Radius::ZERO,
        Color::from_rgb8(251, 146, 60),
    );
    scene.pop_layer();
    common::render_to_png_wgpu("saturate", &scene, 640, 360, Color::WHITE)
}
