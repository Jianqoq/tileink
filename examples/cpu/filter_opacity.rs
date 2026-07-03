#[path = "../common/mod.rs"]
mod common;

use peniko::{Color, kurbo::Rect};
use tileink::{Canvas, Filter, Radius};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut scene = Canvas::new(640, 360);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 640.0, 360.0),
        Radius::ZERO,
        Color::from_rgb8(248, 249, 251),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(120.0, 90.0, 520.0, 270.0),
        Radius::ZERO,
        Color::from_rgb8(226, 232, 240),
    );
    scene.push_filter_layer(Filter::Opacity(0.45), common::canvas_region(640, 360));
    common::fill_rect(
        &mut scene,
        Rect::new(180.0, 70.0, 460.0, 290.0),
        Radius::ZERO,
        Color::from_rgb8(220, 38, 38),
    );
    scene.pop_layer();
    common::render_to_png("filter_opacity", &scene, 640, 360, Color::WHITE)
}
