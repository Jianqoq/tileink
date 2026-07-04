use crate::common;

use peniko::Color;

const TARGET_WIDTH: u32 = 900;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let input = common::example_asset("tiger.svg");
    let (scene, width, height) = common::load_svg_scene(input, TARGET_WIDTH)?;
    common::render_to_png_wgpu("svg_tiger", &scene, width, height, Color::TRANSPARENT)
}
