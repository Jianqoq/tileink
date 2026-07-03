#[path = "../common/mod.rs"]
mod common;

use peniko::{Color, kurbo::Rect};
use tileink::{Canvas, Radius};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut scene = Canvas::new(480, 320);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 480.0, 320.0),
        Radius::ZERO,
        Color::from_rgb8(248, 249, 251),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(48.0, 48.0, 280.0, 200.0),
        Radius {
            top_left: 36.0,
            top_right: 36.0,
            bottom_left: 8.0,
            bottom_right: 8.0,
        },
        Color::from_rgb8(37, 99, 235),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(300.0, 56.0, 432.0, 188.0),
        Radius {
            top_left: 8.0,
            top_right: 40.0,
            bottom_left: 40.0,
            bottom_right: 8.0,
        },
        Color::from_rgb8(220, 38, 38),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(56.0, 208.0, 424.0, 288.0),
        Radius {
            top_left: 12.0,
            top_right: 12.0,
            bottom_left: 28.0,
            bottom_right: 28.0,
        },
        Color::from_rgb8(22, 163, 74),
    );
    common::render_to_png_wgpu("rounded_rect", &scene, 480, 320, Color::WHITE)
}
