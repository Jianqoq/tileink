#[path = "../common/mod.rs"]
mod common;

use peniko::{
    Color,
    kurbo::{Circle, Rect},
};
use tileink::{Filter, Radius, Scene};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut scene = Scene::new(640, 360);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 640.0, 360.0),
        Radius::ZERO,
        Color::from_rgb8(246, 248, 251),
    );
    scene.push_filter_layer(Filter::Invert(1.0), common::canvas_region(640, 360));
    common::fill_rect(
        &mut scene,
        Rect::new(110.0, 70.0, 530.0, 290.0),
        Radius::ZERO,
        Color::from_rgb8(15, 23, 42),
    );
    common::fill_circle(
        &mut scene,
        Circle::new((320.0, 180.0), 82.0),
        Color::from_rgb8(245, 158, 11),
    );
    scene.pop_layer();
    common::render_to_png("invert", &scene, 640, 360, Color::WHITE)
}
