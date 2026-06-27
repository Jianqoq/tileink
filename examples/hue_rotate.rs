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
    scene.push_filter_layer(Filter::HueRotate(120.0), common::canvas_region(640, 360));
    common::fill_rect(
        &mut scene,
        Rect::new(90.0, 80.0, 230.0, 280.0),
        Radius::all(0.0),
        Color::from_rgb8(220, 38, 38),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(250.0, 80.0, 390.0, 280.0),
        Radius::all(0.0),
        Color::from_rgb8(37, 99, 235),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(410.0, 80.0, 550.0, 280.0),
        Radius::all(0.0),
        Color::from_rgb8(22, 163, 74),
    );
    scene.pop_layer();
    common::render_to_png("hue_rotate", &scene, 640, 360, Color::WHITE)
}
