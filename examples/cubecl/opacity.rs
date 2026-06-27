#[path = "../common/mod.rs"]
mod common;

use peniko::{
    Color,
    kurbo::{Affine, Circle, Rect},
};
use tileink::{Radius, Scene};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut scene = Scene::new(360, 260);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 360.0, 260.0),
        Radius::all(0.0),
        Color::from_rgb8(248, 249, 251),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(52.0, 56.0, 210.0, 204.0),
        Radius::all(0.0),
        Color::from_rgb8(226, 232, 240),
    );
    scene.push_opacity_layer(
        common::rect_path(Rect::new(0.0, 0.0, 360.0, 260.0), Radius::all(0.0)),
        Affine::IDENTITY,
        0.1,
        0.45,
    );
    common::fill_rect(
        &mut scene,
        Rect::new(92.0, 76.0, 264.0, 168.0),
        Radius::all(0.0),
        Color::from_rgb8(37, 99, 235),
    );
    common::fill_circle(
        &mut scene,
        Circle::new((236.0, 146.0), 58.0),
        Color::from_rgb8(220, 38, 38),
    );
    scene.pop_layer();
    common::fill_rect(
        &mut scene,
        Rect::new(232.0, 42.0, 308.0, 94.0),
        Radius::all(0.0),
        Color::from_rgb8(22, 163, 74),
    );
    common::render_to_png_cubecl("opacity", &scene, 360, 260, Color::WHITE)
}
