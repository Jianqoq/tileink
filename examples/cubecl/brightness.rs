#[path = "../common/mod.rs"]
mod common;

use peniko::{Color, kurbo::Rect};
use tileink::{Filter, Radius, Scene};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut scene = Scene::new(640, 360);
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
    common::render_to_png_cubecl("brightness", &scene, 640, 360, Color::WHITE)
}
