#[path = "../common/candlestick_scene.rs"]
mod candlestick_scene;
use crate::common;

use peniko::Color;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let (scene, width, height) = candlestick_scene::candlestick_scene();
    common::render_to_png("candlestick", &scene, width, height, Color::WHITE)
}
