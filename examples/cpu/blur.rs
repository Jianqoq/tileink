#[path = "../common/mod.rs"]
mod common;

use peniko::{
    Color,
    kurbo::{Circle, Rect},
};
use tileink::{Filter, Radius, Canvas};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut scene = Canvas::new(1920, 1080);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 1920.0, 1080.0),
        Radius::ZERO,
        Color::from_rgb8(248, 249, 251),
    );
    scene.push_filter_layer(
        Filter::Blur {
            std_dev_x: 28.0,
            std_dev_y: 28.0,
            sampling: Default::default(),
        },
        common::canvas_region(1920, 1080),
    );
    common::fill_circle(
        &mut scene,
        Circle::new((880.0, 520.0), 250.0),
        Color::from_rgba8(37, 99, 235, 230),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(800.0, 360.0, 1240.0, 700.0),
        Radius::ZERO,
        Color::from_rgba8(220, 38, 38, 210),
    );
    scene.pop_layer();
    common::fill_circle(&mut scene, Circle::new((880.0, 520.0), 128.0), Color::WHITE);
    common::render_to_png("blur", &scene, 1920, 1080, Color::WHITE)
}
