#[path = "../common/mod.rs"]
mod common;

use peniko::{
    Color, Compose, Mix,
    kurbo::{Affine, Rect},
};
use tileink::{Radius, Canvas};

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
        Rect::new(150.0, 80.0, 360.0, 280.0),
        Radius::ZERO,
        Color::from_rgb8(37, 99, 235),
    );

    scene.push_blend_layer(
        common::rect_path(Rect::new(0.0, 0.0, 640.0, 360.0), Radius::ZERO),
        Affine::IDENTITY,
        0.1,
        Mix::Multiply,
        Compose::SrcOver,
    );
    common::fill_rect(
        &mut scene,
        Rect::new(280.0, 120.0, 500.0, 300.0),
        Radius::ZERO,
        Color::from_rgb8(220, 38, 38),
    );
    scene.pop_layer();

    common::render_to_png_wgpu("blend", &scene, 640, 360, Color::WHITE)
}
