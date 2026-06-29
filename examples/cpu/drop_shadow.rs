#[path = "../common/mod.rs"]
mod common;

use peniko::{
    Color, Gradient,
    color::palette::css,
    kurbo::{Circle, Rect},
};
use tileink::{Brush, Filter, Radius, Scene};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut scene = Scene::new(640, 360);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 640.0, 360.0),
        Radius::all(0.0),
        Color::from_rgb8(248, 249, 251),
    );

    let shadow =
        Gradient::new_linear((180.0, 0.0), (440.0, 0.0)).with_stops([css::MAGENTA, css::BLUE]);
    scene.push_filter_layer(
        Filter::DropShadow {
            offset_x: 26.0,
            offset_y: 24.0,
            std_dev: 12.0,
            brush: Brush::from_gradient(&shadow),
        },
        common::canvas_region(640, 360),
    );
    common::fill_circle(
        &mut scene,
        Circle::new((300.0, 160.0), 88.0),
        Color::from_rgb8(37, 99, 235),
    );
    scene.pop_layer();

    common::render_to_png("drop_shadow", &scene, 640, 360, Color::WHITE)
}
