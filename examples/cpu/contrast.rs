#[path = "../common/mod.rs"]
mod common;

use peniko::{Color, kurbo::Rect};
use tileink::{Filter, Radius, Scene};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut scene = Scene::new(640, 360);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 640.0, 360.0),
        Radius::all(0.0),
        Color::from_rgb8(246, 248, 251),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(80.0, 80.0, 250.0, 280.0),
        Radius::all(0.0),
        Color::from_rgb8(96, 165, 250),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(170.0, 80.0, 300.0, 280.0),
        Radius::all(0.0),
        Color::from_rgb8(30, 41, 59),
    );
    scene.push_filter_layer(Filter::Contrast(1.8), common::canvas_region(640, 360));
    common::fill_rect(
        &mut scene,
        Rect::new(360.0, 80.0, 530.0, 280.0),
        Radius::all(0.0),
        Color::from_rgb8(96, 165, 250),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(450.0, 80.0, 580.0, 280.0),
        Radius::all(0.0),
        Color::from_rgb8(30, 41, 59),
    );
    scene.pop_layer();
    common::render_to_png("contrast", &scene, 640, 360, Color::WHITE)
}
